//! `carcinify speed run`: under the gate, only timing (design §2, §5, §8).
//!
//! It checks the wheels against the hashes `speed build` wrote, reads the
//! gate's log at its start, times every target in one Python process, reads
//! the log again at its end, and writes the report.

use crate::cli::Options;
use crate::declaration::{self, Loaded};
use crate::figures::{self, Figures, Run, Side};
use crate::gate::{self, Gate, Load, Unreliable};
use carcinify_diff::compare::{Outcome, compare};
use carcinify_diff::provenance::{sha256_bytes, sha256_file};
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
}

/// The verdict, its exit status, the reasons for it and the flags.
pub fn judge(
    targets: &[Timed],
    gate: &Result<Load, Unreliable>,
) -> (&'static str, u8, Vec<String>, Vec<String>) {
    let failed: Vec<String> = targets
        .iter()
        .filter_map(|t| t.failed.as_ref().map(|f| format!("{}: {f}", t.name)))
        .collect();
    if !failed.is_empty() {
        return ("fail", 1, failed, Vec::new());
    }
    let mut unreliable = Vec::new();
    if let Err(u) = gate {
        let (kind, detail) = u.reason();
        unreliable.push(format!("{kind}: {detail}"));
    }
    for t in targets {
        if t.figures.as_ref().is_some_and(|f| f.ratio.is_none()) {
            unreliable.push(format!(
                "every_pair_dropped: {}: every pair of runs was disturbed",
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
    }
    match (unreliable.is_empty(), flags.is_empty()) {
        (false, _) => ("unreliable", 1, unreliable, flags),
        (true, false) => ("pass_with_flags", 0, Vec::new(), flags),
        (true, true) => ("pass", 0, Vec::new(), flags),
    }
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
    let record = check_wheels(wheels, &loaded)?;
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
    // At the end: the readings while the run ran, up to one more.
    let gate = gate.and_then(|g| g.finish(Duration::from_secs(2 * gate::EVERY)));

    let mut timed = Vec::new();
    let mut targets = Vec::new();
    for ((t, case), out) in loaded
        .declaration
        .targets
        .iter()
        .zip(&loaded.target_cases)
        .zip(result["targets"].as_array().into_iter().flatten())
    {
        let case = &loaded.cases[case.ok_or("a refused target was timed")?];
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
        let (failed, difference) = check_output(out, case);
        let figures = failed.is_none().then(|| figures::figures(&runs));
        let spread = |s: &Option<figures::Spread>| {
            s.as_ref().map(|s| json!({"best": s.best, "median": s.median, "worst": s.worst, "spread": s.spread}))
        };
        targets.push(json!({
            "name": t.name,
            "case": {"file": t.case.file, "id": t.case.id, "source": case.source},
            "calls_per_run": t.calls_per_run,
            "failed": failed,
            "difference": difference,
            "original": figures.as_ref().and_then(|f| spread(&f.original)),
            "port": figures.as_ref().and_then(|f| spread(&f.port)),
            "ratio": figures.as_ref().and_then(|f| f.ratio.map(|(median, low, high)| json!({"median": median, "low": low, "high": high}))),
            "pairs": figures.as_ref().map(|f| f.pairs),
            "pairs_dropped": figures.as_ref().map(|f| f.pairs_dropped),
            "short_run": figures.as_ref().map(|f| f.short_run),
            "runs": out["runs"],
        }));
        timed.push(Timed {
            name: t.name.clone(),
            failed,
            figures,
        });
    }
    let (verdict, exit_status, reasons, flags) = judge(&timed, &gate);
    let readings: Vec<J> = gate
        .as_ref()
        .map(|l| {
            l.readings
                .iter()
                .map(|r| json!({"time": r.time, "epoch": r.epoch, "one_minute": r.one_minute}))
                .collect()
        })
        .unwrap_or_default();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
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
        "tools": {
            "carcinify_version": env!("CARGO_PKG_VERSION"),
            "carcinify_sha256": sha256_file(&exe)?,
            "driver_sha256": sha256_bytes(DRIVER.as_bytes()),
        },
        "declaration_sha256": sha256_file(&opts.declaration)?,
        "machine": facts(crate::platform::machine()),
        "build": record,
        "python": result["python"],
        "hash_seed": {"value": opts.hash_seed.value, "chosen": opts.hash_seed.chosen},
        "gate": {
            "run_id": std::env::var("GATE_RUN_ID").ok(),
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
fn time(
    opts: &Options,
    loaded: &Loaded,
    wheels: &Path,
    record: &J,
    work: &Path,
) -> Result<J, String> {
    let suite_dir = loaded.suite_path.parent().unwrap_or(Path::new("/"));
    let write_in = |dir: &str, name: &str, text: &str| -> Result<PathBuf, String> {
        let d = work.join(dir);
        std::fs::create_dir_all(&d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
        std::fs::write(d.join(name), text).map_err(|e| format!("cannot write {name}: {e}"))?;
        Ok(d.join(name))
    };
    // Each alone in its own directory: Python before 3.11 puts a script's
    // directory first on sys.path.
    let driver = write_in("driver", "driver.py", DRIVER)?;
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
                let wheel = wheels.join(
                    record["sides"][side]["wheel"]["file"]
                        .as_str()
                        .unwrap_or_default(),
                );
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
        sides.insert(
            key.to_string(),
            json!({"module": module, "sys_path": sys_path, "roots": roots}),
        );
    }
    if sides["original"]["module"] == sides["port"]["module"] {
        return Err(format!(
            "both sides' modules are named {}: one Python process cannot import both",
            sides["port"]["module"]
        ));
    }
    let mut targets = Vec::new();
    for (i, (t, case)) in loaded
        .declaration
        .targets
        .iter()
        .zip(&loaded.target_cases)
        .enumerate()
    {
        let case = &loaded.cases[case.ok_or("a refused target was timed")?];
        let request = suite::request(case, &work.join(format!("scratch-{i}")))?;
        let request: J = serde_json::from_str(&request).map_err(|e| e.to_string())?;
        targets.push(json!({"name": t.name, "request": request, "calls_per_run": t.calls_per_run}));
    }
    let result_path = work.join("result.json");
    let plan = json!({
        "harness_driver": harness,
        "measure": measure,
        "debug_log": opts.debug_log,
        "result": result_path,
        "runs": loaded.declaration.runs,
        "sides": sides,
        "targets": targets,
    });
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
        .arg(&driver)
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
    // shortcut: no time limit; a port that hangs hangs the run. Add one if
    // a port ever does.
    let status = child.wait().map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(&result_path).map_err(|_| {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        format!(
            "the timing driver gave no result ({status}):\n{}",
            &log[log.len().saturating_sub(4000)..]
        )
    })?;
    serde_json::from_str(&text).map_err(|e| format!("the timing driver's result: {e}"))
}

/// Why a target's output fails it, and the first difference: compared as
/// carcinify diff compares, with the case's own relaxations.
fn check_output(out: &J, case: &suite::Case) -> (Option<String>, Option<J>) {
    if let Some(error) = out["error"].as_str() {
        let last = error.trim_end().lines().last().unwrap_or("");
        return (Some(format!("raised: {last}")), None);
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
        (Ok(a), Ok(b)) => match compare(&a, &b, &case.relax) {
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
            reasons[0].starts_with("every_pair_dropped: b"),
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
