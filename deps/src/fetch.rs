//! Every fact in the report comes through here: one GET, written down.
//!
//! Each response is kept in the cache directory with the time it was
//! fetched. A cached response younger than a day is used again; with
//! `replay` set, only the cache is used and a missing entry is an error,
//! which is how the tests run without a network.
//!
//! Requests carry a User-Agent that names the tool and nothing else: no
//! person, no address, no project. Requests to one host are at least a
//! second apart, which is what the crates.io policy asks of a crawler.

use carcinify_diff::provenance::sha256_bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const USER_AGENT: &str = concat!(
    "carcinify/",
    env!("CARGO_PKG_VERSION"),
    " (dependency check tool)"
);
const FRESH_FOR: u64 = 24 * 60 * 60;

/// The hosts the tool may ask. Anything else is a bug.
const ALLOWED_HOSTS: [&str; 5] = [
    "crates.io",
    "index.crates.io",
    "docs.rs",
    "rustsec.org",
    "pypi.org",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub url: String,
    pub status: u16,
    /// Seconds since 1970-01-01 UTC when the answer was received.
    pub fetched_at: u64,
    pub body: String,
}

/// One line of the report's list of requests.
#[derive(Debug, Clone, Serialize)]
pub struct Logged {
    pub url: String,
    pub status: u16,
    pub fetched_at: String,
    pub from_cache: bool,
}

pub struct Fetcher {
    pub cache: PathBuf,
    pub replay: bool,
    pub refresh: bool,
    last: BTreeMap<String, Instant>,
    pub log: Vec<Logged>,
    /// The answer to each entry of `log`, in the same order.
    answers: Vec<Response>,
    agent: ureq::Agent,
}

fn host(url: &str) -> &str {
    url.trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("")
}

/// `2026-11-15T09:30:00Z` for a time in seconds since the epoch.
pub fn iso(secs: u64) -> String {
    let days = secs / 86400;
    let (y, m, d) = civil(days as i64);
    let rest = secs % 86400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's algorithm.
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Seconds since the epoch for a timestamp that starts `YYYY-MM-DDTHH:MM:SS`.
pub fn parse_time(text: &str) -> Option<u64> {
    let n = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, min, s) = (
        n(11..13).unwrap_or(0),
        n(14..16).unwrap_or(0),
        n(17..19).unwrap_or(0),
    );
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    u64::try_from((era * 146097 + doe - 719468) * 86400 + h * 3600 + min * 60 + s).ok()
}

impl Fetcher {
    pub fn new(cache: PathBuf, replay: bool, refresh: bool) -> Fetcher {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .build();
        Fetcher {
            cache,
            replay,
            refresh,
            last: BTreeMap::new(),
            log: Vec::new(),
            answers: Vec::new(),
            agent: config.into(),
        }
    }

    fn path(&self, url: &str) -> PathBuf {
        self.cache
            .join(format!("{}.json", &sha256_bytes(url.as_bytes())[..20]))
    }

    /// GETs `url`, or returns the cached answer. A 404 is an answer, not an
    /// error: "there is no such crate" is a fact the report needs.
    pub fn get(&mut self, url: &str) -> Result<Response, String> {
        if !ALLOWED_HOSTS.contains(&host(url)) {
            return Err(format!(
                "refusing to fetch {url}: not one of {ALLOWED_HOSTS:?}"
            ));
        }
        // The same URL asked twice in one run is one answer, listed once.
        if let Some(again) = self.log.iter().position(|l| l.url == url)
            && let Some(answer) = self.answers.get(again)
        {
            return Ok(answer.clone());
        }
        let path = self.path(url);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(cached) = serde_json::from_str::<Response>(&text)
            && cached.url == url
            && (self.replay || (!self.refresh && now.saturating_sub(cached.fetched_at) < FRESH_FOR))
        {
            self.log.push(Logged {
                url: url.to_string(),
                status: cached.status,
                fetched_at: iso(cached.fetched_at),
                from_cache: true,
            });
            self.answers.push(cached.clone());
            return Ok(cached);
        }
        if self.replay {
            return Err(format!(
                "no recorded response for {url} in {} (replay mode makes no requests)",
                self.cache.display()
            ));
        }
        if let Some(last) = self.last.get(host(url)) {
            let wait = Duration::from_millis(1100).saturating_sub(last.elapsed());
            std::thread::sleep(wait);
        }
        let result = self.agent.get(url).call();
        self.last.insert(host(url).to_string(), Instant::now());
        let mut response = result.map_err(|e| format!("GET {url}: {e}"))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("GET {url}: {e}"))?;
        if status == 429 || status >= 500 {
            return Err(format!("GET {url}: HTTP {status}; try again later"));
        }
        let fetched = Response {
            url: url.to_string(),
            status,
            fetched_at: now,
            body,
        };
        std::fs::create_dir_all(&self.cache)
            .map_err(|e| format!("cannot create {}: {e}", self.cache.display()))?;
        let text = serde_json::to_string(&fetched).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        self.log.push(Logged {
            url: url.to_string(),
            status,
            fetched_at: iso(now),
            from_cache: false,
        });
        self.answers.push(fetched.clone());
        Ok(fetched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_go_both_ways() {
        for (text, secs) in [
            ("1970-01-01T00:00:00Z", 0),
            ("2026-08-20T12:34:56Z", 1787229296),
            ("2024-02-29T23:59:59.5Z", 1709251199),
            ("2000-03-01T00:00:00Z", 951868800),
        ] {
            assert_eq!(parse_time(text), Some(secs), "{text}");
            assert_eq!(iso(secs)[..19], text[..19]);
        }
        assert_eq!(parse_time("soon"), None);
    }

    #[test]
    fn only_the_listed_hosts_are_asked_and_replay_makes_no_requests() {
        let dir = std::env::temp_dir().join(format!("carcinify-deps-fetch-{}", std::process::id()));
        let mut f = Fetcher::new(dir.clone(), true, false);
        assert!(
            f.get("https://example.com/x")
                .unwrap_err()
                .contains("refusing")
        );
        assert!(
            f.get("https://crates.io/api/v1/crates/serde")
                .unwrap_err()
                .contains("no recorded response")
        );
        assert!(!USER_AGENT.contains('@') && USER_AGENT.starts_with("carcinify/"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
