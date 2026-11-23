//! `carcinify speed build`: the wheels, built before the gate (design §1,
//! RUST-148). It refuses to run under the gate, so that no build can be put
//! back inside the timed window.

use crate::cli::Options;
use crate::declaration::{self, Loaded, Timed};
use carcinify_diff::commit;
use carcinify_diff::measured;
use carcinify_diff::provenance::{git_info, sha256_bytes, sha256_file, tool_version};
use carcinify_diff::python::{self, Built, Tools};
use carcinify_diff::report::Tools as ToolsBlock;
use carcinify_diff::side::{Side, SideConfig};
use carcinify_diff::suite::{Measured, SideSpec};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
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
    refuse_out_git_sees(out, &git)?;
    let mut shared = Vec::new();
    for entry in &loaded.suite.shared {
        let path = suite_dir.join(entry);
        shared.push(
            path.canonicalize()
                .map_err(|e| format!("shared entry {}: {e}", path.display()))?,
        );
    }
    refuse_names_in_common(
        suite_dir,
        &loaded.suite.original,
        &loaded.suite.candidate,
        &loaded.suite.shared,
    )?;
    let work = Work::new()?;
    let tools = Tools {
        python: opts.python.clone(),
        maturin: opts.maturin.clone(),
        hash_seed: 0,
        // So that Cargo compiles the crate, and prints its command line.
        recompile_crate: true,
    };
    let mut sides = serde_json::Map::new();
    let tracked = commit::tracked(&git)?;
    // As carcinify diff requires of a side's directories: what is timed is
    // in the repository the commit describes.
    for (t, timed) in loaded.declaration.targets.iter().zip(&loaded.timed) {
        if let Some(Timed::Caller(path)) = timed
            && !path.starts_with(&git.root)
        {
            return Err(Failure::CannotRun(format!(
                "target {:?}: the caller's file {} is outside the repository, so the commit would not describe it",
                t.name,
                path.display()
            )));
        }
    }
    let mut not_in_commit = Vec::new();
    let mut outside = Vec::new();
    let mut crate_dir = None;
    let mut wheels = Vec::new();
    let mut prepared = Vec::new();
    for (name, spec) in [
        ("original", &loaded.suite.original),
        ("candidate", &loaded.suite.candidate),
    ] {
        let (cfg, built) =
            python::prepare(name, spec, suite_dir, &git.root, &shared, &work, &tools)?;
        let mut side = json!({"kind": built.kind, "module": built.module});
        if let SideSpec::Pyo3 { .. } = spec {
            let args = rustc_args(&work, name, &built)?.ok_or_else(|| {
                format!("{name}: the build log has no command line for the crate")
            })?;
            if args
                .iter()
                .any(|a| a.contains(&format!("feature=\"{COUNTING}\"")))
            {
                return Err(Failure::CannotRun(format!(
                    "{name}: the wheel to be timed was built with the feature {COUNTING}, whose counting allocator would be timed. Build it without: speed build makes its own counted wheel"
                )));
            }
            let wheel = built.wheel.as_ref().ok_or("maturin made no wheel")?;
            side["wheel"] = json!({"file": wheel.file, "sha256": wheel.sha256});
            side["target"] = crate::platform::target(&args)
                .into_iter()
                .map(|(k, v)| (k.to_string(), J::String(v)))
                .collect();
            wheels.push(work.join(format!("{name}-wheel")).join(&wheel.file));
        }
        for dir in built
            .source_dirs
            .iter()
            .chain(&built.crate_dir)
            .chain(&built.workspace_dir)
        {
            if !dir.starts_with(&git.root) {
                return Err(Failure::CannotRun(format!(
                    "{name}: {} is outside the repository, so the commit would not describe it",
                    dir.display()
                )));
            }
        }
        let inputs = commit::build_inputs(&git, &tracked, &built)?;
        not_in_commit.extend(inputs.not_in_commit);
        if built.crate_dir.is_some() {
            outside.extend(inputs.outside);
            crate_dir = crate_dir.or(built.crate_dir.clone());
        }
        sides.insert(name.to_string(), side);
        prepared.push((cfg, built));
    }
    let wheel_of = |side: &str| {
        let file = sides[side]["wheel"]["file"].as_str().unwrap_or_default();
        work.join(format!("{side}-wheel")).join(file)
    };
    let (callers, refused, ran) =
        callers(opts, &loaded, &git, &shared, &work, prepared, &wheel_of)?;
    let mixed = mixed_names(&ran);
    if !mixed.is_empty() {
        return Err(mixing(&mixed).into());
    }
    // Refused as carcinify diff refuses them, unless --allow-dirty: then
    // the files the timing uses are recorded, and the run held to them.
    let used = untracked_used(&git, &tracked, &ran, &crate::run::callers_of(&loaded));
    let untracked_imported = used
        .iter()
        .map(|f| {
            Ok((
                f.path.clone(),
                J::from(sha256_file(&git.root.join(&f.path))?),
            ))
        })
        .collect::<Result<serde_json::Map<String, J>, String>>()?;
    not_in_commit.extend(used);
    carcinify_diff::refuse_outside_commit(opts.allow_dirty, &git, &not_in_commit)?;
    if !refused.is_empty() {
        return Err(Failure::Refused(refused));
    }
    let counts = counts(opts, &loaded, &git, &shared, &work, &tools, &wheel_of)?;
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
        "dirty_sha256": changes(&git)?,
        "python": interpreter(&opts.python)?,
        "sides": sides,
        "callers": callers,
        "untracked_imported": untracked_imported,
        "counts": counts,
        "tools": tools_block(opts, crate_dir.as_deref().unwrap_or(suite_dir), crate_dir.is_some(), outside)?,
    });
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(out.join(RECORD), text + "\n")
        .map_err(|e| format!("cannot write {}: {e}", out.join(RECORD).display()))?;
    Ok(())
}

/// The build's work directory, removed however the build ends.
struct Work(PathBuf);

impl Work {
    fn new() -> Result<Work, String> {
        let dir =
            std::env::temp_dir().join(format!("carcinify-speed-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        Ok(Work(dir))
    }
}

impl std::ops::Deref for Work {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The feature that builds a port's counting allocator and its export,
/// `_speed_allocations()`: allocations, frees, bytes allocated and live
/// bytes, so far.
const COUNTING: &str = "count_allocations";

/// Design §4: what each target's call costs a Python caller in the port's
/// Rust allocations, counted with a second wheel built with
/// count_allocations, which is never timed. Each target in a driver of its
/// own, whose first call of the port is the process's first.
fn counts(
    opts: &Options,
    loaded: &Loaded,
    git: &carcinify_diff::provenance::Git,
    shared: &[PathBuf],
    work: &Path,
    tools: &Tools,
    wheel_of: &dyn Fn(&str) -> PathBuf,
) -> Result<serde_json::Map<String, J>, String> {
    let not = |why: String| json!({ "not_counted": why });
    let each = |why: String| {
        let targets = loaded.declaration.targets.iter();
        Ok(targets
            .map(|t| (t.name.clone(), not(why.clone())))
            .collect())
    };
    let suite_dir = loaded.suite_path.parent().unwrap_or(Path::new("/"));
    let SideSpec::Pyo3 { crate_dir, .. } = &loaded.suite.candidate else {
        return each("the port is Python: there is no binding to count".into());
    };
    if !declares_feature(&suite_dir.join(crate_dir), COUNTING)? {
        return each(format!(
            "the port's crate has no feature {COUNTING}: add it, with a counting allocator and the export _speed_allocations"
        ));
    }
    let mut spec = loaded.suite.candidate.clone();
    if let SideSpec::Pyo3 { features, .. } = &mut spec {
        features.push(COUNTING.into());
    }
    let (_, built) = python::prepare("counted", &spec, suite_dir, &git.root, shared, work, tools)?;
    let file = &built
        .wheel
        .as_ref()
        .ok_or("maturin made no counted wheel")?
        .file;
    let wheel = work.join("counted-wheel").join(file);
    let dir = work.join("counting");
    let counted_wheel = |side: &str| {
        if side == "candidate" {
            wheel.clone()
        } else {
            wheel_of(side)
        }
    };
    let (driver, mut plan) = crate::run::plan(opts, loaded, &dir, &counted_wheel)?;
    plan["count"] = json!(true);
    let mut counts = serde_json::Map::new();
    for (i, (t, timed)) in loaded
        .declaration
        .targets
        .iter()
        .zip(&loaded.timed)
        .enumerate()
    {
        plan["targets"] = json!([match timed {
            Some(Timed::Case(c)) => crate::run::case_target(t, &loaded.cases[*c], &dir, i)?,
            Some(Timed::Caller(path)) => crate::run::caller_target(t, path),
            None => continue,
        }]);
        let path = dir.join(format!("counted-{i}.json"));
        plan["result"] = json!(path);
        let counted = match crate::run::drive(opts, &dir, &driver, &plan, &path)? {
            crate::run::Driven::Result(mut ran) => {
                let out = ran["targets"][0].take();
                match (out["error"].as_str(), out["refused"]["detail"].as_str()) {
                    (Some(e), _) => not(format!(
                        "the call raised: {}",
                        e.trim_end().lines().last().unwrap_or("")
                    )),
                    (_, Some(r)) => not(format!("function_refused: {r}")),
                    _ => out["counts"].clone(),
                }
            }
            crate::run::Driven::TimedOut => {
                not(format!("no result within {} s", opts.timeout.as_secs()))
            }
            crate::run::Driven::Crashed(why) => not(format!("the driver crashed: {why}")),
        };
        counts.insert(t.name.clone(), counted);
    }
    Ok(counts)
}

/// Whether the crate at `crate_dir` declares `feature`, as Cargo reads its
/// manifest. The directory is made canonical first, as the harness makes a
/// crate's: Cargo names the manifest as it was given, and it is compared
/// with the canonical one (RUST-176).
fn declares_feature(crate_dir: &Path, feature: &str) -> Result<bool, String> {
    let crate_dir = crate_dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", crate_dir.display()))?;
    let manifest = crate_dir.join("Cargo.toml");
    let out = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata {}: {}",
            manifest.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let metadata: J = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let manifest = manifest
        .canonicalize()
        .map_err(|e| format!("{}: {e}", manifest.display()))?;
    Ok(metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["manifest_path"].as_str().map(Path::new) == Some(&manifest))
        .any(|p| p["features"].get(feature).is_some()))
}

/// carcinify speed imports both sides into one Python process (RUST-168),
/// where a top-level module name the two sides resolve to different files
/// would resolve to one side's file for both. Each side resolves a name
/// along its own import path, its directories and then the suite's shared
/// entries, first match wins, as Python does (RUST-171). Refused before
/// anything is built: the early, cheap refusal. The driver's check, which
/// asks Python itself once both sides are imported and called, is the
/// whole one (RUST-177).
fn refuse_names_in_common(
    suite_dir: &Path,
    original: &SideSpec,
    candidate: &SideSpec,
    shared: &[String],
) -> Result<(), String> {
    // What each side imports by a top-level name: a .py file, an extension
    // or a package directory, first on its import path first.
    // A directory without __init__.py (a namespace package), the
    // interpreter's own path and the modules speed's drivers import first
    // are left to the driver's check (RUST-177).
    let names = |spec: &SideSpec| {
        let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
        let own: &[String] = match spec {
            SideSpec::Python { path, .. } => path,
            SideSpec::Pyo3 { module, .. } => {
                let top = module.split('.').next().unwrap_or(module);
                found.insert(
                    top.to_string(),
                    PathBuf::from(format!("the compiled module {module}")),
                );
                &[]
            }
        };
        for dir in own.iter().chain(shared) {
            let entries = std::fs::read_dir(suite_dir.join(dir));
            for entry in entries.into_iter().flatten().flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                let name = if entry.path().is_dir() {
                    entry
                        .path()
                        .join("__init__.py")
                        .is_file()
                        .then(|| file.clone())
                } else {
                    file.split_once('.')
                        .filter(|(_, ext)| {
                            matches!(ext.rsplit('.').next(), Some("py" | "so" | "pyd"))
                        })
                        .map(|(stem, _)| stem.to_string())
                };
                if let Some(name) = name {
                    found
                        .entry(name)
                        .or_insert_with(|| Path::new(dir).join(&file));
                }
            }
        }
        found
    };
    let (port, same) = (names(candidate), |a: &Path, b: &Path| {
        let real = |p: &Path| suite_dir.join(p).canonicalize().ok();
        real(a).is_some_and(|a| Some(a) == real(b))
    });
    let common: Vec<String> = names(original)
        .iter()
        .filter_map(|(name, a)| {
            let b = port.get(name)?;
            (!same(a, b)).then(|| format!("{name} ({} and {})", a.display(), b.display()))
        })
        .collect();
    if common.is_empty() {
        return Ok(());
    }
    Err(mixing(&common))
}

/// RUST-177 and RUST-180: the top-level names the two sides would import
/// from different files, as Python resolved them in the drivers' result
/// `ran`, each with its files:
/// - the one process's own check (`mixed` in the driver), whose rule (a),
///   two sides' paths that give two modules, stands only for a name both
///   sides import alone;
/// - a name both sides hold alone, from different origins;
/// - a name a side holds alone from one origin, which the one process
///   holds, or would find on its path, from another.
fn mixed_names(ran: &J) -> Vec<String> {
    let empty = serde_json::Map::new();
    let held = |side: &str| ran["lone"][side]["held"].as_object().unwrap_or(&empty);
    let (o, p) = (held("original"), held("port"));
    let shown = |j: &J| match j {
        J::String(s) => s.clone(),
        J::Array(dirs) => format!(
            "the namespace package in {}",
            dirs.iter()
                .filter_map(J::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    };
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    for m in ran["mixed"].as_array().into_iter().flatten() {
        let name = m["name"].as_str().unwrap_or_default();
        if m["rule"] == "a" && !(o.contains_key(name) && p.contains_key(name)) {
            continue;
        }
        found
            .entry(name.into())
            .or_insert_with(|| m["text"].as_str().unwrap_or_default().into());
    }
    for (name, a) in o {
        if let Some(b) = p.get(name).filter(|b| *b != a) {
            found
                .entry(name.clone())
                .or_insert_with(|| format!("{name} ({} and {})", shown(a), shown(b)));
        }
    }
    let together = ran["held"].as_object().unwrap_or(&empty);
    for (side, mine) in [("original", o), ("port", p)] {
        for (name, a) in mine {
            let b = together
                .get(name)
                .or_else(|| ran["lone"][side]["shared_gives"].get(name))
                .filter(|b| !b.is_null() && *b != a);
            if let Some(b) = b {
                found.entry(name.clone()).or_insert_with(|| {
                    format!(
                        "{name} (the {side} alone imports {}, but in one process with the other side it gets {})",
                        shown(a),
                        shown(b)
                    )
                });
            }
        }
    }
    found.into_values().collect()
}

/// The refusal of top-level names the two sides would import from
/// different files, each named with its files.
fn mixing<S: AsRef<str>>(names: &[S]) -> String {
    let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    format!(
        "the two sides would import a top-level module of one name from different files: {}. carcinify speed imports both sides into one Python process, so one side would import the other's. Rename one, or make each side a package of its own",
        names.join(", ")
    )
}

/// Files the timing driver imported (`ran`, its result), and the callers'
/// files, from inside the repository that git does not track, by their
/// path in it. The interpreter's installed packages are left out, as
/// carcinify diff leaves them out, wherever its environment is.
pub fn untracked_used(
    git: &carcinify_diff::provenance::Git,
    tracked: &BTreeSet<PathBuf>,
    ran: &J,
    callers: &[PathBuf],
) -> Vec<commit::NotInCommit> {
    let paths = |j: &J| -> Vec<PathBuf> {
        j.as_array()
            .into_iter()
            .flatten()
            .filter_map(J::as_str)
            .map(PathBuf::from)
            .collect()
    };
    let packages = paths(&ran["package_dirs"]);
    let imported = paths(&ran["imported"])
        .into_iter()
        .map(|p| (p, "imported by the timing driver"));
    let mut seen = BTreeSet::new();
    imported
        .chain(callers.iter().map(|p| (p.clone(), "a caller's file")))
        .filter(|(p, _)| {
            p.starts_with(&git.root)
                && !packages.iter().any(|d| p.starts_with(d))
                && !tracked.contains(p)
        })
        .filter(|(p, _)| seen.insert(p.clone()))
        .map(|(p, role)| commit::NotInCommit {
            path: p
                .strip_prefix(&git.root)
                .unwrap_or(&p)
                .to_string_lossy()
                .into_owned(),
            role: role.to_string(),
        })
        .collect()
}

/// The calls of each caller that passed, by target; why each other is
/// refused; and the timing driver's result, which says what it imported.
type Callers = (serde_json::Map<String, J>, Vec<String>, J);

/// Design §3: each caller target called twice, untimed, with the original
/// in place, and each call it made of the compared function looked up among
/// the suite's cases with the harness's own measured check, the original
/// started as carcinify diff starts it. The calls of each caller that
/// passes, by target; why each other is refused.
fn callers(
    opts: &Options,
    loaded: &Loaded,
    git: &carcinify_diff::provenance::Git,
    shared: &[PathBuf],
    work: &Path,
    prepared: Vec<(SideConfig, Built)>,
    wheel_of: &dyn Fn(&str) -> PathBuf,
) -> Result<Callers, String> {
    let mut recorded = serde_json::Map::new();
    let mut refused = Vec::new();
    let callers: Vec<_> = loaded
        .declaration
        .targets
        .iter()
        .zip(&loaded.timed)
        .filter_map(|(t, timed)| match (timed, &t.caller) {
            (Some(Timed::Caller(path)), Some(c)) => Some((t, c, path)),
            _ => None,
        })
        .collect();
    let (driver, mut plan) = crate::run::plan(opts, loaded, work, wheel_of)?;
    let result_path = work.join("recorded.json");
    plan["record"] = json!(true);
    plan["result"] = json!(result_path);
    plan["targets"] = callers
        .iter()
        .map(|(t, _, path)| crate::run::caller_target(t, path))
        .collect();
    let mut result = match crate::run::drive(opts, work, &driver, &plan, &result_path)? {
        crate::run::Driven::Result(result) => result,
        crate::run::Driven::TimedOut => {
            return Err(format!(
                "the callers were not recorded within {} s",
                opts.timeout.as_secs()
            ));
        }
        crate::run::Driven::Crashed(why) => return Err(format!("recording the callers: {why}")),
    };
    // RUST-170: each direct target called once on each side, untimed, in a
    // driver of its own, so that what the call imports is seen. How the
    // call ends, a crash or a hang among them, is the run's to judge.
    let mut completed = Vec::new();
    for (i, (t, timed)) in loaded
        .declaration
        .targets
        .iter()
        .zip(&loaded.timed)
        .enumerate()
    {
        let Some(Timed::Case(c)) = timed else {
            continue;
        };
        let called = work.join(format!("called-{i}.json"));
        let calling = work.join(format!("calling-{i}"));
        plan["targets"] = json!([crate::run::case_target(t, &loaded.cases[*c], work, i)?]);
        plan["result"] = json!(called);
        plan["calling"] = json!(calling);
        let ran = crate::run::drive(opts, work, &driver, &plan, &called)?;
        // Said here, judged by speed run: it saves a wait for the gate.
        let failed = match &ran {
            crate::run::Driven::Result(r) => r["targets"][0]["error"].as_str().map(|e| {
                let last = e.trim_end().lines().last().unwrap_or("");
                format!("raised on its case ({last})")
            }),
            crate::run::Driven::TimedOut => Some(format!(
                "was killed after {} s on its case",
                opts.timeout.as_secs()
            )),
            crate::run::Driven::Crashed(why) => {
                let first = why.lines().next().unwrap_or("").split(';').next();
                Some(format!("crashed on its case ({})", first.unwrap_or("")))
            }
        };
        if let Some(failed) = failed {
            let side = std::fs::read_to_string(&calling).unwrap_or_default();
            let who = match side.as_str() {
                "original" | "port" => format!("the {side}"),
                _ => "a side".to_string(),
            };
            eprintln!(
                "carcinify speed: target {:?}: {who} {failed}; speed run will fail it",
                t.name
            );
        } else {
            completed.push(crate::run::case_target(t, &loaded.cases[*c], work, i)?);
        }
        crate::run::take_in(&mut result, &ran);
    }
    // RUST-180: each side alone, in a process of its own on its own path,
    // imported and making the same untimed calls (those that completed),
    // to say which modules each side imports, and from where.
    // shortcut: a caller target's calls are not made alone, as its caller's
    // own imports belong to neither side; what a side imports only through
    // a caller is left to the check in the drivers above; make them alone
    // when a caller target imports per side.
    let mut lone = serde_json::Map::new();
    for side in ["original", "port"] {
        let path = work.join(format!("lone-{side}.json"));
        plan["lone"] = json!(side);
        plan["targets"] = J::Array(completed.clone());
        plan["result"] = json!(path);
        plan["calling"] = json!(work.join(format!("lone-calling-{side}")));
        let alone = |why: String| format!("the {side} alone, in a process of its own: {why}");
        match crate::run::drive(opts, work, &driver, &plan, &path)? {
            crate::run::Driven::Result(r) => {
                lone.insert(
                    side.into(),
                    json!({"held": r["held"], "shared_gives": r["shared_gives"]}),
                );
            }
            crate::run::Driven::TimedOut => {
                return Err(alone(format!(
                    "no result within {} s",
                    opts.timeout.as_secs()
                )));
            }
            crate::run::Driven::Crashed(why) => return Err(alone(why)),
        }
    }
    plan["lone"] = J::Null;
    result["lone"] = J::Object(lone);
    // Run with no callers too: it says what the sides import.
    if callers.is_empty() {
        return Ok((recorded, refused, result));
    }
    let mut prepared = prepared.into_iter();
    let (Some((mut original, original_built)), Some((mut candidate, candidate_built))) =
        (prepared.next(), prepared.next())
    else {
        return Err("both sides must be prepared".into());
    };
    carcinify_diff::complete_inits(
        (&mut original, &original_built),
        (&mut candidate, &candidate_built),
        shared,
        false,
    )?;
    let mut original = Side::start(original, opts.timeout)?;
    let outs = result["targets"].as_array().cloned().unwrap_or_default();
    for (i, ((t, c, _), out)) in callers.iter().zip(&outs).enumerate() {
        let mut refuse = |why: String| refused.push(format!("target {:?}: {why}", t.name));
        if !out["refused"].is_null() {
            let r = &out["refused"];
            refuse(format!(
                "function_refused: {}: {}: {}",
                r["side"], r["status"], r["detail"]
            ));
            continue;
        }
        if let Some(error) = out["error"].as_str() {
            refuse(format!(
                "the caller raised: {}",
                error.trim_end().lines().last().unwrap_or("")
            ));
            continue;
        }
        let (first, second) = (&out["recorded"][0], &out["recorded"][1]);
        if first != second {
            // Each recording's calls and its first few inputs.
            let inputs = |r: &J| {
                let lines: Vec<&str> = r["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(J::as_str)
                    .collect();
                let mut shown = lines.iter().take(3).copied().collect::<Vec<_>>().join("; ");
                if lines.len() > 3 {
                    shown += &format!("; and {} more", lines.len() - 3);
                }
                format!(
                    "{} call(s): {}",
                    r["calls"],
                    if shown.is_empty() { "none" } else { &shown }
                )
            };
            refuse(format!(
                "timed_input_unstable: two untimed calls of the caller reached {} with other inputs: first {}; then {}",
                c.func,
                inputs(first),
                inputs(second)
            ));
            continue;
        }
        let lines: Vec<&str> = first["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(J::as_str)
            .collect();
        let file = work.join(format!("calls-{i}.jsonl"));
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(&file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        let entry = Measured {
            file: file.to_string_lossy().into_owned(),
            func: Some(c.func.clone()),
            not_comparable: None,
        };
        let files = measured::load(Path::new("/"), &[entry], &mut Default::default())?;
        let scratch = work.join("scratch");
        let report = measured::check(
            &files,
            &git.root,
            &loaded.cases,
            &mut original,
            &scratch,
            opts.timeout,
        )?;
        let report = report.first().ok_or("no measured report")?;
        let line = |source: &str| {
            let n: usize = source
                .rsplit(':')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            lines
                .get(n.wrapping_sub(1))
                .copied()
                .unwrap_or(source)
                .to_string()
        };
        let not_compared: Vec<String> = report
            .missing
            .iter()
            .map(|m| {
                line(&m.source)
                    + &m.why
                        .as_ref()
                        .map(|w| format!(" ({w})"))
                        .unwrap_or_default()
            })
            .chain(report.unencodable.iter().map(|u| {
                format!(
                    "{} (holds a {} the value format cannot)",
                    line(&u.source),
                    u.holds
                )
            }))
            .chain(report.warning.map(String::from))
            .collect();
        if !not_compared.is_empty() {
            refuse(format!(
                "timed_input_not_compared: the caller reached {} with input(s) no case of the suite has: {}",
                c.func,
                not_compared.join("; ")
            ));
            continue;
        }
        let lines: Vec<J> = lines
            .iter()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        recorded.insert(
            t.name.clone(),
            json!({"calls": first["calls"], "lines": lines}),
        );
    }
    Ok((recorded, refused, result))
}

/// Refuses an `--out` inside the repository where git does not ignore it:
/// the wheels written there would leave the tree unlike its commit, and
/// speed run would refuse it. Checked before anything is built.
fn refuse_out_git_sees(out: &Path, git: &carcinify_diff::provenance::Git) -> Result<(), String> {
    let out = std::path::absolute(out).map_err(|e| format!("--out {}: {e}", out.display()))?;
    // The nearest directory that exists, by its real path as git gives the
    // root's, then the rest of the path.
    let mut base = out.as_path();
    let mut rest = Vec::new();
    while !base.exists() {
        rest.extend(base.file_name());
        base = base
            .parent()
            .ok_or_else(|| format!("--out {}: no directory of it exists", out.display()))?;
    }
    let mut real = base
        .canonicalize()
        .map_err(|e| format!("--out {}: {e}", out.display()))?;
    real.extend(rest.iter().rev());
    let Ok(inside) = real.strip_prefix(&git.root) else {
        return Ok(());
    };
    let ignored = !inside.as_os_str().is_empty()
        && Command::new("git")
            .arg("-C")
            .arg(&git.root)
            .args(["check-ignore", "-q", "--"])
            .arg(inside)
            .status()
            .map_err(|e| format!("cannot run git: {e}"))?
            .success();
    if ignored {
        return Ok(());
    }
    Err(format!(
        "--out {} is inside the repository, where git does not ignore it: the wheels would leave the tree unlike its commit, and speed run would refuse it. Write them under target/ or outside the repository.",
        out.display()
    ))
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

/// A SHA-256 of the tree's changes from its commit: `git diff HEAD`, then
/// each file git does not track and does not ignore, by path and content.
/// None for a tree that matches its commit.
///
/// shortcut: a change git status does not show (assume-unchanged or
/// skip-worktree) is in `dirty` but not in this hash; hash those files too
/// if anyone uses those flags on code under test.
pub fn changes(git: &carcinify_diff::provenance::Git) -> Result<Option<String>, String> {
    if git.dirty.is_empty() {
        return Ok(None);
    }
    let run = |args: &[&str]| -> Result<Vec<u8>, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&git.root)
            .args(args)
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(out.stdout)
    };
    let mut all = run(&[
        "diff",
        "--binary",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "HEAD",
    ])?;
    let untracked = run(&["ls-files", "--others", "--exclude-standard", "-z"])?;
    for path in untracked.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let name = String::from_utf8_lossy(path);
        all.extend_from_slice(path);
        all.push(0);
        all.extend_from_slice(sha256_file(&git.root.join(name.as_ref()))?.as_bytes());
        all.push(b'\n');
    }
    Ok(Some(sha256_bytes(&all)))
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
    let mut block = serde_json::to_value(&block).map_err(|e| e.to_string())?;
    // RUST-181: the measuring code, for --compare to hold two reports to:
    // the three Python files compiled in, hashed together, and the commit
    // carcinify was built from, with its uncommitted changes.
    let embedded = [
        crate::run::DRIVER,
        carcinify_diff::python::DRIVER,
        carcinify_decide::report::DRIVER,
    ]
    .join("\0");
    block["embedded_sha256"] = json!(sha256_bytes(embedded.as_bytes()));
    block["carcinify_commit"] = json!(env!("CARCINIFY_SOURCE_COMMIT"));
    block["carcinify_dirty"] = json!(env!("CARCINIFY_SOURCE_DIRTY"));
    Ok(block)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_is_found_as_cargo_reads_the_manifest() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture/rs");
        assert_eq!(declares_feature(&fixture, COUNTING), Ok(true));
        assert_eq!(
            declares_feature(Path::new(env!("CARGO_MANIFEST_DIR")), COUNTING),
            Ok(false)
        );
    }

    /// RUST-176: a crate reached through a symlink, as under macOS's /tmp.
    #[cfg(unix)]
    #[test]
    fn a_feature_is_found_through_a_symlinked_path() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture/rs");
        let dir = std::env::temp_dir().join(format!("speed-unit-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let linked = dir.join("crate");
        std::os::unix::fs::symlink(&fixture, &linked).unwrap();
        let found = declares_feature(&linked, COUNTING);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found, Ok(true));
    }
}
