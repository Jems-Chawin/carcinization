//! End-to-end tests: the real binary, real Python processes, a real git
//! repository made for each test. They need `git` and a Python 3.9+
//! interpreter (`python3`, or the one named by `CARCINIFY_PYTHON`).

use serde_json::Value as J;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

struct Repo {
    dir: PathBuf,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Run {
    code: i32,
    report: J,
    stdout: String,
    stderr: String,
}

impl Run {
    fn case(&self, i: usize) -> &J {
        &self.report["cases"][i]
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git must be installed");
    assert!(
        ok.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
}

impl Repo {
    /// A committed repository holding the fixture modules, a suite and one
    /// case file with the given lines.
    fn new(cases: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!(
            "carcinify-diff-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../harness/tests/fixture");
        for (side, file) in [("original", "orig.py"), ("candidate", "port.py")] {
            std::fs::create_dir_all(dir.join(side)).unwrap();
            std::fs::copy(fixture.join(file), dir.join(side).join(file)).unwrap();
        }
        std::fs::write(dir.join("data.bin"), b"real data \xff\x00").unwrap();
        std::fs::write(dir.join("cases.jsonl"), cases).unwrap();
        std::fs::write(dir.join(".gitignore"), "out/\n__pycache__/\n").unwrap();
        let repo = Repo { dir };
        repo.suite(r#""cases": ["cases.jsonl"]"#);
        git(&repo.dir, &["init", "-q"]);
        repo.commit();
        repo
    }

    fn suite(&self, cases: &str) {
        let text = format!(
            r#"{{"schema": "carcinify/diff/suite/1",
  "original": {{"kind": "python", "path": ["original"], "module": "orig"}},
  "candidate": {{"kind": "python", "path": ["candidate"], "module": "port"}},
  {cases}}}"#
        );
        std::fs::write(self.dir.join("suite.json"), text).unwrap();
    }

    fn commit(&self) {
        git(&self.dir, &["add", "-A"]);
        git(&self.dir, &["commit", "-q", "-m", "fixture"]);
    }

    fn report_path(&self) -> PathBuf {
        self.dir.join("out/report.json")
    }

    fn run(&self, extra: &[&str]) -> Run {
        self.run_python(&base_python(), extra)
    }

    fn run_python(&self, python: &str, extra: &[&str]) -> Run {
        let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
            .arg("diff")
            .arg(self.dir.join("suite.json"))
            .arg("--report")
            .arg(self.report_path())
            .args(["--python", python])
            .args(extra)
            .output()
            .unwrap();
        let report = std::fs::read_to_string(self.report_path())
            .map_or(J::Null, |t| serde_json::from_str(&t).unwrap());
        Run {
            code: out.status.code().unwrap(),
            report,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

fn base_python() -> String {
    std::env::var("CARCINIFY_PYTHON").unwrap_or("python3".to_string())
}

fn run(cases: &str) -> Run {
    Repo::new(cases).run(&[])
}

/// Runs one zero-argument fixture function and checks that it fails the
/// run. Returns the case's entry in the report.
fn failing(func: &str) -> J {
    let r = run(&format!(r#"{{"id": "c", "fn": "{func}"}}"#));
    assert_eq!(r.code, 1, "{func}: {}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "fail");
    assert_eq!(r.report["summary"]["cases"], 1);
    assert_eq!(r.report["summary"]["identical"], 0);
    r.case(0).clone()
}

fn bits(case: &J) -> (String, String) {
    let d = &case["first_difference"];
    let get = |side: &str| d[side]["bits"].as_str().unwrap_or("").to_string();
    (get("original"), get("candidate"))
}

#[test]
fn negative_zero_against_zero_fails() {
    let c = failing("neg_zero");
    assert_eq!(c["result"], "different");
    assert_eq!(
        bits(&c),
        (
            "0x8000000000000000".to_string(),
            "0x0000000000000000".to_string()
        )
    );
}

#[test]
fn nan_against_a_number_fails() {
    let c = failing("nan");
    assert_eq!(c["result"], "different");
    assert_eq!(c["first_difference"]["original"]["repr"], "NaN");
}

#[test]
fn nan_against_a_nan_with_another_payload_fails() {
    let c = failing("nan_payload");
    assert_eq!(
        bits(&c),
        (
            "0x7ff8000000000000".to_string(),
            "0x7ff8000000000001".to_string()
        )
    );
}

#[test]
fn infinity_against_the_largest_float_fails() {
    let c = failing("inf");
    assert_eq!(
        bits(&c),
        (
            "0x7ff0000000000000".to_string(),
            "0x7fefffffffffffff".to_string()
        )
    );
}

#[test]
fn an_integer_beyond_64_bits_fails_when_it_is_wrapped() {
    let c = failing("big");
    let d = &c["first_difference"];
    assert_eq!(d["original"]["repr"], "18446744073709551617");
    assert_eq!(d["candidate"]["repr"], "1");
}

#[test]
fn a_different_key_order_fails() {
    let c = failing("order");
    assert_eq!(c["first_difference"]["kind"], "key_order");
}

#[test]
fn nfc_against_nfd_fails() {
    let c = failing("text");
    let d = &c["first_difference"];
    assert_eq!(d["kind"], "value");
    assert!(d["note"].as_str().unwrap().contains("U+00E9 and U+0065"));
}

#[test]
fn a_side_that_crashes_fails_and_the_next_case_still_runs() {
    let r = run(
        "{\"id\": \"dies\", \"fn\": \"crash\"}\n{\"id\": \"after\", \"fn\": \"echo\", \"args\": [1]}\n",
    );
    assert_eq!(r.code, 1);
    assert_eq!(r.case(0)["result"], "error");
    assert_eq!(r.case(0)["errors"][0]["side"], "candidate");
    assert_eq!(r.case(0)["errors"][0]["kind"], "crashed");
    assert_eq!(r.case(1)["result"], "identical");
    assert_eq!(r.report["sides"]["candidate"]["restarts"], 1);
    assert_eq!(r.report["summary"]["errors"], 1);
}

#[test]
fn an_empty_case_set_fails() {
    for cases in ["", "\n\n"] {
        let r = run(cases);
        assert_eq!(r.code, 1, "{}", r.stderr);
        assert_eq!(r.report["verdict"], "fail");
        assert_eq!(r.report["reasons"][0], "no cases were run");
    }
    let repo = Repo::new("");
    repo.suite(r#""cases": []"#);
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!((r.code, &r.report["verdict"]), (1, &J::from("fail")));
}

#[test]
fn identical_outputs_pass_and_the_report_says_what_ran() {
    let cases = r#"{"id": "plain", "fn": "echo", "args": [396.03999999999996, -0.0, 123456789012345678901234567890, "café", null, true], "kwargs": {"k": [1, 2]}}
{"id": "typed", "fn": "echo", "encoding": "typed", "args": [{"t": "float", "bits": "7ff8000000000001"}, {"t": "float", "bits": "fff0000000000000"}, {"t": "bytes", "hex": "00ff"}, {"t": "tuple", "v": [{"t": "int", "v": "-340282366920938463463374607431768211457"}]}, {"t": "str", "cp": [55296]}, {"t": "set", "v": [{"t": "int", "v": "1"}, {"t": "int", "v": "2"}]}, {"t": "map", "v": [[{"t": "int", "v": "1"}, {"t": "none"}]]}]}
"#;
    let r = run(cases);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let rep = &r.report;
    assert_eq!(rep["verdict"], "pass");
    assert_eq!(rep["summary"]["identical"], 2);
    assert_eq!(rep["git"]["commit"].as_str().unwrap().len(), 40);
    assert_eq!(rep["git"]["dirty"], false);
    assert_eq!(rep["inputs"][0]["path"], "cases.jsonl");
    assert_eq!(rep["inputs"][0]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(rep["suite"]["path"], "suite.json");
    assert_eq!(
        rep["sides"]["original"]["files"][0]["path"],
        "original/orig.py"
    );
    assert_eq!(
        rep["sides"]["candidate"]["module_file"],
        "candidate/port.py"
    );
    assert_ne!(
        rep["sides"]["original"]["implementation_sha256"],
        rep["sides"]["candidate"]["implementation_sha256"]
    );
    assert!(
        rep["sides"]["original"]["runtime"]["version"]
            .as_str()
            .unwrap()
            .starts_with('3')
    );
    assert!(
        rep["tools"]["rustc"]
            .as_str()
            .unwrap()
            .starts_with("rustc ")
    );
    assert_eq!(rep["tools"]["harness_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(r.case(0)["original_sha256"], r.case(0)["candidate_sha256"]);
}

#[test]
fn a_hang_is_killed_and_fails() {
    let repo =
        Repo::new("{\"id\": \"hangs\", \"fn\": \"hang\"}\n{\"id\": \"after\", \"fn\": \"echo\"}\n");
    // The hang sleeps for 600 s. The limit is long enough that the case
    // after it, in a newly started Python, answers on a loaded machine too.
    let r = repo.run(&["--timeout-secs", "20"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.case(0)["errors"][0]["kind"], "timeout");
    assert_eq!(r.case(1)["result"], "identical");
}

#[test]
fn both_sides_raising_is_a_pass_only_when_the_case_expects_it() {
    let r = run(r#"{"id": "c", "fn": "raises"}"#);
    assert_eq!(r.code, 1);
    assert_eq!(r.case(0)["result"], "error");
    assert_eq!(r.case(0)["errors"][0]["kind"], "raised");

    let ok = r#"{"id": "returns", "fn": "raises", "args": [false]}"#;
    let r = run(&format!(
        "{ok}\n{}",
        r#"{"id": "c", "fn": "raises", "expect_raise": true}"#
    ));
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(r.case(1)["result"], "identical");
    assert_eq!(r.case(1)["original_outcome"], "raised");
    let f = &r.report["checks"]["functions"][0];
    assert_eq!(
        (&f["both_returned"], &f["both_raised"]),
        (&J::from(1), &J::from(1))
    );

    let r = run(r#"{"id": "c", "fn": "echo", "expect_raise": true}"#);
    assert_eq!(r.code, 1);
    assert_eq!(r.case(0)["errors"][0]["kind"], "did_not_raise");

    let r = run(r#"{"id": "c", "fn": "raises_one_side"}"#);
    assert_eq!(r.code, 1);
    assert_eq!(r.case(0)["first_difference"]["kind"], "outcome");

    let r = run(r#"{"id": "c", "fn": "raises_other_message", "expect_raise": true}"#);
    assert_eq!(r.code, 1);
    assert_eq!(r.case(0)["first_difference"]["kind"], "exception_message");
}

#[test]
fn relaxations_are_opt_in_and_show_in_the_report() {
    let r = run(r#"{"id": "c", "fn": "tolerance"}"#);
    assert_eq!(r.code, 1);
    assert_eq!(r.case(0)["first_difference"]["path"], "$[\"amount\"]");

    let r = run(
        r#"{"id": "c", "fn": "tolerance", "relax": {"floats": [{"path": "$[\"amount\"]", "ulps": 1, "reason": "test"}]}}
{"id": "o", "fn": "order", "relax": {"key_order": [{"path": "$", "reason": "test"}]}}
{"id": "m", "fn": "raises_other_message", "expect_raise": true, "relax": {"exception_message": true}}
{"id": "returns", "fn": "raises_other_message", "args": [false]}"#,
    );
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(r.report["summary"]["identical"], 1);
    assert_eq!(r.report["summary"]["identical_with_relaxations"], 3);
    // Passing through a relaxation is not the same verdict as passing.
    assert_eq!(r.report["verdict"], "pass_with_relaxations");
    assert!(
        r.stdout.contains("PASS WITH RELAXATIONS: 3 of 4 case(s)"),
        "{}",
        r.stdout
    );
    assert!(!r.stdout.lines().any(|l| l == "PASS"));
    let c = r.case(0);
    assert_eq!(c["result"], "identical_with_relaxations");
    assert_eq!(c["relaxations_declared"]["floats"][0]["reason"], "test");
    assert_eq!(c["relaxations_applied"][0]["relaxation"], "float_ulps");
    assert_eq!(
        c["relaxations_applied"][0]["original"]["bits"],
        "0x4078c0a3d70a3d70"
    );
    assert_eq!(
        c["relaxations_applied"][0]["candidate"]["bits"],
        "0x4078c0a3d70a3d71"
    );
    assert_eq!(
        r.case(1)["relaxations_applied"][0]["relaxation"],
        "key_order"
    );
    assert_eq!(
        r.case(2)["relaxations_applied"][0]["relaxation"],
        "exception_message"
    );
}

#[test]
fn a_value_the_format_does_not_cover_fails() {
    let c = failing("unencodable");
    assert_eq!(c["errors"][0]["kind"], "unencodable");
    assert!(
        c["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("decimal.Decimal")
    );
    let c = failing("no_such_function_anywhere");
    assert_eq!(c["errors"][0]["kind"], "no_such_function");
}

#[test]
fn printing_cannot_corrupt_the_protocol() {
    let r = run(r#"{"id": "c", "fn": "noisy", "args": [1]}"#);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
}

#[test]
fn each_side_gets_its_own_copy_of_a_data_file_at_the_same_path() {
    let r = run(
        r#"{"id": "file", "fn": "read", "encoding": "typed", "args": [{"t": "file_path", "path": "data.bin"}]}
{"id": "bytes", "fn": "echo", "encoding": "typed", "args": [{"t": "file_bytes", "path": "data.bin"}]}
{"id": "dir", "fn": "scratch", "encoding": "typed", "args": [{"t": "scratch_dir"}]}"#,
    );
    // The original overwrites its file; the candidate must still read the data.
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["summary"]["identical"], 3);
    let inputs = r.report["inputs"].as_array().unwrap();
    let data = inputs.iter().find(|i| i["path"] == "data.bin").unwrap();
    assert_eq!(data["kind"], "data");
    // SHA-256 of the fixture's 12 bytes, computed independently with `shasum -a 256`.
    assert_eq!(
        data["sha256"],
        "759f79d0d9614e87c9deeca52c9f87cc3daa3470c1a9a4850d4ba52292b078a2"
    );
}

#[test]
fn uncommitted_changes_stop_the_run_unless_allowed() {
    let repo = Repo::new(r#"{"id": "c", "fn": "echo"}"#);
    std::fs::create_dir_all(repo.dir.join("out")).unwrap();
    std::fs::write(repo.report_path(), r#"{"verdict": "pass"}"#).unwrap();
    std::fs::write(
        repo.dir.join("candidate/port.py"),
        "def echo(*a, **k):\n    return [list(a), k]\n",
    )
    .unwrap();

    let r = repo.run(&[]);
    assert_eq!(r.code, 2);
    assert!(r.stderr.contains("uncommitted changes"), "{}", r.stderr);
    // The stale passing report is gone, replaced by one that says why.
    assert_eq!(r.report["verdict"], "error");

    let r = repo.run(&["--allow-dirty"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["git"]["dirty"], true);
    assert!(
        r.report["git"]["uncommitted"][0]
            .as_str()
            .unwrap()
            .contains("candidate/port.py")
    );
    assert!(r.stdout.contains("plus changes that are not in it"));
}

#[test]
fn a_module_that_cannot_be_found_stops_the_run() {
    let repo = Repo::new(r#"{"id": "c", "fn": "echo"}"#);
    std::fs::remove_file(repo.dir.join("candidate/port.py")).unwrap();
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2);
    assert_eq!(r.report["verdict"], "error");
    assert!(r.stderr.contains("import of 'port' failed"), "{}", r.stderr);
}

#[test]
fn a_shadowing_copy_of_the_module_is_refused() {
    // "json" resolves to the standard library, not to the suite's directory.
    let repo = Repo::new(r#"{"id": "c", "fn": "dumps", "args": [1]}"#);
    let suite = std::fs::read_to_string(repo.dir.join("suite.json")).unwrap();
    std::fs::write(
        repo.dir.join("suite.json"),
        suite.replace("\"port\"", "\"json\""),
    )
    .unwrap();
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2);
    assert!(
        r.stderr.contains("another copy is shadowing"),
        "{}",
        r.stderr
    );
}

#[test]
fn a_bad_case_file_stops_the_run_before_anything_runs() {
    let r = run(r#"{"id": "c", "fn": "echo", "expected": 1}"#);
    assert_eq!(r.code, 2);
    assert_eq!(r.report["verdict"], "error");
    assert!(r.stderr.contains("unknown field"), "{}", r.stderr);
}

// ---- A pass must mean the port was compared with the original. ----
// The first tests below are the auditor's reproductions from the T001 audit
// (RUST-001: E1, E1b; RUST-002: E2, E3). Each of them used to end in PASS.

fn error_kind(r: &Run, i: usize) -> &J {
    &r.case(i)["errors"][0]["kind"]
}

#[test]
fn e1_calling_what_every_module_inherits_is_refused() {
    let cases: String = (0..68)
        .map(|i| {
            format!("{{\"id\": \"c{i}\", \"fn\": \"__setattr__\", \"args\": [\"x{i}\", {i}]}}\n")
        })
        .collect();
    let r = run(&cases);
    assert_eq!(r.code, 1, "{}", r.stdout);
    assert_eq!(r.report["summary"]["errors"], 68);
    assert_eq!(error_kind(&r, 0), "refused_function");
    assert!(
        r.case(0)["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("underscore")
    );
    assert_eq!(
        r.report["checks"]["functions"][0]["compared_on_a_returned_value"],
        "fail"
    );
}

#[test]
fn e1b_other_callables_that_are_not_project_functions_are_refused() {
    let r = run(r#"{"id": "init", "fn": "__init__", "args": ["anything"]}
{"id": "reduce", "fn": "__reduce__", "expect_raise": true}
{"id": "imported", "fn": "struct", "expect_raise": true}
{"id": "wrongargs", "fn": "big", "args": [1, 2, 3], "expect_raise": true}
{"id": "class", "fn": "Thing"}
{"id": "private", "fn": "_private"}"#);
    assert_eq!(r.code, 1, "{}", r.stdout);
    assert_eq!(r.report["summary"]["errors"], 6);
    for i in [0, 1, 2, 4, 5] {
        assert_eq!(error_kind(&r, i), "refused_function", "case {i}");
    }
    // Both sides would raise the same TypeError before either body ran.
    assert_eq!(error_kind(&r, 3), "bad_call");
    assert!(
        r.case(3)["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("signature")
    );
}

#[test]
fn a_function_seen_only_raising_fails_the_run() {
    let r = run(r#"{"id": "ok", "fn": "echo", "args": [1]}
{"id": "only-raises", "fn": "raises", "expect_raise": true}"#);
    assert_eq!(r.code, 1, "{}", r.stdout);
    // Every case matched, and that is still not a pass.
    assert_eq!(r.report["summary"]["identical"], 2);
    assert_eq!(r.report["verdict"], "fail");
    assert!(
        r.report["reasons"][0]
            .as_str()
            .unwrap()
            .contains("\"raises\" was never compared on a returned value")
    );
    let f = &r.report["checks"]["functions"];
    assert_eq!(f[0]["compared_on_a_returned_value"], "pass");
    assert_eq!(f[1]["compared_on_a_returned_value"], "fail");
    assert_eq!(f[1]["both_raised"], 1);
}

fn stopped_as_same_code(repo: &Repo) -> Run {
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "error");
    let reason = r.report["reasons"][0].as_str().unwrap();
    assert!(
        reason.contains("the two sides are not different code"),
        "{reason}"
    );
    r
}

#[test]
fn e2_the_same_module_on_both_sides_stops_the_run() {
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    let suite = std::fs::read_to_string(repo.dir.join("suite.json")).unwrap();
    let same = suite.replace(
        r#""path": ["candidate"], "module": "port""#,
        r#""path": ["original"], "module": "orig""#,
    );
    assert_ne!(suite, same);
    std::fs::write(repo.dir.join("suite.json"), same).unwrap();
    stopped_as_same_code(&repo);
}

#[test]
fn e3_a_candidate_that_imports_the_original_stops_the_run() {
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    std::fs::write(
        repo.dir.join("candidate/port.py"),
        "import os, sys\nsys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), \"original\"))\nfrom orig import *  # the \"port\" is the original\n",
    )
    .unwrap();
    let r = stopped_as_same_code(&repo);
    assert!(
        r.stderr
            .contains("original/orig.py, which is in the original side's source directory"),
        "{}",
        r.stderr
    );
}

#[test]
fn a_candidate_that_imports_the_original_later_stops_the_run() {
    // The import happens inside the function, after the first check.
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    std::fs::write(
        repo.dir.join("candidate/port.py"),
        "import os, sys\n\n\ndef tolerance():\n    sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), \"original\"))\n    import orig\n    return orig.tolerance()\n",
    )
    .unwrap();
    stopped_as_same_code(&repo);
}

#[test]
fn a_copy_of_the_original_under_another_name_stops_the_run() {
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    std::fs::copy(
        repo.dir.join("original/orig.py"),
        repo.dir.join("candidate/port.py"),
    )
    .unwrap();
    let r = stopped_as_same_code(&repo);
    assert!(r.stderr.contains("has the same content as"), "{}", r.stderr);
}

#[test]
fn overlapping_source_directories_stop_the_run() {
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    let suite = std::fs::read_to_string(repo.dir.join("suite.json")).unwrap();
    std::fs::write(
        repo.dir.join("suite.json"),
        suite.replace(r#"["candidate"]"#, r#"["candidate", "original"]"#),
    )
    .unwrap();
    stopped_as_same_code(&repo);
}

#[test]
fn a_function_borrowed_from_elsewhere_is_refused_like_any_other_bad_name() {
    // `dumps` is in the port's namespace but is the standard library's code.
    let repo = Repo::new(r#"{"id": "c", "fn": "dumps", "args": [1]}"#);
    for file in ["original/orig.py", "candidate/port.py"] {
        let path = repo.dir.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("from json import dumps\n{text}")).unwrap();
    }
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(error_kind(&r, 0), "refused_function");
    assert!(
        r.case(0)["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("not this side's own code")
    );
}

#[test]
fn a_decorated_function_of_the_module_is_still_callable() {
    let r = run(r#"{"id": "c", "fn": "decorated", "args": [1]}"#);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let f = &r.report["checks"]["functions"][0];
    assert_eq!(f["original_defined_in"][0]["path"], "original/orig.py");
    assert_eq!(f["candidate_defined_in"][0]["path"], "candidate/port.py");
    assert_eq!(
        r.report["checks"]["sides_are_different_code"]["result"],
        "pass"
    );
}

// ---- T001-B: what a pass covers, and what "clean tree" means. ----

#[test]
fn an_all_relaxed_suite_is_not_a_plain_pass_and_fails_under_strict() {
    let repo = Repo::new(
        r#"{"id": "t", "fn": "tolerance", "relax": {"floats": [{"path": "$[*]", "abs": "1e300", "reason": "anything goes"}]}}
{"id": "o", "fn": "order", "relax": {"key_order": [{"path": "$", "reason": "test"}]}}"#,
    );
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass_with_relaxations");
    assert_eq!(r.report["strict"], false);
    assert_eq!(r.report["summary"]["identical"], 0);
    assert!(
        r.stdout.contains("PASS WITH RELAXATIONS: 2 of 2 case(s)"),
        "{}",
        r.stdout
    );

    let r = repo.run(&["--strict"]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass_with_relaxations");
    assert_eq!(r.report["strict"], true);
    assert!(
        r.stdout.contains("--strict makes this a failure"),
        "{}",
        r.stdout
    );
    // The report alone says how the run ended.
    assert_eq!(r.report["outcome"], "fail");
    assert_eq!(r.report["exit_status"], 1);
    let lenient = repo.run(&[]);
    assert_eq!(lenient.report["outcome"], "pass");
    assert_eq!(lenient.report["exit_status"], 0);
}

#[test]
fn strict_changes_nothing_for_an_exact_pass() {
    let repo = Repo::new(r#"{"id": "c", "fn": "echo", "args": [1]}"#);
    let r = repo.run(&["--strict"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass");
    assert!(r.stdout.lines().any(|l| l == "PASS"));
}

#[test]
fn stdout_and_report_say_what_was_exercised() {
    let r = run(r#"{"id": "a", "fn": "echo", "args": [1]}
{"id": "b", "fn": "order", "relax": {"key_order": [{"path": "$", "reason": "test"}]}}"#);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let row = |name: &str| {
        r.stdout
            .lines()
            .find(|l| l.starts_with(name))
            .unwrap_or_else(|| panic!("no row for {name}: {}", r.stdout))
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    // function, cases, identical, relaxed, different, failed
    assert_eq!(row("echo")[1..6], ["1", "1", "0", "0", "0"]);
    assert_eq!(row("order")[1..6], ["1", "0", "1", "0", "0"]);
    let not_called = &r.report["checks"]["public_functions_not_called"];
    for side in ["original", "candidate"] {
        let names: Vec<&str> = not_called[side]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"tolerance") && names.contains(&"boom"),
            "{names:?}"
        );
        assert!(
            !names.contains(&"echo") && !names.contains(&"order"),
            "{names:?}"
        );
        // Not functions of the module, so not on the list either.
        assert!(
            !names
                .iter()
                .any(|n| ["Thing", "StoreError", "struct", "os", "_private"].contains(n)),
            "{names:?}"
        );
    }
    assert!(
        r.stdout.contains("not called by any case (candidate): "),
        "{}",
        r.stdout
    );
}

/// The auditor's E4: the committed candidate imports a file git ignores.
fn repo_with_ignored_import() -> Repo {
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    std::fs::write(
        repo.dir.join(".gitignore"),
        "out/\n__pycache__/\nlocal_*.py\n",
    )
    .unwrap();
    let port = repo.dir.join("candidate/port.py");
    let text = std::fs::read_to_string(&port).unwrap();
    std::fs::write(
        &port,
        format!("{text}\nfrom local_patch import tolerance  # ignored by git\n"),
    )
    .unwrap();
    repo.commit();
    std::fs::write(
        repo.dir.join("candidate/local_patch.py"),
        "def tolerance():\n    return {\"amount\": 396.03999999999996, \"rate\": 396.03999999999996}\n",
    )
    .unwrap();
    repo
}

#[test]
fn e4_an_ignored_file_that_runs_stops_the_run() {
    let repo = repo_with_ignored_import();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "error");
    let reason = r.report["reasons"][0].as_str().unwrap();
    assert!(reason.contains("files that git does not track"), "{reason}");
    assert!(
        reason.contains("candidate/local_patch.py (imported by the candidate side)"),
        "{reason}"
    );
}

#[test]
fn e4_with_allow_dirty_the_ignored_file_is_listed_and_the_tree_is_dirty() {
    let repo = repo_with_ignored_import();
    let r = repo.run(&["--allow-dirty"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let git = &r.report["git"];
    assert_eq!(git["dirty"], true);
    assert_eq!(git["uncommitted"].as_array().unwrap().len(), 0); // git status sees nothing
    assert_eq!(
        git["used_but_not_tracked"][0]["path"],
        "candidate/local_patch.py"
    );
    assert!(
        r.stdout.contains("plus changes that are not in it"),
        "{}",
        r.stdout
    );
}

#[test]
fn e8_a_sides_own_exception_class_matches_by_class_name() {
    let r = run(r#"{"id": "ok", "fn": "boom", "args": [false]}
{"id": "c", "fn": "boom", "expect_raise": true}"#);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass");
    let c = r.case(1);
    assert_eq!(c["result"], "identical");
    let note = c["notes"][0].as_str().unwrap();
    assert!(
        note.contains("orig.StoreError and port.StoreError"),
        "{note}"
    );
    assert!(note.contains("the module part was ignored"), "{note}");
    // A built-in exception carries no such note.
    let r = run(r#"{"id": "ok", "fn": "raises", "args": [false]}
{"id": "c", "fn": "raises", "expect_raise": true}"#);
    assert!(r.case(1).get("notes").is_none());
}

#[test]
fn exception_arguments_are_compared_not_only_the_message() {
    let cases = |relax: &str| {
        format!(
            "{{\"id\": \"ok\", \"fn\": \"args_differ\", \"args\": [false]}}\n{{\"id\": \"c\", \"fn\": \"args_differ\", \"expect_raise\": true{relax}}}"
        )
    };
    let r = run(&cases(""));
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    let d = &r.case(1)["first_difference"];
    assert_eq!(d["kind"], "exception_args");
    assert_eq!(d["original"]["repr"], "tuple of 1 item(s)");
    let r = run(&cases(r#", "relax": {"exception_message": true}"#));
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass_with_relaxations");
}

// ---- T001-D: same-code rules. Names in comments (N3, N4, L1, L2, N8) are
// the auditor's attempts from the T001-A re-audit. ----

/// A repository whose two sides are the given module texts, with an
/// optional helper in common/.
fn layout(orig: &str, port: &str, helper: Option<&str>, cases: &str) -> Repo {
    let repo = Repo::new(cases);
    std::fs::write(repo.dir.join("original/orig.py"), orig).unwrap();
    std::fs::write(repo.dir.join("candidate/port.py"), port).unwrap();
    if let Some(text) = helper {
        std::fs::create_dir_all(repo.dir.join("common")).unwrap();
        std::fs::write(repo.dir.join("common/helper.py"), text).unwrap();
    }
    repo
}

fn write_suite(repo: &Repo, original: &str, candidate: &str, extra: &str) {
    let text = format!(
        r#"{{"schema": "carcinify/diff/suite/1",
  "original": {{"kind": "python", "path": {original}, "module": "orig"}},
  "candidate": {{"kind": "python", "path": {candidate}, "module": "port"}},
  "cases": ["cases.jsonl"]{extra}}}"#
    );
    std::fs::write(repo.dir.join("suite.json"), text).unwrap();
}

const HELPER: &str = "def clean(x):\n    return x.strip()\n";
const ORIG_WITH_HELPER: &str =
    "from helper import clean\n\n\ndef shout(x):\n    return clean(x).upper()\n";
const PORT_WITH_HELPER: &str = "from helper import clean\n\n\ndef shout(x):\n    return \"\".join(c.upper() for c in clean(x))\n";
const SHOUT: &str = r#"{"id": "c", "fn": "shout", "args": [" abc "]}"#;

#[test]
fn l1_a_helper_directory_both_sides_use_passes_once_declared_shared() {
    let repo = layout(ORIG_WITH_HELPER, PORT_WITH_HELPER, Some(HELPER), SHOUT);
    write_suite(
        &repo,
        r#"["original"]"#,
        r#"["candidate"]"#,
        r#", "shared": ["common"]"#,
    );
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass");
    let shared = &r.report["checks"]["shared"][0];
    assert_eq!(shared["path"], "common");
    assert!(shared.get("inside").is_none());
    assert_eq!(shared["files_loaded"][0]["path"], "common/helper.py");
    assert_eq!(shared["files_loaded"][0]["loaded_by"], "both");
    assert!(
        r.stdout
            .contains("shared, run by both sides and not compared (common): common/helper.py"),
        "{}",
        r.stdout
    );
    // The function under test is each side's own.
    let f = &r.report["checks"]["functions"][0];
    assert_eq!(f["original_defined_in"][0]["path"], "original/orig.py");
    assert_eq!(f["candidate_defined_in"][0]["path"], "candidate/port.py");
}

#[test]
fn l1_the_same_helper_directory_not_declared_stops_the_run() {
    let repo = layout(ORIG_WITH_HELPER, PORT_WITH_HELPER, Some(HELPER), SHOUT);
    write_suite(
        &repo,
        r#"["original", "common"]"#,
        r#"["candidate", "common"]"#,
        "",
    );
    let r = stopped_as_same_code(&repo);
    assert!(
        r.stderr
            .contains("both sides name the same source directory"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("listed under \"shared\""), "{}", r.stderr);
    assert!(!r.stderr.contains("is inside"), "{}", r.stderr);
}

#[test]
fn l2_a_candidate_nested_in_the_original_stops_and_says_what_to_do() {
    let repo = layout(
        "def shout(x):\n    return x.upper()\n",
        "",
        None,
        r#"{"id": "c", "fn": "shout", "args": ["a"]}"#,
    );
    std::fs::create_dir_all(repo.dir.join("original/newimpl")).unwrap();
    std::fs::write(
        repo.dir.join("original/newimpl/port.py"),
        "def shout(x):\n    return \"\".join(c.upper() for c in x)\n",
    )
    .unwrap();
    write_suite(&repo, r#"["original"]"#, r#"["original/newimpl"]"#, "");
    let r = stopped_as_same_code(&repo);
    assert!(
        r.stderr
            .contains("original/newimpl is inside the original side's directory"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("Put the two in sibling directories"),
        "{}",
        r.stderr
    );
}

#[test]
fn a_function_under_test_defined_in_a_shared_directory_stops_the_run() {
    let both = "from helper import clean\n";
    let repo = layout(
        both,
        &format!("{both}# the port\n"),
        Some(HELPER),
        r#"{"id": "c", "fn": "clean", "args": [" a "]}"#,
    );
    write_suite(
        &repo,
        r#"["original"]"#,
        r#"["candidate"]"#,
        r#", "shared": ["common"]"#,
    );
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    let reason = r.report["reasons"][0].as_str().unwrap();
    assert!(
        reason.contains("which the suite declares as shared"),
        "{reason}"
    );
}

#[test]
fn a_shared_directory_cannot_be_a_side_or_hold_a_copy_of_one() {
    // Declaring the original's own directory as shared.
    let repo = layout(ORIG_WITH_HELPER, PORT_WITH_HELPER, Some(HELPER), SHOUT);
    write_suite(
        &repo,
        r#"["original"]"#,
        r#"["candidate"]"#,
        r#", "shared": ["common", "original"]"#,
    );
    let r = stopped_as_same_code(&repo);
    assert!(
        r.stderr
            .contains("contains, the original side's whole source directory"),
        "{}",
        r.stderr
    );

    // A copy of the original placed in the shared directory.
    let repo = layout(ORIG_WITH_HELPER, PORT_WITH_HELPER, Some(HELPER), SHOUT);
    std::fs::copy(
        repo.dir.join("original/orig.py"),
        repo.dir.join("common/impl.py"),
    )
    .unwrap();
    write_suite(
        &repo,
        r#"["original"]"#,
        r#"["candidate"]"#,
        r#", "shared": ["common"]"#,
    );
    let r = stopped_as_same_code(&repo);
    assert!(
        r.stderr
            .contains("in a shared directory has the same content as"),
        "{}",
        r.stderr
    );
}

const REACH: &str = "import os, sys\n\nORIGINAL = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), \"original\")\n\n\n";

#[test]
fn n3_importing_the_original_and_hiding_the_import_stops_the_run() {
    let port = format!(
        "{REACH}def tolerance():\n    sys.path.insert(0, ORIGINAL)\n    import orig\n    result = orig.tolerance()\n    del sys.modules[\"orig\"]\n    sys.path.remove(ORIGINAL)\n    return result\n"
    );
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    std::fs::write(repo.dir.join("candidate/port.py"), port).unwrap();
    let r = stopped_as_same_code(&repo);
    assert!(
        r.stderr.contains("the candidate side opened"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("original/orig.py"), "{}", r.stderr);
}

#[test]
fn n4_executing_the_originals_source_stops_the_run() {
    for body in [
        "    scope = {}\n    exec(open(os.path.join(ORIGINAL, \"orig.py\")).read(), scope)\n    return scope[\"tolerance\"]()\n",
        "    import runpy\n    return runpy.run_path(os.path.join(ORIGINAL, \"orig.py\"))[\"tolerance\"]()\n",
        "    fd = os.open(os.path.join(ORIGINAL, \"orig.py\"), os.O_RDONLY)\n    scope = {}\n    exec(os.read(fd, 1 << 20), scope)\n    os.close(fd)\n    return scope[\"tolerance\"]()\n",
    ] {
        let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
        std::fs::write(
            repo.dir.join("candidate/port.py"),
            format!("{REACH}def tolerance():\n{body}"),
        )
        .unwrap();
        let r = stopped_as_same_code(&repo);
        assert!(
            r.stderr.contains("the candidate side opened"),
            "{body}: {}",
            r.stderr
        );
    }
}

#[test]
fn a_bundled_copy_of_the_original_behind_a_wrapper_stops_the_run() {
    // The Python-only form of N8: the port's own function hands the call to
    // an unchanged copy of the original that it ships under another name.
    let repo = Repo::new(r#"{"id": "c", "fn": "tolerance"}"#);
    std::fs::copy(
        repo.dir.join("original/orig.py"),
        repo.dir.join("candidate/_impl.py"),
    )
    .unwrap();
    std::fs::write(
        repo.dir.join("candidate/port.py"),
        "import _impl\n\n\ndef tolerance():\n    return _impl.tolerance()\n",
    )
    .unwrap();
    let r = stopped_as_same_code(&repo);
    assert!(r.stderr.contains("has the same content as"), "{}", r.stderr);
    assert!(r.stderr.contains("candidate/_impl.py"), "{}", r.stderr);
}

#[test]
fn starting_a_process_is_recorded_and_does_not_fail_the_run() {
    let orig = "def answer():\n    return 42\n";
    let port = "import subprocess, sys\n\n\ndef answer():\n    out = subprocess.run([sys.executable, \"-c\", \"print(42)\"], capture_output=True)\n    return int(out.stdout)\n";
    let repo = layout(orig, port, None, r#"{"id": "c", "fn": "answer"}"#);
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let started = &r.report["checks"]["processes_started"];
    assert_eq!(started["original"]["count"], 0);
    assert!(started["candidate"]["count"].as_u64().unwrap() >= 1);
    assert!(
        started["candidate"]["first"][0]
            .as_str()
            .unwrap()
            .contains("subprocess.Popen")
    );
    assert!(
        r.stdout.contains("processes started by the candidate side"),
        "{}",
        r.stdout
    );
}

// ---- T001-E: false stops, and exception base classes. ----

/// Makes a virtualenv at `.venv` inside the repository, ignored by git, the
/// way most projects have one. Returns its interpreter and site-packages.
fn venv_in(repo: &Repo) -> (String, PathBuf) {
    std::fs::write(
        repo.dir.join(".gitignore"),
        "out/\n__pycache__/\n.venv/\nlocal_*\n",
    )
    .unwrap();
    let made = Command::new(base_python())
        .args(["-m", "venv", "--without-pip"])
        .arg(repo.dir.join(".venv"))
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let python = repo.dir.join(".venv/bin/python");
    let site = Command::new(&python)
        .args([
            "-c",
            "import sysconfig; print(sysconfig.get_paths()['purelib'])",
        ])
        .output()
        .unwrap();
    let site = PathBuf::from(String::from_utf8_lossy(&site.stdout).trim());
    assert!(
        site.starts_with(repo.dir.canonicalize().unwrap()) || site.starts_with(&repo.dir),
        "{}",
        site.display()
    );
    (python.to_string_lossy().into_owned(), site)
}

const USES_PACKAGE: &str =
    "import thirdparty_pkg\n\n\ndef shout(x):\n    return thirdparty_pkg.clean(x).upper()\n";
const PORT_USES_PACKAGE: &str = "import thirdparty_pkg\n\n\ndef shout(x):\n    return \"\".join(c.upper() for c in thirdparty_pkg.clean(x))\n";

#[test]
fn a_virtualenv_inside_the_repository_does_not_stop_the_run() {
    // The auditor's P4. An installed package is third-party code, not
    // project code that should have been committed.
    let repo = layout(USES_PACKAGE, PORT_USES_PACKAGE, None, SHOUT);
    let (python, site) = venv_in(&repo);
    std::fs::create_dir_all(site.join("thirdparty_pkg")).unwrap();
    std::fs::write(site.join("thirdparty_pkg/__init__.py"), HELPER).unwrap();
    repo.commit();
    let r = repo.run_python(&python, &[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass");
    assert_eq!(r.report["git"]["dirty"], false);
    assert_eq!(
        r.report["git"]["used_but_not_tracked"],
        serde_json::json!([])
    );
    for side in ["original", "candidate"] {
        let s = &r.report["sides"][side];
        // Some distributions make every interpreter load a file of their
        // own (Debian: /etc/python3.x/sitecustomize.py). The harness lists
        // it, rightly; it is not what this test is about.
        let (theirs, third) = split_distribution_files(s);
        assert_eq!(third.len(), 1, "{third:?}");
        let third = &third[0];
        assert_eq!(third["path"], "site-packages:thirdparty_pkg/__init__.py");
        assert_eq!(third["sha256"].as_str().unwrap().len(), 64);
        assert_eq!(s["external_files"], 1 + theirs);
        // Not listed among the project's own files.
        assert_eq!(s["files"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn with_a_virtualenv_an_untracked_project_file_still_stops_the_run() {
    let port = format!("{PORT_USES_PACKAGE}\nimport local_patch\n");
    let repo = layout(USES_PACKAGE, &port, None, SHOUT);
    let (python, site) = venv_in(&repo);
    std::fs::create_dir_all(site.join("thirdparty_pkg")).unwrap();
    std::fs::write(site.join("thirdparty_pkg/__init__.py"), HELPER).unwrap();
    repo.commit();
    std::fs::write(
        repo.dir.join("candidate/local_patch.py"),
        "PATCHED = True\n",
    )
    .unwrap();
    let r = repo.run_python(&python, &[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    let reason = r.report["reasons"][0].as_str().unwrap();
    assert!(
        reason.contains("candidate/local_patch.py (imported by the candidate side)"),
        "{reason}"
    );
    assert!(!reason.contains("thirdparty_pkg"), "{reason}");
}

#[test]
fn an_editable_installed_project_package_is_project_code() {
    // Editable installs leave the package in the source tree and put a
    // pointer to it in site-packages. Its files are the project's, so they
    // must be in the commit like any other.
    let orig = "import projlib\n\n\ndef shout(x):\n    return projlib.clean(x).upper()\n";
    let port = "import projlib\n\n\ndef shout(x):\n    return \"\".join(c.upper() for c in projlib.clean(x))\n";
    let repo = layout(orig, port, None, SHOUT);
    let (python, site) = venv_in(&repo);
    let libs = repo.dir.canonicalize().unwrap().join("libs");
    std::fs::create_dir_all(libs.join("projlib")).unwrap();
    std::fs::write(
        libs.join("projlib/__init__.py"),
        format!("from .local_extra import *\n{HELPER}"),
    )
    .unwrap();
    std::fs::write(libs.join("projlib/local_extra.py"), "EXTRA = 1\n").unwrap();
    std::fs::write(
        site.join("__editable__.projlib.pth"),
        format!("{}\n", libs.display()),
    )
    .unwrap();
    repo.commit();

    // local_extra.py matches the ignore pattern, so it is not in the commit.
    let r = repo.run_python(&python, &[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    let reason = r.report["reasons"][0].as_str().unwrap();
    assert!(reason.contains("libs/projlib/local_extra.py"), "{reason}");
    assert!(!reason.contains("libs/projlib/__init__.py"), "{reason}");

    // Once every file of the package is tracked, the run passes, and the
    // package is listed with the project's files, not as third-party.
    git(&repo.dir, &["add", "-f", "libs/projlib/local_extra.py"]);
    repo.commit();
    let r = repo.run_python(&python, &[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let side = &r.report["sides"]["original"];
    let files: Vec<&str> = side["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(
        files.contains(&"libs/projlib/__init__.py")
            && files.contains(&"libs/projlib/local_extra.py"),
        "{files:?}"
    );
    assert_eq!(side["external_files"], split_distribution_files(side).0);
}

/// The files a side loaded from outside the repository, split into those
/// the interpreter imports by itself at start-up (Debian's
/// `sitecustomize.py`; counted) and the rest.
fn split_distribution_files(side: &J) -> (usize, Vec<J>) {
    let all = side["third_party_files"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let (theirs, rest): (Vec<J>, Vec<J>) = all
        .into_iter()
        .partition(|f| f["kind"] == "interpreter_start_up");
    (theirs.len(), rest)
}

#[test]
fn a_side_cannot_live_in_site_packages() {
    let repo = layout(USES_PACKAGE, PORT_USES_PACKAGE, None, SHOUT);
    let (python, site) = venv_in(&repo);
    std::fs::create_dir_all(site.join("thirdparty_pkg")).unwrap();
    std::fs::write(site.join("thirdparty_pkg/__init__.py"), HELPER).unwrap();
    std::fs::copy(repo.dir.join("candidate/port.py"), site.join("port.py")).unwrap();
    let site = site.canonicalize().unwrap();
    let rel = site
        .strip_prefix(repo.dir.canonicalize().unwrap())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    write_suite(&repo, r#"["original"]"#, &format!("[\"{rel}\"]"), "");
    repo.commit();
    let r = repo.run_python(&python, &[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr
            .contains("inside the interpreter's installed-package directory"),
        "{}",
        r.stderr
    );
}

#[test]
fn p7_own_exception_classes_with_different_bases_are_a_difference() {
    let module = |base: &str| {
        format!(
            "class StoreError({base}):\n    pass\n\n\ndef boom(fail=True):\n    if fail:\n        raise StoreError(\"corrupt segment\")\n    return 1\n"
        )
    };
    let cases = "{\"id\": \"ok\", \"fn\": \"boom\", \"args\": [false]}\n{\"id\": \"c\", \"fn\": \"boom\", \"expect_raise\": true}";
    let repo = layout(&module("ValueError"), &module("Exception"), None, cases);
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    let d = &r.case(1)["first_difference"];
    assert_eq!(d["kind"], "exception_bases");
    assert_eq!(
        d["original"]["repr"],
        "StoreError < builtins.ValueError < builtins.Exception < builtins.BaseException"
    );
    assert_eq!(
        d["candidate"]["repr"],
        "StoreError < builtins.Exception < builtins.BaseException"
    );

    // The same bases on both sides, through an intermediate class of each
    // side's own: identical, and the note says what was compared.
    let module = |tail: &str| {
        format!(
            "class LedgerError(ValueError):\n    pass\n\n\nclass StoreError(LedgerError):\n    pass\n\n\ndef boom(fail=True):\n    if fail:\n        raise StoreError(\"corrupt segment\")\n    return 1\n{tail}"
        )
    };
    let repo = layout(&module(""), &module("# the port\n"), None, cases);
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let note = r.case(1)["notes"][0].as_str().unwrap();
    assert!(note.contains("class name and base classes"), "{note}");
    assert!(
        note.contains("StoreError < LedgerError < builtins.ValueError"),
        "{note}"
    );
}

// ---- T001-F: where the work is done, and reusing a package's helpers. ----

/// Writes files (path, text) into the repository.
fn write_files(repo: &Repo, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = repo.dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn package_suite(repo: &Repo, shared: &str) {
    let text = format!(
        r#"{{"schema": "carcinify/diff/suite/1",
  "original": {{"kind": "python", "path": ["original"], "module": "pkg.store"}},
  "candidate": {{"kind": "python", "path": ["candidate"], "module": "pkg_new.store"}},
  "cases": ["cases.jsonl"], "shared": {shared}}}"#
    );
    std::fs::write(repo.dir.join("suite.json"), text).unwrap();
}

/// The auditor's H2: the original is a package, one module of it is being
/// ported, and the port keeps using the package's helper module.
fn package_repo(init: &str, port_store: &str) -> Repo {
    let repo = Repo::new(r#"{"id": "c", "fn": "shout", "args": [" abc "]}"#);
    write_files(
        &repo,
        &[
            ("original/pkg/__init__.py", init),
            ("original/pkg/util.py", HELPER),
            (
                "original/pkg/store.py",
                "from .util import clean\n\n\ndef shout(x):\n    return clean(x).upper()\n",
            ),
            ("candidate/pkg_new/__init__.py", ""),
            ("candidate/pkg_new/store.py", port_store),
        ],
    );
    repo
}

const PORT_STORE: &str = "from pkg.util import clean\n\n\ndef shout(x):\n    return \"\".join(c.upper() for c in clean(x))\n";
const PKG_SHARED: &str = r#"["original/pkg/__init__.py", "original/pkg/util.py"]"#;

#[test]
fn h2_a_port_of_one_module_can_reuse_the_packages_helper() {
    let repo = package_repo("", PORT_STORE);
    package_suite(&repo, PKG_SHARED);
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["verdict"], "pass");
    let shared = r.report["checks"]["shared"].as_array().unwrap();
    let util = shared
        .iter()
        .find(|e| e["path"] == "original/pkg/util.py")
        .unwrap();
    assert_eq!(util["inside"], "original");
    assert_eq!(util["files_loaded"][0]["loaded_by"], "both");
    let f = &r.report["checks"]["functions"][0];
    assert_eq!(f["original_defined_in"][0]["path"], "original/pkg/store.py");
    assert_eq!(
        f["candidate_defined_in"][0]["path"],
        "candidate/pkg_new/store.py"
    );
    // The helper's call is counted as shared on both sides.
    assert_eq!(f["original_python_calls"]["shared"], 1);
    assert_eq!(f["candidate_python_calls"]["shared"], 1);
}

#[test]
fn h2_the_rest_of_the_package_stays_off_limits() {
    // Without the declaration the helper cannot be reached at all.
    let repo = package_repo("", PORT_STORE);
    package_suite(&repo, "[]");
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);

    // With it, the module being ported is still out of reach.
    let reaching = "from pkg.util import clean\nimport pkg.store\n\n\ndef shout(x):\n    return pkg.store.shout(x)\n";
    let repo = package_repo("", reaching);
    package_suite(&repo, PKG_SHARED);
    let r = stopped_as_same_code(&repo);
    assert!(r.stderr.contains("original/pkg/store.py"), "{}", r.stderr);

    // A package whose __init__ imports the module being ported cannot be
    // imported by the port without loading the original implementation.
    let repo = package_repo("from .store import shout\n", PORT_STORE);
    package_suite(&repo, PKG_SHARED);
    let r = stopped_as_same_code(&repo);
    assert!(r.stderr.contains("original/pkg/store.py"), "{}", r.stderr);

    // Declaring the module being ported as shared does not get round it:
    // the function under test would then live in shared code.
    let repo = package_repo("", PORT_STORE);
    package_suite(&repo, r#"["original/pkg"]"#);
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr.contains("which the suite declares as shared"),
        "{}",
        r.stderr
    );
}

#[test]
fn h3_a_data_file_in_the_originals_package_can_be_shared() {
    let reads = |module_dir: &str| {
        format!(
            "import json, os\n\nTABLE = os.path.join({module_dir}, \"table.json\")\n\n\ndef shout(x):\n    with open(TABLE) as f:\n        return json.load(f)[\"prefix\"] + x.strip().upper()\n"
        )
    };
    let make = |shared: &str| {
        let repo = Repo::new(r#"{"id": "c", "fn": "shout", "args": [" abc "]}"#);
        write_files(
            &repo,
            &[
                ("original/pkg/__init__.py", ""),
                ("original/pkg/table.json", "{\"prefix\": \">\"}\n"),
                (
                    "original/pkg/store.py",
                    &reads("os.path.dirname(os.path.abspath(__file__))"),
                ),
                ("candidate/pkg_new/__init__.py", ""),
                (
                    "candidate/pkg_new/store.py",
                    &reads(
                        "os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), \"original\", \"pkg\")",
                    ),
                ),
            ],
        );
        package_suite(&repo, shared);
        repo.commit();
        repo
    };
    let r = make("[]").run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr.contains("the candidate side opened"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("original/pkg/table.json"), "{}", r.stderr);

    let r = make(r#"["original/pkg/table.json"]"#).run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(
        r.report["checks"]["shared"][0]["path"],
        "original/pkg/table.json"
    );
}

#[test]
fn h1_a_file_kept_identical_on_both_sides_can_be_shared() {
    let consts = "LIMIT = 3\n";
    let orig = "from consts import LIMIT\n\n\ndef cut(x):\n    return x[:LIMIT]\n";
    let port = "from consts import LIMIT\n\n\ndef cut(x):\n    return \"\".join(list(x)[:LIMIT])\n";
    let make = |extra: &str| {
        let repo = layout(
            orig,
            port,
            None,
            r#"{"id": "c", "fn": "cut", "args": ["abcdef"]}"#,
        );
        write_files(
            &repo,
            &[
                ("original/consts.py", consts),
                ("candidate/consts.py", consts),
            ],
        );
        write_suite(&repo, r#"["original"]"#, r#"["candidate"]"#, extra);
        repo.commit();
        repo
    };
    let r = make("").run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("has the same content as"), "{}", r.stderr);
    assert!(
        r.stderr.contains("list it under \"shared\""),
        "{}",
        r.stderr
    );

    // Declaring the file on one side covers the identical copy on the other.
    let r = make(r#", "shared": ["original/consts.py"]"#).run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let entry = &r.report["checks"]["shared"][0];
    assert_eq!(
        (&entry["path"], &entry["inside"]),
        (&J::from("original/consts.py"), &J::from("original"))
    );
    // Each side loaded its own copy. Shared is by content, so the
    // candidate's identical copy counts as the shared file: loaded by both,
    // with the copy named.
    assert_eq!(entry["files_loaded"][0]["loaded_by"], "both");
    assert_eq!(
        entry["files_loaded"][0]["copies"],
        serde_json::json!(["candidate/consts.py"])
    );
    assert!(
        r.stdout
            .contains("original/consts.py (also as the identical copy candidate/consts.py)"),
        "{}",
        r.stdout
    );
}

#[test]
fn the_output_says_where_each_functions_work_was_done() {
    // Both sides are one-line wrappers round the same shared function: the
    // run passes, and the counts show that neither side did the work itself.
    let wrapper = "from helper import clean\n\n\ndef tidy(x):\n    return clean(x)\n";
    let own_work = "def tidy(x):\n    return _strip(x)\n\n\ndef _strip(x):\n    return x.strip()\n";
    let repo = layout(
        wrapper,
        &format!("{wrapper}# the port\n"),
        Some(HELPER),
        r#"{"id": "c", "fn": "tidy", "args": [" a "]}"#,
    );
    write_suite(
        &repo,
        r#"["original"]"#,
        r#"["candidate"]"#,
        r#", "shared": ["common"]"#,
    );
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let f = &r.report["checks"]["functions"][0];
    for side in ["original", "candidate"] {
        assert_eq!(
            f[format!("{side}_python_calls")],
            serde_json::json!({"own": 1, "shared": 1, "other": 0})
        );
        assert_eq!(
            f[format!("{side}_work")],
            "python, 2 Python calls (1 own, 1 shared)"
        );
    }
    assert!(
        r.stdout
            .contains("    candidate: python, 2 Python calls (1 own, 1 shared)"),
        "{}",
        r.stdout
    );

    // A side that does the work in its own code shows only its own calls,
    // and calls into the standard library are counted apart.
    let stdlib = "import textwrap\n\n\ndef tidy(x):\n    return textwrap.dedent(x).strip()\n";
    let repo = layout(
        stdlib,
        own_work,
        None,
        r#"{"id": "c", "fn": "tidy", "args": [" a "]}"#,
    );
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let f = &r.report["checks"]["functions"][0];
    assert_eq!(f["candidate_work"], "python, 2 Python calls (2 own)");
    assert_eq!(f["original_python_calls"]["own"], 1);
    assert!(f["original_python_calls"]["other"].as_u64().unwrap() >= 1);
    assert!(
        f["original_work"]
            .as_str()
            .unwrap()
            .contains("in other packages")
    );

    // Counting can be switched off, and the output says it was.
    let r = repo.run(&["--no-call-counts"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let f = &r.report["checks"]["functions"][0];
    assert_eq!(f["candidate_work"], "python, Python calls not counted");
    assert!(f.get("candidate_python_calls").is_none());
}

#[test]
fn counting_calls_does_not_change_what_a_function_returns() {
    // Results that depend on the interpreter's own state while the function
    // runs: recursion right up to the limit, and a profile function of the
    // code's own. Each must come out the same with and without counting.
    let module = |tail: &str| {
        format!(
            "import sys\n\n\ndef depth(n):\n    return 0 if n == 0 else 1 + depth(n - 1)\n\n\ndef deep():\n    sys.setrecursionlimit(400)\n    try:\n        return depth(300)\n    finally:\n        sys.setrecursionlimit(1000)\n\n\ndef own_profile():\n    seen = []\n    sys.setprofile(lambda frame, event, arg: seen.append(event))\n    depth(3)\n    sys.setprofile(None)\n    return len([e for e in seen if e == \"call\"])\n{tail}"
        )
    };
    let cases =
        "{\"id\": \"deep\", \"fn\": \"deep\"}\n{\"id\": \"profile\", \"fn\": \"own_profile\"}";
    let repo = layout(&module(""), &module("# the port\n"), None, cases);
    repo.commit();
    let counted = repo.run(&[]);
    let plain = repo.run(&["--no-call-counts"]);
    assert_eq!(counted.code, 0, "{}{}", counted.stdout, counted.stderr);
    assert_eq!(plain.code, 0, "{}{}", plain.stdout, plain.stderr);
    for i in 0..2 {
        assert_eq!(
            counted.case(i)["original_sha256"],
            plain.case(i)["original_sha256"],
            "case {i}"
        );
    }
}

// ---- T001-G: what runs is what is on disk now. ----

#[test]
fn stale_bytecode_from_an_earlier_run_is_not_executed() {
    // RUST-025. Python trusts a cached bytecode file when the source's
    // recorded time and size still match. The port is compiled once as
    // `return 1`, then changed to `return 2` with the same size and time.
    // The original returns 1, so the run must fail; reading the stale cache
    // would make it pass.
    let repo = layout(
        "def answer():\n    return 1\n",
        "def answer():\n    return 1\n",
        None,
        r#"{"id": "c", "fn": "answer"}"#,
    );
    let port = repo.dir.join("candidate/port.py");
    let compiled = Command::new(base_python())
        .args([
            "-c",
            "import py_compile, sys; print(py_compile.compile(sys.argv[1], doraise=True))",
        ])
        .arg(&port)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let cache = PathBuf::from(String::from_utf8_lossy(&compiled.stdout).trim());
    assert!(cache.is_file(), "{}", cache.display());
    let old_time = std::fs::metadata(&port).unwrap().modified().unwrap();
    std::fs::write(&port, "def answer():\n    return 2\n").unwrap();
    let file = std::fs::File::options().write(true).open(&port).unwrap();
    file.set_modified(old_time).unwrap();
    drop(file);
    repo.commit();

    let r = repo.run(&[]);
    assert_eq!(
        r.code, 1,
        "stale bytecode was run: {}{}",
        r.stdout, r.stderr
    );
    let d = &r.case(0)["first_difference"];
    assert_eq!(
        (&d["original"]["repr"], &d["candidate"]["repr"]),
        (&J::from("1"), &J::from("2"))
    );
    let _ = std::fs::remove_file(cache);
}

#[test]
fn a_run_leaves_no_bytecode_in_the_repository() {
    let repo = Repo::new(r#"{"id": "c", "fn": "echo", "args": [1]}"#);
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    for side in ["original", "candidate"] {
        assert!(
            !repo.dir.join(side).join("__pycache__").exists(),
            "{side}/__pycache__ was written"
        );
    }
}

// ---- T001-C: backlog. ----

#[test]
fn e6_a_forged_reply_is_not_taken_for_an_answer() {
    // RUST-006. The port's function writes a well-formed "returned" line to
    // every pipe it can find, hoping one is the protocol channel. It used to
    // be taken as the answer, and the real answer as the next case's.
    let orig = "def forge():\n    return 1\n\n\ndef neg_zero():\n    return -0.0\n";
    let port = "import os, stat\n\n\ndef forge():\n    line = b'{\"status\": \"returned\", \"value\": {\"t\": \"float\", \"bits\": \"8000000000000000\"}}\\n'\n    for fd in range(3, 16):\n        try:\n            if stat.S_ISFIFO(os.fstat(fd).st_mode):\n                os.write(fd, line)\n        except OSError:\n            pass\n    return 1\n\n\ndef neg_zero():\n    return 0.0\n";
    let repo = layout(
        orig,
        port,
        None,
        "{\"id\": \"a\", \"fn\": \"forge\"}\n{\"id\": \"b\", \"fn\": \"neg_zero\"}\n",
    );
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.case(0)["result"], "error");
    assert_eq!(error_kind(&r, 0), "protocol");
    assert!(
        r.case(0)["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("does not carry the request's id")
    );
    // The next case is judged on its own answer: -0.0 against 0.0.
    assert_eq!(r.case(1)["result"], "different");
    assert_eq!(
        r.case(1)["first_difference"]["candidate"]["bits"],
        "0x0000000000000000"
    );
    assert_eq!(r.report["sides"]["candidate"]["restarts"], 1);
}

#[test]
fn c3_code_that_switches_the_call_counter_off_is_reported() {
    // RUST-023. The port removes the profile function and then makes six
    // calls nobody counts. The row used to read "1 Python call (1 own)".
    let orig = "def helper():\n    return 1\n\n\ndef work():\n    return sum(helper() for _ in range(6))\n";
    let port = "import sys\n\n\ndef helper():\n    return 1\n\n\ndef work():\n    sys.setprofile(None)\n    total = 0\n    for _ in range(6):\n        total += helper()\n    return total\n";
    let repo = layout(
        orig,
        port,
        None,
        "{\"id\": \"a\", \"fn\": \"work\"}\n{\"id\": \"b\", \"fn\": \"work\"}\n",
    );
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let f = &r.report["checks"]["functions"][0];
    assert_eq!(f["candidate_python_calls_uncounted_cases"], 2);
    assert!(f.get("candidate_python_calls").is_none());
    let work = f["candidate_work"].as_str().unwrap();
    assert!(
        work.starts_with("python, Python calls unknown; not counted in 2 case(s)"),
        "{work}"
    );
    assert!(
        work.contains("replaced or removed the profile function"),
        "{work}"
    );
    // The original was counted in full: itself, the generator, six helpers.
    assert!(f.get("original_python_calls_uncounted_cases").is_none());
    assert_eq!(f["original_python_calls"]["own"], 2 * (1 + 7 + 6));
}

#[test]
fn the_timeout_message_names_the_counting_flag() {
    // RUST-024.
    let repo = Repo::new("{\"id\": \"hangs\", \"fn\": \"hang\"}\n");
    let r = repo.run(&["--timeout-secs", "20"]);
    assert_eq!(r.code, 1);
    assert!(
        r.case(0)["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("--no-call-counts")
    );
}

#[test]
fn p3_a_file_edited_under_assume_unchanged_stops_the_run() {
    // RUST-015. The port is edited and marked so that `git status` stays
    // silent. The run used to pass with `dirty: false` and a hash that was
    // not the committed file's.
    for flag in ["--assume-unchanged", "--skip-worktree"] {
        let repo = Repo::new(r#"{"id": "c", "fn": "echo", "args": [1]}"#);
        let port = repo.dir.join("candidate/port.py");
        let text = std::fs::read_to_string(&port).unwrap();
        git(&repo.dir, &["update-index", flag, "candidate/port.py"]);
        std::fs::write(&port, format!("{text}\n# a local edit\n")).unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo.dir)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert!(status.stdout.is_empty(), "git status should see nothing");

        let r = repo.run(&[]);
        assert_eq!(r.code, 2, "{flag}: {}{}", r.stdout, r.stderr);
        assert!(
            r.stderr
                .contains("candidate/port.py (differs from the commit; hidden from git status"),
            "{}",
            r.stderr
        );
        let r = repo.run(&["--allow-dirty"]);
        assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
        assert_eq!(r.report["git"]["dirty"], true);

        // The same mark on a file that still matches the commit is harmless.
        std::fs::write(&port, text).unwrap();
        let r = repo.run(&[]);
        assert_eq!(r.code, 0, "{flag}: {}{}", r.stdout, r.stderr);
        assert_eq!(r.report["git"]["dirty"], false);
    }
}

#[test]
fn p5_code_in_a_submodule_is_tracked_code() {
    // RUST-017. The original lives in a submodule, committed, tree clean.
    // Its file used to be reported as not tracked.
    let repo = Repo::new(r#"{"id": "c", "fn": "echo", "args": [1]}"#);
    let library = Repo::new("");
    git(&repo.dir, &["rm", "-q", "-r", "original"]);
    let source = library.dir.to_string_lossy().into_owned();
    git(
        &repo.dir,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &source,
            "vendor",
        ],
    );
    write_suite(&repo, r#"["vendor/original"]"#, r#"["candidate"]"#, "");
    repo.commit();

    let r = repo.run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(r.report["git"]["dirty"], false);
    assert_eq!(
        r.report["git"]["used_but_not_tracked"],
        serde_json::json!([])
    );
    let sub = &r.report["git"]["submodules"][0];
    assert_eq!(sub["path"], "vendor");
    assert_eq!(sub["commit"].as_str().unwrap().len(), 40);
    assert_eq!(
        r.report["sides"]["original"]["files"][0]["path"],
        "vendor/original/orig.py"
    );

    // A change inside the submodule makes the tree dirty, as anywhere else.
    let orig = repo.dir.join("vendor/original/orig.py");
    let text = std::fs::read_to_string(&orig).unwrap();
    std::fs::write(&orig, format!("{text}\n# edited\n")).unwrap();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("vendor"), "{}", r.stderr);
}

#[test]
fn k1_a_copy_of_a_shared_file_is_shared_for_every_purpose() {
    // RUST-022. The original's impl.py does the real work and is declared
    // shared. The candidate carries a byte-identical impl.py and wraps it.
    // The copy was exempt from the content rule as shared, and then its
    // calls were counted as the candidate's own work.
    let implementation =
        "def work(x):\n    return _inner(x) + 1\n\n\ndef _inner(x):\n    return x * 2\n";
    let wrapper = "import impl\n\n\ndef f(x):\n    return impl.work(x)\n";
    let make = |cases: &str| {
        let repo = layout(wrapper, &format!("{wrapper}# the port\n"), None, cases);
        write_files(
            &repo,
            &[
                ("original/impl.py", implementation),
                ("candidate/impl.py", implementation),
            ],
        );
        write_suite(
            &repo,
            r#"["original"]"#,
            r#"["candidate"]"#,
            r#", "shared": ["original/impl.py"]"#,
        );
        repo.commit();
        repo
    };
    let r = make(r#"{"id": "c", "fn": "f", "args": [3]}"#).run(&[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let f = &r.report["checks"]["functions"][0];
    for side in ["original", "candidate"] {
        assert_eq!(
            f[format!("{side}_work")],
            "python, 3 Python calls (1 own, 2 shared)",
            "{side}"
        );
    }
    let entry = &r.report["checks"]["shared"][0]["files_loaded"][0];
    assert_eq!(entry["path"], "original/impl.py");
    assert_eq!(entry["loaded_by"], "both");
    assert_eq!(entry["copies"], serde_json::json!(["candidate/impl.py"]));
    assert!(r.stdout.contains("shared, run by both sides and not compared (original/impl.py): original/impl.py (also as the identical copy candidate/impl.py)"), "{}", r.stdout);
    assert!(
        !r.stdout.contains("loaded by the original side only"),
        "{}",
        r.stdout
    );

    // And a function under test may not live in the copy any more than in
    // the shared file itself.
    let repo = make(r#"{"id": "c", "fn": "work", "args": [3]}"#);
    for side in ["original/orig.py", "candidate/port.py"] {
        let path = repo.dir.join(side);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("from impl import work\n{text}")).unwrap();
    }
    repo.commit();
    let r = repo.run(&[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(
        r.stderr.contains("which the suite declares as shared"),
        "{}",
        r.stderr
    );
}
