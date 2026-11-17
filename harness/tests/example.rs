//! The worked example, end to end through maturin. Ignored by default
//! because it needs maturin 1.16 and a first build downloads crates:
//!
//!     CARCINIFY_PYTHON=$PWD/.venv/bin/python CARCINIFY_MATURIN=$PWD/.venv/bin/maturin \
//!         cargo test -p carcinify --test diff-example -- --ignored
//!
//! It runs on the working tree as it is, so it passes `--allow-dirty`.

use serde_json::Value as J;
use std::path::Path;
use std::process::Command;

fn run(variant: &str) -> (i32, J) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let report = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("example-{variant}.json"));
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .arg("diff")
        .arg(repo.join(format!("examples/float_roundtrip/suite-{variant}.json")))
        .arg("--report")
        .arg(&report)
        .arg("--allow-dirty")
        .output()
        .unwrap();
    let text = std::fs::read_to_string(&report).unwrap();
    let json: J = serde_json::from_str(&text).unwrap();
    assert_ne!(
        json["verdict"],
        "error",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap(), json)
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn the_default_serde_json_build_fails_on_the_data_file() {
    let (code, report) = run("buggy");
    assert_eq!(code, 1);
    let case = &report["cases"][0];
    assert_eq!(case["id"], "data-file-ledger");
    let d = &case["first_difference"];
    assert_eq!(d["path"], "$[2][\"amount\"]");
    assert_eq!(d["record"]["\"id\""], "1003");
    assert_eq!(d["original"]["repr"], "396.03999999999996");
    assert_eq!(d["original"]["bits"], "0x4078c0a3d70a3d70");
    assert_eq!(d["candidate"]["repr"], "396.04");
    assert_eq!(d["candidate"]["bits"], "0x4078c0a3d70a3d71");
    assert_eq!(report["sides"]["candidate"]["kind"], "pyo3");
    assert!(
        report["tools"]["maturin"]
            .as_str()
            .unwrap()
            .starts_with("maturin 1.16")
    );
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn the_float_roundtrip_build_passes() {
    let (code, report) = run("fixed");
    assert_eq!(code, 0);
    assert_eq!(report["verdict"], "pass");
    assert_eq!(report["summary"]["identical"], report["summary"]["cases"]);
    assert_eq!(report["summary"]["identical_with_relaxations"], 0);
    // The cases call `load` and `roundtrip`; `first_amount` has a suite of
    // its own (suite-panic.json), and `save` is exercised only through
    // `roundtrip`, and the report says so on both sides.
    let not_called = &report["checks"]["public_functions_not_called"];
    assert_eq!(
        not_called["original"],
        serde_json::json!(["first_amount", "save"])
    );
    assert_eq!(
        not_called["candidate"],
        serde_json::json!(["first_amount", "save"])
    );
    assert_eq!(report["git"]["used_but_not_tracked"], serde_json::json!([]));
    // Both functions are compiled and did their work without calling Python.
    for f in report["checks"]["functions"].as_array().unwrap() {
        assert_eq!(
            f["candidate_work"], "extension, 0 Python calls",
            "{}",
            f["fn"]
        );
    }
}

/// Copies a directory, writing each file anew, without `target`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dest) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().unwrap().is_dir() {
            if entry.file_name() != "target" {
                copy_tree(&src, &dest);
            }
        } else {
            std::fs::write(dest, std::fs::read(src).unwrap()).unwrap();
        }
    }
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn a_path_dependency_changed_under_an_old_file_time_is_rebuilt() {
    // The auditor's reproduction of the stale build. ledger-core is a path
    // dependency of the PyO3 crate. It is changed to add 1.0 to every amount
    // it reads, the file keeps its old modification time, and the change is
    // committed. Cargo, going by times, would reuse the library it built
    // before and all 68 cases would still pass.
    let dir = std::env::temp_dir().join(format!("carcinify-diff-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    copy_tree(&repo.join("examples/float_roundtrip"), &dir);
    std::fs::write(dir.join(".gitignore"), "out/\ntarget/\n__pycache__/\n").unwrap();
    std::fs::write(
        dir.join("rust-toolchain.toml"),
        std::fs::read(repo.join("rust-toolchain.toml")).unwrap(),
    )
    .unwrap();
    let dir = dir.canonicalize().unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&dir)
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
    let run = || {
        let report = dir.join("out/report.json");
        let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
            .arg("diff")
            .arg(dir.join("suite-fixed.json"))
            .arg("--report")
            .arg(&report)
            .output()
            .unwrap();
        let json: J = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
        (
            out.status.code().unwrap(),
            json,
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "one"]);
    let (code, report, stderr) = run();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(report["summary"]["identical"], 68);

    // An unchanged tree is not rebuilt.
    let (code, report, stderr) = run();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(report["sides"]["candidate"]["rebuilt_from_clean"], false);

    let core = dir.join("rust/core/src/lib.rs");
    let old_time = std::fs::metadata(&core).unwrap().modified().unwrap();
    let text = std::fs::read_to_string(&core).unwrap();
    let reads = "records.push(serde_json::from_str(&line).map_err(StoreError::Json)?);";
    assert!(text.contains(reads));
    let changed = text.replace(reads, "let mut r: Record = serde_json::from_str(&line).map_err(StoreError::Json)?;\n        r.amount += 1.0;\n        records.push(r);");
    std::fs::write(&core, changed).unwrap();
    let file = std::fs::File::options().write(true).open(&core).unwrap();
    file.set_modified(old_time).unwrap();
    drop(file);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "two"]);

    let (code, report, stderr) = run();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        code, 1,
        "the library built from the previous commit was run: {stderr}"
    );
    assert_eq!(report["git"]["dirty"], false);
    assert_eq!(report["sides"]["candidate"]["rebuilt_from_clean"], true);
    // Every case that reads back at least one record now differs.
    assert_eq!(report["summary"]["different"], 53);
    let d = &report["cases"][0]["first_difference"];
    assert_eq!(
        (&d["original"]["repr"], &d["candidate"]["repr"]),
        (&J::from("1250.0"), &J::from("1251.0"))
    );
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn a_tracked_file_compiled_in_from_outside_the_package_is_handled() {
    // RUST-026 and RUST-027. `offset.txt` sits at the top of the example,
    // tracked and committed, and ledger-core compiles it in with a path
    // that leaves the package: include_str!("../../../offset.txt").
    let dir = std::env::temp_dir().join(format!("carcinify-diff-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    copy_tree(&repo.join("examples/float_roundtrip"), &dir);
    std::fs::write(dir.join(".gitignore"), "out/\ntarget/\n__pycache__/\n").unwrap();
    std::fs::write(
        dir.join("rust-toolchain.toml"),
        std::fs::read(repo.join("rust-toolchain.toml")).unwrap(),
    )
    .unwrap();
    let dir = dir.canonicalize().unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&dir)
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
    let run = || {
        let report = dir.join("out/report.json");
        let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
            .arg("diff")
            .arg(dir.join("suite-fixed.json"))
            .arg("--report")
            .arg(&report)
            .output()
            .unwrap();
        let json: J = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
        (
            out.status.code().unwrap(),
            json,
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    std::fs::write(dir.join("offset.txt"), "0\n").unwrap();
    let core = dir.join("rust/core/src/lib.rs");
    let text = std::fs::read_to_string(&core).unwrap();
    let reads = "records.push(serde_json::from_str(&line).map_err(StoreError::Json)?);";
    assert!(text.contains(reads));
    let changed = text.replace(
        reads,
        "let mut r: Record = serde_json::from_str(&line).map_err(StoreError::Json)?;\n        let offset: f64 = include_str!(\"../../../offset.txt\").trim().parse().unwrap();\n        if offset != 0.0 {\n            r.amount += offset;\n        }\n        records.push(r);",
    );
    std::fs::write(&core, changed).unwrap();
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "one"]);

    // RUST-026: the file is tracked; the run must not call it untracked.
    let (code, report, stderr) = run();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(report["summary"]["identical"], 68);
    assert_eq!(report["git"]["used_but_not_tracked"], serde_json::json!([]));

    // RUST-027: the file changes under its old modification time. It is
    // outside the workspace, so no tree id of the workspace changes; the
    // build record must still notice.
    let offset = dir.join("offset.txt");
    let old_time = std::fs::metadata(&offset).unwrap().modified().unwrap();
    std::fs::write(&offset, "1\n").unwrap();
    let file = std::fs::File::options().write(true).open(&offset).unwrap();
    file.set_modified(old_time).unwrap();
    drop(file);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "two"]);
    let (code, report, stderr) = run();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        code, 1,
        "the library built with the old offset was run: {stderr}"
    );
    assert_eq!(report["sides"]["candidate"]["rebuilt_from_clean"], true);
    assert_eq!(report["summary"]["different"], 53);
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn a_rust_panic_is_reported_as_what_python_sees() {
    // The panic part of RUST-010. `first_amount` in the port indexes an
    // empty list, on purpose. The report must show the panic as Python
    // receives it: a PanicException, which is a BaseException.
    let (code, report) = run("panic");
    assert_eq!(code, 1);
    assert_eq!(report["cases"][0]["result"], "identical");
    let case = &report["cases"][1];
    assert_eq!(case["result"], "different");
    assert_eq!(case["candidate_outcome"], "raised");
    let d = &case["first_difference"];
    assert_eq!(d["kind"], "exception_type");
    assert!(
        d["original"]["repr"]
            .as_str()
            .unwrap()
            .starts_with("builtins.IndexError"),
        "{d}"
    );
    assert!(
        d["candidate"]["repr"]
            .as_str()
            .unwrap()
            .starts_with("pyo3_runtime.PanicException"),
        "{d}"
    );
    let note = d["note"].as_str().unwrap();
    assert!(note.contains("original is builtins.IndexError < builtins.LookupError < builtins.Exception < builtins.BaseException"), "{note}");
    assert!(
        note.contains("candidate is pyo3_runtime.PanicException < builtins.BaseException"),
        "{note}"
    );
    // The interpreter survived the panic: no restart was needed.
    assert_eq!(report["sides"]["candidate"]["restarts"], 0);
}
