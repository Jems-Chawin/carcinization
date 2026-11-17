//! End-to-end tests of the real binary on recorded answers.
//!
//! `tests/fixtures/cache` holds answers recorded from crates.io, PyPI,
//! rustsec.org and docs.rs on 2026-11-14 (two sessions), trimmed to the fields the tool
//! reads. With `--replay` the tool makes no request, so these tests need no
//! network. The tests that do go to the network are at the bottom, ignored
//! by default.

use serde_json::Value as J;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../deps/tests/fixtures")
}

struct Work(PathBuf);

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A directory with the fixture's Python files, a copy of the recorded
/// answers, and a proposal with the given dependencies (JSON).
fn work(dependencies: &str) -> Work {
    work_on(r#"["piece.py"]"#, dependencies)
}

/// The same, for the given list of Python files (JSON).
fn work_on(python_files: &str, dependencies: &str) -> Work {
    let dir = std::env::temp_dir().join(format!(
        "carcinify-deps-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("cache")).unwrap();
    for file in ["piece.py", "build_piece.py"] {
        std::fs::copy(fixtures().join(file), dir.join(file)).unwrap();
    }
    for entry in std::fs::read_dir(fixtures().join("cache"))
        .unwrap()
        .flatten()
    {
        std::fs::copy(entry.path(), dir.join("cache").join(entry.file_name())).unwrap();
    }
    let proposal = format!(
        r#"{{"schema": "carcinify/deps/proposal/1", "python_files": {python_files}, "dependencies": {dependencies}}}"#
    );
    std::fs::write(dir.join("proposal.json"), proposal).unwrap();
    Work(dir)
}

/// Changes the recorded answer for the URL ending in `url_end`, as someone
/// with access to the cache directory could.
fn edit_answer(w: &Work, url_end: &str, change: impl Fn(&mut J)) {
    let mut found = 0;
    for entry in std::fs::read_dir(w.0.join("cache")).unwrap().flatten() {
        let mut answer: J =
            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap()).unwrap();
        if answer["url"].as_str().unwrap().ends_with(url_end) {
            change(&mut answer);
            std::fs::write(entry.path(), answer.to_string()).unwrap();
            found += 1;
        }
    }
    assert_eq!(found, 1, "{url_end}");
}

/// Changes the body (JSON) of a recorded answer.
fn edit_body(w: &Work, url_end: &str, change: impl Fn(&mut J)) {
    edit_answer(w, url_end, |answer| {
        let mut body: J = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
        change(&mut body);
        answer["body"] = J::from(body.to_string());
    });
}

fn run(w: &Work) -> (i32, J, String) {
    run_with(w, &[])
}

fn run_with(w: &Work, extra: &[&str]) -> (i32, J, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("deps")
        .args(extra)
        .arg(w.0.join("proposal.json"))
        .arg("--report")
        .arg(w.0.join("report.json"))
        .arg("--markdown")
        .arg(w.0.join("report.md"))
        .arg("--cache")
        .arg(w.0.join("cache"))
        .arg("--replay")
        .output()
        .unwrap();
    let json = std::fs::read_to_string(w.0.join("report.json"))
        .map_or(J::Null, |t| serde_json::from_str(&t).unwrap());
    (
        out.status.code().unwrap(),
        json,
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

const OTEL: &str = r#""opentelemetry": {"pypi": "opentelemetry-api", "search_terms": ["tracing spans"], "candidates": ["opentelemetry"]}"#;
const YAML: &str = r#""yaml": {"pypi": "PyYAML", "search_terms": ["yaml parser"], "candidates": ["serde_yaml", "serde-saphyr"]}"#;
const GENAI: &str = r#""google.genai": {"pypi": "google-genai", "search_terms": ["gemini"], "candidates": ["google-genai", "google-adk"]}"#;
const ZEEP: &str = r#""zeep": {"pypi": "zeep", "search_terms": ["soap client", "wsdl"], "candidates": ["soap-client"]}"#;

fn with_zeep(zeep: &str) -> String {
    format!("{{{OTEL}, {YAML}, {GENAI}, {zeep}}}")
}

fn dependency<'a>(report: &'a J, import: &str) -> &'a J {
    report["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["import"] == import)
        .unwrap_or_else(|| panic!("no {import} in the report"))
}

fn candidate<'a>(dep: &'a J, name: &str) -> &'a J {
    dep["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no candidate {name}"))
}

fn flags(c: &J) -> String {
    c["flags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn the_facts_in_the_report_are_the_ones_fetched() {
    let w = work(&with_zeep(ZEEP));
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    assert_eq!(report["verdict"], "complete");

    // Scope: what the piece imports, with what it uses from each.
    let otel = dependency(&report, "opentelemetry");
    assert_eq!(otel["used"][0]["module"], "opentelemetry");
    assert_eq!(
        otel["used"][0]["uses"],
        serde_json::json!(["trace.get_tracer"])
    );
    let skipped: Vec<(&str, &str)> = report["not_looked_up"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["module"].as_str().unwrap(), n["why"].as_str().unwrap()))
        .collect();
    assert_eq!(
        skipped,
        [
            ("json", "standard_library"),
            ("os.path", "standard_library")
        ]
    );
    // `from google import genai` is an import of google.genai.
    assert_eq!(
        dependency(&report, "google.genai")["used"][0]["uses"],
        serde_json::json!(["genai.Client"])
    );

    // Official, on evidence the crate's publisher could not have typed.
    let crate_ = candidate(otel, "opentelemetry");
    assert_eq!(otel["outcome"], "official_crate");
    assert_eq!(crate_["classification"], "official");
    assert!(crate_["evidence"][0].as_str().unwrap().contains("crates.io owner `github:open-telemetry:rust-publishers` is a GitHub team of `open-telemetry`"));
    assert_eq!(crate_["facts"]["latest_version"], "0.33.1");
    assert_eq!(crate_["facts"]["latest_published"], "2026-10-09T20:45:06Z");
    assert_eq!(crate_["facts"]["fetched_at"].as_str().unwrap().len(), 20);

    // Community, with what a reader needs to judge them.
    let yaml = dependency(&report, "yaml");
    assert_eq!(yaml["outcome"], "community_crates");
    let old = candidate(yaml, "serde_yaml");
    assert_eq!(old["facts"]["latest_version"], "0.9.34+deprecated");
    assert!(
        flags(old).contains("no release for 964 days") && flags(old).contains("RUSTSEC-2018-0005"),
        "{}",
        flags(old)
    );
    let young = candidate(yaml, "serde-saphyr");
    assert!(
        flags(young).contains("9 day(s) old, younger than the minimum publish age of 14"),
        "{}",
        flags(young)
    );
    assert!(flags(young).contains("version(s) were yanked"));
    assert!(
        young["facts"]["downloads_weak_signal"]["note"]
            .as_str()
            .unwrap()
            .contains("dominated by being a dependency")
    );
    assert!(
        flags(candidate(dependency(&report, "zeep"), "soap-client"))
            .contains("the crate itself is 23 day(s) old")
    );

    // Every dependency has the option of not porting it.
    for d in report["dependencies"].as_array().unwrap() {
        assert!(
            d["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o.as_str().unwrap().starts_with("Keep it in Python"))
        );
    }
    // The requests are listed, came from the recording, and name no person.
    let requests = report["requests"].as_array().unwrap();
    assert!(requests.len() > 30 && requests.iter().all(|r| r["from_cache"] == true));
    // A replay makes no request, so it names no User-Agent.
    assert!(report["user_agent"].is_null());
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    assert!(
        md.contains("Downloads, 90 days (weak signal)")
            && md.contains("| `opentelemetry` | official | 0.33.1 | 2026-10-09 |"),
        "{md}"
    );
    assert!(md.contains("## What this report cannot judge"));
    assert!(
        md.contains("recorded requests, read from the cache. None was made on this run.")
            && !md.contains("User-Agent"),
        "{md}"
    );
}

#[test]
fn a_candidate_that_does_not_exist_is_reported_as_such() {
    let (code, report, text) = run(&work(&with_zeep(ZEEP)));
    assert_eq!(code, 0, "{text}");
    let genai = dependency(&report, "google.genai");
    let ghost = candidate(genai, "google-adk");
    assert_eq!(
        (&ghost["exists"], &ghost["classification"]),
        (&J::from(false), &J::from("not_found"))
    );
    assert!(
        ghost["evidence"][0]
            .as_str()
            .unwrap()
            .contains("crates.io answered 404")
    );
    assert!(
        genai["flags"][0]
            .as_str()
            .unwrap()
            .contains("`google-adk` does not exist on crates.io")
    );
    assert!(
        text.contains("google-adk DOES NOT EXIST on crates.io"),
        "{text}"
    );

    // Choosing it is not a flag but a failure.
    let chosen = GENAI.replace(r#""candidates""#, r#""decision": {"choice": "crate:google-adk", "reason": "it is the obvious name"}, "candidates""#);
    let (code, report, text) = run(&work(&format!("{{{OTEL}, {YAML}, {chosen}, {ZEEP}}}")));
    assert_eq!(code, 1, "{text}");
    assert!(
        report["reasons"][0]
            .as_str()
            .unwrap()
            .contains("the decision is `google-adk`, which does not exist")
    );
}

#[test]
fn none_is_not_accepted_without_recorded_searches() {
    // No candidates and no search terms: the claim "there is no crate" as an
    // agent makes it from memory.
    let (code, report, text) = run(&work(&with_zeep(r#""zeep": {"pypi": "zeep"}"#)));
    assert_eq!(code, 1, "{text}");
    let zeep = dependency(&report, "zeep");
    assert_eq!(zeep["outcome"], "unresolved");
    assert!(
        zeep["outcome_reason"]
            .as_str()
            .unwrap()
            .contains("the proposal gave 0 search term(s)"),
        "{text}"
    );
    // The tool searched the name anyway, and the answer is in the report.
    assert_eq!(zeep["searches"][0]["query"], "zeep");
    assert_eq!(zeep["searches"][0]["from"], "tool");

    // Search terms given, results not looked at: still not "none".
    let (code, report, text) = run(&work(&with_zeep(
        r#""zeep": {"pypi": "zeep", "search_terms": ["soap client", "wsdl"]}"#,
    )));
    assert_eq!(code, 1, "{text}");
    let zeep = dependency(&report, "zeep");
    assert_eq!(zeep["outcome"], "unresolved");
    let left: Vec<&str> = zeep["not_examined"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    assert!(
        left.contains(&"soap-client") && left.contains(&"rsoap") && left.len() >= 10,
        "{left:?}"
    );
    assert!(
        zeep["outcome_reason"]
            .as_str()
            .unwrap()
            .contains("neither looked up nor dismissed")
    );

    // Every one of them dismissed with a reason: now it is accepted, and
    // worded as what it is.
    let dismissed: Vec<String> = left
        .iter()
        .map(|n| format!(r#""{n}": "looked at for this test""#))
        .collect();
    let all = format!(
        r#""zeep": {{"pypi": "zeep", "search_terms": ["soap client", "wsdl"], "dismissed": {{{}}}}}"#,
        dismissed.join(", ")
    );
    let (code, report, text) = run(&work(&with_zeep(&all)));
    assert_eq!(code, 0, "{text}");
    let zeep = dependency(&report, "zeep");
    assert_eq!(zeep["outcome"], "none_found_by_these_queries");
    assert!(
        zeep["outcome_reason"]
            .as_str()
            .unwrap()
            .starts_with("none found by these 3 queries on crates.io"),
        "{}",
        zeep["outcome_reason"]
    );
    assert_eq!(zeep["searches"].as_array().unwrap().len(), 3);
    assert_eq!(zeep["dismissed"].as_array().unwrap().len(), left.len());
    // The number dismissed is in the outcome line, with how many are alive.
    assert!(
        zeep["outcome_reason"].as_str().unwrap().contains(&format!(
            "The proposal dismissed {} crate(s), 0 of them plainly alive",
            left.len()
        )),
        "{}",
        zeep["outcome_reason"]
    );
}

#[test]
fn the_check_covers_the_piece_and_nothing_else() {
    // An entry for a library the piece does not import.
    let extra = format!(r#"{{{OTEL}, {YAML}, {GENAI}, {ZEEP}, "numpy": {{"pypi": "numpy"}}}}"#);
    let (code, report, text) = run(&work(&extra));
    assert_eq!(code, 1, "{text}");
    assert!(
        report["reasons"][0]
            .as_str()
            .unwrap()
            .contains("an entry for numpy, which the piece does not import")
    );

    // An import the proposal says nothing about.
    let (code, report, text) = run(&work(&format!("{{{OTEL}, {YAML}, {GENAI}}}")));
    assert_eq!(code, 1, "{text}");
    assert!(
        report["reasons"][0]
            .as_str()
            .unwrap()
            .contains("zeep is imported at piece.py:6 and the proposal has no entry for it"),
        "{text}"
    );
}

#[test]
fn a_name_one_character_from_a_popular_crate_is_flagged() {
    // SYNTHETIC answers, written here, not recorded: a crate `opentelemetri`
    // published five days before the recording, next to the real one.
    let w = work(&with_zeep(ZEEP).replace(
        r#""candidates": ["opentelemetry"]"#,
        r#""candidates": ["opentelemetri"]"#,
    ));
    let fetched: u64 = 1_794_600_000;
    let put = |url: &str, status: u16, body: J| {
        let name = format!(
            "{}.json",
            &carcinify_diff::provenance::sha256_bytes(url.as_bytes())[..20]
        );
        let entry = serde_json::json!({"url": url, "status": status, "fetched_at": fetched, "body": if body.is_string() { body.as_str().unwrap().to_string() } else { body.to_string() }});
        std::fs::write(w.0.join("cache").join(name), entry.to_string()).unwrap();
    };
    let base = "https://crates.io/api/v1/crates/opentelemetri";
    put(
        base,
        200,
        serde_json::json!({"crate": {"name": "opentelemetri", "downloads": 40, "recent_downloads": 40, "repository": "https://github.com/open-telemetry/opentelemetry-rust", "created_at": "2026-11-08T00:00:00Z"},
        "versions": [{"num": "0.33.2", "created_at": "2026-11-08T00:00:00Z", "yanked": false, "license": "Apache-2.0"}]}),
    );
    put(
        &format!("{base}/owners"),
        200,
        serde_json::json!({"users": [{"login": "someone-new", "kind": "user"}]}),
    );
    put(
        "https://rustsec.org/packages/opentelemetri.html",
        404,
        J::from(""),
    );
    put(
        "https://docs.rs/crate/opentelemetri/0.33.2/source/",
        404,
        J::from(""),
    );
    put(
        "https://docs.rs/crate/opentelemetri/0.33.2/source/Cargo.toml",
        404,
        J::from(""),
    );
    put(
        "https://crates.io/api/v1/crates?q=opentelemetri&per_page=10",
        200,
        serde_json::json!({"meta": {"total": 2}, "crates": [
        {"name": "opentelemetry", "downloads": 500000000u64, "recent_downloads": 90000000}, {"name": "opentelemetri", "downloads": 40, "recent_downloads": 40}]}),
    );
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let c = candidate(dependency(&report, "opentelemetry"), "opentelemetri");
    assert!(
        flags(c).contains("one character away from a far more downloaded crate: `opentelemetry`"),
        "{}",
        flags(c)
    );
    assert!(flags(c).contains("the crate itself is"), "{}", flags(c));
    // Its repository link points at the real project. That is not evidence.
    assert_eq!(c["classification"], "community");
    assert!(c["evidence"].as_array().unwrap().iter().any(|e| {
        e.as_str()
            .unwrap()
            .contains("typed by the publisher and is not evidence")
    }));
}

#[test]
fn replay_makes_no_request_and_says_what_is_missing() {
    let w = work(&with_zeep(ZEEP).replace(
        r#""candidates": ["soap-client"]"#,
        r#""candidates": ["tokio"]"#,
    ));
    let (code, report, text) = run(&w);
    assert_eq!(code, 2, "{text}");
    assert_eq!(report["verdict"], "error");
    assert!(
        text.contains("no recorded response for https://crates.io/api/v1/crates/tokio"),
        "{text}"
    );
}

fn live(proposal: &str) -> (i32, J, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let report = tmp.join(format!("{}.json", proposal.replace('/', "-")));
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("deps")
        .arg(
            root.join("skill/steps/02-dependency-check")
                .join(proposal)
                .join("proposal.json"),
        )
        .arg("--report")
        .arg(&report)
        .arg("--cache")
        .arg(tmp.join(format!("live-cache-{}", proposal.replace('/', "-"))))
        // A live test that reads yesterday's answers is not one.
        .arg("--refresh")
        .output()
        .unwrap();
    let json =
        std::fs::read_to_string(&report).map_or(J::Null, |t| serde_json::from_str(&t).unwrap());
    (
        out.status.code().unwrap(),
        json,
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn reasons(report: &J) -> String {
    report["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" | ")
}

const MATURIN: &str = r#"{"maturin": {"pypi": "maturin", "search_terms": ["build python wheels", "pyo3 packaging"], "candidates": ["maturin"]}}"#;

#[test]
fn a_crate_spelled_with_a_hyphen_is_looked_up_under_its_own_name() {
    // RUST-046: crates.io answers for `serde-yaml`; RustSec knows only
    // `serde_yaml`.
    let hyphens = YAML.replace(r#"["serde_yaml", "serde-saphyr"]"#, r#"["serde-yaml", "serde-yml"], "decision": {"choice": "crate:serde-yml", "reason": "for the test"}"#);
    let w = work(&format!("{{{OTEL}, {hyphens}, {GENAI}, {ZEEP}}}"));
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let yaml = dependency(&report, "yaml");
    let old = candidate(yaml, "serde_yaml");
    assert_eq!(old["proposed_as"], "serde-yaml");
    assert_eq!(
        old["facts"]["rustsec_advisories"],
        serde_json::json!(["RUSTSEC-2018-0005"])
    );
    assert_eq!(
        old["facts"]["rustsec_url"],
        "https://rustsec.org/packages/serde_yaml.html"
    );
    assert!(flags(old).contains("RustSec has advisories"), "{text}");
    let yml = candidate(yaml, "serde_yml");
    assert_eq!(
        yml["facts"]["rustsec_advisories"],
        serde_json::json!(["RUSTSEC-2025-0068"])
    );
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    assert!(
        md.contains("| `serde_yaml` (proposed as `serde-yaml`) | community |"),
        "{md}"
    );
    // The decision, typed with a hyphen, is the crate that was looked up.
    assert!(
        flags(yaml).contains("the chosen crate `serde-yml`: RustSec has advisories"),
        "{text}"
    );
}

#[test]
fn an_advisory_answer_that_is_neither_yes_nor_no_is_not_known() {
    let w = work(&with_zeep(ZEEP));
    edit_answer(&w, "rustsec.org/packages/soap-client.html", |a| {
        a["status"] = J::from(403)
    });
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let soap = candidate(dependency(&report, "zeep"), "soap-client");
    assert!(soap["facts"]["rustsec_advisories"].is_null());
    assert!(
        flags(soap).contains("whether RustSec has advisories for this crate is not known"),
        "{text}"
    );
    assert_eq!(report["advisory_lookup"]["works"], true);

    // The page of a crate known to have advisories is not found: the site
    // has changed, and "no page" can no longer be read as "no advisory".
    let w = work(&with_zeep(ZEEP));
    edit_answer(&w, "rustsec.org/packages/serde_yaml.html", |a| {
        a["status"] = J::from(404)
    });
    let (code, report, text) = run(&w);
    assert_eq!(code, 1, "{text}");
    assert!(
        reasons(&report)
            .contains("the advisory lookup did not find the advisories of `serde_yaml`"),
        "{text}"
    );
    assert_eq!(report["advisory_lookup"]["works"], false);
    for dep in report["dependencies"].as_array().unwrap() {
        for c in dep["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["exists"] == true)
        {
            assert!(c["facts"]["rustsec_advisories"].is_null(), "{}", c["name"]);
        }
    }
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    assert!(md.contains("| not known |"), "{md}");
}

#[test]
fn dismissing_every_result_does_not_make_none() {
    // RUST-047 as the auditor did it: PyYAML, no candidates, two search
    // terms, and every crate the searches return dismissed with "no".
    let bare = r#""yaml": {"pypi": "PyYAML", "search_terms": ["yaml parser", "serde yaml"]}"#;
    let (code, report, text) = run(&work(&format!("{{{OTEL}, {bare}, {GENAI}, {ZEEP}}}")));
    assert_eq!(code, 1, "{text}");
    let left: Vec<String> = dependency(&report, "yaml")["not_examined"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    assert!(
        left.len() >= 15 && left.iter().any(|n| n == "serde-saphyr"),
        "{left:?}"
    );

    let dismiss = |reason: &str| {
        let all: Vec<String> = left
            .iter()
            .map(|n| format!(r#""{n}": "{reason}""#))
            .collect();
        let yaml = format!(
            r#""yaml": {{"pypi": "PyYAML", "search_terms": ["yaml parser", "serde yaml"], "dismissed": {{{}}}}}"#,
            all.join(", ")
        );
        work(&format!("{{{OTEL}, {yaml}, {GENAI}, {ZEEP}}}"))
    };
    let (code, report, text) = run(&dismiss("no"));
    assert_eq!(code, 1, "{text}");
    assert!(
        reasons(&report).contains("is dismissed with the reason \"no\""),
        "{text}"
    );
    assert_ne!(
        dependency(&report, "yaml")["outcome"],
        "none_found_by_these_queries"
    );

    // With reasons of some length the run completes, and the outcome is
    // still not "none": the crates set aside are alive, and it says so.
    let w = dismiss("not what the piece needs");
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let yaml = dependency(&report, "yaml");
    assert_eq!(yaml["outcome"], "candidates_dismissed_by_the_proposal");
    let why = yaml["outcome_reason"].as_str().unwrap();
    assert!(why.contains("this is not \"none found\""), "{why}");
    assert!(
        why.contains(&format!(
            "The proposal dismissed {} crate(s), 12 of them plainly alive",
            left.len()
        )) && why.contains("serde-saphyr"),
        "{why}"
    );
    // Each dismissed crate carries the facts the tool fetched for it.
    let saphyr = yaml["dismissed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["crate"]["name"] == "serde-saphyr")
        .unwrap();
    assert_eq!(saphyr["plainly_alive"], true);
    assert_eq!(saphyr["declared_reason"], "not what the piece needs");
    assert_eq!(saphyr["crate"]["facts"]["latest_version"], "2.1.1");
    // Seven of the nineteen are not alive by the stated measure.
    let quiet = yaml["dismissed"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["plainly_alive"] == false)
        .count();
    assert_eq!(quiet, left.len() - 12);
    assert!(
        text.contains(
            "dismissed by the proposal: serde-saphyr [PLAINLY ALIVE] (not what the piece needs)"
        ),
        "{text}"
    );
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    assert!(
        md.contains("| `serde-saphyr` | **yes**: a release 9 days ago and "),
        "{md}"
    );
}

#[test]
fn trusted_publishing_counts_only_from_the_organisation_itself() {
    // A real record: maturin is released from PyO3/maturin, and the Python
    // project `maturin` lives in github.com/pyo3.
    let published = "released by trusted publishing from `PyO3/maturin`, a repository of `pyo3`";
    let evidence = |report: &J, import: &str, name: &str| {
        candidate(dependency(report, import), name)["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let w = work_on(r#"["build_piece.py"]"#, MATURIN);
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        candidate(dependency(&report, "maturin"), "maturin")["facts"]["trusted_publishing"]["repository"],
        "PyO3/maturin"
    );
    assert!(
        evidence(&report, "maturin", "maturin").contains(published),
        "{text}"
    );

    // The same record naming an organisation whose name only ends the same.
    let w = work_on(r#"["build_piece.py"]"#, MATURIN);
    edit_body(&w, "/api/v1/crates/maturin", |b| {
        b["versions"][0]["trustpub_data"]["repository"] = J::from("fake-PyO3/maturin")
    });
    let (_, report, text) = run(&w);
    assert!(
        !evidence(&report, "maturin", "maturin").contains("trusted publishing"),
        "{text}"
    );

    // RUST-048 as the auditor did it, on a crate with no other evidence.
    // SYNTHETIC: soap-client has no such record; these are written by hand.
    let with_record = |record: J| {
        let w = work(&with_zeep(ZEEP));
        edit_body(&w, "/api/v1/crates/soap-client", |b| {
            b["versions"][0]["trustpub_data"] = record.clone()
        });
        let (code, report, text) = run(&w);
        assert_eq!(code, 0, "{text}");
        (
            candidate(dependency(&report, "zeep"), "soap-client")["classification"].clone(),
            evidence(&report, "zeep", "soap-client"),
        )
    };
    let fake = serde_json::json!({"provider": "github", "repository": "fake-mvantellingen/zeep-core", "run_id": "1", "sha": "0"});
    assert_eq!(with_record(fake).0, "community");
    let ending = serde_json::json!({"provider": "github", "repository": "mvantellingen-rs/zeep", "run_id": "1", "sha": "0"});
    assert_eq!(with_record(ending).0, "community");
    let real = serde_json::json!({"provider": "github", "repository": "mvantellingen/zeep-rs", "run_id": "1", "sha": "0"});
    assert_eq!(with_record(real).0, "official");
    // Another host, or a shape the tool has not seen: not used, and said.
    let elsewhere =
        serde_json::json!({"provider": "gitlab", "repository": "mvantellingen/zeep-rs"});
    let (class, said) = with_record(elsewhere);
    assert_eq!(class, "community");
    assert!(
        said.contains("in a shape this tool does not know"),
        "{said}"
    );
    let (class, said) = with_record(J::from("mvantellingen/zeep-rs"));
    assert_eq!(class, "community");
    assert!(
        said.contains("in a shape this tool does not know"),
        "{said}"
    );
}

#[test]
fn a_replayed_report_is_never_called_live() {
    // RUST-049: every answer from the cache, one of them edited.
    let w = work(&with_zeep(ZEEP));
    edit_body(&w, "/api/v1/crates/soap-client", |b| {
        b["crate"]["recent_downloads"] = J::from(9_000_000)
    });
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let n = report["requests"].as_array().unwrap().len();
    assert_eq!(report["source"]["mode"], "replay");
    assert_eq!(report["source"]["answers_from_the_cache"], n);
    assert_eq!(report["source"]["answers_fetched_on_this_run"], 0);
    assert_eq!(
        report["source"]["oldest_cached_answer"],
        "2026-11-14T20:11:09Z"
    );
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    let first = md.lines().nth(2).unwrap();
    assert!(
        first.contains("replayed from a cache: no request was made")
            && first.contains("this is not a live report"),
        "{first}"
    );
    assert!(!md.contains("live lookups"), "{md}");
    assert!(
        text.contains("REPLAYED from the cache, no request made"),
        "{text}"
    );
    // Each URL is listed once, though some are needed more than once.
    let mut urls: Vec<&str> = report["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["url"].as_str().unwrap())
        .collect();
    urls.sort_unstable();
    urls.dedup();
    assert_eq!(urls.len(), n);
}

#[test]
fn official_is_about_the_project_the_proposal_names() {
    // RUST-050: the piece imports `yaml`; the proposal says that is the
    // PyPI project `opentelemetry-api`.
    let wrong = r#""yaml": {"pypi": "opentelemetry-api", "candidates": ["opentelemetry"]}"#;
    let w = work(&format!("{{{OTEL}, {wrong}, {GENAI}, {ZEEP}}}"));
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let yaml = dependency(&report, "yaml");
    assert_eq!(yaml["pypi"]["name_declared_by"], "the proposal");
    assert!(
        flags(yaml).contains("The tool could not check that")
            && flags(yaml).contains("neither name contains the other"),
        "{text}"
    );
    assert!(yaml["outcome_reason"].as_str().unwrap().contains("That this project is the one the piece imports is the proposal's statement and was not checked"), "{text}");
    // The real pair raises no such flag.
    assert!(
        !flags(dependency(&report, "opentelemetry")).contains("could not check"),
        "{text}"
    );
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    assert!(
        md.contains("PyPI project, as the proposal names it: `opentelemetry-api`"),
        "{md}"
    );
}

#[test]
fn imports_made_at_run_time_are_read_or_reported() {
    // RUST-051.
    let source = "import importlib\nfrom importlib import import_module\n\nyaml = importlib.import_module(\"yaml\")\nzeep = __import__(\"zeep\")\nsibling = importlib.import_module(\".sibling\", __package__)\n";
    let w = work_on(r#"["dyn.py"]"#, &format!("{{{YAML}, {ZEEP}}}"));
    std::fs::write(w.0.join("dyn.py"), source).unwrap();
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let yaml = dependency(&report, "yaml");
    assert_eq!(
        (
            &yaml["used"][0]["imported_at_run_time"],
            &yaml["used"][0]["lines"][0]
        ),
        (&J::from(true), &J::from(4))
    );
    assert_eq!(dependency(&report, "zeep")["used"][0]["lines"][0], 5);
    assert_eq!(report["unread_imports"], serde_json::json!([]));

    // A name that is not written in the source.
    std::fs::write(w.0.join("dyn.py"), format!("{source}\n\ndef load(name):\n    return import_module(name)\n\n\nplugin = __import__(\"plug_\" + \"in\")\n")).unwrap();
    let (code, report, text) = run(&w);
    assert_eq!(code, 1, "{text}");
    assert_eq!(report["verdict"], "incomplete");
    assert_eq!(
        report["unread_imports"],
        serde_json::json!([
            {"file": "dyn.py", "line": 10, "call": "import_module(name)", "acknowledged_by_the_proposal": null},
            {"file": "dyn.py", "line": 13, "call": "__import__('plug_' + 'in')", "acknowledged_by_the_proposal": null}
        ])
    );
    assert!(
        reasons(&report).contains(
            "dyn.py:10 imports a module by a name worked out at run time (import_module(name))"
        ),
        "{text}"
    );
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    assert!(md.contains("## Imports the tool could not read"), "{md}");
}

#[test]
fn heavy_use_alone_makes_a_dismissed_crate_alive() {
    // The SOAP searches return nothing alive, and dismissing all of it is
    // "none found". EDITED RECORDING: `wsdl`, with no release for more
    // than three years, is given the downloads of a crate in wide use.
    let (_, first, _) = run(&work(&with_zeep(
        r#""zeep": {"pypi": "zeep", "search_terms": ["soap client", "wsdl"]}"#,
    )));
    let left: Vec<String> = dependency(&first, "zeep")["not_examined"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| format!(r#""{}": "looked at for this test""#, n.as_str().unwrap()))
        .collect();
    let all = format!(
        r#""zeep": {{"pypi": "zeep", "search_terms": ["soap client", "wsdl"], "dismissed": {{{}}}}}"#,
        left.join(", ")
    );
    let w = work(&with_zeep(&all));
    edit_body(&w, "/api/v1/crates/wsdl", |b| {
        b["crate"]["recent_downloads"] = J::from(500_000)
    });
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let zeep = dependency(&report, "zeep");
    assert_eq!(zeep["outcome"], "candidates_dismissed_by_the_proposal");
    assert!(
        zeep["outcome_reason"]
            .as_str()
            .unwrap()
            .contains("1 of them plainly alive")
            && zeep["outcome_reason"].as_str().unwrap().contains(": wsdl;"),
        "{}",
        zeep["outcome_reason"]
    );
    let wsdl = zeep["dismissed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["crate"]["name"] == "wsdl")
        .unwrap();
    let why = wsdl["alive_because"].as_str().unwrap();
    assert!(
        why.starts_with("500000 downloads in the last 90 days, with no release for 1204 days")
            && why.contains("deprecated or unmaintained"),
        "{why}"
    );
    let md = std::fs::read_to_string(w.0.join("report.md")).unwrap();
    assert!(
        md.contains(
            "| `wsdl` | **yes**: 500000 downloads in the last 90 days, with no release for"
        ),
        "{md}"
    );
}

#[test]
fn an_unread_import_can_be_acknowledged_with_a_reason() {
    let source = "from importlib import import_module\nimport yaml\nimport zeep\n\n\ndef load(name):\n    return import_module(name)\n";
    let with = |ack: &str| {
        let w = work_on(
            r#"["dyn.py"]"#,
            &format!(r#"{{{YAML}, {ZEEP}}}, "acknowledged_unread_imports": {ack}"#),
        );
        std::fs::write(w.0.join("dyn.py"), source).unwrap();
        run(&w)
    };
    let (code, report, text) = with("[]");
    assert_eq!(code, 1, "{text}");
    assert!(
        reasons(&report).contains("acknowledge it in the proposal with a reason"),
        "{text}"
    );

    let (code, report, text) = with(
        r#"[{"file": "dyn.py", "line": 7, "reason": "loads the exporters of this project by name; all of them are local modules"}]"#,
    );
    assert_eq!(code, 0, "{text}");
    assert_eq!(report["verdict"], "complete");
    assert!(report["flags"][0].as_str().unwrap().contains("dyn.py:7 imports a module by a name worked out at run time (import_module(name)). The tool cannot read which. The proposal's account of it, not checked: loads the exporters"), "{text}");
    assert_eq!(
        report["unread_imports"][0]["acknowledged_by_the_proposal"],
        "loads the exporters of this project by name; all of them are local modules"
    );
    assert!(text.contains("flag: dyn.py:7 imports a module"), "{text}");

    // A reason that says nothing, and one for a line that has no such call.
    let (code, report, text) = with(r#"[{"file": "dyn.py", "line": 7, "reason": "fine"}]"#);
    assert_eq!(code, 1, "{text}");
    assert!(
        reasons(&report).contains("is acknowledged with the reason \"fine\""),
        "{text}"
    );
    let (code, report, text) = with(
        r#"[{"file": "dyn.py", "line": 7, "reason": "local plugin modules only"}, {"file": "dyn.py", "line": 2, "reason": "nothing is there at all"}]"#,
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        reasons(&report)
            .contains("acknowledges a run-time import at dyn.py:2, and there is none there"),
        "{text}"
    );
}

/// A virtual environment in which the project `project` is installed as
/// far as `importlib.metadata` can tell, providing the module `module`.
fn interpreter_with(w: &Work, project: &str, module: &str) -> String {
    let venv = w.0.join("venv");
    let made = Command::new("python3")
        .args(["-m", "venv", "--without-pip"])
        .arg(&venv)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let lib = std::fs::read_dir(venv.join("lib"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path()
        .join("site-packages");
    let info = lib.join(format!("{}-1.0.dist-info", project.replace('-', "_")));
    std::fs::create_dir_all(&info).unwrap();
    std::fs::write(
        info.join("METADATA"),
        format!("Metadata-Version: 2.1\nName: {project}\nVersion: 1.0\n"),
    )
    .unwrap();
    std::fs::write(info.join("top_level.txt"), format!("{module}\n")).unwrap();
    std::fs::write(info.join("RECORD"), format!("{module}/__init__.py,,\n")).unwrap();
    std::fs::create_dir_all(lib.join(module)).unwrap();
    std::fs::write(lib.join(module).join("__init__.py"), "").unwrap();
    venv.join("bin/python").to_string_lossy().into_owned()
}

#[test]
fn an_installed_library_says_which_project_provides_the_import() {
    // RUST-057. The names `yaml` and `opentelemetry-api` do not resemble
    // each other, so without more the pair is flagged. SYNTHETIC: an
    // interpreter in which a project of that name provides `yaml`.
    let wrong = r#""yaml": {"pypi": "opentelemetry-api", "candidates": ["opentelemetry"]}"#;
    let deps = format!("{{{OTEL}, {wrong}, {GENAI}, {ZEEP}}}");
    let w = work(&deps);
    let python = interpreter_with(&w, "opentelemetry-api", "yaml");
    let (code, report, text) = run_with(&w, &["--python", &python]);
    assert_eq!(code, 0, "{text}");
    let yaml = dependency(&report, "yaml");
    assert!(
        !flags(yaml).contains("comes from the PyPI project"),
        "{text}"
    );
    assert_eq!(
        yaml["pypi"]["provides_the_import"],
        serde_json::json!({"source": "importlib.metadata of the interpreter the tool ran", "agrees_with_the_proposal": true, "projects": ["opentelemetry-api"]})
    );
    assert!(
        !yaml["outcome_reason"]
            .as_str()
            .unwrap()
            .contains("was not checked"),
        "{text}"
    );

    // The interpreter has `yaml` from another project than the one named.
    let w = work(&deps);
    let python = interpreter_with(&w, "PyYAML", "yaml");
    let (code, report, text) = run_with(&w, &["--python", &python]);
    assert_eq!(code, 0, "{text}");
    let yaml = dependency(&report, "yaml");
    assert!(
        flags(yaml).contains("the interpreter the tool ran has `yaml` installed from [\"PyYAML\"]"),
        "{text}"
    );
    assert_eq!(
        yaml["pypi"]["provides_the_import"]["agrees_with_the_proposal"],
        false
    );
    assert!(
        yaml["outcome_reason"]
            .as_str()
            .unwrap()
            .contains("was not checked"),
        "{text}"
    );
}

#[test]
fn a_link_is_not_ownership() {
    // RUST-058, first part. EDITED RECORDING: PyYAML's page on PyPI is
    // given a link to another organisation's repository under a label that
    // is not its source or home.
    let other = r#""yaml": {"pypi": "PyYAML", "candidates": ["opentelemetry"]}"#;
    let w = work(&format!("{{{OTEL}, {other}, {GENAI}, {ZEEP}}}"));
    edit_body(&w, "pypi.org/pypi/PyYAML/json", |b| {
        b["info"]["project_urls"]["Used by"] =
            J::from("https://github.com/open-telemetry/opentelemetry-python")
    });
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let yaml = dependency(&report, "yaml");
    assert_eq!(
        yaml["pypi"]["organisations"],
        serde_json::json!(["github.com/yaml"])
    );
    assert_eq!(
        candidate(yaml, "opentelemetry")["classification"],
        "community"
    );
    assert_eq!(yaml["outcome"], "community_crates");

    // Second part. EDITED RECORDING: zeep's description on PyPI is given a
    // list of alternatives that links to a crate.
    let w = work(&with_zeep(ZEEP));
    edit_body(&w, "pypi.org/pypi/zeep/json", |b| {
        b["info"]["description"] =
            J::from("Alternatives in Rust: https://crates.io/crates/soap-client and others.")
    });
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let zeep = dependency(&report, "zeep");
    let soap = candidate(zeep, "soap-client");
    assert_eq!(soap["classification"], "named_by_upstream");
    let evidence = soap["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        evidence.contains("contains a link to this crate. That is all the tool found")
            && evidence.contains("so it is not shown to be published by them"),
        "{evidence}"
    );
    assert_eq!(zeep["outcome"], "crate_named_by_upstream");
    assert!(
        zeep["outcome_reason"]
            .as_str()
            .unwrap()
            .contains("The tool did not judge in what sense"),
        "{text}"
    );
    // A crate with an owner team keeps `official`, link or no link.
    assert_eq!(
        candidate(dependency(&report, "opentelemetry"), "opentelemetry")["classification"],
        "official"
    );
}

#[test]
fn a_build_script_that_could_not_be_looked_up_is_flagged() {
    let w = work(&with_zeep(ZEEP));
    edit_answer(&w, "docs.rs/crate/soap-client/0.1.9/source/", |a| {
        a["status"] = J::from(404)
    });
    edit_answer(
        &w,
        "docs.rs/crate/soap-client/0.1.9/source/Cargo.toml",
        |a| a["status"] = J::from(404),
    );
    let (code, report, text) = run(&w);
    assert_eq!(code, 0, "{text}");
    let soap = candidate(dependency(&report, "zeep"), "soap-client");
    assert!(soap["facts"]["build_script"].is_null());
    assert!(
        flags(soap).contains("whether it has a build script is not known")
            && flags(soap).contains("whether it is a proc-macro is not known"),
        "{text}"
    );
    assert!(
        text.contains("flag: whether it has a build script is not known"),
        "{text}"
    );
}

#[test]
#[ignore = "goes to crates.io, PyPI, rustsec.org and docs.rs"]
fn live_the_worked_example() {
    let (code, report, text) = live("example");
    assert_eq!(code, 0, "{text}");
    let outcomes: Vec<(&str, &str)> = report["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["import"].as_str().unwrap(),
                d["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        outcomes,
        [
            ("opentelemetry", "official_crate"),
            ("sklearn", "community_crates"),
            ("yaml", "community_crates"),
            ("zeep", "community_crates")
        ]
    );
    assert!(
        report["requests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["from_cache"] == false)
    );
}

#[test]
#[ignore = "goes to crates.io, PyPI, rustsec.org and docs.rs"]
fn live_the_adk_and_gemini_case() {
    let (code, report, text) = live("adk-case");
    assert_eq!(code, 0, "{text}");
    // Whatever exists today, these two hold: crates were found by search,
    // and a name guessed from the Python package is reported as absent.
    let adk = dependency(&report, "google.adk");
    assert!(
        adk["searches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["total"].as_u64().unwrap() > 0)
    );
    assert_eq!(candidate(adk, "google-adk")["exists"], false);
    assert_ne!(adk["outcome"], "unresolved");
}
