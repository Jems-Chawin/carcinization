//! `carcinify speed build`: the wheels, built before the gate (design §1,
//! RUST-148). It refuses to run under the gate, so that no build can be put
//! back inside the timed window.

use crate::cli::Options;
use crate::declaration;
use carcinify_diff::commit;
use carcinify_diff::provenance::{git_info, sha256_bytes, sha256_file, tool_version};
use carcinify_diff::python::{self, Built, Tools};
use carcinify_diff::report::Tools as ToolsBlock;
use carcinify_diff::suite::SideSpec;
use serde_json::{Value as J, json};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The file in `--out` that records the build.
pub const RECORD: &str = "build.json";
pub const SCHEMA: &str = "carcinify/speed/build/1";

pub fn main(opts: &Options) -> ExitCode {
    match build(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Refused(why)) => {
            for line in why {
                eprintln!("carcinify speed: refused: {line}");
            }
            ExitCode::from(1)
        }
        Err(Failure::CannotRun(e)) => {
            eprintln!("carcinify speed: {e}");
            ExitCode::from(2)
        }
    }
}

enum Failure {
    /// Targets the declaration may not time: exit 1.
    Refused(Vec<String>),
    /// Nothing was built: exit 2.
    CannotRun(String),
}

impl<E: Into<String>> From<E> for Failure {
    fn from(e: E) -> Failure {
        Failure::CannotRun(e.into())
    }
}

fn build(opts: &Options) -> Result<(), Failure> {
    if std::env::var_os("GATE_RUN_ID").is_some() {
        return Err(Failure::CannotRun(
            "speed build refuses to run while GATE_RUN_ID is set: build first, then run `carcinify speed run` alone under ci/gate.sh, so that no build raises the load the gate waited for".into(),
        ));
    }
    let out = opts.out.as_ref().ok_or("--out is required")?;
    let loaded = declaration::load(&opts.declaration)?;
    if !loaded.refused.is_empty() {
        return Err(Failure::Refused(loaded.refused));
    }
    let suite_dir = loaded.suite_path.parent().unwrap_or(Path::new("/"));
    let git = tree(suite_dir, opts.allow_dirty)?;
    let mut shared = Vec::new();
    for entry in &loaded.suite.shared {
        let path = suite_dir.join(entry);
        shared.push(
            path.canonicalize()
                .map_err(|e| format!("shared entry {}: {e}", path.display()))?,
        );
    }
    let work = std::env::temp_dir().join(format!("carcinify-speed-build-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("cannot create {}: {e}", work.display()))?;
    let tools = Tools {
        python: opts.python.clone(),
        maturin: opts.maturin.clone(),
        hash_seed: 0,
        // So that Cargo compiles the crate, and prints its command line.
        recompile_crate: true,
    };
    let mut sides = serde_json::Map::new();
    let tracked = commit::tracked(&git)?;
    let mut outside = Vec::new();
    let mut crate_dir = None;
    let mut wheels = Vec::new();
    for (name, spec) in [
        ("original", &loaded.suite.original),
        ("candidate", &loaded.suite.candidate),
    ] {
        let (_, built) = python::prepare(name, spec, suite_dir, &git.root, &shared, &work, &tools)?;
        let mut side = json!({"kind": built.kind, "module": built.module});
        if let SideSpec::Pyo3 { .. } = spec {
            let args = rustc_args(&work, name, &built)?.ok_or_else(|| {
                format!("{name}: the build log has no command line for the crate")
            })?;
            let wheel = built.wheel.as_ref().ok_or("maturin made no wheel")?;
            side["wheel"] = json!({"file": wheel.file, "sha256": wheel.sha256});
            side["target"] = crate::platform::target(&args)
                .into_iter()
                .map(|(k, v)| (k.to_string(), J::String(v)))
                .collect();
            wheels.push(work.join(format!("{name}-wheel")).join(&wheel.file));
        }
        if built.crate_dir.is_some() {
            outside.extend(commit::build_inputs(&git, &tracked, &built)?.outside);
            crate_dir = crate_dir.or(built.crate_dir.clone());
        }
        sides.insert(name.to_string(), side);
    }
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    for wheel in &wheels {
        let dest = out.join(wheel.file_name().unwrap_or_default());
        std::fs::copy(wheel, &dest)
            .map_err(|e| format!("cannot copy {} to {}: {e}", wheel.display(), dest.display()))?;
    }
    let record = json!({
        "schema": SCHEMA,
        "declaration_sha256": sha256_file(&opts.declaration)?,
        "suite_sha256": sha256_file(&loaded.suite_path)?,
        "commit": git.commit,
        "dirty": !git.dirty.is_empty(),
        "python": interpreter(&opts.python)?,
        "sides": sides,
        "tools": tools_block(opts, crate_dir.as_deref().unwrap_or(suite_dir), crate_dir.is_some(), outside)?,
    });
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(out.join(RECORD), text + "\n")
        .map_err(|e| format!("cannot write {}: {e}", out.join(RECORD).display()))?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

/// The repository holding `dir`, refused as carcinify diff refuses it when
/// its tree does not match its commit, unless `allow_dirty`.
pub fn tree(dir: &Path, allow_dirty: bool) -> Result<carcinify_diff::provenance::Git, String> {
    let git = git_info(dir)?;
    if !git.dirty.is_empty() && !allow_dirty {
        return Err(format!(
            "the working tree has uncommitted changes, so the result would not describe commit {}:\n  {}\nCommit them, or pass --allow-dirty to run anyway (the record and the report will say so).",
            &git.commit[..12.min(git.commit.len())],
            git.dirty.join("\n  ")
        ));
    }
    Ok(git)
}

/// The interpreter as it names itself, and its version: what a wheel is
/// built for and imported by.
pub fn interpreter(python: &Path) -> Result<J, String> {
    let out = Command::new(python)
        .args([
            "-I",
            "-c",
            "import sys; print(sys.executable); print(sys.version)",
        ])
        .output()
        .map_err(|e| format!("cannot run {}: {e}", python.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (executable, version) = text
        .trim()
        .split_once('\n')
        .ok_or_else(|| format!("{} did not say what it is", python.display()))?;
    Ok(json!({"executable": executable, "version": version}))
}

/// carcinify diff's tools block, for this binary and the build it made:
/// the tools asked where the crate is, as diff asks them.
fn tools_block(
    opts: &Options,
    dir: &Path,
    built: bool,
    mut outside: Vec<carcinify_diff::report::FileHash>,
) -> Result<J, String> {
    outside.sort_by(|a, b| a.path.cmp(&b.path));
    outside.dedup_by(|a, b| a.path == b.path);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let block = ToolsBlock {
        harness_version: env!("CARGO_PKG_VERSION"),
        harness_sha256: sha256_file(&exe)?,
        driver_sha256: sha256_bytes(crate::run::DRIVER.as_bytes()),
        rustc: tool_version(Path::new("rustc"), &["--version"], dir),
        cargo: tool_version(Path::new("cargo"), &["--version"], dir),
        maturin: built.then(|| tool_version(&opts.maturin, &["--version"], dir)),
        cargo_config_outside_repository: outside,
        build_environment: if built {
            commit::build_environment()
        } else {
            Default::default()
        },
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    };
    serde_json::to_value(&block).map_err(|e| e.to_string())
}

/// The crate's library name, from Cargo.
fn library(crate_dir: &Path) -> Result<String, String> {
    let manifest = crate_dir.join("Cargo.toml");
    let out = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    let meta: J = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("cargo metadata for {}: {e}", manifest.display()))?;
    let manifest = manifest.canonicalize().map_err(|e| e.to_string())?;
    let package = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["manifest_path"].as_str().map(PathBuf::from).as_ref() == Some(&manifest))
        .ok_or_else(|| format!("cargo metadata has no package for {}", manifest.display()))?;
    let lib = package["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| {
            t["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "cdylib" || k == "lib"))
        })
        .and_then(|t| t["name"].as_str())
        .ok_or_else(|| format!("{} has no library", manifest.display()))?;
    Ok(lib.replace('-', "_"))
}

/// The arguments Cargo gave rustc for the side's crate, from its build log.
fn rustc_args(work: &Path, name: &str, built: &Built) -> Result<Option<Vec<String>>, String> {
    let crate_dir = built.crate_dir.as_ref().ok_or("no crate directory")?;
    let log = work.join(format!("{name}-build.log"));
    let text =
        std::fs::read_to_string(&log).map_err(|e| format!("cannot read {}: {e}", log.display()))?;
    Ok(crate::platform::rustc_args(&text, &library(crate_dir)?))
}
