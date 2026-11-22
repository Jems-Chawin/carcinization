//! End-to-end tests: the real binary on a small Python project in a git
//! repository made for the test. They need `git` and `python3`; each runs
//! the project's workload several times, so a test takes a second or two.

use serde_json::Value as J;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const APP: &str = "import time


def busy(n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total


def recorded(items, scale=1, *rest, label='x', **extra):
    items.append(scale)
    if len(items) < 3:
        return recorded(items, scale)
    return len(items)
_UNSET = object()
def with_sentinel(x, y=_UNSET, label=None):
    return x
def _total(xs, limit=10):
    return sum(xs[:limit])
def total(xs, limit=None):
    return _total(xs, len(xs) if limit is None else limit)
def scaled(x, factor=2):
    return x * factor
SETTINGS = {'scale': 1}
def lookup(key, table=SETTINGS):
    return table.get(key)
import functools
def _deco(fn):
    @functools.wraps(fn)
    def wrapper(*args, **kwargs):
        return fn(*args, **kwargs)
    return wrapper
@_deco
def wrapped(x, y=_UNSET):
    return x
@functools.lru_cache(maxsize=None)
def cached(x, y=_UNSET):
    return x
def busy_better(n):
    return n


def tiny(x):
    return x + 1


def many_small(n):
    total = 0
    for i in range(n):
        total = tiny(total)
    return total


def wait(seconds):
    time.sleep(seconds)
    return busy(1000)


def broken():
    raise RuntimeError('the input file is missing')


def two_halves(n):
    return busy(n) + other_half(n)


def other_half(n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total
";

const WORKLOAD: &str = "import app
from app import recorded


def run(n):
    return app.busy(n)


def run_better(n):
    return app.busy_better(n)


def run_small(n):
    return app.many_small(n)


def run_wait(seconds):
    return app.wait(seconds)


def run_broken():
    return app.broken()


def run_nothing():
    return 0


def run_total():
    app._total(list(range(12)))
    return 0


def run_scaled():
    app.scaled.__defaults__ = (3,)
    app.scaled(5)
    return 0


def run_lookup():
    app.SETTINGS['scale'] = 99
    app.lookup('scale', {'scale': 99})
    return 0


def run_decorated():
    app.wrapped(1)
    app.wrapped(2, 3)
    app.cached(1)
    return 0


def run_sentinel():
    from app import with_sentinel
    with_sentinel(1)
    with_sentinel(1, label=None)
    with_sentinel(2, label='k')
    return 0


def run_recorded_in_a_thread():
    import threading
    worker = threading.Thread(target=recorded, args=([7],))
    worker.start()
    worker.join()
    return 0


def run_recorded():
    recorded([1, 2])
    recorded([1, 2])
    recorded([3], scale=2, label='y')
    recorded([3], 2, 5, 6, flag=True)
    recorded([object()])
    return 0


def run_halves(n):
    return app.two_halves(n)


def run_looking_for_the_token(n):
    import sys
    if len(sys.argv) != 1 or any('MEASURED' in a for a in sys.argv):
        raise RuntimeError('the token is in sys.argv: %r' % (sys.argv,))
    return app.busy(n)


def run_printing(n):
    print('MEASURED-000000000000000000000000 {\"runs\": []}')
    print('MEASURED {\"wall_seconds\": 0}')
    return app.busy(n)
";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A committed repository with the small project and a declaration made of
/// the given parts (each a JSON fragment; empty leaves the part out).
fn repo(workload: &str, candidate: &str, algorithm: &str, decision: &str) -> Repo {
    let dir = std::env::temp_dir().join(format!(
        "carcinify-decide-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("proj")).unwrap();
    std::fs::write(dir.join("proj/app.py"), APP).unwrap();
    std::fs::write(dir.join("proj/workload.py"), WORKLOAD).unwrap();
    std::fs::write(dir.join(".gitignore"), "out/\n__pycache__/\n").unwrap();
    let mut parts = vec![
        r#""schema": "carcinify/decide/declaration/2""#.to_string(),
        r#""project_root": "proj""#.to_string(),
        format!(
            r#""workload": {{"description": "a test workload", "file": "proj/workload.py", {workload}, "repeat": 3}}"#
        ),
    ];
    for (key, value) in [
        ("candidate", candidate),
        ("language_or_algorithm", algorithm),
        ("decision", decision),
    ] {
        if !value.is_empty() {
            parts.push(format!(r#""{key}": {value}"#));
        }
    }
    std::fs::write(dir.join("decide.json"), format!("{{{}}}", parts.join(", "))).unwrap();
    let repo = Repo(dir.canonicalize().unwrap());
    git(&repo.0, &["init", "-q"]);
    git(&repo.0, &["add", "-A"]);
    git(&repo.0, &["commit", "-q", "-m", "fixture"]);
    repo
}

/// One measurement at a time. These tests time code; run side by side they
/// take the CPU from each other, and time lost that way reads as waiting.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs the tool. A report that says the machine was busy is not what any
/// test but two are about, so the run is made again, a few times. Every
/// such repeat is printed, and written to the file named in
/// `CARCINIFY_DECIDE_ATTEMPTS` if that is set, so that a tool which began to
/// say `unreliable` on a quiet machine would not go unseen.
fn run(repo: &Repo) -> (i32, J, String) {
    run_with(repo, None, false)
}

fn run_with(repo: &Repo, python: Option<&str>, short_on_purpose: bool) -> (i32, J, String) {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    for attempt in 0.. {
        let result = run_once(repo, python);
        if result.1["verdict"] != "unreliable" {
            // A test workload that is too short tests nothing, and a
            // faster interpreter makes one too short: say so here, where
            // the cause is known, and not in some assertion further on.
            let short = result.1["reasons"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|r| {
                    r.as_str()
                        .is_some_and(|r| r.contains("there is too little to split into shares"))
                });
            assert!(
                short_on_purpose || !short,
                "THE TEST WORKLOAD IS TOO SHORT ON THIS MACHINE: give it more to do in the test, do not lower the tool's minimum\n{}",
                result.2
            );
            return result;
        }
        let name = std::thread::current().name().unwrap_or("?").to_string();
        eprintln!(
            "{name}: attempt {} was unreliable (the machine was busy)",
            attempt + 1
        );
        if let Ok(log) = std::env::var("CARCINIFY_DECIDE_ATTEMPTS") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
            {
                let _ = writeln!(f, "{name}\tattempt {} unreliable", attempt + 1);
            }
        }
        assert!(
            attempt < 5,
            "the machine is too busy to run this test: six runs in a row were unreliable\n{}",
            result.2
        );
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    unreachable!()
}

const MARKDOWN_FOLLOWS: &str = "\n==== report.md ====\n";

fn run_once(repo: &Repo, python: Option<&str>) -> (i32, J, String) {
    let report = repo.0.join("out/report.json");
    // A tool that dies before it writes must not be read as the attempt before.
    let _ = std::fs::remove_file(&report);
    let _ = std::fs::remove_file(repo.0.join("out/report.md"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_carcinify"));
    command.arg("decide");
    if let Some(python) = python {
        command.arg("--python").arg(python);
    }
    let out = command
        .arg(repo.0.join("decide.json"))
        .arg("--report")
        .arg(&report)
        .arg("--markdown")
        .arg(repo.0.join("out/report.md"))
        .output()
        .unwrap();
    let json =
        std::fs::read_to_string(&report).map_or(J::Null, |t| serde_json::from_str(&t).unwrap());
    let markdown = std::fs::read_to_string(repo.0.join("out/report.md")).unwrap_or_default();
    (
        out.status.code().unwrap(),
        json,
        format!(
            "{}{}{MARKDOWN_FOLLOWS}{markdown}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn all(report: &J, key: &str) -> String {
    report[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn has_flag(report: &J, kind: &str) -> bool {
    report["flag_kinds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == kind)
}

const BUSY: &str = r#""function": "run", "args": [8000000]"#;
const WAIT: &str = r#""function": "run_wait", "args": [0.4]"#;
const CANDIDATE: &str = r#"{"file": "app.py", "function": "busy"}"#;
const LANGUAGE: &str = r#"{"statement": "a plain loop over integers in the interpreter", "better_algorithm_in_python": "none_known"}"#;
const PORT: &str = r#"{"choice": "port", "goals": ["speed"], "reason": "the loop is the whole job", "need": {"statement": "the batch runs all day and holds up the next step", "runs_per_day": 20000}}"#;
const NO: &str = r#"{"choice": "do_not_port", "reason": "nobody waits for this"}"#;

#[test]
fn a_port_for_speed_of_code_that_waits_is_contradicted() {
    // Nearly all of the time is a sleep. The candidate holds it all, and
    // nearly none of it is Python.
    let candidate = r#"{"file": "app.py", "function": "wait"}"#;
    let (code, report, text) = run(&repo(WAIT, candidate, LANGUAGE, PORT));
    assert_eq!(code, 1, "{text}");
    assert_eq!(report["verdict"], "contradicted");
    assert_eq!(
        report["where_the_time_goes"]["classification"],
        "mostly_waiting"
    );
    assert!(
        report["where_the_time_goes"]["shares"]["waiting"]
            .as_f64()
            .unwrap()
            > 0.9,
        "{text}"
    );
    // With what it calls the candidate is the whole run...
    assert!(
        report["candidate"]["cumulative_share_of_the_profile"]
            .as_f64()
            .unwrap()
            > 0.9,
        "{text}"
    );
    // ...and its own Python is next to nothing of it.
    assert!(
        report["candidate"]["own_python_share"].as_f64().unwrap() < 0.05,
        "{text}"
    );
    let said = all(&report, "contradictions");
    assert!(
        said.contains("the decision is to port for speed, and the candidate's own Python is")
            && said.contains("the workload would be at most 1.0"),
        "{said}"
    );
    assert!(text.contains("CONTRADICTED"), "{text}");
    // The same measurements do not contradict leaving it alone.
    let (code, report, text) = run(&repo(WAIT, candidate, LANGUAGE, NO));
    assert_eq!(code, 0, "{text}");
    assert_eq!(report["verdict"], "consistent");
    // The report says what the verdict does not mean.
    assert!(
        report["verdict_means"]
            .as_str()
            .unwrap()
            .contains("It does not mean the decision is right")
    );
}

#[test]
fn the_projects_own_python_is_found_and_the_ceiling_is_printed() {
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, PORT));
    assert_eq!(code, 0, "{text}");
    assert_eq!(report["verdict"], "consistent");
    let g = &report["where_the_time_goes"];
    assert_eq!(g["classification"], "mostly_own_python");
    assert_eq!(g["own_python_functions"][0]["name"], "run");
    assert_eq!(g["own_python_functions"][1]["name"], "busy");
    let c = &report["candidate"];
    assert_eq!(
        (
            &c["calls"],
            &c["callers"],
            &c["boundary"]["entered_from_outside"]
        ),
        (&J::from(1), &J::from(1), &J::from(1))
    );
    assert_eq!(c["boundary"]["first_call"]["arguments"]["n"], "int");
    assert_eq!(c["boundary"]["first_call"]["returns"], "int");
    // What was run, on what, from which commit.
    let w = &report["workload"];
    assert_eq!(w["declared"]["args"], serde_json::json!([8000000]));
    assert_eq!(
        w["measured_with_nothing_attached"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(
        w["measured_with_nothing_attached"]["peak_memory_bytes"]
            .as_f64()
            .unwrap()
            > 1e6
    );
    assert_eq!(w["file_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(report["git"]["commit"].as_str().unwrap().len(), 40);
    assert!(report["machine"]["logical_cpus"].as_u64().unwrap() >= 1);
    assert!(
        text.contains("the project's own Python (mostly_own_python)"),
        "{text}"
    );
}

#[test]
fn the_same_workload_measured_twice_is_classified_the_same() {
    let r = repo(BUSY, CANDIDATE, LANGUAGE, PORT);
    let (_, first, _) = run(&r);
    let (_, second, _) = run(&r);
    let class = |r: &J| r["where_the_time_goes"]["classification"].clone();
    assert_eq!(class(&first), class(&second));
    let own = |r: &J| {
        r["where_the_time_goes"]["shares"]["own_python"]
            .as_f64()
            .unwrap()
    };
    // The shares move with what else the machine is doing.
    assert!(
        (own(&first) - own(&second)).abs() < 0.25,
        "{} {}",
        own(&first),
        own(&second)
    );
}

#[test]
fn a_missing_decision_or_answer_is_incomplete() {
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, ""));
    assert_eq!(code, 1, "{text}");
    assert_eq!(report["verdict"], "incomplete");
    assert!(
        all(&report, "reasons").contains("no decision is declared"),
        "{text}"
    );
    // The measurements are in the report all the same.
    assert_eq!(
        report["where_the_time_goes"]["classification"],
        "mostly_own_python"
    );

    let (code, report, text) = run(&repo(BUSY, CANDIDATE, "", PORT));
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "reasons").contains("is not answered (`language_or_algorithm`)"),
        "{text}"
    );

    let (code, report, text) = run(&repo(BUSY, "", LANGUAGE, PORT));
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "reasons").contains("the decision is to port and no candidate piece is named"),
        "{text}"
    );

    let thin = r#"{"choice": "port", "goals": ["speed"], "reason": "faster", "need": {"statement": "the batch runs all day", "runs_per_day": 20000}}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, thin));
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "reasons").contains("the decision's reason is \"faster\""),
        "{text}"
    );
}

#[test]
fn a_workload_that_fails_or_shows_nothing_gives_no_report_to_decide_on() {
    let (code, report, text) = run(&repo(
        r#""function": "run_broken""#,
        CANDIDATE,
        LANGUAGE,
        NO,
    ));
    assert_eq!(code, 2, "{text}");
    assert_eq!(report["verdict"], "error");
    assert!(
        text.contains("the workload failed in the `plain` run")
            && text.contains("RuntimeError: the input file is missing"),
        "{text}"
    );

    // A workload that does nothing: too short to split, and the candidate
    // was never called.
    let (code, report, text) = run_with(
        &repo(r#""function": "run_nothing""#, CANDIDATE, LANGUAGE, NO),
        None,
        true,
    );
    assert_eq!(code, 1, "{text}");
    assert_eq!(report["verdict"], "incomplete");
    let why = all(&report, "reasons");
    assert!(
        why.contains("there is too little to split into shares")
            && why.contains("the candidate `busy` in app.py was not called by the workload"),
        "{why}"
    );

    // One whose code is all outside the project root.
    let r = repo(BUSY, "", LANGUAGE, NO);
    std::fs::create_dir_all(r.0.join("empty")).unwrap();
    std::fs::write(r.0.join("empty/keep"), "").unwrap();
    let text = std::fs::read_to_string(r.0.join("decide.json"))
        .unwrap()
        .replace(r#""project_root": "proj""#, r#""project_root": "empty""#);
    std::fs::write(r.0.join("decide.json"), text).unwrap();
    git(&r.0, &["add", "-A"]);
    git(&r.0, &["commit", "-q", "-m", "another root"]);
    let (code, report, text) = run(&r);
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "reasons").contains("the profile saw none of the project's own Python"),
        "{text}"
    );
}

#[test]
fn a_dirty_tree_and_a_bad_declaration_stop_the_run() {
    let r = repo(BUSY, CANDIDATE, LANGUAGE, PORT);
    std::fs::write(r.0.join("proj/app.py"), format!("{APP}\n# edited\n")).unwrap();
    let (code, report, text) = run(&r);
    assert_eq!(code, 2, "{text}");
    assert_eq!(report["verdict"], "error");
    assert!(text.contains("uncommitted changes"), "{text}");

    // Numbers cannot be typed in: the declaration has no field for them.
    // The need takes a requirement, not a measurement.
    let typed = r#"{"choice": "port", "goals": ["speed"], "reason": "the loop is the whole job", "need": {"statement": "it must be faster", "runs_per_day": 5, "seconds_saved": 10}}"#;
    let (code, _, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, typed));
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("unknown field `seconds_saved`"), "{text}");
    let typed = r#"{"choice": "port", "goals": ["speed"], "reason": "the loop is the whole job", "own_python_share": 0.9}"#;
    let (code, _, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, typed));
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("unknown field `own_python_share`"), "{text}");
    // Nor can code: the workload is a file and a name.
    let (code, _, text) = run(&repo(
        r#""function": "run(1) or exit", "args": []"#,
        CANDIDATE,
        LANGUAGE,
        PORT,
    ));
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("is not a name"), "{text}");
    let (code, _, text) = run(&repo(
        r#""function": "run", "args": [8000000]"#,
        CANDIDATE,
        LANGUAGE,
        PORT,
    )
    .tap_repeat(1));
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("at least 3 are needed"), "{text}");
}

trait Tap {
    fn tap_repeat(self, n: u64) -> Self;
}

impl Tap for Repo {
    /// The same repository with the workload's repeat count changed.
    fn tap_repeat(self, n: u64) -> Repo {
        let text = std::fs::read_to_string(self.0.join("decide.json"))
            .unwrap()
            .replace(r#""repeat": 3"#, &format!(r#""repeat": {n}"#));
        std::fs::write(self.0.join("decide.json"), text).unwrap();
        git(&self.0, &["commit", "-q", "-am", "repeat"]);
        self
    }
}

#[test]
fn result_lines_printed_by_the_workload_are_not_results() {
    let (code, report, text) = run(&repo(
        r#""function": "run_printing", "args": [8000000]"#,
        CANDIDATE,
        LANGUAGE,
        PORT,
    ));
    assert_eq!(code, 0, "{text}");
    assert!(
        report["workload"]["measured_with_nothing_attached"]["wall_seconds"]["fastest"]
            .as_f64()
            .unwrap()
            > 0.05,
        "{text}"
    );
}

#[test]
fn the_algorithm_question_is_set_against_the_decision() {
    // A better algorithm is said to exist and has not been tried.
    let untried = r#"{"statement": "the loop can be replaced by a formula", "better_algorithm_in_python": "exists_untried"}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, untried, PORT));
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "contradictions").contains("has not been tried. Try it first"),
        "{text}"
    );

    // It was tried, the tool measured it, and it removes the cost.
    let tried = r#"{"statement": "the loop can be replaced by a formula", "better_algorithm_in_python": "tried", "tried": {"description": "the formula", "function": "run_better", "args": [8000000]}}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, tried, PORT));
    assert_eq!(code, 1, "{text}");
    let m = &report["language_or_algorithm"]["alternative_measured"];
    assert!(m["times_faster"].as_f64().unwrap() > 100.0, "{text}");
    assert_eq!(m["measured"]["runs"].as_array().unwrap().len(), 3);
    assert!(
        all(&report, "contradictions")
            .contains("Make that change, then run this tool on the improved code and decide there"),
        "{text}"
    );
    // With the decision the measurements point to, it is consistent.
    let fix =
        r#"{"choice": "fix_algorithm_first", "reason": "the formula does the same in no time"}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, tried, fix));
    assert_eq!(code, 0, "{text}");
    assert_eq!(report["verdict"], "consistent");

    // An alternative that is no faster does not support "fix it first".
    let same = r#"{"statement": "believed to be the algorithm", "better_algorithm_in_python": "tried", "tried": {"description": "the same thing again", "function": "run", "args": [8000000]}}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, same, fix));
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "contradictions").contains("it is not faster"),
        "{text}"
    );

    // Memory safety alone is not a reason: Python has it.
    let safety =
        r#"{"choice": "port", "goals": ["memory_safety"], "reason": "Rust is memory safe"}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, safety));
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "contradictions").contains("Python is already memory-safe"),
        "{text}"
    );
    // Types are a goal time cannot speak to.
    let types = r#"{"choice": "port", "goals": ["static_types"], "reason": "the records are passed around untyped"}"#;
    let (code, report, text) = run(&repo(
        WAIT,
        r#"{"file": "app.py", "function": "wait"}"#,
        LANGUAGE,
        types,
    ));
    assert_eq!(code, 0, "{text}");
    assert!(
        all(&report, "flags")
            .contains("Time measurements neither support nor contradict the stated goals"),
        "{text}"
    );
}

#[test]
fn a_candidate_called_very_often_for_very_little_is_flagged() {
    let candidate = r#"{"file": "app.py", "function": "tiny"}"#;
    let (code, report, text) = run(&repo(
        r#""function": "run_small", "args": [15000000]"#,
        candidate,
        LANGUAGE,
        PORT,
    ));
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        report["candidate"]["boundary"]["entered_from_outside"],
        15000000
    );
    assert!(
        report["candidate"]["boundary"]["calls_sampled"]
            .as_u64()
            .unwrap()
            < 1000,
        "{text}"
    );
    assert!(
        all(&report, "flags")
            .contains("the candidate is entered 15000000 times and each call takes about")
            && all(&report, "flags").contains("port the loop that calls it"),
        "{text}"
    );
    assert!(
        all(&report, "flags").contains("the instrumented run took"),
        "{text}"
    );
}

const FASTARRAY: &str = "import sys, types
_core = types.ModuleType(__name__ + '._core')
sys.modules[_core.__name__] = _core


class Array(list):
    def __matmul__(self, other):
        return sum(a * b for a, b in zip(self, other))


class Big(int):
    # A number of this package's own type. Arithmetic on it is done by the
    # interpreter's compiled code, reached by an operator, as with an array.
    pass
";

const ARRAYS: &str = "import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'vendor'))
import fastarray


def norm(v):
    return v @ v


def count_and_norm(v, n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total + (v @ v)


def make(n):
    return fastarray.Array(range(n))


class Holder:
    def __init__(self, n):
        self.values = make(n)


def held(holder, n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total + (holder.values @ holder.values)


def squares(b, times):
    total = 0
    for _ in range(times):
        total += (b * b) % 7
    return total


def squares_in_threads(b, times):
    # Compiled work in two threads while the candidate runs.
    import hashlib, threading
    block = bytes(1 << 20)
    def work():
        for _ in range(times):
            hashlib.sha256(block).digest()
    threads = [threading.Thread(target=work) for _ in range(2)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    return squares(b, 3)


TABLE = make(1000)


def with_global(n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total + (TABLE @ TABLE)


class Slotted:
    __slots__ = ('values',)

    def __init__(self, n):
        self.values = make(n)


def with_slots(box, n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total + (box.values @ box.values)


def closing_over(values):
    def inner(n):
        total = 0
        for i in range(n):
            total += (i * i) % 7
        return total + (values @ values)
    return inner


import collections
import pickle

QUEUE = collections.deque([make(1000)])
FAR = [make(1000)] + list(range(30000))


def with_deque(n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total + (QUEUE[0] @ QUEUE[0])


def with_a_long_list(n):
    # The array is the first of 30,001 things in a list.
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total + (FAR[0] @ FAR[0])


BLOB = pickle.dumps(make(1000))


def hidden(n):
    # The arrays exist only inside the last expression: no name is bound
    # to them, here or in anything this function calls.
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total + (pickle.loads(BLOB) @ pickle.loads(BLOB))
";

const ARRAYS_WORKLOAD: &str = "

def run_arrays(n):
    import arrays
    return arrays.norm(arrays.make(n))


def run_mixed(n):
    import arrays
    return arrays.count_and_norm(arrays.make(1000), n)


class Plain:
    # Stands in for an array: the product costs nothing.
    def __matmul__(self, other):
        return 0


def run_mixed_with_stand_ins(n):
    import arrays
    return arrays.count_and_norm(Plain(), n)


def run_held(n):
    import arrays
    return arrays.held(arrays.Holder(1000), n)


def run_global(n):
    import arrays
    return arrays.with_global(n)


def run_slots(n):
    import arrays
    return arrays.with_slots(arrays.Slotted(1000), n)


def run_closure(n):
    import arrays
    return arrays.closing_over(arrays.make(1000))(n)


def run_busy_beside_arrays(n):
    import arrays
    return app.busy(n)


def run_hidden(n):
    import arrays
    return arrays.hidden(n)


def run_deque(n):
    import arrays
    return arrays.with_deque(n)


def run_long_list(n):
    import arrays
    return arrays.with_a_long_list(n)


def run_in_a_thread(n):
    import threading
    out = []
    worker = threading.Thread(target=lambda: out.append(app.busy(n)))
    worker.start()
    worker.join()
    return out[0]


def _big():
    import fastarray
    return fastarray.Big(7 ** 300000)


def run_squares(times):
    import arrays
    return arrays.squares(_big(), times)


class Small:
    # Stands in for the big number: the product costs nothing.
    def __mul__(self, other):
        return 1


def run_squares_with_stand_ins(times):
    import arrays
    return arrays.squares(Small(), times)


def run_squares_half_replaced(times):
    import arrays
    return arrays.squares(_big(), times // 2) + arrays.squares(Small(), times // 2)


def _on_every_cpu(rounds):
    # Compiled work on every CPU at once: hashing lets go of the lock.
    import hashlib, os, threading
    block = bytes(1 << 20)
    def work():
        for _ in range(rounds):
            hashlib.sha256(block).digest()
    threads = [threading.Thread(target=work) for _ in range(os.cpu_count() or 4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()


def run_on_every_cpu(rounds):
    _on_every_cpu(rounds)
    return app.busy(100000)


def run_squares_after_threads(times):
    # The threaded work is preparation, outside the candidate.
    import arrays
    _on_every_cpu(times)
    return arrays.squares(_big(), times)


def run_squares_after_threads_with_stand_ins(times):
    import arrays
    _on_every_cpu(times)
    return arrays.squares(Small(), times)


def run_squares_in_threads(times):
    import arrays
    return arrays.squares_in_threads(_big(), times)


def run_squares_in_threads_with_stand_ins(times):
    import arrays
    return arrays.squares_in_threads(Small(), times)


def run_with_load_inside(n):
    # Other programs take the CPU only while the work is done.
    import os, subprocess, sys, time
    others = [subprocess.Popen([sys.executable, '-c', 'while True: pass']) for _ in range((os.cpu_count() or 4) * 3)]
    time.sleep(0.5)
    try:
        return app.busy(n)
    finally:
        for other in others:
            other.kill()
        for other in others:
            other.wait()
";

/// The small project with a package made to look compiled to the
/// interpreter (a module with no source file), standing in for one such as
/// NumPy. SYNTHETIC: the tests with the real package are the ignored ones.
/// `fastarray.Big` is real in one respect: arithmetic on it is compiled
/// work reached by an operator, which the profiler books to the Python
/// function it is written in.
fn repo_with_arrays(workload: &str, candidate: &str, decision: &str) -> Repo {
    let r = repo(workload, candidate, LANGUAGE, decision);
    std::fs::create_dir_all(r.0.join("vendor/fastarray")).unwrap();
    std::fs::write(r.0.join("vendor/fastarray/__init__.py"), FASTARRAY).unwrap();
    std::fs::write(r.0.join("proj/arrays.py"), ARRAYS).unwrap();
    std::fs::write(
        r.0.join("proj/workload.py"),
        format!("{WORKLOAD}{ARRAYS_WORKLOAD}"),
    )
    .unwrap();
    git(&r.0, &["add", "-A"]);
    git(&r.0, &["commit", "-q", "-m", "arrays"]);
    r
}

#[test]
fn a_candidate_that_uses_a_compiled_package_gets_no_share_and_no_ceiling() {
    // The package's objects arrive as arguments; the candidate's function
    // names no package.
    let (code, report, text) = run(&repo_with_arrays(
        r#""function": "run_mixed", "args": [12000000]"#,
        r#"{"file": "arrays.py", "function": "count_and_norm"}"#,
        PORT,
    ));
    let c = &report["candidate"];
    assert_eq!(
        c["uses_compiled_packages"],
        serde_json::json!(["fastarray"])
    );
    assert_eq!(
        c["where_compiled_packages_were_seen"]["types_of_the_objects_going_in_and_out"],
        serde_json::json!(["fastarray"])
    );
    // Neither figure is given as a finding.
    assert!(c["own_python_share"].is_null(), "{text}");
    assert!(c["ceiling"]["p"].is_null(), "{text}");
    assert!(
        c["ceiling"]["the_whole_workload_could_be_at_most_this_many_times_faster"].is_null(),
        "{text}"
    );
    assert!(
        c["own_python_share_bounds"]["at_most"].as_f64().unwrap() > 0.5
            && c["own_python_share_bounds"]["at_least"].is_null(),
        "{text}"
    );
    // A port for speed is not consistent until another measurement shows
    // where the time is.
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    assert!(
        all(&report, "reasons").contains("the candidate uses a compiled package")
            && all(&report, "reasons").contains("`candidate.with_stand_ins`"),
        "{text}"
    );
    assert_eq!(
        report["where_the_time_goes"]["classification"],
        "own_python_and_compiled_packages_not_separated"
    );
    let md = report_md(&text);
    assert!(
        md.contains("no share and no ceiling are given as findings"),
        "{md}"
    );
    assert!(!md.contains("**Ceiling:"), "{md}");
    // Every report says what the instruments cannot separate.
    assert!(all(&report, "not_measured").contains("operators on objects of compiled packages"));

    // Leaving it alone is not contradicted by any of this.
    let (code, _, text) = run(&repo_with_arrays(
        r#""function": "run_mixed", "args": [12000000]"#,
        r#"{"file": "arrays.py", "function": "count_and_norm"}"#,
        NO,
    ));
    assert_eq!(code, 0, "{text}");
}

/// The Markdown report, which `run_once` puts after the tool's output.
fn report_md(text: &str) -> String {
    text.split_once(MARKDOWN_FOLLOWS).unwrap().1.to_string()
}

#[test]
fn a_compiled_package_is_found_in_a_helper_and_in_an_attribute() {
    // Through a helper in the same project that names the package.
    let (_, report, text) = run(&repo_with_arrays(
        r#""function": "run_held", "args": [10000000]"#,
        r#"{"file": "arrays.py", "function": "__init__"}"#,
        NO,
    ));
    let seen = &report["candidate"]["where_compiled_packages_were_seen"];
    assert_eq!(
        seen["named_in_the_candidate_or_in_project_functions_it_calls"],
        serde_json::json!({"arrays.py:17 make": ["fastarray"]}),
        "{text}"
    );
    // Held in an attribute of an argument, and never named.
    let (_, report, text) = run(&repo_with_arrays(
        r#""function": "run_held", "args": [10000000]"#,
        r#"{"file": "arrays.py", "function": "held"}"#,
        NO,
    ));
    let c = &report["candidate"];
    assert_eq!(
        c["uses_compiled_packages"],
        serde_json::json!(["fastarray"]),
        "{text}"
    );
    assert_eq!(
        c["where_compiled_packages_were_seen"]["named_in_the_candidate_or_in_project_functions_it_calls"],
        serde_json::json!({}),
        "{text}"
    );
    // A candidate in the same file that touches no such object is given
    // its share as before.
    let (code, report, text) = run(&repo_with_arrays(BUSY, CANDIDATE, PORT));
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        report["candidate"]["uses_compiled_packages"],
        serde_json::json!([])
    );
    assert!(
        report["candidate"]["own_python_share"].as_f64().unwrap() > 0.9,
        "{text}"
    );
}

fn with_stand_ins(candidate: &str, stand_in: &str, args: &str) -> String {
    format!(
        r#"{{"file": "arrays.py", "function": "{candidate}", "with_stand_ins": {{"description": "the same with the package's objects replaced", "function": "{stand_in}", "args": {args}}}}}"#
    )
}

#[test]
fn a_stand_in_workload_gives_bounds_and_the_verdict_says_it_rests_on_them() {
    // The stand-ins remove next to nothing: the time was the candidate's
    // own Python. The port for speed is not contradicted, and the verdict
    // says what that rests on.
    let (code, report, text) = run(&repo_with_arrays(
        r#""function": "run_mixed", "args": [12000000]"#,
        &with_stand_ins("count_and_norm", "run_mixed_with_stand_ins", "[12000000]"),
        PORT,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (0, &J::from("consistent_if_the_stand_ins_are_faithful")),
        "{text}"
    );
    let c = &report["candidate"];
    let bounds = &c["own_python_share_bounds"];
    assert!(bounds["at_least"].as_f64().unwrap() > 0.5, "{text}");
    assert!(
        bounds["at_least"].as_f64().unwrap() <= bounds["at_most"].as_f64().unwrap(),
        "{text}"
    );
    assert!(c["own_python_share"].is_null(), "{text}");
    assert_eq!(
        c["with_stand_ins"]["measured"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    // Said first, in words, wherever the verdict is printed.
    assert!(
        text.contains("CONSISTENT ONLY IF THE STAND-INS ARE FAITHFUL"),
        "{text}"
    );
    let md = report_md(&text);
    assert!(
        md.starts_with("# Should this piece be ported?\n\nVerdict: **consistent_if_the_stand_ins_are_faithful**. No measurement contradicts the declared decision, and that rests on the stand-in workload the author wrote"),
        "{md}"
    );
    // The lower bound is never printed without its condition.
    assert!(
        md.contains("if the stand-ins replace all of the package's work"),
        "{md}"
    );
    // (That such stand-ins are pointed out is tested on fixed figures in
    // the tool's unit tests: the flag is judged by time.)
}

#[test]
fn a_stand_in_that_replaces_nothing_is_refused() {
    // The same function and arguments as the workload.
    let (code, report, text) = run(&repo_with_arrays(
        r#""function": "run_mixed", "args": [12000000]"#,
        &with_stand_ins("count_and_norm", "run_mixed", "[12000000]"),
        PORT,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    assert!(
        all(&report, "reasons")
            .contains("it names the same function and arguments as the workload"),
        "{text}"
    );
    // No lower bound comes from it.
    assert!(
        report["candidate"]["own_python_share_bounds"]["at_least"].is_null(),
        "{text}"
    );
}

const SQUARES: &str = r#""function": "run_squares", "args": [300]"#;

#[test]
fn operator_work_on_a_packages_objects_is_bounded_by_honest_stand_ins() {
    // Squaring a very large number of the package's type: compiled work,
    // reached by an operator, which the profiler books to the candidate.
    let (code, report, text) = run(&repo_with_arrays(
        SQUARES,
        r#"{"file": "arrays.py", "function": "squares"}"#,
        PORT,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    let bounds = &report["candidate"]["own_python_share_bounds"];
    // The profiler gives nearly all of it to the candidate.
    assert!(
        bounds["as_the_profiler_counted_it"].as_f64().unwrap() > 0.8,
        "{text}"
    );
    assert!(bounds["at_least"].is_null(), "{text}");

    // With the number replaced by a stand-in the time goes away: it was
    // never the candidate's own Python.
    let (code, report, text) = run(&repo_with_arrays(
        SQUARES,
        &with_stand_ins("squares", "run_squares_with_stand_ins", "[300]"),
        PORT,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("contradicted")),
        "{text}"
    );
    assert!(
        report["candidate"]["own_python_share_bounds"]["at_most"]
            .as_f64()
            .unwrap()
            < 0.1,
        "{text}"
    );
    assert!(
        all(&report, "contradictions").contains("the candidate's own Python is at most"),
        "{text}"
    );
}

#[test]
fn a_half_replaced_stand_in_is_refused() {
    // Half of the calls still get the real object.
    let (code, report, text) = run(&repo_with_arrays(
        SQUARES,
        &with_stand_ins("squares", "run_squares_half_replaced", "[300]"),
        PORT,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    let why = all(&report, "reasons");
    assert!(
        why.contains("is not a stand-in and is not used")
            && why.contains("large objects of compiled packages still reach the candidate")
            && why.contains("fastarray"),
        "{text}"
    );
    assert!(
        report["candidate"]["own_python_share_bounds"]["at_least"].is_null(),
        "{text}"
    );
}

#[test]
fn an_array_is_found_wherever_a_name_leads_to_it() {
    for (function, candidate) in [
        ("run_global", "with_global"),
        ("run_slots", "with_slots"),
        ("run_closure", "inner"),
        ("run_deque", "with_deque"),
        ("run_long_list", "with_a_long_list"),
    ] {
        let (code, report, text) = run(&repo_with_arrays(
            &format!(r#""function": "{function}", "args": [12000000]"#),
            &format!(r#"{{"file": "arrays.py", "function": "{candidate}"}}"#),
            PORT,
        ));
        let c = &report["candidate"];
        assert_eq!(
            c["uses_compiled_packages"],
            serde_json::json!(["fastarray"]),
            "{function}: {text}"
        );
        assert!(c["own_python_share"].is_null(), "{function}: {text}");
        assert_eq!(
            (code, &report["verdict"]),
            (1, &J::from("incomplete")),
            "{function}: {text}"
        );
    }
}

#[test]
fn a_package_that_is_loaded_and_not_seen_in_the_candidate_has_to_be_answered_for() {
    // The project loads the package; the candidate adds integers.
    let beside = r#""function": "run_busy_beside_arrays", "args": [8000000]"#;
    let (code, report, text) = run(&repo_with_arrays(beside, CANDIDATE, PORT));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    assert!(
        all(&report, "reasons").contains("compiled packages [\"fastarray\"] are loaded in the process, and the tool saw no use of them by the candidate")
            && all(&report, "reasons").contains("`candidate.uses_no_compiled_package`"),
        "{text}"
    );
    let c = &report["candidate"];
    assert_eq!(c["uses_compiled_packages"], serde_json::json!([]));
    // The share is still given: nothing was seen to be wrong with it.
    assert!(c["own_python_share"].as_f64().unwrap() > 0.9, "{text}");

    // The author answers, and the answer is printed as the author's.
    let answered = r#"{"file": "app.py", "function": "busy", "uses_no_compiled_package": "it adds small integers in a loop and touches no array"}"#;
    let (code, report, text) = run(&repo_with_arrays(beside, answered, PORT));
    assert_eq!(
        (code, &report["verdict"]),
        (0, &J::from("consistent_on_the_authors_word")),
        "{text}"
    );
    assert!(text.contains("CONSISTENT ON THE AUTHOR'S WORD"), "{text}");
    assert!(
        all(&report, "flags").contains("The author declares, and the tool has not checked: it adds small integers in a loop and touches no array"),
        "{text}"
    );
    // Not porting needs no answer.
    let (code, _, text) = run(&repo_with_arrays(beside, CANDIDATE, NO));
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_false_statement_passes_only_with_a_verdict_that_says_what_it_rests_on() {
    // The package's objects exist only inside one expression of the
    // candidate. Nothing the tool looks at shows them.
    let hidden = r#""function": "run_hidden", "args": [12000000]"#;
    let (code, report, text) = run(&repo_with_arrays(
        hidden,
        r#"{"file": "arrays.py", "function": "hidden"}"#,
        PORT,
    ));
    assert_eq!(
        report["candidate"]["uses_compiled_packages"],
        serde_json::json!([]),
        "the tool is not expected to find this one: {text}"
    );
    // The package is loaded, so the port does not pass unanswered.
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    // The author answers, wrongly. The port passes, and every place that
    // gives the share says on what condition.
    let (code, report, text) = run(&repo_with_arrays(
        hidden,
        r#"{"file": "arrays.py", "function": "hidden", "uses_no_compiled_package": "it only adds small integers in a loop"}"#,
        PORT,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (0, &J::from("consistent_on_the_authors_word")),
        "{text}"
    );
    let condition = "if the candidate uses no compiled package, which the author states and the tool has not checked";
    assert_eq!(
        report["candidate"]["own_python_share_holds_only_if"],
        condition[3..],
        "{text}"
    );
    let md = report_md(&text);
    assert!(
        md.starts_with("# Should this piece be ported?\n\nVerdict: **consistent_on_the_authors_word**. No measurement contradicts the declared decision, and that rests on a sentence the author typed"),
        "{md}"
    );
    // On the share, on the ceiling, on the best case, and where the port
    // is argued: beside the author's need.
    assert_eq!(md.matches(condition).count(), 4, "{md}");
    assert!(
        md.lines()
            .any(|l| l.starts_with("Beside it, measured:") && l.contains(condition)),
        "{md}"
    );
    // And inside each figure of the JSON, not only beside the share.
    for holder in [
        &report["candidate"]["ceiling"],
        &report["candidate"]["best_case"],
        &report["need_for_speed"],
    ] {
        assert_eq!(holder["holds_only_if"], condition[3..], "{text}");
    }
    assert!(
        text.lines()
            .any(|l| l.starts_with("candidate hidden: own Python") && l.contains(condition)),
        "{text}"
    );
}

#[test]
fn a_candidate_that_runs_in_another_thread_gets_no_share() {
    let (code, report, text) = run(&repo_with_arrays(
        r#""function": "run_in_a_thread", "args": [8000000]"#,
        CANDIDATE,
        PORT,
    ));
    // Not "contradicted": nothing was measured about it.
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    let why = all(&report, "reasons");
    // Which of the two depends on whether this Python's profiler sees
    // other threads at all.
    assert!(
        why.contains("and never on the main thread")
            || why.contains("was not called by the workload"),
        "{text}"
    );
    assert!(report["candidate"]["own_python_share"].is_null(), "{text}");
    assert!(!all(&report, "contradictions").contains("0.0%"), "{text}");
}

#[test]
fn with_no_candidate_a_loaded_package_is_named_beside_the_split() {
    let (_, report, text) = run(&repo_with_arrays(
        r#""function": "run_busy_beside_arrays", "args": [8000000]"#,
        "",
        NO,
    ));
    assert_eq!(
        report["where_the_time_goes"]["own_python_may_include_work_inside_loaded_packages"],
        serde_json::json!(["fastarray"]),
        "{text}"
    );
    assert!(
        text.contains("which may include work inside the compiled packages that are loaded"),
        "{text}"
    );
    assert!(
        report_md(&text).contains("which may include work inside [\"fastarray\"]"),
        "{text}"
    );
    // A project that loads none says nothing of the kind.
    let (_, report, text) = run(&repo(BUSY, "", LANGUAGE, NO));
    assert_eq!(
        report["where_the_time_goes"]["own_python_may_include_work_inside_loaded_packages"],
        serde_json::json!([]),
        "{text}"
    );
}

#[test]
fn load_that_falls_inside_the_runs_is_seen() {
    // Other programs take the CPU only while the workload's function runs:
    // the loops timed before and after each run see a quiet machine.
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    if cfg!(target_os = "macos") && cpus < 4 {
        // Under four CPUs the tool runs no process beside the workload, and
        // on macOS nothing else sees this load: the tool answers
        // `consistent`, as its README says. Written to the stream itself,
        // which the test harness does not capture.
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "load_that_falls_inside_the_runs_is_seen: SKIPPED, {cpus} CPUs: under 4 the tool keeps no watch beside a run and on macOS nothing else sees this load (decide/README.md)"
        );
        return;
    }
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (code, report, text) = run_once(
        &repo_with_arrays(
            r#""function": "run_with_load_inside", "args": [8000000]"#,
            CANDIDATE,
            NO,
        ),
        None,
    );
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("unreliable")),
        "{text}"
    );
    if cfg!(target_os = "macos") {
        // Nothing but the process that computes beside it can see it here.
        assert!(
            all(&report, "machine_was_busy")
                .contains("a second process that does nothing but compute"),
            "{text}"
        );
    }
}

#[test]
fn a_port_for_speed_states_what_speed_is_needed() {
    let port = |need: &str| {
        format!(
            r#"{{"choice": "port", "goals": ["speed"], "reason": "the loop is the whole job"{need}}}"#
        )
    };
    // No need declared.
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, &port("")));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    assert!(
        all(&report, "reasons").contains("no need is declared (`decision.need`)"),
        "{text}"
    );
    // Two numbers, or a sentence that says nothing.
    let (_, report, text) = run(&repo(
        BUSY,
        CANDIDATE,
        LANGUAGE,
        &port(
            r#", "need": {"statement": "faster", "runs_per_day": 5, "workload_must_take_at_most_seconds": 1}"#,
        ),
    ));
    let why = all(&report, "reasons");
    assert!(
        why.contains("the need must give exactly one number")
            && why.contains("the need's statement is \"faster\""),
        "{text}"
    );

    // Within reach: consistent, and the best case is printed beside it.
    let within = r#", "need": {"statement": "the request must be answered while the user waits", "workload_must_take_at_most_seconds": 0.05}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, &port(within)));
    assert_eq!(
        (code, &report["verdict"]),
        (0, &J::from("consistent")),
        "{text}"
    );
    let need = &report["need_for_speed"];
    assert_eq!(
        need["declared_by_the_author"]["workload_must_take_at_most_seconds"],
        0.05
    );
    let (now, best, saved) = (
        need["workload_seconds_now"].as_f64().unwrap(),
        need["workload_seconds_if_the_candidate_cost_nothing"]
            .as_f64()
            .unwrap(),
        need["seconds_saved_per_run_at_best"].as_f64().unwrap(),
    );
    assert!(
        now > 0.05 && best < 0.05 && (now - best - saved).abs() < 1e-9,
        "{text}"
    );
    let md = report_md(&text);
    assert!(
        md.contains("The need, declared by the author, not measured: the request must be answered while the user waits (the workload must take at most 0.05 s).")
            && md.contains("if the candidate's own Python cost nothing at all it would take"),
        "{md}"
    );

    // Out of reach: the candidate is half of the work, so the workload
    // cannot become a hundred times faster.
    let halves = r#""function": "run_halves", "args": [4000000]"#;
    let out_of_reach = r#", "need": {"statement": "the request must be answered while the user waits", "workload_must_take_at_most_seconds": 0.01}"#;
    let (code, report, text) = run(&repo(halves, CANDIDATE, LANGUAGE, &port(out_of_reach)));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("contradicted")),
        "{text}"
    );
    assert!(
        all(&report, "contradictions").contains("no port of this piece can reach the need"),
        "{text}"
    );
    let share = report["candidate"]["own_python_share"].as_f64().unwrap();
    assert!((0.35..0.65).contains(&share), "{text}");

    // Already met.
    let met = r#", "need": {"statement": "the request must be answered within a minute", "workload_must_take_at_most_seconds": 60}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, &port(met)));
    assert_eq!(code, 1, "{text}");
    assert!(
        all(&report, "contradictions").contains("the need is already met"),
        "{text}"
    );

    // Run once a day: nothing refuses it, and the saving is said.
    let rare = r#", "need": {"statement": "the report is made once a day", "runs_per_day": 1}"#;
    let (code, report, text) = run(&repo(BUSY, CANDIDATE, LANGUAGE, &port(rare)));
    assert_eq!(code, 0, "{text}");
    assert!(
        all(&report, "flags").contains("by the declared 1 runs a day"),
        "{text}"
    );
}

#[test]
fn the_token_is_not_in_the_workloads_arguments() {
    let (code, report, text) = run(&repo(
        r#""function": "run_looking_for_the_token", "args": [8000000]"#,
        CANDIDATE,
        LANGUAGE,
        NO,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (0, &J::from("consistent")),
        "{text}"
    );
}

#[test]
fn a_report_made_on_a_busy_machine_says_so_in_its_verdict() {
    // Three programs that never wait for every CPU, started by the test,
    // while a workload of pure computation is measured. Without the check
    // such a run reads as partly waiting.
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut spinners: Vec<std::process::Child> = (0..cpus * 3)
        .map(|_| {
            Command::new("python3")
                .args(["-c", "while True: pass"])
                .spawn()
                .unwrap()
        })
        .collect();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let (code, report, text) = run_once(&repo(BUSY, CANDIDATE, LANGUAGE, PORT), None);
    for s in &mut spinners {
        let _ = s.kill();
        let _ = s.wait();
    }
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("unreliable")),
        "{text}"
    );
    assert!(
        all(&report, "machine_was_busy").contains("a loop of pure computation"),
        "{text}"
    );
    assert!(text.contains("UNRELIABLE: the machine was busy"), "{text}");
    let md = report_md(&text);
    assert!(
        md.starts_with("# Should this piece be ported?\n\nVerdict: **unreliable**. The machine was busy while this was measured."),
        "{md}"
    );
}

const MAT: &str = "import numpy as np


def products_operator(a, b, times):
    total = 0.0
    for _ in range(times):
        total += float((a @ b)[0, 0])
    return total


def products_call(a, b, times):
    total = 0.0
    for _ in range(times):
        total += float(np.dot(a, b)[0, 0])
    return total


def pure(n):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    return total


WEIGHTS = np.random.default_rng(2).random((1200, 1200))


def loop_then_products_global(n, times):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    for _ in range(times):
        total += float((WEIGHTS @ WEIGHTS)[0, 0])
    return total


def loop_then_products(a, b, n, times):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    for _ in range(times):
        total += float((a @ b)[0, 0])
    return total


class Model:
    __slots__ = ('weights',)

    def __init__(self, size):
        self.weights = np.random.default_rng(3).random((size, size))


def loop_then_products_slots(model, n, times):
    total = 0
    for i in range(n):
        total += (i * i) % 7
    for _ in range(times):
        total += float((model.weights @ model.weights)[0, 0])
    return total
";

const MAT_WORKLOAD: &str = "import numpy as np

import mat


def _pair(n):
    rng = np.random.default_rng(1)
    return rng.random((n, n)), rng.random((n, n))


def run_operator(n, times):
    a, b = _pair(n)
    return mat.products_operator(a, b, times)


def run_call(n, times):
    a, b = _pair(n)
    return mat.products_call(a, b, times)


class Shape:
    '''Stands in for an array: the product costs nothing and has the
    shape the caller expects.'''

    def __init__(self, a):
        self.first = a[:1, :1].copy()

    def __matmul__(self, other):
        return self.first


def run_operator_with_stand_ins(n, times):
    a, b = _pair(n)
    return mat.products_operator(Shape(a), Shape(b), times)


def run_operator_half_replaced(n, times):
    a, b = _pair(n)
    return mat.products_operator(a, b, times // 2) + mat.products_operator(Shape(a), Shape(b), times // 2)


def run_operator_views(n, times):
    # The same arrays, each as a view of itself: nothing is replaced.
    a, b = _pair(n)
    return mat.products_operator(a[:], b[:], times)


def run_operator_half_views(n, times):
    a, b = _pair(n)
    return mat.products_operator(a[:], b[:], times // 2) + mat.products_operator(Shape(a), Shape(b), times // 2)


def run_calls_only(n, times):
    a, b = _pair(n)
    total = 0.0
    for _ in range(times):
        total += float(np.dot(a, b)[0, 0])
    return total


def run_global(n, times):
    return mat.loop_then_products_global(n, times)


def run_slots(n, times):
    return mat.loop_then_products_slots(mat.Model(1200), n, times)


def run_pure(n):
    return mat.pure(n)


def _warm(n):
    # Products in the library's threads, outside the candidate.
    a, b = _pair(n)
    for _ in range(4):
        a = (a @ b) / n
    return a


def run_prepared(n, loop, times):
    _warm(n)
    a, b = _pair(n)
    return mat.loop_then_products(a, b, loop, times)


def run_prepared_with_small_arrays(n, loop, times):
    _warm(n)
    a, b = _pair(n)
    return mat.loop_then_products(a[:1, :1].copy(), b[:1, :1].copy(), loop, times)


_KEPT = {}


class Hidden:
    '''Small to look at; its product still multiplies the large arrays.'''

    def __init__(self, a):
        self.first = a[:1, :1].copy()

    def __matmul__(self, other):
        _KEPT['a'] @ _KEPT['b']
        return self.first


def run_prepared_with_hidden_products(n, loop, times):
    _warm(n)
    a, b = _pair(n)
    _KEPT.update(a=a, b=b)
    return mat.loop_then_products(Hidden(a), Hidden(b), loop, times)
";

/// With the real NumPy. Needs an interpreter that has it, named in
/// `CARCINIFY_NUMPY_PYTHON`:
///
///     python3 -m venv target/numpy-venv && target/numpy-venv/bin/pip install numpy
///     CARCINIFY_NUMPY_PYTHON=$PWD/target/numpy-venv/bin/python \
///         cargo test --locked -p carcinify --test decide -- --ignored
#[test]
#[ignore = "needs an interpreter with NumPy in CARCINIFY_NUMPY_PYTHON"]
fn real_numpy_is_not_reported_as_the_projects_own_python() {
    let python = std::env::var("CARCINIFY_NUMPY_PYTHON")
        .expect("set CARCINIFY_NUMPY_PYTHON to an interpreter that has NumPy");
    let modern = Command::new(&python)
        .args([
            "-c",
            "import sys; sys.exit(0 if hasattr(sys, 'monitoring') else 1)",
        ])
        .status()
        .unwrap()
        .success();
    let numpy_repo = |workload: &str, candidate: &str| {
        let r = repo(workload, candidate, LANGUAGE, PORT);
        std::fs::write(r.0.join("proj/mat.py"), MAT).unwrap();
        std::fs::write(r.0.join("proj/workload.py"), MAT_WORKLOAD).unwrap();
        git(&r.0, &["add", "-A"]);
        git(&r.0, &["commit", "-q", "-m", "matrices"]);
        r
    };
    let run = |r: &Repo| run_with(r, Some(&python), false);
    let products = r#""args": [1600, 20]"#;
    for (function, candidate) in [
        ("run_operator", "products_operator"),
        ("run_call", "products_call"),
    ] {
        // The auditor's case: 20 products of 1,600 by 1,600 matrices.
        let (code, report, text) = run(&numpy_repo(
            &format!(r#""function": "{function}", {products}"#),
            &format!(r#"{{"file": "mat.py", "function": "{candidate}"}}"#),
        ));
        let c = &report["candidate"];
        assert_eq!(
            c["uses_compiled_packages"],
            serde_json::json!(["numpy"]),
            "{text}"
        );
        // No share and no ceiling as findings, and the port is not
        // consistent.
        assert!(c["own_python_share"].is_null(), "{text}");
        assert!(
            c["ceiling"]["the_whole_workload_could_be_at_most_this_many_times_faster"].is_null()
                && c["ceiling"]["p"].is_null(),
            "{text}"
        );
        assert_eq!(code, 1, "{text}");
        assert_ne!(report["verdict"], "consistent", "{text}");
        assert_ne!(
            report["where_the_time_goes"]["classification"], "mostly_own_python",
            "{text}"
        );
        let md = report_md(&text);
        assert!(!md.contains("**Ceiling:"), "{md}");
        if modern && function == "run_call" {
            // `sys.monitoring` sees the call form and times it.
            assert_eq!(report["verdict"], "contradicted", "{text}");
            assert!(
                c["own_python_share_bounds"]["at_most"].as_f64().unwrap() < 0.1,
                "{text}"
            );
            assert_eq!(
                report["where_the_time_goes"]["classification"], "mostly_compiled_code",
                "{text}"
            );
        } else {
            // The operator is seen by nothing: the share stays unknown.
            assert_eq!(report["verdict"], "incomplete", "{text}");
            assert!(
                all(&report, "reasons").contains("the candidate uses a compiled package"),
                "{text}"
            );
        }
    }

    // The operator form with a stand-in workload: the product was the time.
    let (code, report, text) = run(&numpy_repo(
        &format!(r#""function": "run_operator", {products}"#),
        r#"{"file": "mat.py", "function": "products_operator", "with_stand_ins": {"description": "arrays replaced by objects whose product costs nothing", "function": "run_operator_with_stand_ins", "args": [1600, 20]}}"#,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("contradicted")),
        "{text}"
    );
    assert!(
        report["candidate"]["own_python_share_bounds"]["at_most"]
            .as_f64()
            .unwrap()
            < 0.2,
        "{text}"
    );

    // A stand-in that is the workload again, and one that leaves half of
    // the products in: neither is taken for a measurement.
    for (stand_in, why) in [
        (
            "run_operator",
            "it names the same function and arguments as the workload",
        ),
        (
            "run_operator_half_replaced",
            "large objects of compiled packages still reach the candidate",
        ),
        // Views own a hundred bytes each and show the whole array.
        (
            "run_operator_views",
            "large objects of compiled packages still reach the candidate",
        ),
        (
            "run_operator_half_views",
            "large objects of compiled packages still reach the candidate",
        ),
    ] {
        let (code, report, text) = run(&numpy_repo(
            &format!(r#""function": "run_operator", {products}"#),
            &format!(
                r#"{{"file": "mat.py", "function": "products_operator", "with_stand_ins": {{"description": "said to replace the arrays", "function": "{stand_in}", "args": [1600, 20]}}}}"#
            ),
        ));
        assert_eq!(
            (code, &report["verdict"]),
            (1, &J::from("incomplete")),
            "{text}"
        );
        assert!(all(&report, "reasons").contains(why), "{text}");
        assert!(
            report["candidate"]["own_python_share_bounds"]["at_least"].is_null(),
            "{text}"
        );
    }

    // The arrays are in a module-level constant, or in an object with
    // `__slots__`: the candidate names no package and takes no array, and
    // a tenth of its time is its own Python.
    for (function, candidate) in [
        ("run_global", "loop_then_products_global"),
        ("run_slots", "loop_then_products_slots"),
    ] {
        let (code, report, text) = run(&numpy_repo(
            &format!(r#""function": "{function}", "args": [2000000, 40]"#),
            &format!(r#"{{"file": "mat.py", "function": "{candidate}"}}"#),
        ));
        let c = &report["candidate"];
        assert_eq!(
            c["uses_compiled_packages"],
            serde_json::json!(["numpy"]),
            "{function}: {text}"
        );
        assert!(c["own_python_share"].is_null(), "{function}: {text}");
        assert_eq!(
            (code, &report["verdict"]),
            (1, &J::from("incomplete")),
            "{function}: {text}"
        );
    }

    // A candidate of pure Python in a project that loads NumPy. Nothing
    // shows it using the package, and its share is given. A port for speed
    // still has to be answered for, because the tool does not find every
    // use.
    let pure = r#""function": "run_pure", "args": [8000000]"#;
    let (code, report, text) = run(&numpy_repo(
        pure,
        r#"{"file": "mat.py", "function": "pure"}"#,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    assert!(
        all(&report, "reasons").contains("compiled packages [\"numpy\"] are loaded in the process"),
        "{text}"
    );
    let c = &report["candidate"];
    assert_eq!(c["uses_compiled_packages"], serde_json::json!([]), "{text}");
    assert!(c["own_python_share"].as_f64().unwrap() > 0.9, "{text}");
    assert_eq!(
        c["compiled_modules_imported_by_its_file"],
        serde_json::json!(["numpy"])
    );
    let (code, report, text) = run(&numpy_repo(
        pure,
        r#"{"file": "mat.py", "function": "pure", "uses_no_compiled_package": "a loop over small integers, no array is touched"}"#,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (0, &J::from("consistent_on_the_authors_word")),
        "{text}"
    );
    assert!(
        all(&report, "flags").contains("The author declares, and the tool has not checked"),
        "{text}"
    );

    // With no candidate named, a workload that is all NumPy calls: the
    // split still says "own Python", and now says what that may hold.
    let r = repo(
        r#""function": "run_calls_only", "args": [1600, 20]"#,
        "",
        LANGUAGE,
        NO,
    );
    std::fs::write(r.0.join("proj/mat.py"), MAT).unwrap();
    std::fs::write(r.0.join("proj/workload.py"), MAT_WORKLOAD).unwrap();
    git(&r.0, &["add", "-A"]);
    git(&r.0, &["commit", "-q", "-m", "matrices"]);
    let (_, report, text) = run(&r);
    assert_eq!(
        report["where_the_time_goes"]["own_python_may_include_work_inside_loaded_packages"],
        serde_json::json!(["numpy"]),
        "{text}"
    );
    assert!(
        report_md(&text).contains("which may include work inside [\"numpy\"]"),
        "{text}"
    );
    // The library's threads were at work just before the candidate runs,
    // and for a moment afterwards they spin and do nothing: with small
    // arrays in the candidate that is not work inside it, and the stand-in
    // is used. On Linux NumPy's library does this by default.
    let prepared = r#""function": "run_prepared", "args": [1600, 3000000, 20]"#;
    let stand_in = |function: &str| {
        format!(
            r#"{{"file": "mat.py", "function": "loop_then_products", "with_stand_ins": {{"description": "the arrays replaced by one-element ones", "function": "{function}", "args": [1600, 3000000, 20]}}}}"#
        )
    };
    let (_, report, text) = run(&numpy_repo(
        prepared,
        &stand_in("run_prepared_with_small_arrays"),
    ));
    assert!(
        !all(&report, "reasons").contains("is not a stand-in"),
        "{text}"
    );
    assert!(
        report["candidate"]["own_python_share_bounds"]["at_least"].is_f64(),
        "{text}"
    );
    // The flag is there exactly when the report's own figures call for it.
    // Whether NumPy's idle threads spin (on Linux they do by default) is
    // not asserted: on a busy machine they get no CPU to spin on.
    let inside = &report["candidate"]["with_stand_ins"]["inside_the_candidate"];
    assert_eq!(
        has_flag(&report, "stand_in_more_cpu_than_wall"),
        inside["cpu_seconds_of_all_threads"].as_f64().unwrap()
            > 1.05 * inside["wall_seconds"].as_f64().unwrap(),
        "{text}"
    );
    // The products still done, out of sight, while the candidate runs:
    // refused, because the large arrays are found where the code that
    // runs inside the candidate names them.
    let (code, report, text) = run(&numpy_repo(
        prepared,
        &stand_in("run_prepared_with_hidden_products"),
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &J::from("incomplete")),
        "{text}"
    );
    assert!(
        all(&report, "reasons")
            .contains("large objects of compiled packages still reach the candidate"),
        "{text}"
    );
    assert!(
        report["candidate"]["own_python_share_bounds"]["at_least"].is_null(),
        "{text}"
    );
}

#[test]
fn a_workload_that_uses_every_cpu_is_not_taken_for_a_busy_machine() {
    // Its own threads take every CPU. The process that computes beside it
    // is kept off the CPU by the workload, not by other programs.
    let (code, report, text) = run(&repo_with_arrays(
        r#""function": "run_on_every_cpu", "args": [700]"#,
        "",
        NO,
    ));
    assert_eq!(
        (code, &report["verdict"]),
        (0, &J::from("consistent")),
        "{text}"
    );
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    if cpus >= 4 && !cfg!(windows) {
        let said = "so load inside the run could not be watched";
        assert!(all(&report, "flags").contains(said), "{text}");
        assert!(report_md(&text).contains(said), "{text}");
        assert!(
            text.lines()
                .any(|l| l.starts_with("flag: the workload itself used") && l.contains(said)),
            "{text}"
        );
        // Once, however many runs it was true of.
        assert_eq!(all(&report, "flags").matches(said).count(), 1, "{text}");
    }
    assert_eq!(all(&report, "machine_was_busy"), "", "{text}");
}

#[test]
fn threads_at_work_outside_the_candidate_do_not_refuse_a_stand_in() {
    // Preparation in threads, then the candidate: the stand-in keeps the
    // preparation and replaces the package's object.
    let (_, report, text) = run(&repo_with_arrays(
        r#""function": "run_squares_after_threads", "args": [300]"#,
        &with_stand_ins(
            "squares",
            "run_squares_after_threads_with_stand_ins",
            "[300]",
        ),
        PORT,
    ));
    let stand_ins = &report["candidate"]["with_stand_ins"];
    assert_eq!(
        stand_ins["refused_because"],
        serde_json::json!([]),
        "{text}"
    );
    assert!(
        report["candidate"]["own_python_share_bounds"]["at_least"].is_number(),
        "{text}"
    );

    // Threads at work while the candidate runs are said, with the
    // figures, and refuse nothing: idle threads of a linear algebra
    // library look the same for a moment after a product.
    let (_, report, text) = run(&repo_with_arrays(
        r#""function": "run_squares_in_threads", "args": [300]"#,
        &with_stand_ins(
            "squares_in_threads",
            "run_squares_in_threads_with_stand_ins",
            "[300]",
        ),
        PORT,
    ));
    let stand_ins = &report["candidate"]["with_stand_ins"];
    assert_eq!(
        stand_ins["refused_because"],
        serde_json::json!([]),
        "{text}"
    );
    let inside = &stand_ins["inside_the_candidate"];
    assert!(
        inside["cpu_seconds_of_all_threads"].as_f64().unwrap()
            > 1.05 * inside["wall_seconds"].as_f64().unwrap(),
        "{text}"
    );
    assert!(has_flag(&report, "stand_in_more_cpu_than_wall"), "{text}");
    assert!(
        all(&report, "flags").contains("OPENBLAS_NUM_THREADS=1 or OMP_NUM_THREADS=1")
            && all(&report, "flags").contains("for the candidate's 1 entry"),
        "{text}"
    );
}

#[test]
fn record_calls_writes_each_distinct_call_of_the_candidate_as_it_came_in() {
    let repo = repo(
        r#""function": "run_recorded", "args": []"#,
        r#"{"file": "app.py", "function": "recorded"}"#,
        "",
        "",
    );
    let calls = repo.0.join("out/workload.calls.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("decide")
        .arg(repo.0.join("decide.json"))
        .arg("--report")
        .arg(repo.0.join("out/report.json"))
        .arg("--record-calls")
        .arg(&calls)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let text = std::fs::read_to_string(&calls)
        .unwrap_or_else(|e| panic!("{e}: {stdout}{}", String::from_utf8_lossy(&out.stderr)));
    let int = |v: u32| format!(r#"{{"t": "int", "v": "{v}"}}"#);
    let list = |v: &[u32]| {
        let items: Vec<String> = v.iter().map(|x| int(*x)).collect();
        format!(r#"{{"t": "list", "v": [{}]}}"#, items.join(", "))
    };
    let text_value = |v: &str| format!(r#"{{"t": "str", "v": "{v}"}}"#);
    // The list is as it came in, before the call appended to it; the call
    // made twice is one line; calls from inside the candidate are not its
    // inputs; keyword arguments stay by keyword; defaults are written as
    // the call received them.
    let expected = [
        format!(
            r#"{{"id": "call-1", "fn": "recorded", "encoding": "typed", "args": [{}, {}], "kwargs": {{"label": {}}}}}"#,
            list(&[1, 2]),
            int(1),
            text_value("x")
        ),
        format!(
            r#"{{"id": "call-2", "fn": "recorded", "encoding": "typed", "args": [{}, {}], "kwargs": {{"label": {}}}}}"#,
            list(&[3]),
            int(2),
            text_value("y")
        ),
        format!(
            r#"{{"id": "call-3", "fn": "recorded", "encoding": "typed", "args": [{}, {}, {}, {}], "kwargs": {{"label": {}, "flag": {{"t": "bool", "v": true}}}}}}"#,
            list(&[3]),
            int(2),
            int(5),
            int(6),
            text_value("x")
        ),
        r#"{"id": "call-4", "fn": "recorded", "unencodable": "builtins.object"}"#.to_string(),
    ];
    assert_eq!(text.lines().collect::<Vec<_>>(), expected, "{text}");
    let report: J =
        serde_json::from_str(&std::fs::read_to_string(repo.0.join("out/report.json")).unwrap())
            .unwrap();
    let rec = &report["recorded_calls"];
    assert_eq!(
        (&rec["calls"], &rec["lines"]),
        (&J::from(5), &J::from(4)),
        "{rec}"
    );
    assert_eq!(
        rec["unencodable"],
        serde_json::json!([{"id": "call-4", "holds": "builtins.object"}])
    );
    assert!(stdout.contains("builtins.object"), "{stdout}");
}

#[test]
fn record_calls_sees_a_call_made_in_a_worker_thread() {
    let repo = repo(
        r#""function": "run_recorded_in_a_thread", "args": []"#,
        r#"{"file": "app.py", "function": "recorded"}"#,
        "",
        "",
    );
    let calls = repo.0.join("out/thread.calls.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("decide")
        .arg(repo.0.join("decide.json"))
        .arg("--report")
        .arg(repo.0.join("out/report.json"))
        .arg("--record-calls")
        .arg(&calls)
        .output()
        .unwrap();
    let text = std::fs::read_to_string(&calls).unwrap_or_else(|e| {
        panic!(
            "{e}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    // The call from inside itself, in the same thread, is still left out.
    assert_eq!(
        text,
        r#"{"id": "call-1", "fn": "recorded", "encoding": "typed", "args": [{"t": "list", "v": [{"t": "int", "v": "7"}]}, {"t": "int", "v": "1"}], "kwargs": {"label": {"t": "str", "v": "x"}}}"#.to_string() + "\n"
    );
}

#[test]
fn record_calls_leaves_out_a_default_it_cannot_write_and_writes_the_rest() {
    let repo = repo(
        r#""function": "run_sentinel", "args": []"#,
        r#"{"file": "app.py", "function": "with_sentinel"}"#,
        "",
        "",
    );
    let calls = repo.0.join("out/sentinel.calls.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("decide")
        .arg(repo.0.join("decide.json"))
        .arg("--report")
        .arg(repo.0.join("out/report.json"))
        .arg("--record-calls")
        .arg(&calls)
        .output()
        .unwrap();
    let text = std::fs::read_to_string(&calls).unwrap_or_else(|e| {
        panic!(
            "{e}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    // The sentinel is left out, so label goes by name; its default, None,
    // is written as received: with_sentinel(1) and with_sentinel(1,
    // label=None) are one call.
    assert_eq!(
        text,
        concat!(
            r#"{"id": "call-1", "fn": "with_sentinel", "encoding": "typed", "args": [{"t": "int", "v": "1"}], "kwargs": {"label": {"t": "none"}}}"#,
            "\n",
            r#"{"id": "call-2", "fn": "with_sentinel", "encoding": "typed", "args": [{"t": "int", "v": "2"}], "kwargs": {"label": {"t": "str", "v": "k"}}}"#,
            "\n"
        )
    );
}

#[test]
fn record_calls_needs_a_file_named_as_a_recording() {
    // A path outside the checkout: a run that went further would write it.
    let report =
        std::env::temp_dir().join(format!("carcinify-decide-name-{}.json", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .args([
            "decide",
            "declaration.json",
            "--record-calls",
            "calls.jsonl",
            "--report",
        ])
        .arg(&report)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("--record-calls calls.jsonl: the file's name must end in .calls.jsonl"),
        "{stderr}"
    );
    assert!(!report.exists(), "a refused run wrote its report");
}

#[test]
fn a_recorded_call_is_written_with_the_defaults_it_received_so_another_default_cannot_match() {
    // The auditor's three: a private function renamed by `fn` to a public
    // one with another default; defaults reassigned after definition; a
    // default dict changed in place. The harness binds with the defaults of
    // the function it calls, freshly imported, so each call is another
    // input than the case that looks like it.
    let repo = repo(
        r#""function": "run_total", "args": []"#,
        r#"{"file": "app.py", "function": "_total"}"#,
        "",
        "",
    );
    let runs = [
        ("_total", "run_total", "total"),
        ("scaled", "run_scaled", "scaled"),
        ("lookup", "run_lookup", "lookup"),
    ];
    record_each(&repo, &runs);
    std::fs::create_dir_all(repo.0.join("rec")).unwrap();
    for (_, _, name) in runs {
        std::fs::copy(
            repo.0.join(format!("out/{name}.calls.jsonl")),
            repo.0.join(format!("rec/{name}.calls.jsonl")),
        )
        .unwrap();
    }
    std::fs::create_dir_all(repo.0.join("port")).unwrap();
    // The harness refuses two sides of the same content.
    std::fs::write(repo.0.join("port/app.py"), format!("{APP}# the port\n")).unwrap();
    std::fs::write(
        repo.0.join("cases.jsonl"),
        concat!(
            r#"{"id": "total-12", "fn": "total", "args": [[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]]}"#,
            "\n",
            r#"{"id": "scaled-5", "fn": "scaled", "args": [5]}"#,
            "\n",
            r#"{"id": "lookup-scale", "fn": "lookup", "args": ["scale"]}"#,
            "\n"
        ),
    )
    .unwrap();
    std::fs::write(
        repo.0.join("suite.json"),
        r#"{"schema": "carcinify/diff/suite/1",
 "original": {"kind": "python", "path": ["proj"], "module": "app"},
 "candidate": {"kind": "python", "path": ["port"], "module": "app"},
 "cases": ["cases.jsonl"],
 "measured": [
   {"file": "rec/total.calls.jsonl", "fn": "total"},
   {"file": "rec/scaled.calls.jsonl"},
   {"file": "rec/lookup.calls.jsonl"}]}"#,
    )
    .unwrap();
    git(&repo.0, &["add", "-A"]);
    git(&repo.0, &["commit", "-q", "-m", "cases"]);
    let report = repo.0.join("out/diff.json");
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("diff")
        .arg(repo.0.join("suite.json"))
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let r: J = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert_eq!(r["summary"]["identical"], 3, "{text}");
    assert!(
        r["reasons"]
            .to_string()
            .contains("measured_input_not_compared"),
        "{text}"
    );
    for m in r["measured"].as_array().unwrap() {
        assert_eq!(
            (m["lines"].as_u64(), m["compared"].as_u64()),
            (Some(1), Some(0)),
            "{}: {text}",
            m["file"]
        );
    }
}

/// Runs decide --record-calls once per (candidate, workload function,
/// name), each into out/<name>.calls.jsonl, which is ignored, so that the
/// next run still sees a clean tree.
fn record_each(repo: &Repo, runs: &[(&str, &str, &str)]) {
    for (candidate, workload, name) in runs.iter().copied() {
        std::fs::write(
            repo.0.join(format!("decide-{name}.json")),
            format!(
                r#"{{"schema": "carcinify/decide/declaration/2", "project_root": "proj", "workload": {{"description": "a test workload", "file": "proj/workload.py", "function": "{workload}", "args": [], "repeat": 3}}, "candidate": {{"file": "app.py", "function": "{candidate}"}}}}"#
            ),
        )
        .unwrap();
    }
    git(&repo.0, &["add", "-A"]);
    git(&repo.0, &["commit", "-q", "-m", "declarations"]);
    for (_, _, name) in runs.iter().copied() {
        // Into out/, which is ignored, so the next run still sees a clean tree.
        let calls = repo.0.join(format!("out/{name}.calls.jsonl"));
        let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
            .arg("decide")
            .arg(repo.0.join(format!("decide-{name}.json")))
            .arg("--report")
            .arg(repo.0.join(format!("out/decide-{name}.json")))
            .arg("--record-calls")
            .arg(&calls)
            .output()
            .unwrap();
        assert!(
            calls.exists(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn record_calls_finds_the_defaults_of_a_candidate_behind_a_decorator() {
    // The module holds the wrapper; inspect.unwrap reaches the function
    // through __wrapped__, so the sentinel is left out, not written.
    let repo = repo(
        r#""function": "run_decorated", "args": []"#,
        r#"{"file": "app.py", "function": "wrapped"}"#,
        "",
        "",
    );
    record_each(
        &repo,
        &[
            ("wrapped", "run_decorated", "wrapped"),
            ("cached", "run_decorated", "cached"),
        ],
    );
    let read = |name: &str| {
        std::fs::read_to_string(repo.0.join(format!("out/{name}.calls.jsonl"))).unwrap()
    };
    assert_eq!(
        read("wrapped"),
        concat!(
            r#"{"id": "call-1", "fn": "wrapped", "encoding": "typed", "args": [{"t": "int", "v": "1"}], "kwargs": {}}"#,
            "\n",
            r#"{"id": "call-2", "fn": "wrapped", "encoding": "typed", "args": [{"t": "int", "v": "2"}, {"t": "int", "v": "3"}], "kwargs": {}}"#,
            "\n"
        )
    );
    assert_eq!(
        read("cached"),
        r#"{"id": "call-1", "fn": "cached", "encoding": "typed", "args": [{"t": "int", "v": "1"}], "kwargs": {}}"#.to_string() + "\n"
    );
}
