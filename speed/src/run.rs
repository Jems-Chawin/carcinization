//! `carcinify speed run`: under the gate, only timing (design §2, §5, §8).
//!
//! It checks the wheels against the hashes `speed build` wrote, reads the
//! gate's log at its start, times every target in one Python process, reads
//! the log again at its end, and writes the report.

use crate::cli::Options;
use crate::declaration::{self, Loaded};
use crate::figures::{self, Figures, Run, Side};
use crate::gate::{self, Gate, Load, Unreliable};
use carcinify_diff::commit;
use carcinify_diff::compare::{Outcome, Relax, compare};
use carcinify_diff::provenance::sha256_file;
use carcinify_diff::suite::{self, SideSpec};
use carcinify_diff::value::Value;
use serde_json::{Value as J, json};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

pub const SCHEMA: &str = "carcinify/speed/report/1";
pub const DRIVER: &str = include_str!("driver.py");

pub fn main(opts: &Options) -> ExitCode {
    match run(opts) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("carcinify speed: {e}");
            ExitCode::from(2)
        }
    }
}

/// A target's result: why it failed, or its figures.
pub struct Timed {
    pub name: String,
    pub failed: Option<String>,
    pub figures: Option<Figures>,
    /// The share of its time the process computing beside the target's
    /// timing was kept off the CPU; `None` when no watch was kept.
    pub kept_off_beside: Option<f64>,
    /// Why the cold thread could not be measured.
    pub cold_not_measured: Option<String>,
}

/// The verdict, its exit status, the reasons (every target's, each by its
/// name, and the gate's) and the flags.
pub fn judge(
    targets: &[Timed],
    gate: &Result<Load, Unreliable>,
) -> (&'static str, u8, Vec<String>, Vec<String>) {
    let failed: Vec<String> = targets
        .iter()
        .filter_map(|t| t.failed.as_ref().map(|f| format!("{}: {f}", t.name)))
        .collect();
    let mut unreliable = Vec::new();
    if let Err(u) = gate {
        let (kind, detail) = u.reason();
        unreliable.push(format!("{kind}: {detail}"));
    }
    for t in targets {
        if let Some(lost) = t
            .kept_off_beside
            .filter(|l| *l > carcinify_decide::report::BUSY_WHILE_RUNNING)
        {
            unreliable.push(format!(
                "{}: busy_beside: a second process that does nothing but compute was kept off the CPU for {:.0}% of its time: other programs were taking the CPU while the target was timed",
                t.name,
                lost * 100.0
            ));
        }
        if t.figures.as_ref().is_some_and(|f| f.ratio.is_none()) {
            unreliable.push(format!(
                "{}: every_pair_dropped: every pair of runs was disturbed",
                t.name
            ));
        }
    }
    let mut flags = Vec::new();
    if gate.as_ref().is_ok_and(|l| l.at_or_over_limit) {
        flags.push(format!("load_at_or_over_{}", gate::LIMIT));
    }
    for t in targets {
        if t.figures.as_ref().is_some_and(|f| f.short_run) {
            flags.push(format!("short_run: {}", t.name));
        }
        if let (Some(_), Some(why)) = (&t.figures, &t.cold_not_measured) {
            flags.push(format!("cold_thread_not_measured: {}: {why}", t.name));
        }
        if t.figures.is_some() && t.kept_off_beside.is_none() {
            flags.push(format!(
                "load_beside_not_watched: {}: no process computed beside its timing, or what it saw could not be read (this machine has fewer than {} CPUs, the timing took under 0.05 s, or the process did not start), so load that began and ended inside a run was not watched; the loops timed before and after each run were used alone",
                t.name,
                carcinify_decide::watch::MIN_CPUS_TO_WATCH
            ));
        }
    }
    // The worst verdict, with every reason for it and for any milder one,
    // so that one target's failure hides no other's disturbed runs.
    let (verdict, exit) = match (failed.is_empty(), unreliable.is_empty(), flags.is_empty()) {
        (false, _, _) => ("fail", 1),
        (true, false, _) => ("unreliable", 1),
        (true, true, false) => ("pass_with_flags", 0),
        (true, true, true) => ("pass", 0),
    };
    (
        verdict,
        exit,
        failed.into_iter().chain(unreliable).collect(),
        flags,
    )
}

fn run(opts: &Options) -> Result<u8, String> {
    let report_path = opts.report.as_ref().ok_or("--report is required")?;
    let wheels = opts.wheels.as_ref().ok_or("--wheels is required")?;
    let loaded = declaration::load(&opts.declaration)?;
    if !loaded.refused.is_empty() {
        let report = json!({"schema": SCHEMA, "verdict": "fail", "exit_status": 1, "reasons": loaded.refused});
        write(report_path, &report)?;
        return Ok(1);
    }
    let mut record = check_wheels(wheels, &loaded)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let sha256 = sha256_file(&exe)?;
    if record["tools"]["harness_sha256"] != sha256.as_str() {
        return Err(format!(
            "this carcinify ({sha256}) is another carcinify than the one that ran speed build ({}): run `carcinify speed build` again with this one",
            record["tools"]["harness_sha256"]
        ));
    }
    // What is timed must be what was built for: the same declaration,
    // suite, commit and interpreter, in a tree that matches its commit.
    let suite_dir = loaded.suite_path.parent().unwrap_or(Path::new("/"));
    let git = crate::build::tree(suite_dir, opts.allow_dirty)?;
    let suite_sha256 = sha256_file(&loaded.suite_path)?;
    let now = [
        (
            "declaration",
            J::from(sha256_file(&opts.declaration)?),
            &record["declaration_sha256"],
        ),
        (
            "suite",
            J::from(suite_sha256.as_str()),
            &record["suite_sha256"],
        ),
        ("commit", J::from(git.commit.as_str()), &record["commit"]),
        (
            "interpreter",
            crate::build::interpreter(&opts.python)?,
            &record["python"],
        ),
    ];
    for (what, now, built) in now {
        if &now != built {
            return Err(format!(
                "the {what} is {now}, not the {built} that speed build built for: run `carcinify speed build` again"
            ));
        }
    }
    // Files git does not track that the build used (with --allow-dirty):
    // as they were then.
    for (path, then) in record["untracked_imported"]
        .as_object()
        .into_iter()
        .flatten()
    {
        let now = sha256_file(&git.root.join(path)).unwrap_or_else(|_| "none".into());
        if Some(now.as_str()) != then.as_str() {
            return Err(format!(
                "{path}, which git does not track, changed since speed build (SHA-256 {} then, {now} now): run `carcinify speed build` again",
                then.as_str().unwrap_or("")
            ));
        }
    }
    // --allow-dirty allows a tree that does not match its commit, not one
    // whose changes are not the build's.
    let dirty_sha256 = crate::build::changes(&git)?;
    if J::from(dirty_sha256.clone()) != record["dirty_sha256"] {
        return Err(format!(
            "the tree's uncommitted changes differ from those speed build built with (SHA-256 {} now, {} then): run `carcinify speed build` again",
            dirty_sha256.as_deref().unwrap_or("none"),
            record["dirty_sha256"].as_str().unwrap_or("none")
        ));
    }
    // The build's tools block is the report's.
    let tools = record
        .as_object_mut()
        .and_then(|r| r.remove("tools"))
        .unwrap_or_default();
    // At the start: the gate this run is under, by its log.
    let gate = match &opts.load_log {
        None => Err(Unreliable::NoGate(
            "no --load-log: the run was not given the gate's log".into(),
        )),
        Some(log) => Gate::open(log, std::env::var("GATE_RUN_ID").ok().as_deref()),
    };
    let work = std::env::temp_dir().join(format!("carcinify-speed-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let result = time(opts, &loaded, wheels, &record, &work);
    let _ = std::fs::remove_dir_all(&work);
    let result = result?;
    // What the timing imported that git does not track must be what speed
    // build saw: with --allow-dirty it recorded those files, and without
    // it there were none.
    let built = record["untracked_imported"].as_object();
    let unseen: Vec<String> =
        crate::build::untracked_used(&git, &commit::tracked(&git)?, &result, &callers_of(&loaded))
            .into_iter()
            .filter(|f| !built.is_some_and(|b| b.contains_key(&f.path)))
            .map(|f| format!("{} ({})", f.path, f.role))
            .collect();
    if !unseen.is_empty() {
        return Err(format!(
            "the timing used files that git does not track and that speed build did not see:\n  {}\nCommit them, or run `carcinify speed build` again",
            unseen.join("\n  ")
        ));
    }
    // At the end: the readings while the run ran, up to one more.
    let reading =
        |r: &gate::Reading| json!({"time": r.time, "epoch": r.epoch, "one_minute": r.one_minute});
    let opened = gate.as_ref().ok().map(|g| {
        (
            g.header().to_string(),
            g.opening().iter().map(reading).collect::<Vec<J>>(),
        )
    });
    let gate = gate.and_then(|g| g.finish(Duration::from_secs(2 * gate::EVERY)));

    let no_relaxation = Relax::default();
    let mut timed = Vec::new();
    let mut targets = Vec::new();
    for ((t, what), out) in loaded
        .declaration
        .targets
        .iter()
        .zip(&loaded.timed)
        .zip(result["targets"].as_array().into_iter().flatten())
    {
        // A caller target compares its outputs with no relaxation, the
        // harness's default, and is held to the calls speed build checked.
        let (relax, case, caller, built) = match (what, &t.case, &t.caller) {
            (Some(declaration::Timed::Case(i)), Some(c), None) => {
                let case = &loaded.cases[*i];
                let named = json!({"file": c.file, "id": c.id, "source": case.source});
                (&case.relax, named, J::Null, None)
            }
            (Some(declaration::Timed::Caller(_)), None, Some(c)) => {
                let named = json!({"file": c.file, "function": c.function, "fn": c.func, "replace": c.replace});
                let built = &record["callers"][&t.name];
                (
                    &no_relaxation,
                    J::Null,
                    named,
                    Some((c.func.as_str(), built)),
                )
            }
            _ => return Err("a refused target was timed".into()),
        };
        let runs: Vec<Run> = out["runs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| Run {
                side: if r["side"] == "original" {
                    Side::Original
                } else {
                    Side::Port
                },
                seconds_per_call: r["seconds_per_call"].as_f64().unwrap_or(f64::NAN),
                calls: r["calls"].as_u64().unwrap_or(0) as usize,
                kept_off_the_cpu: r["kept_off_the_cpu"].as_f64().unwrap_or(1.0),
            })
            .collect();
        let (failed, difference) = check_output(out, relax, built);
        let figures = failed.is_none().then(|| figures::figures(&runs));
        let cold = &out["cold_thread"];
        let threads: Vec<(f64, f64)> = cold["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                (
                    t["cold"].as_f64().unwrap_or(f64::NAN),
                    t["warm"].as_f64().unwrap_or(f64::NAN),
                )
            })
            .collect();
        let cold_thread = (!cold.is_null()).then(|| json!({
            "cheap_call": cold["cheap_call"],
            "threads": threads.len(),
            "cold_minus_warm": figures::cold_minus_warm(&threads).map(|(median, low, high)| json!({"median": median, "low": low, "high": high})),
            "per_thread": cold["threads"],
            "not_measured": cold["not_measured"],
        }));
        let spread = |s: &Option<figures::Spread>| {
            s.as_ref().map(|s| json!({"best": s.best, "median": s.median, "worst": s.worst, "spread": s.spread}))
        };
        targets.push(json!({
            "name": t.name,
            "case": case,
            "caller": caller,
            "calls_per_run": t.calls_per_run,
            "failed": failed,
            "difference": difference,
            "original": figures.as_ref().and_then(|f| spread(&f.original)),
            "port": figures.as_ref().and_then(|f| spread(&f.port)),
            "ratio": figures.as_ref().and_then(|f| f.ratio.map(|(median, low, high)| json!({"median": median, "low": low, "high": high}))),
            "pairs": figures.as_ref().map(|f| f.pairs),
            "pairs_dropped": figures.as_ref().map(|f| f.pairs_dropped),
            "short_run": figures.as_ref().map(|f| f.short_run),
            "cold_thread": cold_thread,
            "runs": out["runs"],
            "watched_beside": out["watched_beside"],
            "arguments_changed_by_call": out["arguments_changed_by_call"],
        }));
        timed.push(Timed {
            name: t.name.clone(),
            failed,
            figures,
            kept_off_beside: out["watched_beside"]["kept_off_the_cpu"].as_f64(),
            cold_not_measured: cold["not_measured"].as_str().map(String::from),
        });
    }
    let (verdict, exit_status, reasons, flags) = judge(&timed, &gate);
    let readings: Vec<J> = gate
        .as_ref()
        .map(|l| l.readings.iter().map(reading).collect())
        .unwrap_or_default();
    let facts = |f: Vec<crate::platform::Fact>| -> J {
        f.into_iter()
            .map(|(k, v)| (k.to_string(), J::String(v)))
            .collect()
    };
    let report = json!({
        "schema": SCHEMA,
        "verdict": verdict,
        "exit_status": exit_status,
        "reasons": reasons,
        "flags": flags,
        "tools": tools,
        "declaration_sha256": sha256_file(&opts.declaration)?,
        "run": {"commit": git.commit, "dirty": !git.dirty.is_empty(), "dirty_sha256": dirty_sha256, "suite_sha256": suite_sha256},
        "machine": facts(crate::platform::machine()),
        "build": record,
        "python": result["python"],
        "hash_seed": {"value": opts.hash_seed.value, "chosen": opts.hash_seed.chosen},
        "gate": {
            "run_id": std::env::var("GATE_RUN_ID").ok(),
            "header": opened.as_ref().map(|o| &o.0),
            "rule": {"readings": gate::READINGS, "under": gate::LIMIT, "seconds_apart": gate::EVERY},
            "opening": opened.as_ref().map(|o| &o.1),
            "readings": readings,
        },
        "targets": targets,
    });
    write(report_path, &report)?;
    for t in &timed {
        match (&t.failed, t.figures.as_ref().and_then(|f| f.ratio)) {
            (Some(f), _) => println!("{:<24} failed: {f}", t.name),
            (None, Some((median, low, high))) => {
                println!(
                    "{:<24} original ÷ port {median:.3} (pairs {low:.3} to {high:.3})",
                    t.name
                )
            }
            (None, None) => println!("{:<24} no figure: every pair was dropped", t.name),
        }
    }
    println!("verdict: {verdict}");
    for line in reasons.iter().chain(&flags) {
        println!("  {line}");
    }
    Ok(exit_status)
}

/// The build record, once every wheel in it is in `dir` with the SHA-256
/// that `speed build` wrote.
fn check_wheels(dir: &Path, loaded: &Loaded) -> Result<J, String> {
    let path = dir.join(crate::build::RECORD);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "no build record {} ({e}): run `carcinify speed build` first, before the gate",
            path.display()
        )
    })?;
    let record: J = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for (side, spec) in [
        ("original", &loaded.suite.original),
        ("candidate", &loaded.suite.candidate),
    ] {
        let wheel = &record["sides"][side]["wheel"];
        if wheel.is_null() {
            if let SideSpec::Pyo3 { .. } = spec {
                return Err(format!(
                    "the build record has no wheel for the {side}, which the suite builds with PyO3: run `carcinify speed build` again"
                ));
            }
            continue;
        }
        let file = dir.join(wheel["file"].as_str().unwrap_or_default());
        if !file.is_file() {
            return Err(format!(
                "the {side}'s wheel {} is missing: run `carcinify speed build` again",
                file.display()
            ));
        }
        let sha256 = sha256_file(&file)?;
        if wheel["sha256"] != sha256.as_str() {
            return Err(format!(
                "{}: its SHA-256 is {sha256}, not the {} that speed build wrote: run `carcinify speed build` again",
                file.display(),
                wheel["sha256"]
            ));
        }
    }
    Ok(record)
}

/// Runs the driver on every target, and gives its result.
/// The files of the declaration's callers.
pub fn callers_of(loaded: &Loaded) -> Vec<PathBuf> {
    loaded
        .timed
        .iter()
        .filter_map(|t| match t {
            Some(declaration::Timed::Caller(path)) => Some(path.clone()),
            _ => None,
        })
        .collect()
}

/// A caller target as the timing driver takes it.
pub fn caller_target(t: &declaration::Target, path: &Path) -> J {
    let c = t.caller.as_ref();
    json!({"name": t.name, "calls_per_run": t.calls_per_run, "caller": {
        "path": path, "function": c.map(|c| &c.function), "fn": c.map(|c| &c.func), "replace": c.map(|c| &c.replace)}})
}

/// The plan every run of the timing driver shares: its files, each side as
/// it imports them (a compiled side's wheel is `wheel_of` the side,
/// unpacked into `work`), and the shared files. The driver itself, and the
/// plan.
pub fn plan(
    opts: &Options,
    loaded: &Loaded,
    work: &Path,
    wheel_of: &dyn Fn(&str) -> PathBuf,
) -> Result<(PathBuf, J), String> {
    let suite_dir = loaded.suite_path.parent().unwrap_or(Path::new("/"));
    let write_in = |dir: &str, name: &str, text: &str| -> Result<PathBuf, String> {
        let d = work.join(dir);
        std::fs::create_dir_all(&d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
        std::fs::write(d.join(name), text).map_err(|e| format!("cannot write {name}: {e}"))?;
        Ok(d.join(name))
    };
    // Each alone in its own directory: Python before 3.11 puts a script's
    // directory first on sys.path.
    // Not "driver": carcinify diff writes its own driver there.
    let driver = write_in("speed-driver", "driver.py", DRIVER)?;
    let harness = write_in("harness", "driver.py", carcinify_diff::python::DRIVER)?;
    let measure = write_in("measure", "measure.py", carcinify_decide::report::DRIVER)?;
    let canonical = |rel: &String| {
        let p = suite_dir.join(rel);
        p.canonicalize()
            .map_err(|e| format!("{}: {e}", p.display()))
    };
    let shared: Vec<PathBuf> = loaded
        .suite
        .shared
        .iter()
        .map(canonical)
        .collect::<Result<_, _>>()?;
    let mut sides = serde_json::Map::new();
    for (side, key, spec) in [
        ("original", "original", &loaded.suite.original),
        ("candidate", "port", &loaded.suite.candidate),
    ] {
        let (module, roots) = match spec {
            SideSpec::Python { path, module } => (
                module,
                path.iter().map(canonical).collect::<Result<Vec<_>, _>>()?,
            ),
            SideSpec::Pyo3 { module, .. } => {
                let wheel = wheel_of(side);
                let unpacked = work.join(format!("{side}-module"));
                let unzip = Command::new(&opts.python)
                    .args(["-I", "-m", "zipfile", "-e"])
                    .arg(&wheel)
                    .arg(&unpacked)
                    .output()
                    .map_err(|e| format!("cannot unpack {}: {e}", wheel.display()))?;
                if !unzip.status.success() {
                    return Err(format!(
                        "cannot unpack {}: {}",
                        wheel.display(),
                        String::from_utf8_lossy(&unzip.stderr)
                    ));
                }
                (module, vec![unpacked])
            }
        };
        let sys_path: Vec<&PathBuf> = roots.iter().chain(&shared).collect();
        // What carcinify diff's driver needs to find a function by its own
        // rules: the side's directories, and whether it must be compiled.
        let (require_extension, python_functions) = match spec {
            SideSpec::Pyo3 {
                python_functions, ..
            } => (true, python_functions.clone()),
            SideSpec::Python { .. } => (false, Vec::new()),
        };
        let init = json!({"must_load_from": roots, "require_extension": require_extension, "python_functions": python_functions});
        sides.insert(
            key.to_string(),
            json!({"module": module, "sys_path": sys_path, "roots": roots, "init": init}),
        );
    }
    if sides["original"]["module"] == sides["port"]["module"] {
        return Err(format!(
            "both sides' modules are named {}: one Python process cannot import both",
            sides["port"]["module"]
        ));
    }
    let shared_hashes: Vec<String> = carcinify_diff::distinct::contents(&shared)?
        .into_keys()
        .collect();
    let plan = json!({
        "harness_driver": harness,
        "measure": measure,
        "debug_log": opts.debug_log,
        "runs": loaded.declaration.runs,
        "sides": sides,
        "shared_roots": shared,
        "shared_hashes": shared_hashes,
    });
    Ok((driver, plan))
}

fn time(
    opts: &Options,
    loaded: &Loaded,
    wheels: &Path,
    record: &J,
    work: &Path,
) -> Result<J, String> {
    let wheel_of = |side: &str| {
        wheels.join(
            record["sides"][side]["wheel"]["file"]
                .as_str()
                .unwrap_or_default(),
        )
    };
    let (driver, base) = plan(opts, loaded, work, &wheel_of)?;
    let mut targets = Vec::new();
    for (i, (t, timed)) in loaded
        .declaration
        .targets
        .iter()
        .zip(&loaded.timed)
        .enumerate()
    {
        let case = match timed {
            Some(declaration::Timed::Case(c)) => &loaded.cases[*c],
            Some(declaration::Timed::Caller(path)) => {
                targets.push(caller_target(t, path));
                continue;
            }
            None => return Err("a refused target was timed".into()),
        };
        let request = suite::request(case, &work.join(format!("scratch-{i}")))?;
        let request: J = serde_json::from_str(&request).map_err(|e| e.to_string())?;
        targets.push(json!({"name": t.name, "request": request, "calls_per_run": t.calls_per_run}));
    }
    // One driver process for each target, with decide's process computing
    // beside it for as long as it runs: load that begins and ends inside a
    // run falls between the loops timed before and after it.
    let mut result = json!({"targets": [], "imported": []});
    for (i, target) in targets.into_iter().enumerate() {
        let result_path = work.join(format!("result-{i}.json"));
        let mut plan = base.clone();
        plan["result"] = json!(result_path);
        plan["targets"] = json!([target]);
        let watch = carcinify_decide::watch::Watch::start(&opts.python);
        let ran = drive(opts, work, &driver, &plan, &result_path);
        let beside = watch.finish();
        // A driver that gave no result fails its own target alone.
        let failed = |why: &str, text: String| json!({"targets": [{"name": plan["targets"][0]["name"], "runs": [], "outputs": {}, why: text}]});
        let mut ran = match ran? {
            Driven::Result(ran) => ran,
            Driven::TimedOut => failed(
                "timeout",
                format!(
                    "no result within {} s; the driver was killed",
                    opts.timeout.as_secs()
                ),
            ),
            Driven::Crashed(why) => failed("crashed", why),
        };
        let mut out = ran["targets"][0].take();
        out["watched_beside"] = beside.map_or(
            J::Null,
            |r| json!({"seconds": r.seconds, "kept_off_the_cpu": r.kept_off_the_cpu}),
        );
        if !ran["python"].is_null() {
            result["python"] = ran["python"].take();
            result["package_dirs"] = ran["package_dirs"].take();
        }
        if let (Some(all), Some(these)) = (
            result["imported"].as_array_mut(),
            ran["imported"].as_array(),
        ) {
            all.extend(these.iter().cloned());
        }
        result["targets"]
            .as_array_mut()
            .ok_or("no targets")?
            .push(out);
    }
    Ok(result)
}

/// How a run of the timing driver ended.
pub enum Driven {
    /// With its result.
    Result(J),
    /// Without one within the time limit: the driver was killed.
    TimedOut,
    /// Without one: how it exited, and the end of its log.
    Crashed(String),
}

/// One run of the timing driver on `plan`.
pub fn drive(
    opts: &Options,
    work: &Path,
    driver: &Path,
    plan: &J,
    result_path: &Path,
) -> Result<Driven, String> {
    let log_path = work.join("driver.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    // As carcinify diff starts a side: no user site-packages, no PYTHON*
    // variable but the hash seed and PYTHONSAFEPATH, bytecode cached in the
    // work directory.
    let mut command = Command::new(&opts.python);
    command
        .arg("-s")
        .arg("-X")
        .arg(format!("pycache_prefix={}", work.join("pycache").display()))
        .arg(driver)
        .stdin(Stdio::piped())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"PYTHON") {
            command.env_remove(name);
        }
    }
    command
        .env("PYTHONHASHSEED", opts.hash_seed.value.to_string())
        .env("PYTHONSAFEPATH", "1");
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", opts.python.display()))?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        stdin
            .write_all(plan.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
    }
    // The time limit, as carcinify diff has one for each case and side.
    let deadline = std::time::Instant::now() + opts.timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Driven::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let Ok(text) = std::fs::read_to_string(result_path) else {
        let log = std::fs::read(&log_path).unwrap_or_default();
        let tail = String::from_utf8_lossy(&log[log.len().saturating_sub(2000)..]);
        return Ok(Driven::Crashed(match tail.trim() {
            "" => format!("the driver exited ({status}) without a result, and its log is empty"),
            tail => format!("the driver exited ({status}) without a result; its log ends:\n{tail}"),
        }));
    };
    serde_json::from_str(&text)
        .map(Driven::Result)
        .map_err(|e| format!("the timing driver's result: {e}"))
}

/// Why a target's output fails it, and the first difference: compared as
/// carcinify diff compares, with the case's own relaxations.
/// Recorded calls as the measured check compares them: each by its
/// function and its arguments' canonical values, as a set. Neither the
/// order of the calls nor of a set's items counts: both follow the hash
/// seed, and speed build and speed run each draw their own.
fn canonical(lines: &[J]) -> std::collections::BTreeSet<String> {
    let value = |v: &mut J| {
        if let Ok(decoded) = Value::decode(v) {
            *v = J::from(decoded.canon());
        }
    };
    lines
        .iter()
        .map(|line| {
            let mut line = line.clone();
            if let Some(fields) = line.as_object_mut() {
                fields.remove("id"); // numbered in the order the calls came
            }
            line.get_mut("args")
                .and_then(J::as_array_mut)
                .into_iter()
                .flatten()
                .for_each(value);
            line.get_mut("kwargs")
                .and_then(J::as_object_mut)
                .into_iter()
                .flat_map(|k| k.values_mut())
                .for_each(value);
            line.to_string()
        })
        .collect()
}

/// Why a timed target failed, if it did, and the first difference between
/// the sides' outputs, if that was why. A caller target is first held to
/// the calls of `fn` that speed build recorded and checked (`built`).
fn check_output(out: &J, relax: &Relax, built: Option<(&str, &J)>) -> (Option<String>, Option<J>) {
    if let Some(timeout) = out["timeout"].as_str() {
        return (Some(format!("timeout: {timeout}")), None);
    }
    if let Some(crashed) = out["crashed"].as_str() {
        return (Some(format!("crashed: {crashed}")), None);
    }
    // What the call does to its arguments a caller sees, as it sees the
    // output.
    let changed = &out["arguments_changed_by_call"];
    if changed.is_object() && changed["original"] != changed["port"] {
        let did = |side: &str| {
            if changed[side] == true {
                "changed its arguments"
            } else {
                "left its arguments as they were"
            }
        };
        return (
            Some(format!(
                "arguments_changed_differently: the original's call {}, and the port's {}",
                did("original"),
                did("port")
            )),
            None,
        );
    }
    let refused = &out["refused"];
    if !refused.is_null() {
        let detail = refused["detail"].as_str().unwrap_or("");
        return (
            Some(format!(
                "function_refused: {}: {}: {detail}",
                refused["side"].as_str().unwrap_or(""),
                refused["status"].as_str().unwrap_or("")
            )),
            None,
        );
    }
    if let Some(error) = out["error"].as_str() {
        let last = error.trim_end().lines().last().unwrap_or("");
        return (Some(format!("raised: {last}")), None);
    }
    if let Some((func, built)) = built {
        let run = &out["recorded"];
        let lines: Vec<J> = run["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| serde_json::from_str(l.as_str()?).ok())
            .collect();
        let built_lines = built["lines"].as_array().map_or(&[][..], Vec::as_slice);
        if run["calls"] != built["calls"] || canonical(&lines) != canonical(built_lines) {
            return (
                Some(format!(
                    "timed_input_unstable: the caller's untimed call in this run reached {func} {} time(s) with {} input(s), where speed build recorded and checked {} with {}: {}",
                    run["calls"],
                    lines.len(),
                    built["calls"],
                    built["lines"].as_array().map_or(0, Vec::len),
                    J::from(lines)
                )),
                None,
            );
        }
    }
    let outcome = |side: &str| -> Result<Outcome, String> {
        let o = &out["outputs"][side];
        match o["status"].as_str() {
            Some("returned") => Value::decode(&o["value"]).map(Outcome::Returned),
            _ => Err(format!(
                "the {side}'s output cannot be compared: {}",
                o["detail"]
            )),
        }
    };
    match (outcome("original"), outcome("port")) {
        (Ok(a), Ok(b)) => match compare(&a, &b, relax) {
            Ok(_) => (None, None),
            Err(d) => (
                Some(format!("output_differs: at {} ({})", d.path, d.kind)),
                serde_json::to_value(&*d).ok(),
            ),
        },
        (Err(e), _) | (_, Err(e)) => (Some(format!("output_not_compared: {e}")), None),
    }
}

fn write(path: &Path, report: &J) -> Result<(), String> {
    let text = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timed(
        name: &str,
        failed: Option<&str>,
        ratio: Option<(f64, f64, f64)>,
        short_run: bool,
    ) -> Timed {
        Timed {
            name: name.into(),
            failed: failed.map(String::from),
            figures: failed.is_none().then_some(Figures {
                original: None,
                port: None,
                ratio,
                pairs: 3,
                pairs_dropped: if ratio.is_none() { 3 } else { 0 },
                short_run,
            }),
            kept_off_beside: Some(0.0),
            cold_not_measured: None,
        }
    }

    fn quiet() -> Result<Load, Unreliable> {
        Ok(Load {
            readings: Vec::new(),
            at_or_over_limit: false,
        })
    }

    #[test]
    fn every_pair_of_a_target_dropped_makes_the_run_unreliable() {
        let targets = [
            timed("a", None, Some((2.0, 1.9, 2.1)), false),
            timed("b", None, None, false),
        ];
        let (verdict, exit, reasons, _) = judge(&targets, &quiet());
        assert_eq!((verdict, exit), ("unreliable", 1));
        assert!(
            reasons[0].starts_with("b: every_pair_dropped: "),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_target_whose_process_beside_was_kept_off_the_cpu_is_unreliable() {
        let mut busy = timed("a", None, Some((2.0, 1.9, 2.1)), false);
        busy.kept_off_beside = Some(0.2);
        let (verdict, exit, reasons, _) = judge(&[busy], &quiet());
        assert_eq!((verdict, exit), ("unreliable", 1));
        assert!(reasons[0].starts_with("a: busy_beside: "), "{reasons:?}");
        let mut quiet_beside = timed("a", None, Some((2.0, 1.9, 2.1)), false);
        quiet_beside.kept_off_beside = Some(0.1);
        assert_eq!(judge(&[quiet_beside], &quiet()).0, "pass");
    }

    /// Design §5 and decide's flag: with no process beside a target, load
    /// that began and ended inside one of its runs was not watched.
    #[test]
    fn a_target_timed_with_no_watch_beside_it_is_flagged() {
        let mut unwatched = timed("a", None, Some((2.0, 1.9, 2.1)), false);
        unwatched.kept_off_beside = None;
        let watched = timed("b", None, Some((2.0, 1.9, 2.1)), false);
        let (verdict, exit, _, flags) = judge(&[unwatched, watched], &quiet());
        assert_eq!((verdict, exit), ("pass_with_flags", 0));
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert!(
            flags[0].starts_with("load_beside_not_watched: a: "),
            "{flags:?}"
        );
    }

    /// The verdict is the worst, and the reasons are every target's, each
    /// named: a failed target does not hide another's disturbed runs.
    #[test]
    fn a_failed_run_lists_every_targets_reasons_each_by_its_name() {
        let targets = [
            timed("a", Some("output_differs: at $"), None, false),
            timed("b", None, None, false),
        ];
        let gate = Err(Unreliable::NoGate("none".into()));
        let (verdict, exit, reasons, _) = judge(&targets, &gate);
        assert_eq!((verdict, exit), ("fail", 1));
        assert_eq!(reasons.len(), 3, "{reasons:?}");
        assert_eq!(reasons[0], "a: output_differs: at $");
        assert!(reasons[1].starts_with("no_gate: "), "{reasons:?}");
        assert!(
            reasons[2].starts_with("b: every_pair_dropped: "),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_failed_target_fails_the_run_before_anything_else() {
        let targets = [
            timed("a", Some("output_differs: at $"), None, false),
            timed("b", None, None, true),
        ];
        let gate = Err(Unreliable::NoGate("none".into()));
        assert_eq!(judge(&targets, &gate).0, "fail");
    }

    #[test]
    fn flags_leave_a_pass_with_flags_and_the_load_flag_names_the_limit() {
        let gate = Ok(Load {
            readings: Vec::new(),
            at_or_over_limit: true,
        });
        let (verdict, exit, _, flags) =
            judge(&[timed("a", None, Some((2.0, 1.9, 2.1)), true)], &gate);
        assert_eq!((verdict, exit), ("pass_with_flags", 0));
        assert_eq!(flags, ["load_at_or_over_6", "short_run: a"]);
        assert_eq!(
            judge(&[timed("a", None, Some((2.0, 1.9, 2.1)), false)], &quiet()).0,
            "pass"
        );
    }
}
