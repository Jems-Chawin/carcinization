//! Everything specific to Python: the driver that runs inside the
//! interpreter, and building a PyO3 crate with maturin.
//!
//! Another source language would get a sibling of this module. Nothing
//! outside it knows what Python is.

pub mod fresh;

use crate::provenance::sha256_file;
use crate::side::SideConfig;
use crate::suite::SideSpec;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DRIVER: &str = include_str!("driver.py");

pub struct Tools {
    pub python: PathBuf,
    pub maturin: PathBuf,
    /// `PYTHONHASHSEED` for both sides.
    pub hash_seed: u32,
    /// Compile a PyO3 side's crate again even when Cargo would find it
    /// fresh, its dependencies not, so that the build log holds Cargo's
    /// command line for it (carcinify speed reads its flags there).
    pub recompile_crate: bool,
}

/// The parts of a side's report entry that are known before it runs.
#[derive(Serialize, Default)]
pub struct Built {
    pub kind: &'static str,
    pub module: String,
    /// Directories the module may be loaded from; anything else is refused.
    #[serde(skip)]
    pub load_roots: Vec<PathBuf>,
    /// Python side: directories put on `sys.path`.
    #[serde(skip)]
    pub source_dirs: Vec<PathBuf>,
    /// Directories the suite declares as shared between the sides.
    #[serde(skip)]
    pub shared_roots: Vec<PathBuf>,
    /// PyO3 side: functions the suite allows to be Python.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub python_functions: Vec<String>,
    /// PyO3 side: the crate directory.
    #[serde(skip)]
    pub crate_dir: Option<PathBuf>,
    /// PyO3 side: the root of the crate's Cargo workspace, which holds
    /// Cargo.lock and any sibling crates the build pulls in.
    #[serde(skip)]
    pub workspace_dir: Option<PathBuf>,
    /// PyO3 side: true when the crate was cleaned before this build because
    /// its inputs were not those of the last build in the target directory
    /// (or there was no record of one). False for an incremental build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rebuilt_from_clean: Option<bool>,
    /// PyO3 side: the target directory this run built in.
    #[serde(skip)]
    pub target_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wheel: Option<Wheel>,
}

#[derive(Serialize)]
pub struct Wheel {
    pub file: String,
    pub sha256: String,
}

/// Removes the crate's own build from the target directory, and none of its
/// dependencies'. Cargo's output goes to `log`.
fn clean_crate(crate_dir: &Path, target_dir: &Path, log: &std::fs::File) -> Result<(), String> {
    let manifest = crate_dir.join("Cargo.toml");
    let pkgid = Command::new("cargo")
        .args(["pkgid", "--manifest-path"])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cannot run cargo pkgid: {e}"))?;
    let pkgid = String::from_utf8_lossy(&pkgid.stdout).trim().to_string();
    let status = Command::new("cargo")
        .args(["clean", "--release", "-p", &pkgid, "--target-dir"])
        .arg(target_dir)
        .arg("--manifest-path")
        .arg(&manifest)
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log.try_clone().map_err(|e| e.to_string())?)
        .status()
        .map_err(|e| format!("cannot run cargo clean: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo clean -p {pkgid:?} failed ({status})"))
    }
}

/// What a failed build's log shows in its message: 3000 bytes from the
/// compiler's first error on, or the last 3000 when there is none. A
/// verbose build puts more than that after the error.
fn tail(path: &Path) -> String {
    let text = std::fs::read(path).unwrap_or_default();
    let error = (0..text.len())
        .find(|&i| (i == 0 || text[i - 1] == b'\n') && text[i..].starts_with(b"error"));
    let shown = match error {
        Some(start) => &text[start..text.len().min(start + 3000)],
        None => &text[text.len().saturating_sub(3000)..],
    };
    String::from_utf8_lossy(shown).into_owned()
}

fn utf8(p: &Path) -> Result<String, String> {
    p.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("path is not UTF-8: {}", p.display()))
}

/// The root of the Cargo workspace a crate belongs to: where Cargo.lock and
/// any sibling crates the build pulls in live.
pub fn cargo_workspace(name: &str, crate_dir: &Path) -> Result<PathBuf, String> {
    let located = Command::new("cargo")
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .current_dir(crate_dir)
        .output()
        .map_err(|e| format!("{name}: cannot run cargo: {e}"))?;
    if !located.status.success() {
        return Err(format!(
            "{name}: {} is not a Cargo package: {}",
            crate_dir.display(),
            String::from_utf8_lossy(&located.stderr).trim()
        ));
    }
    Path::new(String::from_utf8_lossy(&located.stdout).trim())
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "cargo gave no workspace path".to_string())
}

/// maturin's command line for a PyO3 side, and the target directory it
/// builds in.
fn wheel_command(
    tools: &Tools,
    crate_dir: &Path,
    workspace_dir: &Path,
    out: &Path,
    features: &[String],
    no_default_features: bool,
) -> Result<(Vec<String>, PathBuf), String> {
    let mut cmd = vec![
        utf8(&tools.maturin)?,
        "build".to_string(),
        "--release".to_string(),
        "--locked".to_string(),
        "--interpreter".to_string(),
        utf8(&tools.python)?,
        "--manifest-path".to_string(),
        utf8(&crate_dir.join("Cargo.toml"))?,
        "--out".to_string(),
        utf8(out)?,
    ];
    // Cargo puts the finished library at one path per target
    // directory whatever the features, so two runs of the same crate
    // with different features could hand each other's library to
    // maturin. Each feature set gets a target directory of its own.
    let mut variant = features.to_vec();
    variant.sort();
    variant.insert(
        0,
        if no_default_features {
            "bare"
        } else {
            "default"
        }
        .to_string(),
    );
    let variant: String = variant
        .join("+")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "+-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if tools.recompile_crate {
        // Cargo's command line for each crate it compiles goes to
        // the build log: carcinify speed reads the flags from it.
        cmd.push("-v".to_string());
    }
    cmd.push("--target-dir".to_string());
    let target_dir = workspace_dir.join("target/carcinify-diff").join(variant);
    cmd.push(utf8(&target_dir)?);
    if no_default_features {
        cmd.push("--no-default-features".to_string());
    }
    if !features.is_empty() {
        cmd.push("--features".to_string());
        cmd.push(features.join(","));
    }
    Ok((cmd, target_dir))
}

/// The one wheel maturin wrote into `out`, unpacked into `unpacked`.
fn unpack_wheel(name: &str, tools: &Tools, out: &Path, unpacked: &Path) -> Result<PathBuf, String> {
    let wheels: Vec<PathBuf> = std::fs::read_dir(out)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "whl"))
        .collect();
    let [wheel] = wheels.as_slice() else {
        return Err(format!(
            "{name}: expected one wheel from maturin, found {}",
            wheels.len()
        ));
    };
    let unzip = Command::new(&tools.python)
        .args(["-I", "-m", "zipfile", "-e"])
        .arg(wheel)
        .arg(unpacked)
        .output()
        .map_err(|e| format!("{name}: cannot unpack wheel: {e}"))?;
    if !unzip.status.success() {
        return Err(format!(
            "{name}: cannot unpack wheel: {}",
            String::from_utf8_lossy(&unzip.stderr)
        ));
    }
    Ok(wheel.clone())
}

/// How the side is started: the interpreter with its options and
/// environment, and the driver's `init`.
fn side_config(
    name: &'static str,
    module: &str,
    built: &Built,
    shared: &[PathBuf],
    driver: &Path,
    work: &Path,
    tools: &Tools,
) -> Result<SideConfig, String> {
    let strings = |dirs: &[PathBuf]| dirs.iter().map(|p| utf8(p)).collect::<Result<Vec<_>, _>>();
    let roots = strings(&built.load_roots)?;
    let shared_roots = strings(shared)?;
    let sys_path = [roots.clone(), shared_roots.clone()].concat();
    let init = serde_json::json!({
        "op": "init",
        "module": module,
        // The side's own directories first, then the shared ones.
        "sys_path": sys_path,
        "must_load_from": roots,
        "shared_roots": shared_roots,
        // Set by the caller.
        "shared_hashes": [],
        // Filled in by the caller once both sides are prepared.
        "other_roots": [],
        // A PyO3 side's functions must be the compiled ones, unless the
        // suite names them as Python on purpose.
        "require_extension": built.kind == "pyo3",
        "python_functions": built.python_functions,
        // Set by the caller.
        "count_calls": true,
        // The driver checks that this is what it runs under.
        "hash_seed": tools.hash_seed,
    });
    Ok(SideConfig {
        name,
        program: tools.python.clone(),
        // What `-I` gave, except that the hash seed takes effect (`-I`
        // includes `-E`, which drops PYTHONHASHSEED with every other
        // variable): no PYTHONPATH, no user site-packages, no script
        // directory or current directory on sys.path. The module comes from
        // where the suite says, or not at all.
        // -s: no user site-packages.
        // env_remove: every PYTHON* variable the harness was started with,
        // which is what `-E` ignored; then PYTHONHASHSEED, and
        // PYTHONSAFEPATH=1, which is `-P` on Python 3.11 and later (no
        // script directory on sys.path). Earlier versions ignore it, so the
        // driver sits alone in its own directory and takes that off sys.path
        // itself. A version probe would cost a process start for a flag.
        // pycache_prefix: bytecode is cached in the work directory, which is
        // new on every run. Python trusts a cache file whose recorded source
        // time and size match, so a cache left beside the source by an
        // earlier run could be executed in place of a source file that was
        // changed without its time or size changing. This way none is read
        // from the repository and none is written into it.
        args: vec![
            "-s".to_string(),
            "-X".to_string(),
            format!(
                "pycache_prefix={}",
                utf8(&work.join(format!("{name}-pycache")))?
            ),
            utf8(driver)?,
        ],
        env_remove: std::env::vars_os()
            .map(|(name, _)| name)
            .filter(|name| name.as_encoded_bytes().starts_with(b"PYTHON"))
            .collect(),
        env: vec![
            ("PYTHONHASHSEED".to_string(), tools.hash_seed.to_string()),
            ("PYTHONSAFEPATH".to_string(), "1".to_string()),
        ],
        init,
        log: work.join(format!("{name}.log")),
    })
}

/// Gets one side ready to run. For a PyO3 crate that means building a wheel
/// with maturin now, from the working tree, and unpacking it into `work`:
/// the module that runs is the one just built, never one already installed.
pub fn prepare(
    name: &'static str,
    spec: &SideSpec,
    suite_dir: &Path,
    repo_root: &Path,
    shared: &[PathBuf],
    work: &Path,
    tools: &Tools,
) -> Result<(SideConfig, Built), String> {
    // Alone in its own directory: Python before 3.11 puts the script's
    // directory first on sys.path, and there it holds nothing else.
    let driver_dir = work.join("driver");
    std::fs::create_dir_all(&driver_dir).map_err(|e| format!("cannot write driver: {e}"))?;
    let driver = driver_dir.join("driver.py");
    std::fs::write(&driver, DRIVER).map_err(|e| format!("cannot write driver: {e}"))?;
    let dir = |rel: &str| {
        let p = suite_dir.join(rel);
        p.canonicalize()
            .map_err(|e| format!("{name}: {}: {e}", p.display()))
    };
    let (module, built) = match spec {
        SideSpec::Python { path, module } => {
            let dirs = path.iter().map(|p| dir(p)).collect::<Result<Vec<_>, _>>()?;
            if dirs.is_empty() {
                return Err(format!("{name}: \"path\" must name at least one directory"));
            }
            let built = Built {
                kind: "python",
                module: module.clone(),
                load_roots: dirs.clone(),
                source_dirs: dirs,
                ..Built::default()
            };
            (module, built)
        }
        SideSpec::Pyo3 {
            crate_dir,
            module,
            features,
            no_default_features,
            python_functions,
        } => {
            let crate_dir = dir(crate_dir)?;
            let workspace_dir = cargo_workspace(name, &crate_dir)?;
            let out = work.join(format!("{name}-wheel"));
            let unpacked = work.join(format!("{name}-module"));
            let (cmd, target_dir) = wheel_command(
                tools,
                &crate_dir,
                &workspace_dir,
                &out,
                features,
                *no_default_features,
            )?;
            let build = fresh::Build {
                repo_root,
                crate_dir: &crate_dir,
                workspace_dir: &workspace_dir,
                target_dir: &target_dir,
                local: std::cell::OnceCell::new(),
            };
            // One run at a time per target directory, from the check of the
            // build record to the unpacked wheel. Otherwise a second run of
            // the same crate and features could clean the directory while
            // this one is building in it. The lock file sits beside the
            // target directory, since a clean may remove the directory.
            let lock_path = target_dir.with_extension("lock");
            if let Some(parent) = lock_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{name}: cannot create {}: {e}", parent.display()))?;
            }
            let lock = std::fs::File::create(&lock_path)
                .map_err(|e| format!("{name}: cannot create {}: {e}", lock_path.display()))?;
            lock.lock()
                .map_err(|e| format!("{name}: cannot lock {}: {e}", lock_path.display()))?;
            let cleaned = build.clean_if_inputs_changed()?;
            let log_path = work.join(format!("{name}-build.log"));
            let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
            if tools.recompile_crate {
                // Under the lock, as the clean above.
                clean_crate(&crate_dir, &target_dir, &log)?;
            }
            eprintln!(
                "carcinify diff: building {name} with maturin ({})",
                crate_dir.display()
            );
            let status = Command::new(&cmd[0])
                .args(&cmd[1..])
                .current_dir(&crate_dir)
                .stdout(log.try_clone().map_err(|e| e.to_string())?)
                .stderr(log)
                .status()
                .map_err(|e| {
                    format!(
                        "{name}: cannot run maturin ({}): {e}",
                        tools.maturin.display()
                    )
                })?;
            if !status.success() {
                build.forget();
                return Err(format!(
                    "{name}: maturin build failed ({status}):\n{}",
                    tail(&log_path)
                ));
            }
            build.record()?;
            let wheel = &unpack_wheel(name, tools, &out, &unpacked)?;
            drop(lock);
            let built = Built {
                kind: "pyo3",
                module: module.clone(),
                python_functions: python_functions.clone(),
                load_roots: vec![unpacked],
                crate_dir: Some(crate_dir),
                workspace_dir: Some(workspace_dir),
                rebuilt_from_clean: Some(cleaned),
                target_dir: Some(target_dir),
                build_command: Some(cmd),
                wheel: Some(Wheel {
                    file: wheel
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    sha256: sha256_file(wheel)?,
                }),
                ..Built::default()
            };
            (module, built)
        }
    };
    let mut built = built;
    built.shared_roots = shared.to_vec();
    let cfg = side_config(name, module, &built, shared, &driver, work, tools)?;
    Ok((cfg, built))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(name: &str, text: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("carcinify-tail-{}-{name}.log", std::process::id()));
        std::fs::write(&path, text).unwrap();
        path
    }

    /// A failed build's log as a verbose build writes it: the error, then
    /// more than 3000 bytes of notes and Cargo's command line.
    #[test]
    fn a_failed_builds_message_starts_at_the_compilers_first_error() {
        let notes = "note: required by a bound in `pyo3::impl_::extract_argument`\n".repeat(80);
        let text = format!(
            "   Compiling speed-port v0.1.0\n     Running `rustc --crate-name speed_port src/lib.rs`\nerror[E0277]: the trait bound `Vec<u8>: PyClass` is not satisfied\n  --> src/lib.rs:9:5\n{notes}\nCaused by:\n  process didn't exit successfully: `rustc --crate-name speed_port`\n"
        );
        let path = log("error", &text);
        let shown = tail(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(
            shown.starts_with("error[E0277]: the trait bound"),
            "{shown}"
        );
    }

    #[test]
    fn a_log_without_an_error_line_shows_its_end() {
        let text = format!("{}the last line\n", "warning: unused\n".repeat(400));
        let path = log("end", &text);
        let shown = tail(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(
            shown.ends_with("the last line\n") && shown.len() <= 3000,
            "{shown}"
        );
    }
}
