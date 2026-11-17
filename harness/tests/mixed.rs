//! A PyO3 side with a Python layer over its compiled core, built by maturin
//! in a temporary repository. Ignored by default because it needs maturin
//! 1.16 and compiles PyO3 from scratch (about a minute):
//!
//!     CARCINIFY_PYTHON=$PWD/.venv/bin/python CARCINIFY_MATURIN=$PWD/.venv/bin/maturin \
//!         cargo test -p carcinify --test diff-mixed -- --ignored
//!
//! One test, several commits in one repository, so the crate is compiled
//! once and rebuilt incrementally. The steps: a declared Python function, an
//! undeclared one, a bundled copy of the original, an ignored file packaged
//! into the wheel, a PyO3 exception class with the right and the wrong base,
//! and a stale dep-info record from an earlier build.

use serde_json::Value as J;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Copies a file by writing it anew. `std::fs::copy` keeps the source's
/// modification time on some systems, and Cargo decides whether to rebuild
/// by comparing times: a source file swapped in with an old time would not
/// be compiled.
fn put(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    std::fs::write(to, std::fs::read(from).unwrap()).unwrap();
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dest) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().unwrap().is_dir() {
            if entry.file_name() != "target" {
                copy_tree(&src, &dest);
            }
        } else {
            put(&src, &dest);
        }
    }
}

struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Repo {
    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit(&self) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", "step"]);
    }

    /// Runs the suite on the committed tree: (exit status, report, stdout, stderr).
    fn run(&self) -> (i32, J, String, String) {
        self.run_with(&[])
    }

    fn run_with(&self, extra: &[&str]) -> (i32, J, String, String) {
        let report = self.0.join("out/report.json");
        let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
            .arg("diff")
            .arg(self.0.join("suite.json"))
            .arg("--report")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        let json = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        (
            out.status.code().unwrap(),
            json,
            text(&out.stdout),
            text(&out.stderr),
        )
    }

    fn edit(&self, file: &str, change: impl Fn(String) -> String) {
        let path = self.0.join(file);
        std::fs::write(&path, change(std::fs::read_to_string(&path).unwrap())).unwrap();
    }
}

#[test]
#[ignore = "needs maturin; see the top of this file"]
fn a_mixed_wheel_built_by_maturin_step_by_step() {
    let dir = std::env::temp_dir().join(format!("carcinify-diff-mixed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../harness/tests/mixed");
    copy_tree(&fixture, &dir);
    std::fs::write(
        dir.join(".gitignore"),
        "out/\ntarget/\n__pycache__/\nlocal_*\n",
    )
    .unwrap();
    let repo = Repo(dir.canonicalize().unwrap());
    repo.git(&["init", "-q"]);
    repo.commit();

    // 1. `label` is a Python function in the wheel and the suite says so:
    //    the run passes and shows which function is compiled and which is not.
    let (code, report, stdout, stderr) = repo.run();
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(report["verdict"], "pass");
    let functions = &report["checks"]["functions"];
    assert_eq!(
        (
            &functions[0]["fn"],
            &functions[0]["candidate_implementation"]
        ),
        (&J::from("scale"), &J::from("extension"))
    );
    assert_eq!(
        (
            &functions[1]["fn"],
            &functions[1]["candidate_implementation"]
        ),
        (&J::from("label"), &J::from("python"))
    );
    assert_eq!(functions[1]["original_implementation"], "python");
    assert!(
        functions[0]["candidate_defined_in"][0]["path"]
            .as_str()
            .unwrap()
            .starts_with("wheel:port_rs/_native.")
    );
    assert_eq!(
        functions[1]["candidate_defined_in"][0]["path"],
        "wheel:port_rs/__init__.py"
    );
    //    The compiled function did its work without calling any Python;
    //    the Python one is one Python call, in the wheel.
    assert_eq!(functions[0]["candidate_work"], "extension, 0 Python calls");
    assert_eq!(
        functions[1]["candidate_work"],
        "python, 1 Python call (1 in wheel)"
    );
    assert!(
        stdout.contains("    candidate: extension, 0 Python calls"),
        "{stdout}"
    );
    assert_eq!(
        report["sides"]["candidate"]["python_functions"],
        serde_json::json!(["label"])
    );

    //    `check` raises the port's own StoreError, made by PyO3's
    //    create_exception! with ValueError as its base, like the original's.
    let raised = &report["cases"][3];
    assert_eq!(raised["result"], "identical", "{stdout}");
    let note = raised["notes"][0].as_str().unwrap();
    assert!(
        note.contains("orig.StoreError and port_rs.StoreError"),
        "{note}"
    );
    assert!(
        note.contains("StoreError < builtins.ValueError < builtins.Exception"),
        "{note}"
    );

    //    The list of Python functions must be exact (the auditor's D3): a
    //    name that is compiled, or that does not exist, stops the run.
    for (extra, why) in [
        ("scale", "is a compiled function"),
        ("no_such_fn", "is not a function of the module"),
    ] {
        repo.edit("suite.json", |s| {
            s.replace(r#"["label"]"#, &format!(r#"["label", "{extra}"]"#))
        });
        repo.commit();
        let (code, report, _, stderr) = repo.run();
        assert_eq!(code, 2, "{stderr}");
        let reason = report["reasons"][0].as_str().unwrap();
        assert!(
            reason.contains(&format!("lists '{extra}', which {why}")),
            "{reason}"
        );
        put(fixture.join("suite.json"), repo.0.join("suite.json"));
    }
    repo.commit();

    // 2. The same wheel without the declaration: the run stops (the
    //    auditor's N7).
    repo.edit("suite.json", |s| {
        s.replace(r#", "python_functions": ["label"]"#, "")
    });
    repo.commit();
    let (code, report, _, stderr) = repo.run();
    assert_eq!(code, 2, "{stderr}");
    let reason = report["reasons"][0].as_str().unwrap();
    assert!(reason.contains("'label' is a Python function"), "{reason}");
    assert!(reason.contains("python_functions"), "{reason}");

    // 3. The auditor's N8: the compiled `scale` hands the call to an
    //    unchanged copy of the original shipped in the wheel.
    put(
        repo.0.join("py/orig.py"),
        repo.0.join("rs/python/port_rs/_impl.py"),
    );
    put(
        repo.0.join("rs/delegating_lib.rs"),
        repo.0.join("rs/src/lib.rs"),
    );
    repo.edit("cases.jsonl", |s| {
        s.lines().next().unwrap().to_string() + "\n"
    });
    repo.commit();
    let (code, report, _, stderr) = repo.run();
    assert_eq!(code, 2, "{stderr}");
    let reason = report["reasons"][0].as_str().unwrap();
    assert!(
        reason.contains("the two sides are not different code"),
        "{reason}"
    );
    assert!(reason.contains("has the same content as"), "{reason}");
    assert!(reason.contains("port_rs/_impl.py"), "{reason}");
    assert!(reason.contains("py/orig.py"), "{reason}");

    //    The auditor's D2: the same, with one comment line added to the
    //    copy. No file comparison can catch an edited copy, so the run
    //    passes; what gives it away is that the "compiled" function made a
    //    Python call in the wheel.
    repo.edit("rs/python/port_rs/_impl.py", |s| format!("# edited\n{s}"));
    repo.commit();
    let (code, report, stdout, stderr) = repo.run();
    assert_eq!(code, 0, "{stdout}{stderr}");
    let scale = &report["checks"]["functions"][0];
    assert_eq!(scale["candidate_implementation"], "extension");
    //    (Three calls in the wheel on this first and only call: the copy's
    //    module body, its class body and its `scale`. The rest is the
    //    import machinery loading the copy.)
    assert!(
        scale["candidate_python_calls"]["own"].as_u64().unwrap() >= 1,
        "{scale:#}"
    );
    let work = scale["candidate_work"].as_str().unwrap();
    assert!(
        work.starts_with("extension, ") && work.contains(" in wheel"),
        "{work}"
    );
    assert!(
        stdout.contains(&format!("    candidate: {work}")),
        "{stdout}"
    );

    // 4. A file git ignores that maturin packages into the wheel: the run
    //    stops although `git status` is clean.
    std::fs::remove_file(repo.0.join("rs/python/port_rs/_impl.py")).unwrap();
    for file in ["rs/src/lib.rs", "cases.jsonl", "suite.json"] {
        put(fixture.join(file), repo.0.join(file));
    }
    repo.commit();
    std::fs::write(
        repo.0.join("rs/python/port_rs/local_patch.py"),
        "PATCHED = True\n",
    )
    .unwrap();
    let (code, report, _, stderr) = repo.run();
    assert_eq!(code, 2, "{stderr}");
    let reason = report["reasons"][0].as_str().unwrap();
    assert!(
        reason.contains("rs/python/port_rs/local_patch.py (packaged into the wheel)"),
        "{reason}"
    );

    // 5. With the ignored file gone the first result is back.
    std::fs::remove_file(repo.0.join("rs/python/port_rs/local_patch.py")).unwrap();
    let (code, report, stdout, stderr) = repo.run();
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(report["summary"]["identical"], 4);

    // 6. The PyO3 exception gets the wrong base class: same name, same
    //    message, same arguments, and a caller's `except ValueError` would
    //    no longer catch it.
    repo.edit("rs/src/lib.rs", |s| {
        s.replace(
            "pyo3::exceptions::PyValueError",
            "pyo3::exceptions::PyException",
        )
    });
    repo.commit();
    let (code, report, stdout, stderr) = repo.run();
    assert_eq!(code, 1, "{stdout}{stderr}");
    let d = &report["cases"][3]["first_difference"];
    assert_eq!(d["kind"], "exception_bases");
    assert_eq!(
        d["original"]["repr"],
        "StoreError < builtins.ValueError < builtins.Exception < builtins.BaseException"
    );
    assert_eq!(
        d["candidate"]["repr"],
        "StoreError < builtins.Exception < builtins.BaseException"
    );
    assert_eq!(report["summary"]["identical"], 3);

    // 7. The auditor's stale dep-info reproduction. First a commit whose
    //    Rust source includes a file git ignores: that stops the run.
    put(fixture.join("rs/src/lib.rs"), repo.0.join("rs/src/lib.rs"));
    repo.edit("rs/src/lib.rs", |s| {
        s + "\npub const _EXTRA: &str = include_str!(\"local_extra.txt\");\n"
    });
    std::fs::write(repo.0.join("rs/src/local_extra.txt"), "extra\n").unwrap();
    repo.commit();
    let (code, report, _, stderr) = repo.run();
    assert_eq!(code, 2, "{stderr}");
    let reason = report["reasons"][0].as_str().unwrap();
    assert!(
        reason.contains("rs/src/local_extra.txt (read by the build: compiled into the crate"),
        "{reason}"
    );
    let (code, report, stdout, stderr) = repo.run_with(&["--allow-dirty"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        report["git"]["used_but_not_tracked"][0]["path"],
        "rs/src/local_extra.txt"
    );

    //    Then a commit that no longer includes it and bumps the version,
    //    with the ignored file still on disk and no `cargo clean`. The old
    //    build's records still name the file; this build does not use it.
    put(fixture.join("rs/src/lib.rs"), repo.0.join("rs/src/lib.rs"));
    for file in ["rs/Cargo.toml", "rs/Cargo.lock"] {
        repo.edit(file, |s| {
            s.replacen(
                "name = \"port_rs\"\nversion = \"0.1.0\"",
                "name = \"port_rs\"\nversion = \"0.1.1\"",
                1,
            )
        });
    }
    repo.commit();
    assert!(repo.0.join("rs/src/local_extra.txt").exists());
    let (code, report, stdout, stderr) = repo.run();
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(report["verdict"], "pass");
    assert_eq!(report["git"]["dirty"], false);
    assert_eq!(report["git"]["used_but_not_tracked"], serde_json::json!([]));

    // 8. A source file replaced by different content that keeps its old
    //    modification time (cp -p, rsync -t, an unpacked archive). Cargo
    //    goes by file times and would reuse the library it built before;
    //    the harness must notice that the inputs changed and rebuild. The
    //    new `scale` gives 22 for 7, which only the new source can produce.
    let lib = repo.0.join("rs/src/lib.rs");
    let old_time = std::fs::metadata(&lib).unwrap().modified().unwrap();
    repo.edit("rs/src/lib.rs", |s| s.replace("x * 3", "x * 3 + 1"));
    let file = std::fs::File::options().write(true).open(&lib).unwrap();
    file.set_modified(old_time).unwrap();
    drop(file);
    assert_eq!(
        std::fs::metadata(&lib).unwrap().modified().unwrap(),
        old_time
    );
    repo.commit();
    let (code, report, stdout, stderr) = repo.run();
    assert_eq!(
        code, 1,
        "the library of the previous commit was run: {stdout}{stderr}"
    );
    let d = &report["cases"][0]["first_difference"];
    assert_eq!(
        (&d["original"]["repr"], &d["candidate"]["repr"]),
        (&J::from("21"), &J::from("22"))
    );
    assert_eq!(report["git"]["dirty"], false);
}
