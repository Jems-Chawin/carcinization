//! `carcinify speed`, run as a user runs it, on the fixture in
//! `speed/tests/fixture`. The tests marked ignored build the PyO3 port and
//! need maturin 1.16:
//!
//!     CARCINIFY_PYTHON=$PWD/.venv/bin/python CARCINIFY_MATURIN=$PWD/.venv/bin/maturin \
//!         cargo test -p carcinify --test speed -- --ignored

use carcinify_diff::provenance::sha256_file;
use serde_json::{Value as J, json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../speed/tests/fixture")
        .canonicalize()
        .unwrap()
}

/// A declaration in a directory of its own, naming one of the fixture's
/// suites, with each target's case file and id.
fn declare(name: &str, suite: &str, targets: &[(&str, &str, &str)]) -> PathBuf {
    let targets: Vec<J> = targets
        .iter()
        .map(|(target, file, id)| {
            json!({"name": target, "case": {"file": file, "id": id}, "calls_per_run": 2})
        })
        .collect();
    declare_targets(name, suite, targets)
}

/// A caller target: the fixture's caller `function`, which reaches `func`
/// through the library's attribute of that name.
fn caller(name: &str, function: &str, func: &str) -> J {
    json!({"name": name, "calls_per_run": 2, "caller": {
        "file": "callers/callers.py", "function": function, "fn": func, "replace": format!("library.{func}")}})
}

fn declare_targets(name: &str, suite: &str, targets: Vec<J>) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("speed-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let declaration = json!({
        "schema": "carcinify/speed/declaration/1",
        "suite": fixture().join(suite),
        "runs": 3,
        "targets": targets,
    });
    let path = dir.join("speed.json");
    std::fs::write(&path, declaration.to_string()).unwrap();
    path
}

/// `carcinify speed <args> --allow-dirty`: the fixture is in this
/// repository, whose tree may not match its commit while it is worked on.
/// Never under a gate unless `env` says so.
fn speed(args: &[&Path], env: &[(&str, &str)]) -> (i32, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_carcinify"));
    command
        .arg("speed")
        .args(args)
        .arg("--allow-dirty")
        .env_remove("GATE_RUN_ID");
    for (name, value) in env {
        command.env(name, value);
    }
    let out = command.output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn read(path: &Path) -> J {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

#[test]
fn speed_build_refuses_to_run_under_the_gate() {
    let declaration = declare(
        "under-gate",
        "suite-python.json",
        &[("nap", "cases.jsonl", "nap-2")],
    );
    let out = declaration.with_file_name("wheels");
    let (code, err) = speed(
        &[Path::new("build"), &declaration, Path::new("--out"), &out],
        &[("GATE_RUN_ID", "4f2a9c1e7b3d5a60")],
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("GATE_RUN_ID"), "{err}");
    assert!(!out.exists());
}

#[test]
fn a_target_must_name_a_case_of_the_suite_that_returns() {
    for (n, (file, id, why)) in [
        (
            "outside.jsonl",
            "nap-outside",
            "not one of the suite's case files",
        ),
        ("cases.jsonl", "nap-missing", "has no case \"nap-missing\""),
        ("cases.jsonl", "nap-raises", "expect_raise"),
    ]
    .into_iter()
    .enumerate()
    {
        let declaration = declare(
            &format!("refused-{n}"),
            "suite-python.json",
            &[("nap", file, id)],
        );
        let out = declaration.with_file_name("wheels");
        let (code, err) = speed(
            &[Path::new("build"), &declaration, Path::new("--out"), &out],
            &[],
        );
        assert_eq!(code, 1, "{id}: {err}");
        assert!(
            err.contains("target \"nap\"") && err.contains(why),
            "{id}: {err}"
        );
        assert!(!out.exists(), "{id}");
    }
}

#[test]
fn speed_build_of_a_python_port_writes_its_record_and_no_wheel() {
    let declaration = declare(
        "python-port",
        "suite-python.json",
        &[("nap", "cases.jsonl", "nap-2")],
    );
    let out = declaration.with_file_name("wheels");
    let (code, err) = speed(
        &[Path::new("build"), &declaration, Path::new("--out"), &out],
        &[],
    );
    assert_eq!(code, 0, "{err}");
    let record = read(&out.join("build.json"));
    assert_eq!(record["schema"], "carcinify/speed/build/1");
    assert_eq!(
        record["declaration_sha256"],
        sha256_file(&declaration).unwrap()
    );
    assert_eq!(record["sides"]["original"]["kind"], "python");
    assert_eq!(record["sides"]["candidate"]["kind"], "python");
    assert!(record["sides"]["candidate"]["wheel"].is_null(), "{record}");
    // carcinify diff's tools block, for the binary that built.
    let tools = &record["tools"];
    let exe = Path::new(env!("CARGO_BIN_EXE_carcinify"));
    assert_eq!(tools["harness_sha256"], sha256_file(exe).unwrap());
    assert_eq!(tools["driver_sha256"].as_str().map(str::len), Some(64));
    assert!(
        tools["rustc"].as_str().unwrap().starts_with("rustc "),
        "{tools}"
    );
    assert!(
        tools["cargo"].as_str().unwrap().starts_with("cargo "),
        "{tools}"
    );
    assert!(
        tools["maturin"].is_null(),
        "nothing was built with maturin: {tools}"
    );
    assert_eq!(tools["os"], std::env::consts::OS);
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn speed_build_writes_the_ports_wheel_its_hash_and_the_target_cpu_it_was_built_for() {
    let declaration = declare("pyo3", "suite.json", &[("nap", "cases.jsonl", "nap-2")]);
    let out = declaration.with_file_name("wheels");
    let built = |env: &[(&str, &str)]| {
        let (code, err) = speed(
            &[Path::new("build"), &declaration, Path::new("--out"), &out],
            env,
        );
        assert_eq!(code, 0, "{err}");
        let record = read(&out.join("build.json"));
        let candidate = &record["sides"]["candidate"];
        assert_eq!(candidate["kind"], "pyo3");
        let wheel = out.join(candidate["wheel"]["file"].as_str().unwrap());
        assert_eq!(candidate["wheel"]["sha256"], sha256_file(&wheel).unwrap());
        let maturin = record["tools"]["maturin"].as_str().unwrap().to_string();
        assert!(maturin.starts_with("maturin 1.16"), "{maturin}");
        candidate["target"]["target_cpu"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // Twice: Cargo prints its command line only for a crate it compiles,
    // so the second build must compile the crate again, which Cargo would
    // find fresh.
    for _ in 0..2 {
        let cpu = built(&[]);
        assert!(cpu.starts_with("default of "), "{cpu}");
    }
    assert_eq!(built(&[("RUSTFLAGS", "-C target-cpu=generic")]), "generic");
}

/// `speed build` on the Python fixture, into the declaration's directory.
fn built(declaration: &Path) -> PathBuf {
    let wheels = declaration.with_file_name("wheels");
    let (code, err) = speed(
        &[Path::new("build"), declaration, Path::new("--out"), &wheels],
        &[],
    );
    assert_eq!(code, 0, "{err}");
    wheels
}

/// Why the machine was too busy for the run, if it was: every pair of a
/// target dropped as disturbed, or the process beside a target kept off
/// the CPU. Read from each target's own figures: a failed run's reasons say
/// only why it failed, though another target was disturbed.
fn too_busy(report: &J) -> Vec<String> {
    let mut why = Vec::new();
    for t in report["targets"].as_array().into_iter().flatten() {
        if !t["failed"].is_null() {
            continue;
        }
        let name = t["name"].as_str().unwrap_or("?");
        if t["ratio"].is_null() {
            why.push(format!("{name}: every pair of runs was dropped"));
        }
        let beside = t["watched_beside"]["kept_off_the_cpu"].as_f64();
        if let Some(lost) = beside.filter(|l| *l > carcinify_decide::report::BUSY_WHILE_RUNNING) {
            why.push(format!(
                "{name}: the process beside it was kept off the CPU for {:.0}% of its time",
                lost * 100.0
            ));
        }
    }
    why
}

/// A run, again while the machine was too busy for it, up to five times.
/// Each repeat is printed, and written to the file named in
/// `CARCINIFY_SPEED_ATTEMPTS` when that is set; ci/run-all.sh counts them,
/// so that a drop rule which began to fire on a quiet machine would show.
fn quiet(attempt: &mut dyn FnMut() -> (i32, String, J)) -> (i32, String, J) {
    let mut last = attempt();
    for n in 1..5 {
        let why = too_busy(&last.2);
        if why.is_empty() {
            break;
        }
        let name = std::thread::current().name().unwrap_or("?").to_string();
        let line = format!("{name}\tattempt {n} too busy: {}", why.join("; "));
        eprintln!("{line}");
        if let Ok(log) = std::env::var("CARCINIFY_SPEED_ATTEMPTS") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
            {
                let _ = writeln!(f, "{line}");
            }
        }
        last = attempt();
    }
    last
}

/// One `speed run` at a time in this test binary. Each run keeps a driver
/// and decide's watching process computing without pause; several at once
/// on one machine make each other's runs busy, and every retry with them.
static TIMING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `speed run`, with the report beside the declaration.
fn run(
    declaration: &Path,
    wheels: &Path,
    extra: &[&Path],
    env: &[(&str, &str)],
) -> (i32, String, J) {
    timed_run(declaration, wheels, extra, env).0
}

/// `run`, and how long `speed run` itself took, without the wait for
/// another test's run to end.
fn timed_run(
    declaration: &Path,
    wheels: &Path,
    extra: &[&Path],
    env: &[(&str, &str)],
) -> ((i32, String, J), std::time::Duration) {
    let report = declaration.with_file_name("report.json");
    let _ = std::fs::remove_file(&report);
    let mut args = vec![
        Path::new("run"),
        declaration,
        Path::new("--wheels"),
        wheels,
        Path::new("--report"),
        &report,
    ];
    args.extend_from_slice(extra);
    let one = TIMING.lock().unwrap_or_else(|e| e.into_inner());
    let start = std::time::Instant::now();
    let (code, err) = speed(&args, env);
    let took = start.elapsed();
    drop(one);
    let json =
        std::fs::read_to_string(&report).map_or(J::Null, |t| serde_json::from_str(&t).unwrap());
    ((code, err, json), took)
}

#[test]
fn speed_run_refuses_wheels_that_speed_build_did_not_write() {
    let declaration = declare(
        "no-wheels",
        "suite-python.json",
        &[("nap", "cases.jsonl", "nap-2")],
    );
    let (code, err, report) = run(&declaration, &declaration.with_file_name("none"), &[], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("speed build"), "{err}");
    assert!(report.is_null());
    // A record that names a wheel which is missing, then one whose bytes
    // are not those hashed.
    let wheels = built(&declaration);
    let record_path = wheels.join("build.json");
    let mut record = read(&record_path);
    record["sides"]["candidate"]["wheel"] = json!({"file": "port.whl", "sha256": "0".repeat(64)});
    std::fs::write(&record_path, record.to_string()).unwrap();
    let (code, err, _) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("port.whl"), "{err}");
    std::fs::write(wheels.join("port.whl"), "not the wheel that was built").unwrap();
    let (code, err, _) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("SHA-256"), "{err}");
    // A record that another carcinify wrote: the report's tools block
    // would not describe the binary that times.
    let wheels = built(&declaration);
    let mut record = read(&record_path);
    record["tools"]["harness_sha256"] = json!("0".repeat(64));
    std::fs::write(&record_path, record.to_string()).unwrap();
    let (code, err, _) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("another carcinify"), "{err}");
}

/// A gate log as ci/gate.sh writes it, opened by the one rule just now for
/// the run id given, and a thread that appends a reading every second
/// until the returned flag is set.
fn open_gate(
    log: &Path,
    id: &str,
) -> (
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    let stamp = |e: u64| format!("{:02}:{:02}:{:02} {e}", e / 3600 % 24, e / 60 % 60, e % 60);
    let start = now() - 300;
    let mut text = format!(
        "2026-11-23 {} wait: 30 readings under 6, 10 s apart, run {id}, then: carcinify speed run\n",
        stamp(start)
    );
    for i in 0..30 {
        text.push_str(&format!(
            "{} load 1.00 1.00 1.00 ({} under 6 in a row)\n",
            stamp(start + 10 * i),
            i + 1
        ));
    }
    text.push_str(&format!(
        "{} gate open; start: carcinify speed run\n",
        stamp(start + 290)
    ));
    // The last reading of the wait was 10 s ago; the gate opened with it.
    std::fs::write(log, text).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (flag, log) = (stop.clone(), log.to_path_buf());
    let appender = std::thread::spawn(move || {
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let line = format!("{} load 1.00 1.00 1.00\n", stamp(now()));
            std::io::Write::write_all(
                &mut std::fs::OpenOptions::new().append(true).open(&log).unwrap(),
                line.as_bytes(),
            )
            .unwrap();
        }
    });
    (stop, appender)
}

#[test]
fn the_sides_alternate_in_pairs_and_a_port_ten_times_slower_gives_about_a_tenth() {
    let declaration = declare(
        "timed",
        "suite-python.json",
        &[
            ("nap", "cases.jsonl", "nap-2"),
            ("nap-0", "cases.jsonl", "nap-0"),
        ],
    );
    let wheels = built(&declaration);
    let (log, debug) = (
        declaration.with_file_name("gate.log"),
        declaration.with_file_name("debug.log"),
    );
    let id = "4f2a9c1e7b3d5a60";
    let (code, err, report) = quiet(&mut || {
        let _ = std::fs::remove_file(&debug);
        // Readings 10 s apart before the opening, then every second: the
        // gap from the opening to the first is 11 s at most.
        let (stop, appender) = open_gate(&log, id);
        let ran = run(
            &declaration,
            &wheels,
            &[
                Path::new("--load-log"),
                &log,
                Path::new("--hash-seed"),
                Path::new("1234"),
                Path::new("--debug-log"),
                &debug,
            ],
            &[("GATE_RUN_ID", id)],
        );
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        appender.join().unwrap();
        ran
    });
    assert_eq!(code, 0, "{err}\n{report:#}");
    assert_eq!(report["schema"], "carcinify/speed/report/1");
    // The tools block is the build's, once.
    let record = read(&wheels.join("build.json"));
    assert_eq!(report["tools"], record["tools"]);
    assert!(report["build"]["tools"].is_null(), "{report:#}");
    // nap-0's runs are two calls that sleep 0 ms: under 1 ms.
    assert_eq!(report["verdict"], "pass_with_flags", "{report:#}");
    assert_eq!(report["flags"], json!(["short_run: nap-0"]), "{report:#}");
    assert_eq!(
        report["hash_seed"],
        json!({"value": 1234, "chosen": "given"})
    );
    assert!(
        !report["gate"]["readings"].as_array().unwrap().is_empty(),
        "{report:#}"
    );
    // The gate's header, its rule, and the readings the rule was checked on.
    let gate = &report["gate"];
    assert!(
        gate["header"]
            .as_str()
            .unwrap()
            .contains(&format!(", run {id}, then: ")),
        "{gate:#}"
    );
    assert_eq!(
        gate["rule"],
        json!({"readings": 30, "under": 6.0, "seconds_apart": 10})
    );
    let opening = gate["opening"].as_array().unwrap();
    assert_eq!(opening.len(), 30, "{gate:#}");
    assert!(opening.iter().all(|r| r["one_minute"] == 1.0), "{gate:#}");
    let nap = &report["targets"][0];
    assert_eq!(nap["name"], "nap");
    // decide's process beside the measurement, kept for each target on a
    // machine with 4 CPUs or more.
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get());
    let beside = &nap["watched_beside"];
    if cpus >= 4 {
        assert!(
            beside["kept_off_the_cpu"].is_f64() && beside["seconds"].as_f64().unwrap() > 0.1,
            "{nap:#}"
        );
    } else {
        assert!(beside.is_null(), "{nap:#}");
    }
    let ratio = nap["ratio"]["median"].as_f64().unwrap();
    assert!(
        (0.05..0.2).contains(&ratio),
        "original ÷ port is {ratio}: {nap:#}"
    );
    assert_eq!(
        (nap["pairs"].as_u64(), nap["short_run"].as_bool()),
        (Some(3), Some(false))
    );
    assert!(nap["port"]["median"].as_f64().unwrap() > 0.02, "{nap:#}");
    // The debug log: the hash seed, then for each target an untimed
    // gc.collect() before each run, the runs in ABBA order.
    let text = std::fs::read_to_string(&debug).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("hash seed 1234"), "{text}");
    let runs: Vec<&str> = lines
        .iter()
        .filter(|l| l.starts_with("nap: run "))
        .map(|l| l.split_whitespace().nth(3).unwrap())
        .collect();
    assert_eq!(
        runs,
        [
            "original,",
            "port,",
            "port,",
            "original,",
            "original,",
            "port,"
        ],
        "{text}"
    );
    for (i, l) in lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains(": run "))
    {
        assert!(lines[i - 1].ends_with(": gc.collect()"), "{text}");
        assert!(l.contains(", 2 calls, timed"), "{text}");
    }
}

#[test]
fn a_run_not_under_the_gate_is_unreliable_and_keeps_its_figures() {
    let declaration = declare(
        "ungated",
        "suite-python.json",
        &[("nap", "cases.jsonl", "nap-2")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(code, 1, "{err}");
    assert_eq!(report["verdict"], "unreliable");
    assert!(
        report["reasons"][0]
            .as_str()
            .unwrap()
            .starts_with("no_gate"),
        "{report:#}"
    );
    assert!(
        report["targets"][0]["ratio"]["median"].is_f64(),
        "{report:#}"
    );
}

#[test]
fn a_timed_call_whose_output_differs_from_the_originals_fails() {
    let declaration = declare(
        "differs",
        "suite-python.json",
        &[("wrong", "cases.jsonl", "wrong-1")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(code, 1, "{err}");
    assert_eq!(report["verdict"], "fail", "{report:#}");
    let target = &report["targets"][0];
    assert!(
        target["failed"]
            .as_str()
            .unwrap()
            .starts_with("output_differs"),
        "{target:#}"
    );
    assert!(
        target["ratio"].is_null(),
        "no figure for a call not compared: {target:#}"
    );
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn speed_run_times_the_pyo3_port_from_the_wheel_speed_build_wrote() {
    let declaration = declare("pyo3-run", "suite.json", &[("nap", "cases.jsonl", "nap-2")]);
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("unreliable")),
        "{err}\n{report:#}"
    );
    assert_eq!(report["build"]["sides"]["candidate"]["kind"], "pyo3");
    let ratio = report["targets"][0]["ratio"]["median"].as_f64().unwrap();
    assert!(
        (0.05..0.2).contains(&ratio),
        "original ÷ port is {ratio}: {report:#}"
    );
}

#[test]
fn a_function_carcinify_diff_would_refuse_is_not_timed() {
    let declaration = declare(
        "refused-function",
        "suite-python.json",
        &[("basename", "cases.jsonl", "basename-a")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(code, 1, "{err}");
    assert_eq!(report["verdict"], "fail", "{report:#}");
    let failed = report["targets"][0]["failed"].as_str().unwrap();
    assert!(
        failed.starts_with("function_refused: original: refused: 'basename' is defined in "),
        "{failed}"
    );
    assert!(
        report["targets"][0]["runs"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{report:#}"
    );
}

#[test]
fn a_function_that_changes_its_arguments_gets_a_fresh_copy_for_each_call() {
    let declaration = declare(
        "changes-arguments",
        "suite-python.json",
        &[
            ("pop", "cases.jsonl", "pop-first"),
            ("nap", "cases.jsonl", "nap-0"),
        ],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    // Not under the gate, so unreliable; but nothing failed.
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("unreliable")),
        "{err}\n{report:#}"
    );
    let pop = &report["targets"][0];
    assert!(pop["failed"].is_null(), "{pop:#}");
    assert_eq!(
        pop["arguments_changed_by_call"],
        json!({"original": true, "port": true}),
        "{pop:#}"
    );
    assert!(pop["ratio"]["median"].is_f64(), "{pop:#}");
    assert_eq!(
        report["targets"][1]["arguments_changed_by_call"],
        json!({"original": false, "port": false})
    );
}

#[test]
fn a_target_that_runs_past_the_time_limit_fails_and_the_next_is_still_timed() {
    let declaration = declare(
        "time-limit",
        "suite-python.json",
        &[
            ("hang", "cases.jsonl", "hang"),
            ("nap", "cases.jsonl", "nap-2"),
        ],
    );
    // speed build calls hang once too, untimed, under the same limit.
    let wheels = declaration.with_file_name("wheels");
    let limit = [Path::new("--timeout-secs"), Path::new("5")];
    let (code, err) = speed(
        &[
            &[
                Path::new("build"),
                &declaration,
                Path::new("--out"),
                &wheels,
            ][..],
            &limit,
        ]
        .concat(),
        &[],
    );
    assert_eq!(code, 0, "{err}");
    // Each attempt timed alone: the retries quiet() makes when the machine
    // is busy, and the waits for other tests' runs, are not this run's.
    let mut took = Vec::new();
    let (code, err, report) = quiet(&mut || {
        let (ran, t) = timed_run(&declaration, &wheels, &limit, &[]);
        took.push(t);
        ran
    });
    let attempts = format!("{} attempt(s), each taking {took:?}", took.len());
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{attempts}\n{err}\n{report:#}"
    );
    let failed = report["targets"][0]["failed"].as_str().unwrap_or("");
    assert!(
        failed.starts_with("timeout: no result within 5 s"),
        "{attempts}\n{report:#}"
    );
    assert!(
        report["targets"][1]["ratio"]["median"].is_f64(),
        "{attempts}\n{report:#}"
    );
    // hang's first call alone takes 60 s, so an attempt under 60 s shows
    // the limit cut it short. The limit applies to every target's driver:
    // nap's, with Python's start and its calibrations, took more than 1 s
    // on a busy machine, so the limit is 5 s.
    assert!(
        took.iter().all(|t| *t < std::time::Duration::from_secs(60)),
        "{attempts}"
    );
}

/// In a repository of its own, a copy of the Python fixture: what speed run
/// times must be what speed build built for, in a tree that matches its
/// commit.
#[test]
fn speed_run_refuses_what_speed_build_did_not_build_for() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-bound");
    let _ = std::fs::remove_dir_all(&dir);
    let repo = dir.join("repo");
    for sub in ["py", "py-port"] {
        std::fs::create_dir_all(repo.join(sub)).unwrap();
    }
    for file in [
        "py/speed_orig.py",
        "py-port/speed_port_py.py",
        "cases.jsonl",
        "suite-python.json",
    ] {
        std::fs::copy(fixture().join(file), repo.join(file)).unwrap();
    }
    let declaration = repo.join("speed.json");
    let declare = |runs: u32| {
        let text = json!({"schema": "carcinify/speed/declaration/1", "suite": "suite-python.json", "runs": runs,
            "targets": [{"name": "nap", "case": {"file": "cases.jsonl", "id": "nap-2"}, "calls_per_run": 2}]});
        std::fs::write(&declaration, text.to_string()).unwrap();
    };
    declare(3);
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo)
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
    };
    std::fs::write(repo.join(".gitignore"), "target/\n").unwrap();
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "fixture"]);
    // RUST-165: wheels written where git sees them would leave the tree
    // unlike its commit, and speed run would refuse it: refused before
    // anything is built. Where git ignores them, they may go.
    for (out, refused) in [
        (repo.join("wheels"), true),
        (repo.join("target/wheels"), false),
    ] {
        let ran = Command::new(env!("CARGO_BIN_EXE_carcinify"))
            .args(["speed", "build"])
            .arg(&declaration)
            .arg("--out")
            .arg(&out)
            .env_remove("GATE_RUN_ID")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&ran.stderr);
        if refused {
            assert_eq!(ran.status.code(), Some(2), "{err}");
            assert!(err.contains("target/ or outside the repository"), "{err}");
            assert!(!out.exists(), "{err}");
        } else {
            assert_eq!(ran.status.code(), Some(0), "{err}");
        }
    }
    let wheels = dir.join("wheels");
    let report = dir.join("report.json");
    let speed = |step: &str, extra: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_carcinify"));
        command
            .arg("speed")
            .arg(step)
            .arg(&declaration)
            .env_remove("GATE_RUN_ID");
        match step {
            "build" => command.arg("--out").arg(&wheels),
            _ => command
                .arg("--wheels")
                .arg(&wheels)
                .arg("--report")
                .arg(&report),
        };
        let out = command.args(extra).output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (code, err) = speed("build", &[]);
    assert_eq!(code, 0, "{err}");
    let build = read(&wheels.join("build.json"));
    assert_eq!(build["dirty"], false);
    assert!(build["python"]["version"].is_string(), "{build:#}");

    // The original changed after the build: ten times slower, same output.
    let original = repo.join("py/speed_orig.py");
    let text = std::fs::read_to_string(&original).unwrap();
    std::fs::write(
        &original,
        text.replace("time.sleep(ms / 1000)", "time.sleep(10 * ms / 1000)"),
    )
    .unwrap();
    let (code, err) = speed("run", &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("uncommitted changes"), "{err}");
    let (code, err) = speed("build", &[]);
    assert_eq!(
        code, 2,
        "a build too refuses a tree that does not match its commit: {err}"
    );
    // With --allow-dirty too: these changes are not the build's, which had
    // none.
    let (code, err) = speed("run", &["--allow-dirty"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("uncommitted changes differ"), "{err}");
    // Built and timed with the same changes, the report says so.
    let (code, err) = speed("build", &["--allow-dirty"]);
    assert_eq!(code, 0, "{err}");
    let (code, err) = speed("run", &["--allow-dirty"]);
    assert_eq!(code, 1, "{err}");
    let run = read(&report);
    assert_eq!(
        (&run["run"]["dirty"], &run["build"]["dirty"]),
        (&json!(true), &json!(true)),
        "{run:#}"
    );
    assert!(run["run"]["dirty_sha256"].is_string(), "{run:#}");
    assert_eq!(run["run"]["dirty_sha256"], run["build"]["dirty_sha256"]);
    assert_eq!(run["run"]["suite_sha256"], build["suite_sha256"]);
    // A file git does not track, added since: other changes.
    std::fs::write(repo.join("py/notes.txt"), "more").unwrap();
    let (code, err) = speed("run", &["--allow-dirty"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("uncommitted changes differ"), "{err}");
    std::fs::remove_file(repo.join("py/notes.txt")).unwrap();
    // Committed: another commit than the build's.
    git(&["commit", "-q", "-am", "slower"]);
    let (code, err) = speed("run", &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("commit"), "{err}");
    git(&["reset", "-q", "--hard", "HEAD~1"]);
    let (code, err) = speed("build", &[]);
    assert_eq!(code, 0, "built again on the clean tree: {err}");
    // Another declaration, another suite.
    declare(4);
    let (code, err) = speed("run", &["--allow-dirty"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("declaration"), "{err}");
    git(&["checkout", "-q", "--", "speed.json"]);
    std::fs::write(
        repo.join("suite-python.json"),
        std::fs::read_to_string(fixture().join("suite-python.json")).unwrap() + "\n",
    )
    .unwrap();
    let (code, err) = speed("run", &["--allow-dirty"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("suite"), "{err}");
    git(&["checkout", "-q", "--", "suite-python.json"]);
    // Another interpreter: the same one by another path is another
    // sys.executable, as a virtual environment's is.
    let python = std::env::var("CARCINIFY_PYTHON").unwrap_or_else(|_| "python3".into());
    let resolved = String::from_utf8(
        Command::new(&python)
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let other = dir.join("other-python");
    std::os::unix::fs::symlink(resolved.trim(), &other).unwrap();
    let (code, err) = speed("run", &["--python", other.to_str().unwrap()]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("interpreter"), "{err}");
    // And as it was built: timed.
    let (code, err) = speed("run", &[]);
    assert_eq!(code, 1, "unreliable, not under the gate: {err}");
    assert_eq!(read(&report)["run"]["dirty"], false);
}

/// A cached original would be timed on cache hits after the first call.
#[test]
fn a_cached_original_is_refused_as_carcinify_diff_refuses_it() {
    let declaration = declare(
        "cached",
        "suite-python.json",
        &[("square", "cases.jsonl", "square-3")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let failed = report["targets"][0]["failed"].as_str().unwrap_or("");
    assert_eq!(
        failed,
        "function_refused: original: refused: 'square' is a functools._lru_cache_wrapper, not a function",
        "{report:#}"
    );
    assert!(
        report["targets"][0]["runs"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{report:#}"
    );
}

/// Design §3: a caller is called twice, untimed, with the original in
/// place, and each call it makes of the compared function is looked up
/// among the suite's cases, before anything is timed.
#[test]
fn a_caller_is_built_with_its_calls_recorded_and_found_among_the_cases() {
    let declaration = declare_targets(
        "caller",
        "suite-python.json",
        vec![caller("two-naps", "two_naps", "nap")],
    );
    let wheels = built(&declaration);
    let calls = &read(&wheels.join("build.json"))["callers"]["two-naps"];
    // Two calls of nap(2), one input.
    assert_eq!(calls["calls"], 2, "{calls:#}");
    let lines = calls["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1, "{calls:#}");
    assert_eq!(lines[0]["fn"], "nap", "{calls:#}");
}

#[test]
fn a_caller_that_reaches_the_function_with_an_input_no_case_has_is_refused() {
    let declaration = declare_targets(
        "caller-no-case",
        "suite-python.json",
        vec![caller("no-case", "a_nap_no_case_has", "nap")],
    );
    let out = declaration.with_file_name("wheels");
    let (code, err) = speed(
        &[Path::new("build"), &declaration, Path::new("--out"), &out],
        &[],
    );
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("target \"no-case\": timed_input_not_compared: "),
        "{err}"
    );
    assert!(!out.join("build.json").exists());
}

#[test]
fn a_caller_whose_two_recordings_differ_is_refused() {
    let declaration = declare_targets(
        "caller-unstable",
        "suite-python.json",
        vec![caller("changing", "naps_that_change", "nap")],
    );
    let out = declaration.with_file_name("wheels");
    let (code, err) = speed(
        &[Path::new("build"), &declaration, Path::new("--out"), &out],
        &[],
    );
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("target \"changing\": timed_input_unstable: "),
        "{err}"
    );
}

#[test]
fn a_target_names_a_case_or_a_caller() {
    let mut both = caller("both", "two_naps", "nap");
    both["case"] = json!({"file": "cases.jsonl", "id": "nap-2"});
    let mut neither = caller("neither", "two_naps", "nap");
    neither.as_object_mut().unwrap().remove("caller");
    let mut missing = caller("missing", "two_naps", "nap");
    missing["caller"]["file"] = json!("callers/not-there.py");
    let declaration = declare_targets(
        "case-or-caller",
        "suite-python.json",
        vec![both, neither, missing],
    );
    let out = declaration.with_file_name("wheels");
    let (code, err) = speed(
        &[Path::new("build"), &declaration, Path::new("--out"), &out],
        &[],
    );
    assert_eq!(code, 1, "{err}");
    for name in ["both", "neither"] {
        assert!(
            err.contains(&format!(
                "target \"{name}\": a target names a case or a caller, one of them"
            )),
            "{err}"
        );
    }
    assert!(
        err.contains("target \"missing\": the caller's file \"callers/not-there.py\" is not there"),
        "{err}"
    );
}

/// Design §1: each side's function is put where the library calls it from,
/// and the caller is timed.
#[test]
fn a_caller_is_timed_with_each_sides_function_in_place() {
    let declaration = declare_targets(
        "caller-timed",
        "suite-python.json",
        vec![caller("two-naps", "two_naps", "nap")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("unreliable")),
        "not under the gate: {err}\n{report:#}"
    );
    let target = &report["targets"][0];
    assert!(target["failed"].is_null(), "{target:#}");
    assert_eq!(target["caller"]["function"], "two_naps", "{target:#}");
    let ratio = target["ratio"]["median"].as_f64().unwrap();
    // The port's nap sleeps ten times as long.
    assert!((0.05..0.2).contains(&ratio), "{ratio}: {target:#}");
}

#[test]
fn a_caller_whose_outputs_differ_fails() {
    let declaration = declare_targets(
        "caller-differs",
        "suite-python.json",
        vec![caller("wrong", "a_wrong", "wrong")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let failed = report["targets"][0]["failed"].as_str().unwrap_or("");
    assert!(failed.starts_with("output_differs: "), "{report:#}");
}

/// What is timed is what speed build checked: the run records the
/// original's untimed call again.
#[test]
fn a_caller_that_reaches_other_inputs_in_the_run_than_in_the_build_fails() {
    let declaration = declare_targets(
        "caller-elsewhere",
        "suite-python.json",
        vec![caller("set", "a_nap_set_by_the_environment", "nap")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = run(&declaration, &wheels, &[], &[("SPEED_FIXTURE_NAP", "0")]);
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let failed = report["targets"][0]["failed"].as_str().unwrap_or("");
    assert!(failed.starts_with("timed_input_unstable: "), "{report:#}");
    assert!(report["targets"][0]["ratio"].is_null(), "{report:#}");
}

/// The compiled port's function put in place of the library's.
#[test]
#[ignore = "needs maturin; see the top of this file"]
fn a_caller_is_timed_with_the_pyo3_ports_function_in_place() {
    let declaration = declare_targets(
        "pyo3-caller",
        "suite.json",
        vec![caller("two-naps", "two_naps", "nap")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("unreliable")),
        "{err}\n{report:#}"
    );
    assert_eq!(report["build"]["sides"]["candidate"]["kind"], "pyo3");
    let ratio = report["targets"][0]["ratio"]["median"].as_f64().unwrap();
    assert!(
        (0.05..0.2).contains(&ratio),
        "original ÷ port is {ratio}: {report:#}"
    );
}

/// Design §10 step 4: the caller refused for an input no case has passes
/// once the case is added. In a copy of the Python fixture with a git
/// repository of its own, whose cases can be changed.
#[test]
fn a_caller_refused_for_an_input_no_case_has_passes_with_the_case_added() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-caller-case-added");
    let _ = std::fs::remove_dir_all(&dir);
    let repo = Repo::new(&dir.join("fixture"), &PYTHON_FIXTURE);
    let declaration = dir.join("speed.json");
    let text = json!({"schema": "carcinify/speed/declaration/1", "suite": "fixture/suite-python.json", "runs": 3,
        "targets": [caller("no-case", "a_nap_no_case_has", "nap")]});
    std::fs::write(&declaration, text.to_string()).unwrap();
    let wheels = dir.join("wheels");
    let build = || {
        speed(
            &[
                Path::new("build"),
                &declaration,
                Path::new("--out"),
                &wheels,
            ],
            &[],
        )
    };
    let (code, err) = build();
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("timed_input_not_compared"), "{err}");
    let cases = repo.0.join("cases.jsonl");
    let mut text = std::fs::read_to_string(&cases).unwrap();
    text.push_str("{\"id\": \"nap-3\", \"fn\": \"nap\", \"args\": [3]}\n");
    std::fs::write(&cases, text).unwrap();
    repo.commit("the case nap-3");
    let (code, err) = build();
    assert_eq!(code, 0, "{err}");
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("unreliable")),
        "not under the gate: {err}\n{report:#}"
    );
    assert!(
        report["targets"][0]["ratio"]["median"].is_f64(),
        "{report:#}"
    );
}

/// speed build's work directory goes however the build ends.
#[test]
fn a_refused_build_leaves_no_work_directory_behind() {
    let declaration = declare_targets(
        "refused-work",
        "suite-python.json",
        vec![caller("no-case", "a_nap_no_case_has", "nap")],
    );
    let child = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .args(["speed", "build"])
        .arg(&declaration)
        .arg("--out")
        .arg(declaration.with_file_name("wheels"))
        .arg("--allow-dirty")
        .env_remove("GATE_RUN_ID")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let work = std::env::temp_dir().join(format!("carcinify-speed-build-{}", child.id()));
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!work.exists(), "{} is left behind", work.display());
}

/// A port that kills its interpreter fails its own target, named, and the
/// next target is timed in a driver of its own.
#[test]
fn a_target_whose_driver_crashes_fails_and_the_next_is_still_timed() {
    let declaration = declare(
        "crash",
        "suite-python.json",
        &[
            ("crash", "cases.jsonl", "crash-1"),
            ("nap", "cases.jsonl", "nap-2"),
        ],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let failed = report["targets"][0]["failed"].as_str().unwrap_or("");
    // SIGKILL, so that no system writes a crash report for the test.
    assert!(
        failed.starts_with("crashed: the driver exited (signal: 9 (SIGKILL)) without a result"),
        "{report:#}"
    );
    assert!(
        report["targets"][1]["ratio"]["median"].is_f64(),
        "{report:#}"
    );
}

/// A port that consumes or edits the caller's data where the original does
/// not differs in what a caller sees, though the outputs agree.
#[test]
fn a_port_that_changes_its_arguments_where_the_original_does_not_fails() {
    let declaration = declare(
        "changes-differently",
        "suite-python.json",
        &[("first", "cases.jsonl", "first-1")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = run(&declaration, &wheels, &[], &[]);
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let target = &report["targets"][0];
    assert_eq!(
        target["arguments_changed_by_call"],
        json!({"original": false, "port": true}),
        "{target:#}"
    );
    let failed = target["failed"].as_str().unwrap_or("");
    assert!(
        failed.starts_with("arguments_changed_differently: "),
        "{target:#}"
    );
}

/// RUST-166: a set's items, like the calls themselves, come in hash order,
/// and speed build and speed run each draw their own seed. The run is held
/// to the build as the measured check compares: by canonical value.
#[test]
fn a_caller_passing_a_set_of_strings_is_held_to_its_build_at_another_hash_seed() {
    let declaration = declare_targets(
        "caller-set",
        "suite-python.json",
        vec![caller("letters", "a_set_of_letters", "size")],
    );
    let wheels = declaration.with_file_name("wheels");
    let seed = |n: &'static str| [Path::new("--hash-seed"), Path::new(n)];
    let (code, err) = speed(
        &[
            &[
                Path::new("build"),
                &declaration,
                Path::new("--out"),
                &wheels,
            ][..],
            &seed("1"),
        ]
        .concat(),
        &[],
    );
    assert_eq!(code, 0, "{err}");
    for n in ["2", "3"] {
        let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &seed(n), &[]));
        assert_eq!(
            (code, &report["verdict"]),
            (1, &json!("unreliable")),
            "seed {n}: {err}\n{report:#}"
        );
        assert!(
            report["targets"][0]["failed"].is_null(),
            "seed {n}: {report:#}"
        );
    }
    // Another input still fails.
    let (code, err, report) = run(
        &declaration,
        &wheels,
        &seed("2"),
        &[("SPEED_FIXTURE_LETTERS", "abcdefgz")],
    );
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let failed = report["targets"][0]["failed"].as_str().unwrap_or("");
    assert!(failed.starts_with("timed_input_unstable: "), "{report:#}");
}

/// The Python fixture's files: both sides, the callers, the cases and the
/// suite.
const PYTHON_FIXTURE: [&str; 6] = [
    "py/speed_orig.py",
    "py-port/speed_port_py.py",
    "callers/callers.py",
    "callers/library.py",
    "cases.jsonl",
    "suite-python.json",
];

/// A git repository of its own under the tests' directory, holding copies
/// of the fixture's `files`, committed.
struct Repo(PathBuf);

impl Repo {
    fn new(dir: &Path, files: &[&str]) -> Repo {
        let _ = std::fs::remove_dir_all(dir);
        for file in files {
            std::fs::create_dir_all(dir.join(file).parent().unwrap()).unwrap();
            std::fs::copy(fixture().join(file), dir.join(file)).unwrap();
        }
        let repo = Repo(dir.to_path_buf());
        repo.git(&["init", "-q"]);
        repo.commit("fixture");
        repo
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.0)
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

    fn commit(&self, message: &str) {
        self.git(&["add", "."]);
        self.git(&["commit", "-q", "-m", message]);
    }
}

/// `carcinify speed <args>`, as given: no --allow-dirty added.
fn speed_as_given(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("speed")
        .args(args)
        .env_remove("GATE_RUN_ID")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// RUST-167: a module the sides import from inside the repository that git
/// does not track, here one it ignores, is refused as carcinify diff
/// refuses it. With --allow-dirty the build records its SHA-256, and the
/// run is held to it.
#[test]
fn a_module_git_ignores_is_refused_or_held_to_the_builds_copy() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-ignored-import");
    let repo = Repo::new(&dir.join("repo"), &PYTHON_FIXTURE);
    let original = repo.0.join("py/speed_orig.py");
    let text = std::fs::read_to_string(&original).unwrap();
    std::fs::write(&original, text + "\nimport vendored  # git ignores it\n").unwrap();
    std::fs::write(repo.0.join(".gitignore"), "py/vendored.py\n").unwrap();
    repo.commit("import a module git ignores");
    std::fs::write(repo.0.join("py/vendored.py"), "SCALE = 1\n").unwrap();
    let declaration = dir.join("speed.json");
    let text = json!({"schema": "carcinify/speed/declaration/1", "suite": "repo/suite-python.json", "runs": 3,
        "targets": [{"name": "nap", "case": {"file": "cases.jsonl", "id": "nap-2"}, "calls_per_run": 2}]});
    std::fs::write(&declaration, text.to_string()).unwrap();
    let (d, w, r) = (
        declaration.to_str().unwrap().to_string(),
        dir.join("wheels").to_str().unwrap().to_string(),
        dir.join("report.json").to_str().unwrap().to_string(),
    );
    let (code, err) = speed_as_given(&["build", &d, "--out", &w]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("py/vendored.py (imported by the timing driver)"),
        "{err}"
    );
    let (code, err) = speed_as_given(&["build", &d, "--out", &w, "--allow-dirty"]);
    assert_eq!(code, 0, "{err}");
    let built = read(&dir.join("wheels/build.json"));
    assert!(
        built["untracked_imported"]["py/vendored.py"].is_string(),
        "{built:#}"
    );
    let (code, err) = speed_as_given(&["run", &d, "--wheels", &w, "--report", &r, "--allow-dirty"]);
    assert_eq!(code, 1, "timed, not under the gate: {err}");
    std::fs::write(repo.0.join("py/vendored.py"), "SCALE = 10\n").unwrap();
    let (code, err) = speed_as_given(&["run", &d, "--wheels", &w, "--report", &r, "--allow-dirty"]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("py/vendored.py") && err.contains("changed since speed build"),
        "{err}"
    );
}

/// RUST-167: what is timed must be in the repository the commit describes,
/// as carcinify diff requires of a side's directories.
#[test]
fn a_side_or_a_callers_file_outside_the_repository_is_refused() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-outside");
    let repo = Repo::new(&dir.join("repo"), &PYTHON_FIXTURE);
    for file in [
        "callers/callers.py",
        "callers/library.py",
        "py/speed_orig.py",
    ] {
        std::fs::create_dir_all(dir.join(file).parent().unwrap()).unwrap();
        std::fs::copy(fixture().join(file), dir.join(file)).unwrap();
    }
    let suite = std::fs::read_to_string(repo.0.join("suite-python.json")).unwrap();
    std::fs::write(
        repo.0.join("suite-outside.json"),
        suite.replace("[\"py\"]", "[\"../py\"]"),
    )
    .unwrap();
    repo.commit("a suite whose original is outside");
    let mut outside_caller = caller("outside", "two_naps", "nap");
    outside_caller["caller"]["file"] = json!("../callers/callers.py");
    let direct =
        json!({"name": "nap", "case": {"file": "cases.jsonl", "id": "nap-2"}, "calls_per_run": 2});
    for (name, suite, target, says) in [
        (
            "caller",
            "repo/suite-python.json",
            outside_caller,
            "the caller's file",
        ),
        ("side", "repo/suite-outside.json", direct, "original: "),
    ] {
        let declaration = dir.join(format!("{name}.json"));
        let text = json!({"schema": "carcinify/speed/declaration/1", "suite": suite, "runs": 3, "targets": [target]});
        std::fs::write(&declaration, text.to_string()).unwrap();
        let out = dir.join(format!("{name}-wheels"));
        let (code, err) = speed_as_given(&[
            "build",
            declaration.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--allow-dirty",
        ]);
        assert_eq!(code, 2, "{name}: {err}");
        assert!(
            err.contains(says) && err.contains("is outside the repository"),
            "{name}: {err}"
        );
    }
}

/// Design §2, step 5: in each of `runs` new threads, a cheap call into the
/// port's module, then the port's first call there (cold) and its next
/// (warm), each timed alone. The fixture's port makes a workspace on a
/// thread's first nap, in 5 ms.
#[test]
fn a_cold_thread_is_reported_as_cold_minus_warm_paired_per_thread() {
    let declaration = declare(
        "cold",
        "suite-python.json",
        &[("nap", "cases.jsonl", "nap-2")],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("unreliable")),
        "{err}\n{report:#}"
    );
    let cold = &report["targets"][0]["cold_thread"];
    assert_eq!(
        cold["cheap_call"], "the compared function with no arguments",
        "{cold:#}"
    );
    assert_eq!(cold["threads"], 3, "{cold:#}");
    let median = cold["cold_minus_warm"]["median"]
        .as_f64()
        .unwrap_or(f64::NAN);
    assert!((0.002..0.05).contains(&median), "{median}: {cold:#}");
}

/// A call with no arguments would run a function that needs none, and warm
/// the thread: such a port exports _speed_noop for the cheap call.
#[test]
fn a_function_with_no_required_parameter_needs_speed_noop_for_its_cold_thread() {
    let declaration = declare(
        "cold-zero",
        "suite-python.json",
        &[("zero", "cases.jsonl", "zero")],
    );
    let wheels = built(&declaration);
    let (_, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    let flags: Vec<&str> = report["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(J::as_str)
        .collect();
    assert!(
        flags
            .iter()
            .any(|f| f.starts_with("cold_thread_not_measured: zero: ")),
        "{err}\n{report:#}"
    );
    assert!(
        report["targets"][0]["cold_thread"]["cold_minus_warm"].is_null(),
        "{report:#}"
    );
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-cold-noop");
    let repo = Repo::new(&dir.join("repo"), &PYTHON_FIXTURE);
    let port = repo.0.join("py-port/speed_port_py.py");
    let text = std::fs::read_to_string(&port).unwrap();
    std::fs::write(&port, text + "\n\ndef _speed_noop():\n    pass\n").unwrap();
    repo.commit("the port exports _speed_noop");
    let declaration = dir.join("speed.json");
    let text = json!({"schema": "carcinify/speed/declaration/1", "suite": "repo/suite-python.json", "runs": 3,
        "targets": [{"name": "zero", "case": {"file": "cases.jsonl", "id": "zero"}, "calls_per_run": 2}]});
    std::fs::write(&declaration, text.to_string()).unwrap();
    let (d, w, r) = (
        declaration.to_str().unwrap().to_string(),
        dir.join("wheels").to_str().unwrap().to_string(),
        dir.join("report.json").to_str().unwrap().to_string(),
    );
    let (code, err) = speed_as_given(&["build", &d, "--out", &w]);
    assert_eq!(code, 0, "{err}");
    let (code, err) = speed_as_given(&["run", &d, "--wheels", &w, "--report", &r]);
    assert_eq!(code, 1, "unreliable, not under the gate: {err}");
    let report = read(Path::new(&r));
    let cold = &report["targets"][0]["cold_thread"];
    assert_eq!(cold["cheap_call"], "_speed_noop", "{cold:#}");
    assert!(cold["cold_minus_warm"]["median"].is_f64(), "{cold:#}");
}

/// Design §10 step 5, on the PyO3 port: the cheap call, with no arguments,
/// raises while PyO3 parses the arguments, before the body takes the
/// thread's workspace; the first real call takes it.
#[test]
#[ignore = "needs maturin; see the top of this file"]
fn the_cheap_call_leaves_the_pyo3_ports_workspace_to_the_first_real_call() {
    let declaration = declare(
        "pyo3-cold",
        "suite.json",
        &[("nap", "cases.jsonl", "nap-2")],
    );
    let wheels = built(&declaration);
    let wheel = std::fs::read_dir(&wheels)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "whl"))
        .unwrap();
    let unpacked = declaration.with_file_name("unpacked");
    let python = std::env::var("CARCINIFY_PYTHON").unwrap_or_else(|_| "python3".into());
    let unzip = Command::new(&python)
        .args(["-I", "-m", "zipfile", "-e"])
        .arg(&wheel)
        .arg(&unpacked)
        .status()
        .unwrap();
    assert!(unzip.success());
    let script = r"
import sys, threading
sys.path.insert(0, sys.argv[1])
import speed_port
seen = []
def thread():
    before = speed_port.workspaces()
    try:
        speed_port.nap()
    except TypeError:
        pass
    cheap = speed_port.workspaces()
    speed_port.nap(0)
    first = speed_port.workspaces()
    speed_port.nap(0)
    seen.append((before, cheap, first, speed_port.workspaces()))
t = threading.Thread(target=thread)
t.start()
t.join()
print(seen[0])
";
    let out = Command::new(&python)
        .args(["-I", "-c", script])
        .arg(&unpacked)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "(0, 0, 1, 1)",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("unreliable")),
        "{err}\n{report:#}"
    );
    let cold = &report["targets"][0]["cold_thread"];
    let median = cold["cold_minus_warm"]["median"]
        .as_f64()
        .unwrap_or(f64::NAN);
    assert!((0.002..0.05).contains(&median), "{median}: {cold:#}");
}

/// Step 5, decision 4: a port that raises on a new thread where the
/// original does not fails, with the port's traceback; where the original
/// raises too, the cold thread is not measured, a flag.
#[test]
fn a_port_that_raises_only_on_a_new_thread_fails() {
    let declaration = declare(
        "new-thread-raises",
        "suite-python.json",
        &[
            ("lookup", "cases.jsonl", "lookup-a"),
            ("main-only", "cases.jsonl", "main-only-1"),
        ],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let lookup = &report["targets"][0];
    let failed = lookup["failed"].as_str().unwrap_or("");
    assert!(
        failed.starts_with("raised_in_new_thread: AttributeError"),
        "{lookup:#}"
    );
    let traceback = lookup["cold_thread"]["raised_in_new_thread"]
        .as_str()
        .unwrap_or("");
    assert!(traceback.contains("_imported.table[key]"), "{lookup:#}");
    assert!(report["targets"][1]["failed"].is_null(), "{report:#}");
    let flags: Vec<&str> = report["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(J::as_str)
        .collect();
    assert!(
        flags
            .iter()
            .any(|f| f.starts_with("cold_thread_not_measured: main-only: ")
                && f.contains("the original raised there too")),
        "{flags:#?}"
    );
}

/// Step 5, decision 5: the port's outputs on a new thread are compared
/// with the original's, as its first output is. Per-thread state that
/// gives another answer on a fresh thread fails.
#[test]
fn a_port_that_answers_otherwise_on_a_new_thread_fails() {
    let declaration = declare(
        "new-thread-differs",
        "suite-python.json",
        &[
            ("doubled", "cases.jsonl", "doubled-3"),
            ("nap", "cases.jsonl", "nap-2"),
        ],
    );
    let wheels = built(&declaration);
    let (code, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let doubled = &report["targets"][0];
    let failed = doubled["failed"].as_str().unwrap_or("");
    assert!(
        failed.starts_with("output_differs_in_new_thread: thread 1, first call: at "),
        "{doubled:#}"
    );
    assert!(doubled["difference"].is_object(), "{doubled:#}");
    assert!(report["targets"][1]["failed"].is_null(), "{report:#}");
}

/// Step 5, decision 6: a target that already failed gets no cold thread.
/// Its driver never makes one, and the report says why.
#[test]
fn a_target_that_already_failed_gets_no_cold_thread() {
    let declaration = declare(
        "cold-failed",
        "suite-python.json",
        &[
            ("wrong", "cases.jsonl", "wrong-1"),
            ("nap", "cases.jsonl", "nap-2"),
        ],
    );
    let wheels = built(&declaration);
    let log = declaration.with_file_name("debug.log");
    let (code, err, report) = quiet(&mut || {
        let _ = std::fs::remove_file(&log);
        run(
            &declaration,
            &wheels,
            &[Path::new("--debug-log"), &log],
            &[],
        )
    });
    assert_eq!(
        (code, &report["verdict"]),
        (1, &json!("fail")),
        "{err}\n{report:#}"
    );
    let wrong = &report["targets"][0]["cold_thread"];
    assert_eq!(
        (&wrong["not_measured"], &wrong["threads"]),
        (&json!("target failed"), &json!(0)),
        "{wrong:#}"
    );
    let log = std::fs::read_to_string(&log).unwrap();
    assert!(
        !log.contains("wrong: cold thread") && log.contains("nap: cold thread, 3 threads"),
        "{log}"
    );
}

/// RUST-168: both sides are imported into one process, so a top-level
/// module name both sides' directories provide would resolve to one
/// side's file for both. speed build refuses it before building.
#[test]
fn a_module_name_both_sides_provide_is_refused() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-same-helper");
    let repo = Repo::new(&dir.join("repo"), &PYTHON_FIXTURE);
    std::fs::write(repo.0.join("py/helper.py"), "FACTOR = 1\n").unwrap();
    std::fs::write(repo.0.join("py-port/helper.py"), "FACTOR = 10\n").unwrap();
    repo.commit("a helper of the same name on each side");
    let declaration = dir.join("speed.json");
    let text = json!({"schema": "carcinify/speed/declaration/1", "suite": "repo/suite-python.json", "runs": 3,
        "targets": [{"name": "nap", "case": {"file": "cases.jsonl", "id": "nap-2"}, "calls_per_run": 2}]});
    std::fs::write(&declaration, text.to_string()).unwrap();
    let wheels = dir.join("wheels");
    let _ = std::fs::remove_dir_all(&wheels);
    let (code, err) = speed_as_given(&[
        "build",
        declaration.to_str().unwrap(),
        "--out",
        wheels.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("helper (py/helper.py and py-port/helper.py)")
            && err.contains("one Python process"),
        "{err}"
    );
    assert!(!wheels.exists(), "nothing is built");
}

/// RUST-168: the sides' directories stay on sys.path for the whole
/// driver, so that a module imported only when called is found, as in
/// carcinify diff.
#[test]
fn a_sibling_module_imported_when_called_is_timed() {
    let declaration = declare(
        "lazy",
        "suite-python.json",
        &[("lazy", "cases.jsonl", "lazy-1")],
    );
    let wheels = built(&declaration);
    let (_, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    let lazy = &report["targets"][0];
    assert!(
        lazy["failed"].is_null() && lazy["ratio"]["median"].is_f64(),
        "{err}\n{lazy:#}"
    );
}

/// RUST-169: the cheap call is decided by the compared function's
/// signature, before any call. A function that binds with no arguments,
/// here nap(ms=None) whose body makes the workspace and then raises
/// TypeError, needs _speed_noop: its cold thread is not measured, a flag.
#[test]
fn a_function_whose_parameters_all_have_defaults_needs_speed_noop() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-cold-defaults");
    let repo = Repo::new(&dir.join("repo"), &PYTHON_FIXTURE);
    let port = repo.0.join("py-port/speed_port_py.py");
    let text = std::fs::read_to_string(&port)
        .unwrap()
        .replacen("def nap(ms):", "def nap(ms=None):", 1)
        .replacen(
            "    if ms:\n        time.sleep(10 * ms / 1000)\n",
            "    time.sleep(10 * ms / 1000)\n",
            1,
        );
    assert!(
        text.contains("def nap(ms=None):") && !text.contains("    if ms:\n        time.sleep(10")
    );
    std::fs::write(&port, text).unwrap();
    repo.commit("nap(ms=None), whose body raises TypeError after making the workspace");
    let declaration = dir.join("speed.json");
    let text = json!({"schema": "carcinify/speed/declaration/1", "suite": "repo/suite-python.json", "runs": 3,
        "targets": [{"name": "nap", "case": {"file": "cases.jsonl", "id": "nap-2"}, "calls_per_run": 2}]});
    std::fs::write(&declaration, text.to_string()).unwrap();
    let (d, w, r) = (
        declaration.to_str().unwrap().to_string(),
        dir.join("wheels").to_str().unwrap().to_string(),
        dir.join("report.json").to_str().unwrap().to_string(),
    );
    let (code, err) = speed_as_given(&["build", &d, "--out", &w]);
    assert_eq!(code, 0, "{err}");
    let (code, err) = speed_as_given(&["run", &d, "--wheels", &w, "--report", &r]);
    assert_eq!(code, 1, "unreliable, not under the gate: {err}");
    let report = read(Path::new(&r));
    let flags: Vec<&str> = report["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(J::as_str)
        .collect();
    assert!(
        flags
            .iter()
            .any(|f| f.starts_with("cold_thread_not_measured: nap: ") && f.contains("_speed_noop")),
        "{flags:#?}"
    );
    assert!(
        report["targets"][0]["cold_thread"]["cold_minus_warm"].is_null(),
        "{report:#}"
    );
}

/// RUST-170: speed build calls each direct target once on each side,
/// untimed, so that a module imported only when called is seen. Refused
/// without --allow-dirty when git does not track it; with it, recorded,
/// and the run proceeds.
#[test]
fn a_module_imported_only_when_called_is_seen_by_speed_build() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-lazy-ignored");
    let repo = Repo::new(&dir.join("repo"), &PYTHON_FIXTURE);
    std::fs::write(repo.0.join(".gitignore"), "py-port/lazy_helper.py\n").unwrap();
    repo.commit("git ignores the lazy helper");
    std::fs::copy(
        fixture().join("py-port/lazy_helper.py"),
        repo.0.join("py-port/lazy_helper.py"),
    )
    .unwrap();
    let declaration = dir.join("speed.json");
    let text = json!({"schema": "carcinify/speed/declaration/1", "suite": "repo/suite-python.json", "runs": 3,
        "targets": [{"name": "lazy", "case": {"file": "cases.jsonl", "id": "lazy-1"}, "calls_per_run": 2}]});
    std::fs::write(&declaration, text.to_string()).unwrap();
    let (d, w, r) = (
        declaration.to_str().unwrap().to_string(),
        dir.join("wheels").to_str().unwrap().to_string(),
        dir.join("report.json").to_str().unwrap().to_string(),
    );
    let (code, err) = speed_as_given(&["build", &d, "--out", &w]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("py-port/lazy_helper.py (imported by the timing driver)"),
        "{err}"
    );
    let (code, err) = speed_as_given(&["build", &d, "--out", &w, "--allow-dirty"]);
    assert_eq!(code, 0, "{err}");
    let built = read(&dir.join("wheels/build.json"));
    assert!(
        built["untracked_imported"]["py-port/lazy_helper.py"].is_string(),
        "{built:#}"
    );
    let (code, err) = speed_as_given(&["run", &d, "--wheels", &w, "--report", &r, "--allow-dirty"]);
    assert_eq!(code, 1, "timed, not under the gate: {err}");
}

/// Design §4: a Python port has no binding whose allocations could be
/// counted. Its counts say so, with no flag.
#[test]
fn a_python_port_is_not_counted() {
    let declaration = declare(
        "python-counts",
        "suite-python.json",
        &[("nap", "cases.jsonl", "nap-2")],
    );
    let wheels = built(&declaration);
    let (_, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    let counts = &report["targets"][0]["counts"];
    assert!(
        counts["not_counted"]
            .as_str()
            .unwrap_or("")
            .contains("Python"),
        "{err}\n{counts:#}"
    );
    let flags: Vec<&str> = report["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(J::as_str)
        .collect();
    assert!(
        !flags.iter().any(|f| f.starts_with("counts_not_measured")),
        "{flags:#?}"
    );
}

/// Design §10 step 6, with RUST-150: speed build counts the port's Rust
/// allocations with a second wheel, built with count_allocations and never
/// timed. The fixture's fill(100) makes 100 boxes and the vector that holds
/// them, and frees them all: the warm counts are exact. Its first call also
/// makes the 4096-byte workspace it keeps, reported apart.
#[test]
#[ignore = "needs maturin; see the top of this file"]
fn the_ports_allocations_are_counted_warm_with_the_first_call_apart() {
    let declaration = declare(
        "pyo3-counts",
        "suite.json",
        &[("fill", "cases.jsonl", "fill-100")],
    );
    let wheels = built(&declaration);
    let (_, err, report) = quiet(&mut || run(&declaration, &wheels, &[], &[]));
    let counts = &report["targets"][0]["counts"];
    assert_eq!(
        counts["warm"],
        json!({"allocations": 101, "frees": 101, "bytes": 1600, "live_bytes": 0}),
        "{err}\n{counts:#}"
    );
    assert_eq!(counts["first"]["live_bytes"], 4096, "{counts:#}");
    assert_eq!(counts["workspace_kept_bytes"], 4096, "{counts:#}");
    // fill's result alone, one int: the call is made as the caller makes
    // it, with nothing of the tool's left beside it.
    let python = &counts["python"];
    assert_eq!(
        python["objects_left_to_the_caller"]["blocks"], 1,
        "{counts:#}"
    );
    assert!(python["peak_bytes"].is_u64(), "{counts:#}");
    let what = counts["what"].as_str().unwrap_or("");
    assert!(
        what.contains("nothing subtracted") && what.contains("carcinify structure"),
        "{what}"
    );
    // The counted wheel is the build's own: only the timed wheel is kept.
    let kept = std::fs::read_dir(&wheels)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "whl"))
        .count();
    assert_eq!(kept, 1);
}

/// Design §10 step 6: a timed wheel built with count_allocations is
/// refused, since its counting allocator would be timed.
#[test]
#[ignore = "needs maturin; see the top of this file"]
fn a_timed_wheel_built_with_count_allocations_is_refused() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("speed-counted-timed-suite");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let suite = dir.join("suite.json");
    let cases = fixture().join("cases.jsonl");
    let text = json!({"schema": "carcinify/diff/suite/1",
        "original": {"kind": "python", "path": [fixture().join("py")], "module": "speed_orig"},
        "candidate": {"kind": "pyo3", "crate": fixture().join("rs"), "module": "speed_port", "features": ["count_allocations"]},
        "cases": [&cases]});
    std::fs::write(&suite, text.to_string()).unwrap();
    let declaration = declare(
        "counted-timed",
        suite.to_str().unwrap(),
        &[("nap", cases.to_str().unwrap(), "nap-2")],
    );
    let wheels = declaration.with_file_name("wheels");
    let (code, err) = speed(
        &[
            Path::new("build"),
            &declaration,
            Path::new("--out"),
            &wheels,
        ],
        &[],
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("count_allocations") && err.contains("would be timed"),
        "{err}"
    );
}
