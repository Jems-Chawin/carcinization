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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("speed-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let targets: Vec<J> = targets
        .iter()
        .map(|(target, file, id)| {
            json!({"name": target, "case": {"file": file, "id": id}, "calls_per_run": 2})
        })
        .collect();
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

/// `carcinify speed <args>`, never under a gate unless `env` says so.
fn speed(args: &[&Path], env: &[(&str, &str)]) -> (i32, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_carcinify"));
    command.arg("speed").args(args).env_remove("GATE_RUN_ID");
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

/// Whether every pair of some target was dropped as disturbed: the
/// machine was too busy for the run.
fn too_busy(report: &J) -> bool {
    report["reasons"].as_array().into_iter().flatten().any(|r| {
        r.as_str()
            .is_some_and(|r| r.starts_with("every_pair_dropped"))
    })
}

/// A run, again while the machine was too busy for it, up to five times.
fn quiet(attempt: &mut dyn FnMut() -> (i32, String, J)) -> (i32, String, J) {
    let mut last = attempt();
    for _ in 1..5 {
        if !too_busy(&last.2) {
            break;
        }
        last = attempt();
    }
    last
}

/// `speed run`, with the report beside the declaration.
fn run(
    declaration: &Path,
    wheels: &Path,
    extra: &[&Path],
    env: &[(&str, &str)],
) -> (i32, String, J) {
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
    let (code, err) = speed(&args, env);
    let json =
        std::fs::read_to_string(&report).map_or(J::Null, |t| serde_json::from_str(&t).unwrap());
    (code, err, json)
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
    let nap = &report["targets"][0];
    assert_eq!(nap["name"], "nap");
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
