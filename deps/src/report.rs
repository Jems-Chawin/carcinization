//! The proposal an agent writes, the lookups the tool makes, and the report.
//!
//! An agent proposes: which PyPI project an import belongs to, what to
//! search for, which crates to look at, which search results to set aside
//! and why, and what it would choose. The tool does the rest: it reads the
//! imports from the Python, runs every search, fetches every fact, decides
//! the classification and the outcome from what came back, and writes it
//! all down with the time it was fetched.

use crate::fetch::{Fetcher, Logged, iso, parse_time};
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const PROPOSAL_SCHEMA: &str = "carcinify/deps/proposal/1";
pub const REPORT_SCHEMA: &str = "carcinify/deps/report/2";
/// How many results of each search have to be accounted for before "none".
pub const TOP: usize = 5;
/// A dismissal needs a reason of at least this many words.
pub const MIN_REASON_WORDS: usize = 3;
/// A crate is called plainly alive when its latest release is at most this
/// many days old and it has at least `ALIVE_DOWNLOADS` downloads in the
/// last 90 days.
pub const ALIVE_DAYS: u64 = 730;
pub const ALIVE_DOWNLOADS: u64 = 1000;
/// Or, whatever the date of its last release, when it has at least this
/// many downloads in the last 90 days: such a crate exists and is in wide
/// use, even if it is deprecated or unmaintained.
pub const ALIVE_HEAVY_USE: u64 = 100_000;
/// A crate that has advisories, looked up on every run to see that the
/// advisory lookup works at all.
pub const ADVISORY_PROBE: &str = "serde_yaml";
/// Searches the agent has to add, beyond the two names the tool always
/// searches, before "none" is accepted.
pub const MIN_DESCRIPTIVE_SEARCHES: usize = 2;
const DRIVER: &str = include_str!("imports.py");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub schema: String,
    /// The Python files of the piece being ported, relative to this file.
    pub python_files: Vec<String>,
    /// One entry per third-party import the piece uses, keyed by the
    /// import (`yaml`, or `google.adk` for a namespace package).
    pub dependencies: BTreeMap<String, Proposed>,
    /// Imports made at run time under a name the tool cannot read, which
    /// the author has looked at: where each is, and what it loads.
    #[serde(default)]
    pub acknowledged_unread_imports: Vec<Acknowledged>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledged {
    pub file: String,
    pub line: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proposed {
    /// The project's name on PyPI.
    pub pypi: String,
    /// Searches to run, beyond the PyPI name and the import name, which the
    /// tool always runs. Words for what the library does.
    #[serde(default)]
    pub search_terms: Vec<String>,
    /// Crates to look up.
    #[serde(default)]
    pub candidates: Vec<String>,
    /// For a candidate, where the organisation behind the Python library
    /// itself names the crate (a page of its documentation or repository).
    /// The tool does not fetch it; it is shown as the proposal's statement.
    #[serde(default)]
    pub declared_official_evidence: BTreeMap<String, String>,
    /// Search results looked at and set aside, with the reason.
    #[serde(default)]
    pub dismissed: BTreeMap<String, String>,
    /// What the agent would do, shown as its statement.
    pub decision: Option<Decision>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// `crate:<name>`, `keep_in_python`, `keep_part_in_python` or `write_it`.
    pub choice: String,
    pub reason: String,
}

pub struct Options {
    pub proposal: PathBuf,
    pub cache: PathBuf,
    pub replay: bool,
    pub refresh: bool,
    pub python: PathBuf,
    pub min_age_days: u64,
}

#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    /// `complete` or `incomplete`.
    pub verdict: &'static str,
    pub exit_status: u8,
    pub reasons: Vec<String>,
    /// What no lookup can tell, whatever the report says.
    pub not_judged: [&'static str; 5],
    pub generated_at: String,
    /// Where the answers came from: fetched now, or taken from the cache.
    pub source: J,
    /// Whether the advisory lookup was seen to work on this run.
    pub advisory_lookup: J,
    /// Imports made at run time with a name the tool could not read.
    pub unread_imports: Vec<J>,
    /// Things about the whole report for a reader to look at.
    pub flags: Vec<String>,
    pub min_publish_age_days: u64,
    pub proposal: J,
    pub python: J,
    pub dependencies: Vec<Dependency>,
    /// Imports that were not looked up, and why.
    pub not_looked_up: Vec<J>,
    pub requests: Vec<Logged>,
    /// What the requests made on this run carried. `None` when the report
    /// is a replay: no request was made, and what the recorded ones carried
    /// is not kept with them.
    pub user_agent: Option<&'static str>,
}

pub const NOT_JUDGED: [&str; 5] = [
    "Whether a crate covers the part of the library the piece uses. The report lists what the piece uses; someone has to read the crate's documentation against that list.",
    "Whether a crate is correct, sound, or well written. Downloads, age and release dates say that people use it and that it is maintained, not that it is right.",
    "Whether a crate is what it claims to be. Its repository link, description and name are typed by whoever published it, and are not evidence of anything. Download counts are a weak signal: they are dominated by being a dependency of something else, and a crate with 245 million downloads has been taken over. Only an owner that is a GitHub team of the organisation, or a release through trusted publishing from its repository, is checked by crates.io.",
    "Whether \"none found by these queries\" means none exists. A crate's name often has nothing in common with the Python library's. It means these searches of crates.io, on this day, returned nothing that was kept; the queries are in the report so that they can be run again and added to.",
    "Whether porting the dependency is worth it at all. Keeping it in Python is listed for every dependency because it is often the right choice, not as a fallback.",
];

#[derive(Serialize)]
pub struct Dependency {
    pub import: String,
    /// Modules of it the piece imports, with lines and the names used.
    pub used: Vec<J>,
    pub pypi: J,
    pub searches: Vec<Search>,
    pub candidates: Vec<Candidate>,
    /// Crates in the top results of a search that are neither candidates
    /// nor dismissed.
    pub not_examined: Vec<String>,
    /// Crates the proposal set aside, each with its reason and the facts
    /// the tool fetched for it.
    pub dismissed: Vec<Dismissed>,
    /// `official_crate`, `crate_named_by_upstream`, `community_crates`,
    /// `none_found_by_these_queries`,
    /// `candidates_dismissed_by_the_proposal` or `unresolved`.
    pub outcome: &'static str,
    pub outcome_reason: String,
    pub options: Vec<String>,
    pub declared_decision: Option<Decision>,
    pub flags: Vec<String>,
}

#[derive(Serialize)]
pub struct Search {
    pub query: String,
    /// `tool` for the two names always searched, `proposal` otherwise.
    pub from: &'static str,
    pub url: String,
    pub fetched_at: String,
    pub total: u64,
    pub top: Vec<J>,
}

#[derive(Serialize)]
pub struct Dismissed {
    /// The reason, as the proposal gives it.
    pub declared_reason: String,
    /// True when the crate has a release in the last `ALIVE_DAYS` days and
    /// `ALIVE_DOWNLOADS` downloads in the last 90, or `ALIVE_HEAVY_USE`
    /// downloads in the last 90 whatever its last release.
    pub plainly_alive: bool,
    /// Which of the two made it so, in words.
    pub alive_because: Option<String>,
    #[serde(rename = "crate")]
    pub krate: Candidate,
}

#[derive(Serialize)]
pub struct Candidate {
    /// The name as crates.io has it.
    pub name: String,
    /// The name the proposal gave, when it is spelled differently.
    pub proposed_as: Option<String>,
    pub exists: bool,
    /// `official`, `named_by_upstream`, `community`, or `not_found`.
    pub classification: &'static str,
    pub evidence: Vec<String>,
    pub facts: J,
    pub flags: Vec<String>,
}

fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Paths under github.com that are not an account.
const NOT_OWNERS: [&str; 16] = [
    "sponsors",
    "orgs",
    "apps",
    "marketplace",
    "features",
    "about",
    "topics",
    "collections",
    "settings",
    "users",
    "login",
    "join",
    "pricing",
    "explore",
    "search",
    "security",
];

/// `github.com/<org>` and `gitlab.com/<org>` owners named in some URLs,
/// lower-cased, each with its host (`github` or `gitlab`). An organisation
/// on one host is not the organisation of that name on the other.
fn orgs(urls: &[String]) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for url in urls {
        let lower = url.to_lowercase();
        let rest = lower.split("://").nth(1).unwrap_or(&lower);
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        for (site, host) in [("github", "github.com/"), ("gitlab", "gitlab.com/")] {
            if let Some(path) = rest.strip_prefix(host) {
                let org = path.split(['/', '?', '#']).next().unwrap_or("").to_string();
                if !org.is_empty()
                    && !NOT_OWNERS.contains(&org.as_str())
                    && !out.contains(&(site, org.clone()))
                {
                    out.push((site, org));
                }
            }
        }
    }
    out
}

/// Crate names with `-` and `_` taken as the same, as crates.io does.
fn same_crate(a: &str, b: &str) -> bool {
    a.replace('_', "-")
        .eq_ignore_ascii_case(&b.replace('_', "-"))
}

/// Advisory ids on a RustSec package page. `None` when the answer is
/// neither the page (200) nor "no such page" (404): then it is not known.
fn advisories(status: u16, body: &str) -> Option<Vec<String>> {
    match status {
        404 => Some(Vec::new()),
        200 => {
            let mut found: Vec<String> = Vec::new();
            for part in body.split("RUSTSEC-").skip(1) {
                let id: String = part
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                let id = format!("RUSTSEC-{id}");
                if id.len() == 17 && !found.contains(&id) {
                    found.push(id);
                }
            }
            Some(found)
        }
        _ => None,
    }
}

/// True when `text` (lower case) has `link` followed by something that
/// cannot be part of a crate name, so `docs.rs/serde` is not found in
/// `docs.rs/serde_json`.
fn links_to(text: &str, link: &str) -> bool {
    text.match_indices(link).any(|(at, _)| {
        !text[at + link.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    })
}

/// Whether the PyPI project's name and the import's name show that one is
/// the other: compared with everything but letters and digits removed.
fn name_shows_import(pypi: &str, import: &str) -> bool {
    let norm = |s: &str| {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_lowercase()
    };
    let (project, whole) = (norm(pypi), norm(import));
    let top = norm(import.split('.').next().unwrap_or(import));
    !project.is_empty()
        && (project.contains(&whole)
            || whole.contains(&project)
            || (top.len() >= 3 && project.contains(&top) && !import.contains('.')))
}

fn days_between(earlier: u64, later: u64) -> u64 {
    later.saturating_sub(earlier) / 86400
}

type Org = (&'static str, String);

fn pypi(f: &mut Fetcher, name: &str) -> Result<(J, Vec<Org>, String), String> {
    let r = f.get(&format!("https://pypi.org/pypi/{}/json", encode(name)))?;
    if r.status != 200 {
        return Ok((
            serde_json::json!({"name": name, "exists": false, "status": r.status, "fetched_at": iso(r.fetched_at)}),
            Vec::new(),
            String::new(),
        ));
    }
    let j: J = serde_json::from_str(&r.body).map_err(|e| format!("PyPI answer for {name}: {e}"))?;
    let info = &j["info"];
    // The organisation is read from the links a project gives for its own
    // source and home, not from every link it lists: a funding page, a
    // changelog or a link to someone else's project names other accounts.
    let own = |label: &str| {
        let label = label.to_lowercase();
        ["source", "repo", "code", "home", "github", "gitlab"]
            .iter()
            .any(|w| label.contains(w))
    };
    let mut urls: Vec<String> = info["project_urls"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(label, _)| own(label))
                .filter_map(|(_, url)| url.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    urls.extend(info["home_page"].as_str().map(str::to_string));
    let upstream = orgs(&urls);
    Ok((
        serde_json::json!({
            "name": info["name"], "name_declared_by": "the proposal", "exists": true, "version": info["version"], "summary": info["summary"],
            "license": info["license_expression"].as_str().or(info["license"].as_str()).map(|l| l.chars().take(80).collect::<String>()),
            "project_urls": info["project_urls"], "home_page": info["home_page"],
            "organisations_from": "the project's source, repository and home links on PyPI",
            "organisations": upstream.iter().map(|(site, org)| format!("{site}.com/{org}")).collect::<Vec<_>>(), "fetched_at": iso(r.fetched_at),
        }),
        upstream,
        info["description"].as_str().unwrap_or("").to_lowercase(),
    ))
}

fn search(f: &mut Fetcher, query: &str, from: &'static str) -> Result<Search, String> {
    let url = format!(
        "https://crates.io/api/v1/crates?q={}&per_page=10",
        encode(query)
    );
    let r = f.get(&url)?;
    let j: J = serde_json::from_str(&r.body)
        .map_err(|e| format!("crates.io search for {query:?}: {e}"))?;
    let top = j["crates"].as_array().cloned().unwrap_or_default();
    let top = top.iter().map(|c| serde_json::json!({
        "name": c["name"], "description": c["description"].as_str().map(|d| d.trim().chars().take(140).collect::<String>()),
        "downloads": c["downloads"], "recent_downloads": c["recent_downloads"], "newest_version": c["newest_version"], "updated_at": c["updated_at"],
    })).collect();
    Ok(Search {
        query: query.to_string(),
        from,
        url,
        fetched_at: iso(r.fetched_at),
        total: j["meta"]["total"].as_u64().unwrap_or(0),
        top,
    })
}

/// Edits (insert, delete, replace one character) needed to turn `a` into
/// `b`, with `-` and `_` taken as the same.
fn edit_distance(a: &str, b: &str) -> usize {
    let norm = |s: &str| {
        s.replace('_', "-")
            .to_lowercase()
            .chars()
            .collect::<Vec<_>>()
    };
    let (a, b) = (norm(a), norm(b));
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { previous } else { previous + 1 };
            previous = row[j + 1];
            row[j + 1] = cost.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

struct Upstream<'a> {
    orgs: &'a [Org],
    /// The Python project's description on PyPI, lower-cased.
    description: &'a str,
    /// What the proposal says about where upstream names this crate.
    declared: Option<&'a String>,
    /// False when the advisory lookup was not seen to work on this run.
    advisories_work: bool,
    /// False for a dismissed crate: the look-alike search and the build
    /// facts from docs.rs are left out.
    full: bool,
}

fn candidate(
    f: &mut Fetcher,
    name: &str,
    up: &Upstream,
    min_age_days: u64,
) -> Result<Candidate, String> {
    let upstream = up.orgs;
    let url = format!("https://crates.io/api/v1/crates/{}", encode(name));
    let r = f.get(&url)?;
    if r.status == 404 {
        return Ok(Candidate {
            name: name.to_string(),
            proposed_as: None,
            exists: false,
            classification: "not_found",
            evidence: vec![format!(
                "crates.io answered 404 for {url} at {}",
                iso(r.fetched_at)
            )],
            facts: J::Null,
            flags: vec!["there is no crate of this name on crates.io".to_string()],
        });
    }
    if r.status != 200 {
        return Err(format!("{url}: HTTP {}", r.status));
    }
    let now = r.fetched_at;
    let j: J =
        serde_json::from_str(&r.body).map_err(|e| format!("crates.io answer for {name}: {e}"))?;
    let (c, versions) = (
        &j["crate"],
        j["versions"].as_array().cloned().unwrap_or_default(),
    );
    // crates.io answers for `serde-yaml` and `serde_yaml` alike. Everything
    // from here on uses the name it has, not the one that was typed: other
    // services know only that one.
    let proposed = name;
    let name = c["name"].as_str().or(c["id"].as_str()).unwrap_or(proposed);
    let url = format!("https://crates.io/api/v1/crates/{}", encode(name));
    let time = |v: &J| v["created_at"].as_str().and_then(parse_time);
    let live: Vec<&J> = versions.iter().filter(|v| v["yanked"] != true).collect();
    let latest = live.iter().copied().max_by_key(|v| time(v));
    let yanked: Vec<&str> = versions
        .iter()
        .filter(|v| v["yanked"] == true)
        .filter_map(|v| v["num"].as_str())
        .collect();
    let first = versions.iter().filter_map(time).min();
    let last_year = versions
        .iter()
        .filter_map(time)
        .filter(|t| days_between(*t, now) <= 365)
        .count();

    let owners_r = f.get(&format!("{url}/owners"))?;
    let owners: J = serde_json::from_str(&owners_r.body).unwrap_or(J::Null);
    let owners: Vec<J> = owners["users"].as_array().cloned().unwrap_or_default();
    let owner_list: Vec<J> = owners
        .iter()
        .map(|o| serde_json::json!({"login": o["login"], "kind": o["kind"]}))
        .collect();

    // RustSec keeps a page per crate that has advisories. 200 is the page,
    // 404 is no page; anything else, or a lookup that did not work on a
    // crate known to have advisories, and the answer is not known.
    let sec = f.get(&format!(
        "https://rustsec.org/packages/{}.html",
        encode(name)
    ))?;
    let advisories = advisories(sec.status, &sec.body).filter(|_| up.advisories_work);

    // Build script and proc-macro: crates.io does not say. The source view
    // of docs.rs lists the files and shows Cargo.toml; best effort.
    let mut build_script = J::Null;
    let mut proc_macro = J::Null;
    if let Some(version) = latest.and_then(|v| v["num"].as_str()).filter(|_| up.full) {
        let base = format!(
            "https://docs.rs/crate/{}/{}/source/",
            encode(name),
            encode(version)
        );
        if let Ok(listing) = f.get(&base)
            && listing.status == 200
        {
            build_script = J::Bool(
                listing.body.contains(">build.rs<") || listing.body.contains("/source/build.rs"),
            );
        }
        if let Ok(manifest) = f.get(&format!("{base}Cargo.toml"))
            && manifest.status == 200
        {
            let mut text = String::new();
            let mut in_tag = false;
            for ch in manifest.body.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => text.push(ch),
                    _ => {}
                }
            }
            let squeezed: String = text.split_whitespace().collect();
            proc_macro = J::Bool(
                squeezed.contains("proc-macro=true") || squeezed.contains("proc_macro=true"),
            );
            if build_script == J::Bool(false) && squeezed.contains("build=&quot;") {
                build_script = J::Bool(true);
            }
        }
    }

    // Classification. A crate is `official` only on evidence that the
    // publisher of the crate could not have typed: who crates.io says owns
    // it, how crates.io says it was released, or what the Python project
    // itself says. The crate's own repository link is not evidence.
    let mut evidence = Vec::new();
    let mut class = "community";
    let on_github = |org: &str| {
        upstream
            .iter()
            .any(|(site, u)| *site == "github" && u == org)
    };
    for owner in &owners {
        // A team owner's login is `github:<org>:<team>`; crates.io checks
        // team membership with GitHub.
        let login = owner["login"].as_str().unwrap_or("").to_lowercase();
        let mut parts = login.split(':');
        if let (Some("github"), Some(org)) = (parts.next(), parts.next())
            && on_github(org)
        {
            class = "official";
            evidence.push(format!("crates.io owner `{login}` is a GitHub team of `{org}`, the organisation that hosts the Python project (checked by crates.io)"));
        }
    }
    // Trusted publishing: crates.io records the repository the release was
    // built in, as `{"provider": "github", "repository": "<owner>/<name>"}`
    // (read from real records of jiff, bon, pyo3, maturin, rand and
    // uv-version on 2026-11-14 (UTC)). The owner is compared whole. A record in
    // any other shape is not used.
    if let Some(record) = latest.map(|v| &v["trustpub_data"]).filter(|t| !t.is_null()) {
        let owner = record["repository"]
            .as_str()
            .and_then(|r| r.split_once('/'))
            .map(|(owner, _)| owner.to_lowercase());
        match (record["provider"].as_str(), owner) {
            (Some("github"), Some(owner)) if on_github(&owner) => {
                class = "official";
                evidence.push(format!("the latest version was released by trusted publishing from `{}`, a repository of `{owner}`, the organisation that hosts the Python project (recorded by crates.io)", record["repository"].as_str().unwrap_or("")));
            }
            (Some("github"), Some(_)) => {}
            _ => evidence.push("the latest version has a trusted-publishing record in a shape this tool does not know; it was not used".to_string()),
        }
    }
    let lower = name.to_lowercase();
    let spellings = [
        lower.clone(),
        lower.replace('_', "-"),
        lower.replace('-', "_"),
    ];
    if spellings.iter().any(|n| {
        links_to(up.description, &format!("crates.io/crates/{n}"))
            || links_to(up.description, &format!("docs.rs/{n}"))
    }) {
        // A link is not ownership. It gets a class of its own.
        if class != "official" {
            class = "named_by_upstream";
        }
        evidence.push("the Python project's description on PyPI contains a link to this crate. That is all the tool found: it did not read the sentence around the link, which may present the crate as the project's own, as a port by someone else, or as an alternative. Read it".to_string());
    }
    if class != "official" {
        let lives = upstream.iter().map(|(_, o)| o.as_str()).collect::<Vec<_>>();
        evidence.push(match (upstream.is_empty(), class) {
            (true, _) => "the Python project names no GitHub or GitLab organisation in its source and home links on PyPI, so there is no owner to look for".to_string(),
            (false, "named_by_upstream") => format!("no crates.io owner is a team of {lives:?} (where the Python project lives) and the crate was not released by trusted publishing from there, so it is not shown to be published by them"),
            (false, _) => format!("no crates.io owner is a team of {lives:?} (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it"),
        });
        let repo: Vec<String> = c["repository"]
            .as_str()
            .map(str::to_string)
            .into_iter()
            .collect();
        if let Some((_, org)) = orgs(&repo).iter().find(|o| upstream.contains(o)) {
            evidence.push(format!("its repository link points into `{org}`. That link is typed by the publisher and is not evidence; it is a reason to go and check, nothing more"));
        }
    }
    if let Some(statement) = up.declared {
        evidence.push(format!(
            "declared by the proposal, not fetched or checked by the tool: {statement}"
        ));
    }

    // A name one edit away from a much more downloaded crate is how a
    // typosquat looks, and a proposed candidate is where one gets in.
    let mine = c["downloads"].as_u64().unwrap_or(0);
    let mut look_alikes = Vec::new();
    if up.full {
        let near = search(f, name, "tool")?;
        for hit in &near.top {
            let (other, theirs) = (
                hit["name"].as_str().unwrap_or(""),
                hit["downloads"].as_u64().unwrap_or(0),
            );
            if other != name
                && edit_distance(other, name) <= 1
                && theirs >= mine.saturating_mul(50).max(10_000)
            {
                look_alikes.push(format!("`{other}` ({theirs} downloads against {mine})"));
            }
        }
    }

    let mut flags = Vec::new();
    let latest_time = latest.and_then(time);
    if !look_alikes.is_empty() {
        flags.push(format!("the name is one character away from a far more downloaded crate: {}. Check that this is the crate you mean", look_alikes.join(", ")));
    }
    if let Some(age) = first.map(|t| days_between(t, now)).filter(|age| *age < 90) {
        flags.push(format!("the crate itself is {age} day(s) old"));
    }
    if let Some(t) = latest_time {
        let age = days_between(t, now);
        if age < min_age_days {
            flags.push(format!("the latest version is {age} day(s) old, younger than the minimum publish age of {min_age_days}: do not take it yet"));
        }
        if age > 730 {
            flags.push(format!("no release for {} days", age));
        }
    }
    if !yanked.is_empty() {
        flags.push(format!(
            "{} version(s) were yanked: {}",
            yanked.len(),
            yanked
                .iter()
                .take(6)
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if live.is_empty() {
        flags.push("every version is yanked".to_string());
    }
    match &advisories {
        Some(ids) if !ids.is_empty() => flags.push(format!("RustSec has advisories for this crate: {}. Read them; they may concern old versions only", ids.join(", "))),
        Some(_) => {}
        None => flags.push("whether RustSec has advisories for this crate is not known: the lookup did not give a usable answer".to_string()),
    }
    if up.full && build_script.is_null() {
        flags.push("whether it has a build script is not known: the docs.rs source view could not be read. Look at the crate's files before taking it".to_string());
    }
    if up.full && proc_macro.is_null() {
        flags.push(
            "whether it is a proc-macro is not known: its Cargo.toml could not be read on docs.rs"
                .to_string(),
        );
    }
    if build_script == J::Bool(true) {
        flags.push(
            "has a build script, which runs on your machine at build time: name it in the review"
                .to_string(),
        );
    }
    if proc_macro == J::Bool(true) {
        flags.push(
            "is a proc-macro, which runs in the compiler at build time: name it in the review"
                .to_string(),
        );
    }
    if c["repository"].is_null() {
        flags.push("names no repository".to_string());
    }
    let facts = serde_json::json!({
        "description": c["description"].as_str().map(str::trim),
        "repository": c["repository"],
        "owners": owner_list,
        "first_published": first.map(iso),
        "latest_version": latest.map(|v| v["num"].clone()),
        "latest_published": latest_time.map(iso),
        "latest_age_days": latest_time.map(|t| days_between(t, now)),
        "versions": versions.len(),
        "versions_in_the_last_365_days": last_year,
        "yanked_versions": yanked,
                "downloads_weak_signal": {
            "all_time": c["downloads"],
            "recent_90_days": c["recent_downloads"],
            "note": "dominated by being a dependency of other crates; says little about quality or safety",
        },
        "crate_age_days": first.map(|t| days_between(t, now)),
        "license": latest.map(|v| v["license"].clone()),
        "rust_version": latest.map(|v| v["rust_version"].clone()),
        "links_native_library": latest.map(|v| v["lib_links"].clone()),
        "trusted_publishing": latest.map(|v| v["trustpub_data"].clone()),
        "build_script": build_script,
        "proc_macro": proc_macro,
        "build_facts_from": "docs.rs source view, best effort; null means it could not be learned",
        "rustsec_advisories": advisories,
        "rustsec_url": sec.url,
        "rustsec_status": sec.status,
        "rustsec_checked_at": iso(sec.fetched_at),
        "fetched_at": iso(now),
    });
    Ok(Candidate {
        name: name.to_string(),
        proposed_as: Some(proposed.to_string()).filter(|p| p != name),
        exists: true,
        classification: class,
        evidence,
        facts,
        flags,
    })
}

const KEEP: &str = "Keep it in Python and call across the boundary, or keep the part of the piece that uses it in Python. For numerical, scientific and ML libraries, whose work is already done in compiled code, this is usually the right choice.";

fn scan(python: &Path, files: &[PathBuf]) -> Result<J, String> {
    let script =
        std::env::temp_dir().join(format!("carcinify-deps-imports-{}.py", std::process::id()));
    std::fs::write(&script, DRIVER).map_err(|e| e.to_string())?;
    let out = std::process::Command::new(python)
        .arg("-I")
        .arg(&script)
        .args(files)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", python.display()));
    let _ = std::fs::remove_file(&script);
    let out = out?;
    if !out.status.success() {
        return Err(format!(
            "reading the Python failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("import list: {e}"))
}

pub fn run(opts: &Options) -> Result<Report, String> {
    let path = opts
        .proposal
        .canonicalize()
        .map_err(|e| format!("{}: {e}", opts.proposal.display()))?;
    let here = path.parent().ok_or("the proposal has no directory")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let proposal: Proposal =
        serde_json::from_str(&text).map_err(|e| format!("bad proposal {}: {e}", path.display()))?;
    if proposal.schema != PROPOSAL_SCHEMA {
        return Err(format!(
            "proposal schema is {:?}, expected {PROPOSAL_SCHEMA:?}",
            proposal.schema
        ));
    }
    let files: Vec<PathBuf> = proposal.python_files.iter().map(|f| here.join(f)).collect();
    if files.is_empty() {
        return Err(
            "the proposal names no Python file: the check is of a piece of code, not of a project"
                .to_string(),
        );
    }
    let scanned = scan(&opts.python, &files)?;
    let mut f = Fetcher::new(opts.cache.clone(), opts.replay, opts.refresh);
    let mut reasons = Vec::new();

    // Which third-party modules does the piece import, and which proposal
    // entry (the longest matching prefix) covers each?
    let mut used: BTreeMap<String, Vec<J>> = BTreeMap::new();
    let mut not_looked_up = Vec::new();
    let shown = |file: &str| {
        Path::new(file)
            .strip_prefix(here)
            .unwrap_or(Path::new(file))
            .to_string_lossy()
            .into_owned()
    };
    // An import whose name is worked out when the program runs cannot be
    // read from the source. The list of dependencies is then not known to
    // be whole.
    let mut unread_imports = Vec::new();
    let mut report_flags = Vec::new();
    let mut acknowledged = vec![false; proposal.acknowledged_unread_imports.len()];
    for (file, calls) in scanned["unread"].as_object().into_iter().flatten() {
        for call in calls.as_array().into_iter().flatten() {
            let (file, text) = (shown(file), call["call"].as_str().unwrap_or(""));
            let ack = proposal
                .acknowledged_unread_imports
                .iter()
                .position(|a| a.file == file && Some(a.line) == call["line"].as_u64());
            let reason = ack.map(|i| {
                acknowledged[i] = true;
                proposal.acknowledged_unread_imports[i].reason.clone()
            });
            match &reason {
                Some(r) if r.split_whitespace().count() >= MIN_REASON_WORDS => report_flags.push(format!("{file}:{} imports a module by a name worked out at run time ({text}). The tool cannot read which. The proposal's account of it, not checked: {r}", call["line"])),
                Some(r) => reasons.push(format!("{file}:{}: the run-time import is acknowledged with the reason {r:?}; say what it loads, in at least {MIN_REASON_WORDS} words", call["line"])),
                None => reasons.push(format!("{file}:{} imports a module by a name worked out at run time ({text}); the tool cannot read which, so the list of dependencies may be short. Look at what it loads, then acknowledge it in the proposal with a reason, or change the code", call["line"])),
            }
            unread_imports.push(serde_json::json!({"file": file, "line": call["line"], "call": text, "acknowledged_by_the_proposal": reason}));
        }
    }
    for (a, _) in proposal
        .acknowledged_unread_imports
        .iter()
        .zip(&acknowledged)
        .filter(|(_, used)| !**used)
    {
        reasons.push(format!("the proposal acknowledges a run-time import at {}:{}, and there is none there: remove it", a.file, a.line));
    }
    for (file, imports) in scanned["files"].as_object().into_iter().flatten() {
        for import in imports.as_array().into_iter().flatten() {
            let module = import["module"].as_str().unwrap_or("");
            let mut entry = import.clone();
            entry["file"] = J::from(
                Path::new(file)
                    .strip_prefix(here)
                    .unwrap_or(Path::new(file))
                    .to_string_lossy()
                    .into_owned(),
            );
            if import["kind"] != "third_party" {
                not_looked_up.push(serde_json::json!({"module": module, "why": import["kind"], "file": entry["file"]}));
                continue;
            }
            // The longest entry that is the module or a package above it. A
            // name taken from a namespace package counts as its own module:
            // `from google import genai` is an import of `google.genai`.
            let names: Vec<String> = import["uses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(J::as_str)
                .map(|u| format!("{module}.{}", u.split('.').next().unwrap_or(u)))
                .collect();
            let key = proposal
                .dependencies
                .keys()
                .filter(|k| {
                    module == *k
                        || module.starts_with(&format!("{k}."))
                        || names
                            .iter()
                            .any(|n| n == *k || n.starts_with(&format!("{k}.")))
                })
                .max_by_key(|k| k.len());
            match key {
                Some(k) => used.entry(k.clone()).or_default().push(entry),
                None => reasons.push(format!(
                    "{module} is imported at {}:{} and the proposal has no entry for it",
                    entry["file"].as_str().unwrap_or(""),
                    import["lines"][0]
                )),
            }
        }
    }
    for key in proposal
        .dependencies
        .keys()
        .filter(|k| !used.contains_key(*k))
    {
        reasons.push(format!("the proposal has an entry for {key}, which the piece does not import: the check covers what the piece uses, not the project's whole list"));
    }

    // The advisory lookup reads a web page. Before trusting "no page" to
    // mean "no advisory", see that a crate known to have one is found.
    let mut advisory_lookup = serde_json::json!({"checked": false});
    let mut advisories_work = true;
    if !used.is_empty() {
        let url = format!("https://rustsec.org/packages/{ADVISORY_PROBE}.html");
        let probe = f.get(&url)?;
        advisories_work = advisories(probe.status, &probe.body).is_some_and(|ids| !ids.is_empty());
        advisory_lookup = serde_json::json!({"checked": true, "probe": ADVISORY_PROBE, "url": url, "status": probe.status, "works": advisories_work, "fetched_at": iso(probe.fetched_at)});
        if !advisories_work {
            reasons.push(format!("the advisory lookup did not find the advisories of `{ADVISORY_PROBE}`, which has some ({url} answered {}), so advisories are reported as not known for every crate", probe.status));
        }
    }

    let mut dependencies = Vec::new();
    for (import, used) in used {
        let p = &proposal.dependencies[&import];
        let (pypi_facts, upstream, description) = pypi(&mut f, &p.pypi)?;
        let mut flags = Vec::new();
        // PyPI's JSON does not say which modules a project provides, so
        // that the project is the one behind this import is the proposal's
        // word. Where the two names do not even resemble each other, say so.
        // Where the library is installed in the interpreter the tool was
        // given, that interpreter says which project provides the import.
        let top = import.split('.').next().unwrap_or(&import);
        let installed: Vec<String> = scanned["distributions"][top]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(J::as_str)
            .map(str::to_string)
            .collect();
        let project = |n: &str| n.to_lowercase().replace(['_', '.'], "-");
        let (name_shown, provided_by) = if installed.iter().any(|d| project(d) == project(&p.pypi))
        {
            (
                true,
                serde_json::json!({"source": "importlib.metadata of the interpreter the tool ran", "agrees_with_the_proposal": true, "projects": installed}),
            )
        } else if !installed.is_empty() {
            flags.push(format!("the proposal says `{import}` comes from the PyPI project {:?}, and the interpreter the tool ran has `{top}` installed from {installed:?}. Everything said about an \"official\" crate here is about the project the proposal names", p.pypi));
            (
                false,
                serde_json::json!({"source": "importlib.metadata of the interpreter the tool ran", "agrees_with_the_proposal": false, "projects": installed}),
            )
        } else {
            let alike = name_shows_import(&p.pypi, &import);
            if !alike {
                flags.push(format!("the proposal says `{import}` comes from the PyPI project {:?}. The tool could not check that: `{top}` is not installed in the interpreter it ran, PyPI does not publish which modules a project provides, and neither name contains the other. Everything said about an \"official\" crate here is about that project", p.pypi));
            }
            (
                alike,
                serde_json::json!({"source": if alike { "the two names resemble each other; the library is not installed in the interpreter the tool ran" } else { "not checked" }, "agrees_with_the_proposal": J::Null, "projects": []}),
            )
        };
        let mut pypi_facts = pypi_facts;
        pypi_facts["provides_the_import"] = provided_by;
        if pypi_facts["exists"] == false {
            flags.push(format!(
                "PyPI has no project named {:?}: check the name the proposal gives for this import",
                p.pypi
            ));
        }
        // The two names are always searched; the agent cannot leave them out.
        let mut queries: Vec<(String, &'static str)> = vec![(p.pypi.clone(), "tool")];
        let import_name = import.replace('.', " ");
        if !queries
            .iter()
            .any(|(q, _)| q.eq_ignore_ascii_case(&import_name))
        {
            queries.push((import_name, "tool"));
        }
        let mut descriptive = 0;
        for term in &p.search_terms {
            if !queries.iter().any(|(q, _)| q.eq_ignore_ascii_case(term)) {
                queries.push((term.clone(), "proposal"));
                descriptive += 1;
            }
        }
        let mut searches = Vec::new();
        for (query, from) in &queries {
            searches.push(search(&mut f, query, from)?);
        }
        let mut candidates = Vec::new();
        for name in &p.candidates {
            let up = Upstream {
                orgs: &upstream,
                description: &description,
                declared: p.declared_official_evidence.get(name),
                advisories_work,
                full: true,
            };
            candidates.push(candidate(&mut f, name, &up, opts.min_age_days)?);
        }
        // A dismissed crate gets the same lookups as a candidate, so that
        // the reason can be read beside what the crate is.
        let mut dismissed = Vec::new();
        for (name, reason) in &p.dismissed {
            if p.candidates.iter().any(|c| same_crate(c, name)) {
                reasons.push(format!(
                    "{import}: `{name}` is both a candidate and dismissed"
                ));
            }
            if reason.split_whitespace().count() < MIN_REASON_WORDS {
                reasons.push(format!("{import}: `{name}` is dismissed with the reason {reason:?}. A reason says what the crate is and why that is not what the piece needs, in at least {MIN_REASON_WORDS} words"));
            }
            let up = Upstream {
                orgs: &upstream,
                description: &description,
                declared: None,
                advisories_work,
                full: false,
            };
            let krate = candidate(&mut f, name, &up, opts.min_age_days)?;
            let age = krate.facts["latest_age_days"].as_u64();
            let recent = krate.facts["downloads_weak_signal"]["recent_90_days"]
                .as_u64()
                .unwrap_or(0);
            let alive_because = match (krate.exists, age) {
                (true, Some(days)) if days <= ALIVE_DAYS && recent >= ALIVE_DOWNLOADS => Some(
                    format!("a release {days} days ago and {recent} downloads in the last 90 days"),
                ),
                (true, days) if recent >= ALIVE_HEAVY_USE => Some(format!(
                    "{recent} downloads in the last 90 days, with {}: it exists and is in wide use, though it may be deprecated or unmaintained",
                    days.map_or("no release date known".to_string(), |d| format!(
                        "no release for {d} days"
                    ))
                )),
                _ => None,
            };
            dismissed.push(Dismissed {
                declared_reason: reason.clone(),
                plainly_alive: alive_because.is_some(),
                alive_because,
                krate,
            });
        }
        let seen = |hit: &str| {
            p.candidates
                .iter()
                .chain(p.dismissed.keys())
                .any(|c| same_crate(c, hit))
                || candidates.iter().any(|c| same_crate(&c.name, hit))
                || dismissed.iter().any(|d| same_crate(&d.krate.name, hit))
        };
        let mut not_examined: Vec<String> = Vec::new();
        for s in &searches {
            for hit in s.top.iter().take(TOP).filter_map(|h| h["name"].as_str()) {
                if !seen(hit) && !not_examined.iter().any(|n| n == hit) {
                    not_examined.push(hit.to_string());
                }
            }
        }
        let alive: Vec<&str> = dismissed
            .iter()
            .filter(|d| d.plainly_alive)
            .map(|d| d.krate.name.as_str())
            .collect();
        let dismissals = match dismissed.len() {
            0 => String::new(),
            n => format!(
                " The proposal dismissed {n} crate(s), {} of them plainly alive (a release in the last {ALIVE_DAYS} days and at least {ALIVE_DOWNLOADS} downloads in the last 90, or at least {ALIVE_HEAVY_USE} downloads in the last 90 whatever the last release){}; the reasons are the proposal's and are listed with each crate's facts below.",
                alive.len(),
                if alive.is_empty() {
                    String::new()
                } else {
                    format!(": {}", alive.join(", "))
                }
            ),
        };
        let existing: Vec<&Candidate> = candidates.iter().filter(|c| c.exists).collect();
        let (outcome, outcome_reason) = if existing.iter().any(|c| c.classification == "official") {
            (
                "official_crate",
                format!(
                    "a candidate is published by the organisation behind the PyPI project {:?}, on evidence its publisher could not have typed.{}",
                    p.pypi,
                    if name_shown {
                        ""
                    } else {
                        " That this project is the one the piece imports is the proposal's statement and was not checked."
                    }
                ),
            )
        } else if existing
            .iter()
            .any(|c| c.classification == "named_by_upstream")
        {
            (
                "crate_named_by_upstream",
                format!(
                    "the description of the PyPI project {:?} links to a candidate. The tool did not judge in what sense: as the project's own crate, a port by someone else, or an alternative. No candidate is shown to be published by the organisation behind the project.{}",
                    p.pypi,
                    if name_shown {
                        ""
                    } else {
                        " That this project is the one the piece imports is the proposal's statement and was not checked."
                    }
                ),
            )
        } else if !existing.is_empty() {
            ("community_crates", "candidates exist; none could be tied to the organisation behind the Python library by evidence the tool can check".to_string())
        } else if descriptive < MIN_DESCRIPTIVE_SEARCHES {
            (
                "unresolved",
                format!(
                    "no candidate exists, and \"none found\" is not accepted yet: beyond the two names the tool searches, the proposal gave {descriptive} search term(s) for what the library does; at least {MIN_DESCRIPTIVE_SEARCHES} are needed"
                ),
            )
        } else if !not_examined.is_empty() {
            (
                "unresolved",
                format!(
                    "no candidate exists, and \"none found\" is not accepted yet: {} crate(s) in the top {TOP} of the searches were neither looked up nor dismissed with a reason: {}",
                    not_examined.len(),
                    not_examined.join(", ")
                ),
            )
        } else if !alive.is_empty() {
            (
                "candidates_dismissed_by_the_proposal",
                "no candidate was put forward, and this is not \"none found\": the searches returned crates that are plainly alive and the proposal set them aside. Whether that was right is for a reader to judge from the reasons and the facts below.".to_string(),
            )
        } else {
            (
                "none_found_by_these_queries",
                format!(
                    "none found by these {} queries on crates.io: every crate in the top {TOP} of each was dismissed and none of those is plainly alive. A crate under a name these queries do not reach would not be seen.",
                    searches.len()
                ),
            )
        };
        let outcome_reason = format!("{outcome_reason}{dismissals}");
        if outcome == "unresolved" {
            reasons.push(format!("{import}: {outcome_reason}"));
        }
        for name in p
            .declared_official_evidence
            .keys()
            .filter(|n| !p.candidates.iter().any(|c| same_crate(c, n)))
        {
            reasons.push(format!(
                "{import}: evidence is declared for `{name}`, which is not among the candidates"
            ));
        }
        for c in candidates.iter().filter(|c| !c.exists) {
            flags.push(format!(
                "the proposed candidate `{}` does not exist on crates.io",
                c.name
            ));
        }
        if let Some(d) = &p.decision {
            if let Some(name) = d.choice.strip_prefix("crate:") {
                match candidates.iter().find(|c| same_crate(&c.name, name) || c.proposed_as.as_deref().is_some_and(|p| same_crate(p, name))) {
                    None => reasons.push(format!("{import}: the decision is `{name}`, which was not looked up; add it to the candidates")),
                    Some(c) if !c.exists => reasons.push(format!("{import}: the decision is `{name}`, which does not exist on crates.io")),
                    Some(c) => flags.extend(c.flags.iter().map(|f| format!("the chosen crate `{name}`: {f}"))),
                }
            } else if !["keep_in_python", "keep_part_in_python", "write_it"]
                .contains(&d.choice.as_str())
            {
                reasons.push(format!("{import}: unknown decision {:?}", d.choice));
            }
        }
        let mut options: Vec<String> = existing
            .iter()
            .map(|c| format!("Use the crate `{}` ({}).", c.name, c.classification))
            .collect();
        options.push(KEEP.to_string());
        options.push("Write the part the piece needs in Rust, if what it uses is small. The list of names used, above, is the size of that job.".to_string());
        dependencies.push(Dependency {
            import,
            used,
            pypi: pypi_facts,
            searches,
            candidates,
            not_examined,
            dismissed,
            outcome,
            outcome_reason,
            options,
            declared_decision: p.decision.clone(),
            flags,
        });
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // In replay the report is dated by its newest recorded answer, so the
    // same recordings give the same report.
    let newest = f.log.iter().filter_map(|l| parse_time(&l.fetched_at)).max();
    let generated = if opts.replay {
        newest.unwrap_or(now)
    } else {
        now
    };
    let from_cache = f.log.iter().filter(|l| l.from_cache).count();
    let oldest = f
        .log
        .iter()
        .filter(|l| l.from_cache)
        .filter_map(|l| parse_time(&l.fetched_at))
        .min();
    let source = serde_json::json!({
        "mode": if opts.replay { "replay" } else if from_cache == 0 { "live" } else { "live_with_cache" },
        "answers": f.log.len(),
        "answers_fetched_on_this_run": f.log.len() - from_cache,
        "answers_from_the_cache": from_cache,
        "oldest_cached_answer": oldest.map(iso),
        "note": "An answer from the cache is a file in a directory the caller names. The tool cannot tell whether it was changed after it was fetched.",
    });
    Ok(Report {
        schema: REPORT_SCHEMA,
        verdict: if reasons.is_empty() {
            "complete"
        } else {
            "incomplete"
        },
        exit_status: if reasons.is_empty() { 0 } else { 1 },
        reasons,
        not_judged: NOT_JUDGED,
        generated_at: iso(generated),
        source,
        advisory_lookup,
        unread_imports,
        flags: report_flags,
        min_publish_age_days: opts.min_age_days,
        proposal: serde_json::json!({"file": path.file_name().map(|n| n.to_string_lossy().into_owned()), "sha256": carcinify_diff::provenance::sha256_file(&path)?}),
        python: serde_json::json!({"interpreter": scanned["python"], "files": proposal.python_files}),
        dependencies,
        not_looked_up,
        requests: f.log,
        user_agent: (!opts.replay).then_some(crate::fetch::USER_AGENT),
    })
}

/// The report as Markdown, for a person to read.
pub fn markdown(r: &Report) -> String {
    let cached = r.source["answers_from_the_cache"].as_u64().unwrap_or(0);
    let oldest = r.source["oldest_cached_answer"]
        .as_str()
        .unwrap_or("not known");
    let how = match r.source["mode"].as_str() {
        Some("replay") => format!(
            "**replayed from a cache: no request was made.** All {cached} answers were read from files, the oldest fetched {oldest}. A cache is a directory of files that can be edited; this is not a live report"
        ),
        Some("live") => format!(
            "from live lookups: all {} answers were fetched on this run",
            r.requests.len()
        ),
        _ => format!(
            "from lookups, {cached} of {} answers taken from the cache (the oldest fetched {oldest}) and the rest fetched on this run",
            r.requests.len()
        ),
    };
    let mut o = format!(
        "# Dependency check\n\nGenerated {} by carcinify deps, {how}. Every fact below comes from those answers, each with the time it was fetched; the proposal supplied only names to look up, search terms, dismissals and decisions.\n\nVerdict: **{}**",
        r.generated_at, r.verdict
    );
    for reason in &r.reasons {
        o.push_str(&format!("\n- {reason}"));
    }
    o.push_str(&format!(
        "\n\nMinimum publish age applied: {} days.\n",
        r.min_publish_age_days
    ));
    let text = |v: &J| match v {
        J::Null => "not known".to_string(),
        J::String(s) => s.clone(),
        other => other.to_string(),
    };
    for d in &r.dependencies {
        o.push_str(&format!("\n## `{}`\n\n", d.import));
        for u in &d.used {
            let names: Vec<&str> = u["uses"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(J::as_str)
                .collect();
            o.push_str(&format!(
                "- The piece imports `{}` ({}, line {}) and uses: {}\n",
                text(&u["module"]),
                text(&u["file"]),
                text(&u["lines"][0]),
                if names.is_empty() {
                    "the module itself".to_string()
                } else {
                    names.join(", ")
                }
            ));
        }
        o.push_str(&format!(
            "- PyPI project, as the proposal names it: `{}`, version {}, hosted by {}\n",
            text(&d.pypi["name"]),
            text(&d.pypi["version"]),
            text(&d.pypi["organisations"])
        ));
        o.push_str(&format!(
            "\n**Outcome: `{}`.** {}\n",
            d.outcome, d.outcome_reason
        ));
        if !d.candidates.is_empty() {
            o.push_str("\n| Crate | Class | Latest | Released | First release | Releases, last year | Downloads, 90 days (weak signal) | Licence | Owners | Build script | Proc-macro | Advisories |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n");
            for c in &d.candidates {
                if !c.exists {
                    o.push_str(&format!(
                        "| `{}` | **does not exist** | | | | | | | | | | |\n",
                        c.name
                    ));
                    continue;
                }
                let x = &c.facts;
                let owners: Vec<String> = x["owners"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|w| text(&w["login"]))
                    .collect();
                let day = |v: &J| text(v).chars().take(10).collect::<String>();
                let shown = match &c.proposed_as {
                    Some(p) => format!("`{}` (proposed as `{p}`)", c.name),
                    None => format!("`{}`", c.name),
                };
                o.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                    shown,
                    c.classification,
                    text(&x["latest_version"]),
                    day(&x["latest_published"]),
                    day(&x["first_published"]),
                    x["versions_in_the_last_365_days"],
                    x["downloads_weak_signal"]["recent_90_days"],
                    text(&x["license"]),
                    owners.join(", "),
                    text(&x["build_script"]),
                    text(&x["proc_macro"]),
                    x["rustsec_advisories"]
                        .as_array()
                        .map_or("not known".to_string(), |a| a.len().to_string())
                ));
            }
            for c in &d.candidates {
                for e in &c.evidence {
                    o.push_str(&format!("\n- `{}`: {e}", c.name));
                }
                for flag in &c.flags {
                    o.push_str(&format!("\n- `{}`, flag: {flag}", c.name));
                }
            }
            o.push('\n');
        }
        o.push_str("\nSearches run:\n\n");
        for s in &d.searches {
            let names: Vec<String> = s
                .top
                .iter()
                .take(TOP)
                .map(|h| format!("`{}`", text(&h["name"])))
                .collect();
            o.push_str(&format!(
                "- `{}` ({}; {} results; fetched {}): {}\n",
                s.query,
                s.from,
                s.total,
                s.fetched_at,
                if names.is_empty() {
                    "nothing".to_string()
                } else {
                    names.join(", ")
                }
            ));
        }
        if !d.dismissed.is_empty() {
            o.push_str("\nDismissed by the proposal. The reason is the proposal's; the rest was fetched:\n\n| Crate | Plainly alive | Latest | Released | Releases, last year | Downloads, 90 days (weak signal) | Advisories | Reason given |\n|---|---|---|---|---|---|---|---|\n");
            for x in &d.dismissed {
                let c = &x.krate;
                if !c.exists {
                    o.push_str(&format!(
                        "| `{}` | **does not exist** | | | | | | {} |\n",
                        c.name,
                        x.declared_reason.replace('|', "/")
                    ));
                    continue;
                }
                let facts = &c.facts;
                o.push_str(&format!(
                    "| `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
                    c.name,
                    match &x.alive_because {
                        Some(why) => format!("**yes**: {why}"),
                        None => "no".to_string(),
                    },
                    text(&facts["latest_version"]),
                    text(&facts["latest_published"])
                        .chars()
                        .take(10)
                        .collect::<String>(),
                    facts["versions_in_the_last_365_days"],
                    facts["downloads_weak_signal"]["recent_90_days"],
                    facts["rustsec_advisories"]
                        .as_array()
                        .map_or("not known".to_string(), |a| a.len().to_string()),
                    x.declared_reason.replace('|', "/")
                ));
            }
        }
        if !d.not_examined.is_empty() {
            o.push_str(&format!(
                "\nIn the top {TOP} of a search and not examined: {}\n",
                d.not_examined
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        o.push_str("\nOptions:\n\n");
        for option in &d.options {
            o.push_str(&format!("- {option}\n"));
        }
        if let Some(dec) = &d.declared_decision {
            o.push_str(&format!(
                "\nDecision declared in the proposal: `{}`. {}\n",
                dec.choice, dec.reason
            ));
        }
        for flag in &d.flags {
            o.push_str(&format!("\nFlag: {flag}\n"));
        }
    }
    if !r.unread_imports.is_empty() {
        o.push_str("\n## Imports the tool could not read\n\n");
        for u in &r.unread_imports {
            o.push_str(&format!(
                "- {}, line {}: `{}`. {}\n",
                text(&u["file"]),
                u["line"],
                text(&u["call"]),
                match u["acknowledged_by_the_proposal"].as_str() {
                    Some(r) => format!("The proposal's account, not checked: {r}"),
                    None => "Not acknowledged in the proposal.".to_string(),
                }
            ));
        }
    }
    o.push_str("\n## What this report cannot judge\n\n");
    for line in r.not_judged {
        o.push_str(&format!("- {line}\n"));
    }
    match r.user_agent {
        Some(agent) => o.push_str(&format!("\n## Requests made\n\n{} requests, User-Agent `{agent}`. The URLs are in the JSON report; run them again to check.\n", r.requests.len())),
        None => o.push_str(&format!("\n## Requests recorded\n\n{} recorded requests, read from the cache. None was made on this run. The URLs are in the JSON report; run them to check.\n", r.requests.len())),
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_edit_apart_with_dash_and_underscore_the_same() {
        assert_eq!(edit_distance("reqwest", "reqwests"), 1);
        assert_eq!(edit_distance("serde_yaml", "serde-yaml"), 0);
        assert_eq!(edit_distance("serde_yaml", "serde_yml"), 1);
        assert_eq!(edit_distance("arrayref", "arrayrefs"), 1);
        assert_eq!(edit_distance("proc-macro1", "proc-macro2"), 1);
        assert_eq!(edit_distance("tokio", "serde"), 5);
    }

    #[test]
    fn organisations_come_from_repository_hosts_only() {
        let urls = [
            "https://github.com/Open-Telemetry/opentelemetry-python/tree/main".to_string(),
            "https://opentelemetry.io".to_string(),
            "https://gitlab.com/group/project".to_string(),
        ];
        assert_eq!(
            orgs(&urls),
            [
                ("github", "open-telemetry".to_string()),
                ("gitlab", "group".to_string())
            ]
        );
        assert!(orgs(&["https://example.com/github.com/x".to_string()]).is_empty());
        // A funding link is not an owner.
        let funding = [
            "https://github.com/sponsors/samuelcolvin".to_string(),
            "https://github.com/samuelcolvin/rtoml".to_string(),
        ];
        assert_eq!(orgs(&funding), [("github", "samuelcolvin".to_string())]);
    }

    #[test]
    fn a_link_to_a_crate_ends_where_the_name_ends() {
        let text = "see https://docs.rs/serde_json and crates.io/crates/polars.";
        assert!(links_to(text, "docs.rs/serde_json") && links_to(text, "crates.io/crates/polars"));
        assert!(!links_to(text, "docs.rs/serde") && !links_to(text, "crates.io/crates/pola"));
    }

    #[test]
    fn an_advisory_page_is_read_only_when_it_is_one() {
        assert_eq!(advisories(404, ""), Some(vec![]));
        assert_eq!(
            advisories(200, "<a>RUSTSEC-2018-0005</a> RUSTSEC-2018-0005"),
            Some(vec!["RUSTSEC-2018-0005".to_string()])
        );
        assert_eq!(advisories(503, "RUSTSEC-2018-0005"), None);
        assert_eq!(advisories(301, ""), None);
    }

    #[test]
    fn a_project_name_is_compared_with_the_import() {
        assert!(
            name_shows_import("PyYAML", "yaml") && name_shows_import("google-adk", "google.adk")
        );
        assert!(
            name_shows_import("opentelemetry-api", "opentelemetry")
                && name_shows_import("zeep", "zeep")
        );
        assert!(
            !name_shows_import("opentelemetry-api", "yaml")
                && !name_shows_import("scikit-learn", "sklearn")
        );
        assert!(!name_shows_import("google-genai", "google.adk"));
    }
}
