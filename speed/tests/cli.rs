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
        candidate["target"]["target_cpu"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // Twice: the second build finds the crate fresh, and Cargo prints its
    // command line only for a crate it compiles.
    for _ in 0..2 {
        let cpu = built(&[]);
        assert!(cpu.starts_with("default of "), "{cpu}");
    }
    assert_eq!(built(&[("RUSTFLAGS", "-C target-cpu=generic")]), "generic");
}
