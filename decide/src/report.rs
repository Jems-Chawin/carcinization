//! The declaration an author writes, the measurements the tool makes, and
//! the report.
//!
//! The author declares: which workload to run, which piece is the
//! candidate, whether the cost is the language's or the algorithm's, and
//! the decision. The tool runs the workload itself, several times with
//! nothing attached and then under instruments, and sets every declared
//! statement beside what it measured. No number in the report is typed.

use carcinify_diff::provenance::{git_info, sha256_bytes, sha256_file, tool_version};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const DECLARATION_SCHEMA: &str = "carcinify/decide/declaration/2";
pub const REPORT_SCHEMA: &str = "carcinify/decide/report/2";
const DRIVER: &str = include_str!("measure.py");

/// Fewer uninstrumented runs than this say nothing about how much the
/// numbers vary.
pub const MIN_REPEAT: u64 = 3;
/// A workload shorter than this, in seconds, is too short to split.
pub const MIN_WALL_SECONDS: f64 = 0.2;
/// "Port this for speed" is contradicted when the candidate's own Python
/// is less than this share of the workload's time: the whole workload could
/// then gain at most a factor of 1.25, were the piece to cost nothing.
pub const MIN_SHARE_TO_PORT_FOR_SPEED: f64 = 0.20;
/// An alternative in Python that takes at most this fraction of the
/// original's time has removed most of the cost; the decision is then made
/// on the improved code.
pub const ALGORITHM_REMOVED_MOST: f64 = 0.5;
/// One category holding at least this share names the workload.
pub const DOMINANT: f64 = 0.5;
/// A statement or reason needs at least this many words.
pub const MIN_WORDS: usize = 3;
/// A candidate entered at least `MANY_CALLS` times, each taking less than
/// `SMALL_CALL_SECONDS`, is flagged: the call across the boundary would be
/// a large part of each.
pub const MANY_CALLS: u64 = 1000;
pub const SMALL_CALL_SECONDS: f64 = 1e-6;
/// At least this many Python objects in and out of a typical call are
/// flagged: each has to be converted.
pub const MANY_OBJECTS: f64 = 10_000.0;
/// An instrumented run this many times slower than a plain one is flagged.
pub const SLOWDOWN_TO_FLAG: f64 = 2.0;
/// Above this share the ceiling 1 / (1 - p) is so large, and so sensitive
/// to the last percent, that it is not printed.
pub const CEILING_NOT_SHOWN_ABOVE: f64 = 0.95;
/// Plain runs that differ by more than this fraction of their median are
/// flagged.
pub const NOISY_SPREAD: f64 = 0.25;
/// Before and after every run the measuring script times a short loop of
/// pure computation. When the loop was kept off the CPU for more than this
/// fraction of its time, other programs were taking the CPU, and time the
/// workload lost the same way would read as waiting.
pub const BUSY_CALIBRATION: f64 = 0.10;
/// The same for the process that computes beside the measured one for as
/// long as it runs. Two processes are then at work, so a little more is
/// allowed.
pub const BUSY_WHILE_RUNNING: f64 = 0.15;
/// A measured process that itself used this fraction of the CPUs or more,
/// on average, is not judged by the watching process, nor by the time it
/// spent waiting for a CPU: what the watching process loses beside such a
/// workload it may have lost to the workload, and the workload's threads
/// wait for each other. The loops before and after still judge the run.
///
/// Why half and not "all but one or two": the figure is the CPU time the
/// process got, not what it asked for. Eight threads on eight CPUs were
/// measured at 5.9 CPUs on a machine where other programs took the rest,
/// just under a line of six, and the report then turned on which side of
/// the line a run fell. Below half, a process beside the workload has
/// CPUs to itself on a quiet machine, and what it loses is other programs.
pub const NOT_WATCHED_ABOVE_THIS_SHARE_OF_THE_CPUS: f64 = 0.5;
/// Stand-ins that remove less than this share of the workload's time are
/// flagged: either the package does almost no work, or they replace little.
pub const STAND_INS_REMOVE_LITTLE: f64 = 0.05;
/// An object of a compiled package larger than this, in bytes, that still
/// reaches the candidate in the stand-in workload makes it no stand-in.
pub const STAND_IN_OBJECT_BYTES: f64 = 65_536.0;
/// The same for all the objects of one package together.
pub const STAND_IN_TOTAL_BYTES: f64 = 1_048_576.0;
/// Long calls into a compiled package that are still made in the stand-in
/// workload, as a share of the original's time, above which it is not a
/// stand-in.
pub const STAND_INS_LEAVE_CALLS: f64 = 0.01;
/// CPU time of all threads over wall time, while the candidate runs in a
/// stand-in workload, above which it is flagged. One thread gives 1.
pub const STAND_IN_CPU_OVER_WALL: f64 = 1.05;
/// More CPU than wall time per entry of the candidate than this, in a
/// stand-in workload, and the flag says it is probably work. Chosen:
/// idle threads were measured at 0.6 s at most, with one library on one
/// machine with 8 CPUs.
pub const STAND_IN_SURPLUS_PER_ENTRY_SECONDS: f64 = 1.0;
/// A port for speed that would save less than this many seconds a day, by
/// the declared number of runs, is flagged.
pub const SMALL_SAVING_SECONDS_PER_DAY: f64 = 60.0;
pub const GOALS: [&str; 4] = [
    "speed",
    "static_types",
    "errors_at_compile_time",
    "memory_safety",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub schema: String,
    /// The directory holding the project's own Python, relative to this
    /// file. Code under it is "the project's own"; everything else is a
    /// library.
    pub project_root: String,
    pub workload: Workload,
    /// The piece that might be ported.
    pub candidate: Option<Candidate>,
    pub language_or_algorithm: Option<Algorithm>,
    pub decision: Option<Decision>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    /// What it is and why it is representative, in words.
    pub description: String,
    /// A Python file, relative to the declaration, and a function in it.
    pub file: String,
    pub function: String,
    #[serde(default)]
    pub args: Vec<J>,
    /// How many times to run it with nothing attached.
    pub repeat: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// A file under the project root, relative to it, and a function in it.
    pub file: String,
    pub function: String,
    /// The line of its `def`, when the file has two functions of the name.
    pub line: Option<u64>,
    /// For a candidate that uses a compiled package: the same workload with
    /// the package's work replaced by stand-ins of the same shape. The tool
    /// times it; what the stand-ins removed is the most the profiler can
    /// have counted wrongly as the candidate's own Python.
    pub with_stand_ins: Option<Tried>,
    /// The author's statement that the candidate does no work in a compiled
    /// package, for when such a package is loaded in the process and the
    /// tool found no use of it by the candidate. Printed as declared.
    pub uses_no_compiled_package: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Algorithm {
    /// Is the cost the language's or the algorithm's? The author's answer.
    pub statement: String,
    /// `none_known`, `exists_untried` or `tried`.
    pub better_algorithm_in_python: String,
    /// For `tried`: the same work done the better way, which the tool
    /// measures in the same run.
    pub tried: Option<Tried>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tried {
    pub description: String,
    /// The workload's file if left out.
    pub file: Option<String>,
    pub function: String,
    #[serde(default)]
    pub args: Vec<J>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// `port`, `fix_algorithm_first` or `do_not_port`.
    pub choice: String,
    /// For `port`: what it is for. Any of `GOALS`.
    #[serde(default)]
    pub goals: Vec<String>,
    pub reason: String,
    /// For a port for speed: what speed is needed.
    pub need: Option<Need>,
}

/// What a port for speed has to achieve: a sentence, and one number.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Need {
    /// Who needs it and why, in words.
    pub statement: String,
    /// The workload has to finish within this many seconds.
    pub workload_must_take_at_most_seconds: Option<f64>,
    /// Or: the workload runs this many times a day.
    pub runs_per_day: Option<f64>,
}

pub struct Options {
    pub declaration: PathBuf,
    pub python: PathBuf,
    pub allow_dirty: bool,
}

pub const NOT_MEASURED: [&str; 10] = [
    "Load that came and went inside a run. The machine is judged busy from a short loop of pure computation timed before and after every run, and on Linux from the time the main thread was ready and not running. On other systems a burst of load that began and ended inside one run is not seen. Slow disks and networks are not seen at all: only the CPU is tested. A workload whose own threads or child processes keep running after it returns makes the machine read as busy.",
    "Whether the workload is representative. The tool ran what the declaration names; a small or unusual input gives a true profile of the wrong thing. The file, function, arguments and times are above so that a reader can judge.",
    "Compiled work reached without a function call. Arithmetic on large integers, string building, and operators on objects of compiled packages (`a @ b`, `a + b` on arrays) run in compiled code and are counted as time of the Python function they are written in. No instrument used here sees them. Such a function looks like the best candidate and a port of it gains nothing.",
    "Compiled work reached by calling an object that is not a built-in function: most of NumPy's functions (`np.dot`, `np.add`), a compiled type called to make an object, the standard library's JSON encoder. The profiler counts it as time of the Python function that makes the call. On Python 3.12 and later the tool times such calls made while the candidate runs with `sys.monitoring` and takes the long ones off the candidate's share; on older Pythons it does not see them.",
    "Every use of a compiled package. The tool looks for one in five places: the names used by the candidate and by the project's functions it calls; the types of the objects that go in and come out of the candidate, as far as lists, dictionaries, instance attributes and slots lead; the local variables of those functions when they return and the globals they name; the built-in functions called while it runs; and on Python 3.12 and later every call made while it runs. An object of a compiled package that is never bound to a name, or sits deeper than the walk goes, and is used only through operators, is found by none of them. When such a package is loaded in the process and nothing was found, the report says so.",
    "What the stand-ins do. Where the candidate uses a compiled package the author names the same workload with the package's work replaced; the tool times it. It refuses one that is the workload again, that does not call the candidate, that still passes large objects of the package to it, or (Python 3.12 and later) that still makes long calls into the package. It cannot check the rest: a stand-in that still does the work on data the checks do not see leaves the candidate's share too high, and one that skips Python work as well makes it too low.",
    "Work in other threads, in child processes and on a GPU. The profile covers the main thread. To a comparison of wall time with this process's CPU time, a child process or a GPU at work looks the same as waiting; CPU time of child processes is given where the operating system reports it, GPU time is not seen at all.",
    "Whether the cost is the language's or the algorithm's. That is the author's judgment, printed as declared; where an alternative in Python was named, the tool measured it.",
    "What a port would cost to write, test and keep, and what the call across the boundary would cost. The number of calls and the data per call are measured; the cost of converting them is not.",
    "Whether static types or errors caught at compile time are worth a port here. Time measurements neither support nor contradict those goals.",
];

fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

fn round(x: f64, digits: i32) -> f64 {
    let m = 10f64.powi(digits);
    (x * m).round() / m
}

/// What the phases give.
struct Phases {
    plain: J,
    profile: J,
    /// With a candidate: the hook on it, and the calls made while it runs.
    boundary: Option<J>,
    calls: Option<J>,
    /// The alternative's plain runs, with its file.
    tried: Option<(J, PathBuf)>,
    /// The stand-in workload: its plain runs, the calls made while the
    /// candidate runs in it, the hook on the candidate, and its file.
    stand_in: Option<(J, J, J, PathBuf)>,
}

/// What a reader should look at. Each flag has a kind, and says whether it
/// was raised from a time measured in this run (it may then be missing or
/// present on another machine) or from a count or a declaration.
#[derive(Default)]
struct Flags {
    texts: Vec<String>,
    kinds: Vec<J>,
}

impl Flags {
    fn has(&self, kind: &str) -> bool {
        self.kinds.iter().any(|k| k["kind"] == kind)
    }

    fn push(&mut self, kind: &'static str, judged_by_time: bool, text: String) {
        self.kinds
            .push(json!({"kind": kind, "judged_by_time": judged_by_time, "text": text}));
        self.texts.push(text);
    }
}

/// What to say when the process uses more CPU time than wall time while
/// the candidate runs in a stand-in workload: nothing, that idle threads
/// do this, or that it is more than idle threads were measured to cost.
fn stand_in_threads_flag(cpu: f64, wall: f64, entries: u64) -> Option<String> {
    if cpu <= STAND_IN_CPU_OVER_WALL * wall {
        return None;
    }
    let surplus = cpu - wall;
    let per_entry = surplus / entries.max(1) as f64;
    let each = if entries == 1 {
        "the candidate's 1 entry".to_string()
    } else {
        format!("each of the candidate's {entries} entries")
    };
    let figures = format!(
        "while the candidate runs in the stand-in workload, the process uses more CPU time than wall time ({cpu:.3} s of CPU in {wall:.3} s, {surplus:.3} s more, {per_entry:.3} s for {each})"
    );
    let reading = if per_entry > STAND_IN_SURPLUS_PER_ENTRY_SECONDS {
        "That is more than idle threads were measured to cost (at most 0.6 s per entry): the candidate is probably still doing threaded work in the stand-in, and then the bounds count that work as the candidate's own Python"
    } else {
        "This alone does not show work inside the candidate: the idle threads of a linear algebra library use CPU for a moment after a threaded product, wherever the product was"
    };
    Some(format!(
        "{figures}. {reading}. To tell the two apart, run again with the library set to one thread (OPENBLAS_NUM_THREADS=1 or OMP_NUM_THREADS=1) and compare the candidate's time in the stand-in workload, `candidate.with_stand_ins.inside_the_candidate.wall_seconds`: about the same, and the threads were idle; longer, and they were working"
    ))
}

/// What to say about a stand-in workload from the share of the original's
/// time it took: nothing, that it is slower, or that it removed little.
fn stand_in_flag(fraction: f64) -> Option<(&'static str, String)> {
    if fraction > 1.05 {
        Some((
            "stand_ins_slower",
            format!(
                "the stand-in workload took {:.0}% of the original's time: it is slower, so it shows nothing about compiled work",
                fraction * 100.0
            ),
        ))
    } else if 1.0 - fraction < STAND_INS_REMOVE_LITTLE {
        Some((
            "stand_ins_remove_little",
            format!(
                "the stand-ins removed {:.1}% of the workload's time. Either the package does almost no work here, or the stand-ins do not replace it: read the stand-in workload before believing the lower bound",
                (1.0 - fraction).max(0.0) * 100.0
            ),
        ))
    } else {
        None
    }
}

/// How many CPUs a run used on average: CPU time of all its threads over
/// wall time.
fn cpus_used(run: &J) -> f64 {
    let wall = run["wall_seconds"].as_f64().unwrap_or(0.0);
    if wall > 0.0 {
        run["cpu_seconds"].as_f64().unwrap_or(0.0) / wall
    } else {
        0.0
    }
}

/// Did the measured process itself leave no CPU to watch it with?
fn fills_the_machine(used: f64, logical_cpus: &J) -> bool {
    logical_cpus
        .as_f64()
        .is_some_and(|cpus| used >= cpus * NOT_WATCHED_ABOVE_THIS_SHARE_OF_THE_CPUS)
}

/// Was this plain run made while other programs were taking the CPU?
fn disturbed(run: &J, logical_cpus: &J) -> bool {
    let over = |key: &str| run[key].as_f64().is_some_and(|x| x > BUSY_CALIBRATION);
    let wall = run["wall_seconds"].as_f64().unwrap_or(0.0);
    over("calibration_before")
        || over("calibration_after")
        || (!fills_the_machine(cpus_used(run), logical_cpus)
            && run["seconds_ready_and_not_running"]
                .as_f64()
                .is_some_and(|x| wall > 0.0 && x > BUSY_CALIBRATION * wall))
}

/// Says so when this program's own loop was kept off the CPU while the
/// process ran.
fn watched(what: &str, phase: &J, busy: &mut Vec<String>, flags: &mut Flags) {
    let used = phase["cpus_used_by_the_measured_process"]
        .as_f64()
        .unwrap_or(0.0);
    if !phase["watched_from_outside"].is_null() && fills_the_machine(used, &phase["logical_cpus"]) {
        // Said once, for the first run it was seen in.
        if !flags.has("load_inside_the_run_not_watched") {
            flags.push("load_inside_the_run_not_watched", true, format!(
                "the workload itself used {used:.1} CPUs of {}, so load inside the run could not be watched (first during {what}). For such a run the machine is judged by the loops timed before and after it",
                phase["logical_cpus"]
            ));
        }
        return;
    }
    let cpus = phase["logical_cpus"].as_u64().unwrap_or(0);
    if phase["watched_from_outside"].is_null()
        && (1..crate::watch::MIN_CPUS_TO_WATCH as u64).contains(&cpus)
        && !flags.has("load_inside_the_run_not_watched_few_cpus")
    {
        // Booked with the flags judged by time: like them it depends on the
        // machine, and must not differ between two machines' reports.
        flags.push("load_inside_the_run_not_watched_few_cpus", true, format!(
            "this machine has {cpus} CPUs, and with fewer than {} no process is run beside the workload, so load inside a run was not watched{}",
            crate::watch::MIN_CPUS_TO_WATCH,
            if cfg!(target_os = "linux") {
                ". The loops timed before and after each run are used, and for the plain runs the time the process spent ready and not running is still used"
            } else {
                ". The machine is judged by the loops timed before and after each run alone, which do not see load that begins and ends inside one"
            }
        ));
    }
    if let Some(lost) = phase["watched_from_outside"]["kept_off_the_cpu"]
        .as_f64()
        .filter(|lost| *lost > BUSY_WHILE_RUNNING)
    {
        busy.push(format!(
            "during {what}, a second process that does nothing but compute was kept off the CPU for {:.0}% of its time: other programs were taking the CPU while the workload ran",
            lost * 100.0
        ));
    }
}

/// Says so when the machine was busy around an instrumented run.
fn busy_during(what: &str, phase: &J, busy: &mut Vec<String>, flags: &mut Flags) {
    let m = &phase["machine"];
    let worst = m["calibration_before"]
        .as_f64()
        .unwrap_or(0.0)
        .max(m["calibration_after"].as_f64().unwrap_or(0.0));
    watched(
        what,
        &json!({
            "watched_from_outside": phase["watched_from_outside"],
            "cpus_used_by_the_measured_process": cpus_used(&phase["process"]),
            "logical_cpus": m["logical_cpus"],
        }),
        busy,
        flags,
    );
    if worst > BUSY_CALIBRATION {
        busy.push(format!(
            "around {what}, a loop of pure computation was kept off the CPU for {:.0}% of its time (load average {} on {} logical CPUs)",
            worst * 100.0,
            m["load_average_after"][0].as_f64().map_or("not known".to_string(), |l| format!("{l:.1}")),
            m["logical_cpus"]
        ));
    }
}

/// Busy plain runs: none usable is a reason not to trust the report, some
/// is a flag.
fn busy_plain(what: &str, totals: &J, busy: &mut Vec<String>, flags: &mut Flags) {
    let (quiet, all) = (
        totals["quiet_runs"].as_u64().unwrap_or(0),
        totals["runs"].as_array().map_or(0, Vec::len) as u64,
    );
    watched(
        &format!("the plain runs of {what}"),
        &totals["calibration_outside"],
        busy,
        flags,
    );
    if quiet == 0 {
        busy.push(format!("in all {all} plain runs of {what}, other programs were taking the CPU: a loop of pure computation timed before and after each run was kept off the CPU for more than {:.0}% of its time", BUSY_CALIBRATION * 100.0));
    } else if quiet < all {
        flags.push("runs_left_out", true, format!("{} of the {all} plain runs of {what} were made while other programs were taking the CPU and are left out; the figures use the fastest of the other {quiet}", all - quiet));
    }
}

struct Runner<'a> {
    python: &'a Path,
    root: &'a Path,
    token: String,
    dir: PathBuf,
}

impl Runner<'_> {
    /// Runs one phase in a process of its own and returns what it measured.
    fn phase(&self, phase: &str, spec: &J) -> Result<J, String> {
        let spec_path = self.dir.join(format!("{phase}.json"));
        std::fs::write(&spec_path, spec.to_string())
            .map_err(|e| format!("cannot write {}: {e}", spec_path.display()))?;
        let script = self.dir.join("measure.py");
        std::fs::write(&script, DRIVER).map_err(|e| e.to_string())?;
        // `-I`: the environment and the user's site directory do not reach
        // the measurement. The project root is put on the path by the script.
        // The token goes in on standard input, not on the command line,
        // so the workload does not find it in its own arguments.
        let mut child = Command::new(self.python)
            .arg("-I")
            .arg(&script)
            .args([phase, &spec_path.to_string_lossy()])
            .current_dir(self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", self.python.display()))?;
        // Watched from here for as long as it runs: see `watch`.
        let watch = crate::watch::Watch::start(self.python);
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = writeln!(stdin, "{}", self.token);
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("cannot run {}: {e}", self.python.display()))?;
        let watched = watch.finish();
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err
                .lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return Err(format!(
                "the workload failed in the `{phase}` run ({}):\n  {}",
                out.status,
                tail.join("\n  ")
            ));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = text
            .lines()
            .filter_map(|l| l.strip_prefix(&format!("{} ", self.token)))
            .collect();
        match lines.as_slice() {
            [one] => {
                let mut result: J = serde_json::from_str(one)
                    .map_err(|e| format!("the `{phase}` run's result could not be read: {e}"))?;
                result["watched_from_outside"] = match watched {
                    Some(w) => {
                        json!({"seconds": round(w.seconds, 3), "kept_off_the_cpu": round(w.kept_off_the_cpu, 4)})
                    }
                    None => J::Null,
                };
                Ok(result)
            }
            other => Err(format!(
                "the `{phase}` run printed {} result line(s) where one was expected; the result is not trusted",
                other.len()
            )),
        }
    }
}

/// Wall and CPU figures of the plain runs.
fn summary(plain: &J) -> Result<J, String> {
    let runs = plain["runs"].as_array().cloned().unwrap_or_default();
    if runs.is_empty() {
        return Err("the plain run gave no measurement".to_string());
    }
    // Runs made while other programs were taking the CPU are left out,
    // unless every run was such a one; the caller then says so.
    let cpus = &plain["machine"]["logical_cpus"];
    let quiet: Vec<J> = runs
        .iter()
        .filter(|r| !disturbed(r, cpus))
        .cloned()
        .collect();
    let most_cpus = runs.iter().map(cpus_used).fold(0.0, f64::max);
    let used = if quiet.is_empty() { &runs } else { &quiet };
    let get = |key: &str| {
        used.iter()
            .map(|r| r[key].as_f64().unwrap_or(0.0))
            .collect::<Vec<_>>()
    };
    let (wall, cpu) = (get("wall_seconds"), get("cpu_seconds"));
    let (lo, hi) = (
        wall.iter().copied().fold(f64::MAX, f64::min),
        wall.iter().copied().fold(0.0, f64::max),
    );
    let m = median(&wall);
    Ok(json!({
        "runs": runs,
        "quiet_runs": quiet.len(),
        "wall_seconds": {"median": m, "fastest": lo, "slowest": hi, "spread_of_median": if m > 0.0 { round((hi - lo) / m, 3) } else { 0.0 }},
        "cpu_seconds_median": median(&cpu),
        // The fastest run is the one the rest of the machine disturbed
        // least; a run that lost the CPU to something else has wall time
        // that is neither work nor waiting of its own.
        "fastest_run": used.iter().min_by(|a, b| a["wall_seconds"].as_f64().unwrap_or(0.0).total_cmp(&b["wall_seconds"].as_f64().unwrap_or(0.0))),
        "calibration": plain["machine"],
        "calibration_outside": {"watched_from_outside": plain["watched_from_outside"], "cpus_used_by_the_measured_process": round(most_cpus, 2), "logical_cpus": cpus},
        "main_thread_cpu_seconds_median": median(&get("main_thread_cpu_seconds")),
        "child_process_cpu_seconds_median": median(&get("child_process_cpu_seconds")),
        "peak_memory_bytes": plain["peak_memory_bytes"],
    }))
}

pub fn run(opts: &Options) -> Result<J, String> {
    let path = opts
        .declaration
        .canonicalize()
        .map_err(|e| format!("{}: {e}", opts.declaration.display()))?;
    let here = path
        .parent()
        .ok_or("the declaration file has no directory")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decl: Declaration = serde_json::from_str(&text)
        .map_err(|e| format!("bad declaration {}: {e}", path.display()))?;
    if decl.schema != DECLARATION_SCHEMA {
        return Err(format!(
            "declaration schema is {:?}, expected {DECLARATION_SCHEMA:?}",
            decl.schema
        ));
    }
    let git = git_info(here)?;
    if !git.dirty.is_empty() && !opts.allow_dirty {
        return Err(format!(
            "the working tree has uncommitted changes, so the report would not describe commit {}:\n  {}\nCommit them, or pass --allow-dirty to run anyway (the report will say so).",
            &git.commit[..12.min(git.commit.len())],
            git.dirty.join("\n  ")
        ));
    }
    let root = here
        .join(&decl.project_root)
        .canonicalize()
        .map_err(|e| format!("project_root {}: {e}", decl.project_root))?;
    let shown = |p: &Path| {
        p.strip_prefix(&git.root)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned()
    };
    let file = |name: &str, base: &Path, what: &str| -> Result<PathBuf, String> {
        let p = base
            .join(name)
            .canonicalize()
            .map_err(|e| format!("{what} {name}: {e}"))?;
        if !p.starts_with(&git.root) {
            return Err(format!(
                "{what} {name} is outside the repository, so the commit does not describe it"
            ));
        }
        Ok(p)
    };
    let workload_file = file(&decl.workload.file, here, "the workload file")?;
    if !is_ident(&decl.workload.function) {
        return Err(format!(
            "the workload's function {:?} is not a name",
            decl.workload.function
        ));
    }
    if decl.workload.repeat < MIN_REPEAT {
        return Err(format!(
            "the workload is to be run {} time(s); at least {MIN_REPEAT} are needed to see how much the times vary",
            decl.workload.repeat
        ));
    }
    let candidate_file = match &decl.candidate {
        Some(c) => {
            if !is_ident(&c.function) {
                return Err(format!(
                    "the candidate's function {:?} is not a name",
                    c.function
                ));
            }
            let p = file(&c.file, &root, "the candidate's file")?;
            if !p.starts_with(&root) {
                return Err(format!(
                    "the candidate's file {} is not under the project root",
                    c.file
                ));
            }
            Some(p)
        }
        None => None,
    };

    let seed = format!("{:?} {}", std::time::SystemTime::now(), std::process::id());
    let dir = std::env::temp_dir().join(format!(
        "carcinify-decide-{}",
        &sha256_bytes(seed.as_bytes())[..16]
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let runner = Runner {
        python: &opts.python,
        root: &root,
        token: format!("MEASURED-{}", &sha256_bytes(seed.as_bytes())[16..40]),
        dir: dir.clone(),
    };
    let spec = |file: &Path, function: &str, args: &[J]| {
        json!({
            "project_root": root, "workload_file": file, "function": function, "args": args, "repeat": decl.workload.repeat,
            "candidate": decl.candidate.as_ref().map(|c| json!({"file": c.file, "function": c.function, "line": c.line})),
        })
    };
    let main_spec = spec(&workload_file, &decl.workload.function, &decl.workload.args);
    let measured = (|| -> Result<Phases, String> {
        let plain = runner.phase("plain", &main_spec)?;
        let profile = runner.phase("profile", &main_spec)?;
        let (boundary, calls) = if decl.candidate.is_some() {
            (
                Some(runner.phase("boundary", &main_spec)?),
                Some(runner.phase("calls", &main_spec)?),
            )
        } else {
            (None, None)
        };
        let other = |t: &Tried, what: &str| -> Result<(J, PathBuf), String> {
            if !is_ident(&t.function) {
                return Err(format!("{what}'s function {:?} is not a name", t.function));
            }
            let f = match &t.file {
                Some(name) => file(name, here, &format!("{what}'s file"))?,
                None => workload_file.clone(),
            };
            Ok((spec(&f, &t.function, &t.args), f))
        };
        let tried = match decl
            .language_or_algorithm
            .as_ref()
            .and_then(|a| a.tried.as_ref())
        {
            Some(t) => {
                let (spec, f) = other(t, "the alternative")?;
                Some((runner.phase("plain", &spec)?, f))
            }
            None => None,
        };
        let stand_in = match decl
            .candidate
            .as_ref()
            .and_then(|c| c.with_stand_ins.as_ref())
        {
            Some(t) => {
                let (spec, f) = other(t, "the stand-in workload")?;
                Some((
                    runner.phase("plain", &spec)?,
                    runner.phase("calls", &spec)?,
                    runner.phase("boundary", &spec)?,
                    f,
                ))
            }
            None => None,
        };
        Ok(Phases {
            plain,
            profile,
            boundary,
            calls,
            tried,
            stand_in,
        })
    })();
    let _ = std::fs::remove_dir_all(&dir);
    let Phases {
        plain,
        profile,
        boundary,
        calls,
        tried,
        stand_in,
    } = measured?;

    let mut reasons: Vec<String> = Vec::new();
    let mut contradictions: Vec<String> = Vec::new();
    let mut flags = Flags::default();
    // Why the measurements cannot be trusted at all: the machine was busy.
    let mut busy: Vec<String> = Vec::new();

    // Totals, from the runs with nothing attached.
    let totals = summary(&plain)?;
    busy_plain("the workload", &totals, &mut busy, &mut flags);
    busy_during(
        "the run under the profiler",
        &profile,
        &mut busy,
        &mut flags,
    );
    if let Some(c) = &calls
        && c["available"] == true
    {
        busy_during(
            "the run that times calls into compiled code",
            c,
            &mut busy,
            &mut flags,
        );
    }
    // Shares and comparisons use the fastest run; the median and the
    // spread are printed beside it.
    let wall = totals["fastest_run"]["wall_seconds"]
        .as_f64()
        .unwrap_or(0.0);
    let cpu = totals["fastest_run"]["cpu_seconds"].as_f64().unwrap_or(0.0);
    let spread = totals["wall_seconds"]["spread_of_median"]
        .as_f64()
        .unwrap_or(0.0);
    let children = totals["child_process_cpu_seconds_median"]
        .as_f64()
        .unwrap_or(0.0);
    if wall < MIN_WALL_SECONDS {
        reasons.push(format!("the workload takes {:.3} s; under {MIN_WALL_SECONDS} s there is too little to split into shares. Give it more to do", wall));
    }
    let waiting = if wall > 0.0 {
        (1.0 - cpu / wall).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if spread > NOISY_SPREAD {
        flags.push("runs_differ", true, format!("the plain runs differ by {:.0}% of their median: the machine was busy, or the workload does not do the same thing each time. The figures use the fastest run; measure again on a quiet machine before relying on them", spread * 100.0));
    }
    if cpu > wall * 1.05 {
        flags.push("more_cpu_than_wall", true, format!("the process used {cpu:.3} s of CPU in {wall:.3} s: more than one thread was at work. The profile covers the main thread only, and other programs that leave a CPU free can slow such a workload without the tool seeing it"));
    }
    if wall > 0.0 && children > wall * 0.05 {
        flags.push("child_processes", true, format!("child processes used {children:.3} s of CPU. To this process that is waiting; it is work, done elsewhere, and a port of this code would not touch it"));
    }

    // Shares, from the instrumented run.
    let profiled = profile["profiled_seconds"].as_f64().unwrap_or(0.0);
    let functions = profile["own_python_functions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if profiled <= 0.0 || functions.is_empty() {
        reasons.push("the profile saw none of the project's own Python: check `project_root`, and that the workload calls into the project".to_string());
    }
    let share = |key: &str| {
        if profiled > 0.0 {
            profile["seconds_by_where"][key].as_f64().unwrap_or(0.0) / profiled
        } else {
            0.0
        }
    };
    // Waiting happens inside compiled calls (a sleep, a read from a
    // socket), so it is taken out of the compiled share.
    // The time that was not waiting is split in the profile's proportions,
    // so that the four shares add up to one.
    let raw = [
        share("own_python"),
        share("library_python"),
        (share("compiled") - waiting).max(0.0),
    ];
    let scale = if raw.iter().sum::<f64>() > 0.0 {
        (1.0 - waiting) / raw.iter().sum::<f64>()
    } else {
        0.0
    };
    let (mut own, library, mut compiled) = (raw[0] * scale, raw[1] * scale, raw[2] * scale);
    let slowdown = if wall > 0.0 {
        profile["wall_seconds"].as_f64().unwrap_or(0.0) / wall
    } else {
        0.0
    };
    if slowdown >= SLOWDOWN_TO_FLAG {
        flags.push("slow_profile", true, format!("the instrumented run took {slowdown:.1} times as long as a plain one. The profiler's cost falls on every call, so the shares lean towards code that makes many small calls; the totals are from the plain runs and are not affected"));
    }
    if library >= MIN_SHARE_TO_PORT_FOR_SPEED {
        flags.push("library_python", true, format!("{:.0}% of the time is in library Python, not in the project's code. Porting the project's code does not touch it; whether a Rust crate replaces the library is the question of the dependency check. Part of it may be compiled work the profiler cannot see", library * 100.0));
    }

    // The candidate. Its own Python as a share of the time: one figure
    // when nothing compiled is involved, otherwise bounds.
    let mut candidate = J::Null;
    let mut at_most = None;
    let mut at_least = None;
    let mut uses_compiled = false;
    let mut rests_on_stand_ins = false;
    let mut rests_on_statement = false;
    let mut unseen_packages: Vec<String> = Vec::new();
    let mut compiled_packages: Vec<String> = Vec::new();
    // Calls into compiled objects that the profiler booked as the
    // project's Python and `sys.monitoring` timed, as a share of the time.
    let mut moved = 0.0;
    if let Some(c) = &decl.candidate {
        let matches: Vec<&J> = functions
            .iter()
            .filter(|f| {
                f["file"].as_str().map(Path::new) == Some(Path::new(&c.file))
                    && f["name"] == c.function.as_str()
                    && c.line.is_none_or(|l| f["line"].as_u64() == Some(l))
            })
            .collect();
        match matches.as_slice() {
            [] => reasons.push(format!("the candidate `{}` in {} was not called by the workload, so nothing is known about it", c.function, c.file)),
            [f] if boundary.as_ref().is_some_and(|b| b["entered_from_outside"].as_u64() == Some(0)) => {
                // The profiler saw it called and the hook on the main
                // thread never saw it entered: it ran in another thread,
                // where none of the instruments reach.
                reasons.push(format!("the candidate `{}` was called {} time(s), and never on the main thread. The instruments cover the main thread only, so its share is not known and none is given. Measure a workload that calls it directly", c.function, f["calls"]));
                candidate = json!({"declared": c, "ran_outside_the_main_thread": true, "calls": f["calls"], "boundary": boundary});
            }
            [f] => {
                let cumulative = f["cumulative_seconds"].as_f64().unwrap_or(0.0);
                let p = if profiled > 0.0 { f["made_of_seconds"]["own_python"].as_f64().unwrap_or(0.0) / profiled } else { 0.0 };
                let calls_made = f["calls"].as_u64().unwrap_or(0);
                let b = boundary.clone().unwrap_or(J::Null);
                let entered = b["entered_from_outside"].as_u64().unwrap_or(0);
                // Scaled back by the profiler's slowdown: roughly what a
                // call costs when nothing is attached.
                let per_call = if entered > 0 && slowdown > 0.0 { cumulative / entered as f64 / slowdown } else { 0.0 };
                if entered >= MANY_CALLS && per_call < SMALL_CALL_SECONDS {
                    flags.push("many_small_calls", true, format!("the candidate is entered {entered} times and each call takes about {:.2} microseconds. A call from Python into Rust costs on the order of a tenth of a microsecond before any argument is converted, so much of the gain would be spent on the boundary: port the loop that calls it", per_call * 1e6));
                }
                let crossing = b["objects_in_per_call"]["median"].as_f64().unwrap_or(0.0) + b["objects_out_per_call"]["median"].as_f64().unwrap_or(0.0);
                if crossing >= MANY_OBJECTS {
                    flags.push("many_objects_crossing", false, format!("a typical call of the candidate takes in and gives back {crossing:.0} Python objects. Each has to be converted at the boundary unless the design avoids it; that cost is not in these measurements"));
                }

                // Does the candidate, or what it calls, use a compiled
                // package? Five places to see it.
                let monitored = calls.clone().unwrap_or(J::Null);
                let third_party = |phase: &J| -> Vec<String> {
                    phase["packages"].as_object().into_iter().flatten().filter(|(_, v)| v["standard_library"] == false).map(|(k, _)| k.clone()).collect()
                };
                // Long calls into packages from outside the standard library,
                // seen by the profiler or not, as seconds.
                let long_third_party = |phase: &J| -> f64 {
                    phase["packages"].as_object().into_iter().flatten().filter(|(_, v)| v["standard_library"] == false).map(|(_, v)| v["not_seen_by_the_profiler"]["long_seconds"].as_f64().unwrap_or(0.0) + v["seen_by_the_profiler"]["long_seconds"].as_f64().unwrap_or(0.0)).sum()
                };
                // Long calls the profiler does not see, as seconds.
                let unseen = |phase: &J| -> f64 {
                    phase["packages"].as_object().into_iter().flatten().map(|(_, v)| v["not_seen_by_the_profiler"]["long_seconds"].as_f64().unwrap_or(0.0)).sum()
                };
                let named = profile["compiled_packages_named_in_functions_the_candidate_reaches"].clone();
                let evidence = json!({
                    "named_in_the_candidate_or_in_project_functions_it_calls": named,
                    "types_of_the_objects_going_in_and_out": b["compiled_packages_of_the_objects_crossing"],
                    "built_in_functions_called_while_it_runs": b["compiled_packages_called_while_it_runs"],
                    "local_variables_and_named_globals": b["compiled_packages_of_the_locals_and_globals"],
                    "calls_seen_by_sys_monitoring_while_it_runs": if monitored["available"] == true { J::from(third_party(&monitored)) } else { J::Null },
                });
                let mut packages: Vec<String> = Vec::new();
                for found in evidence.as_object().into_iter().flatten().map(|(_, v)| v) {
                    match found {
                        J::Array(a) => packages.extend(a.iter().filter_map(|x| x.as_str().map(String::from))),
                        J::Object(o) => o.iter().for_each(|(k, v)| match v {
                            J::Array(a) => packages.extend(a.iter().filter_map(|x| x.as_str().map(String::from))),
                            _ => packages.push(k.clone()),
                        }),
                        _ => {}
                    }
                }
                packages.sort();
                packages.dedup();
                uses_compiled = !packages.is_empty();
                compiled_packages = packages.clone();

                // What `sys.monitoring` timed: taken off the top.
                let monitored_wall = monitored["wall_seconds"].as_f64().unwrap_or(0.0);
                let unseen_share = if monitored_wall > 0.0 { (unseen(&monitored) / monitored_wall).min(p) } else { 0.0 };
                moved = unseen_share;
                if !uses_compiled && unseen_share > 0.01 {
                    flags.push("unseen_standard_library_calls", true, format!("{:.1}% of the time is calls from the candidate's code to compiled objects of the standard library that the profiler does not see; it is counted in the candidate's share", unseen_share * 100.0));
                }
                // What the stand-ins removed: the most that can be compiled
                // work counted as the candidate's Python.
                let mut stand_ins = J::Null;
                let mut removed = None;
                let mut stand_in_fraction = None;
                if let (Some((plain, calls_left, hook_left, file)), Some(declared)) = (&stand_in, &c.with_stand_ins) {
                    let t = summary(plain)?;
                    busy_plain("the stand-in workload", &t, &mut busy, &mut flags);
                    let w = t["fastest_run"]["wall_seconds"].as_f64().unwrap_or(0.0);
                    let fraction = if wall > 0.0 { w / wall } else { 0.0 };
                    // Is it a stand-in at all? What the tool can check.
                    let mut refused: Vec<String> = Vec::new();
                    if file == &workload_file && declared.function == decl.workload.function && declared.args == decl.workload.args {
                        refused.push("it names the same function and arguments as the workload, so it replaces nothing".to_string());
                    }
                    if hook_left["entered_from_outside"].as_u64().unwrap_or(0) == 0 {
                        refused.push("the candidate is not called in it; all of the candidate's own Python has to run in the stand-in workload".to_string());
                    }
                    // Objects of the package may still reach the candidate
                    // (a stand-in for a product returns a small array). A
                    // large one means the data is still there to work on.
                    let still = hook_left["largest_object_of_each_compiled_package_bytes"].clone();
                    let large: Vec<String> = still.as_object().into_iter().flatten().filter(|(_, v)| v.as_f64().is_some_and(|bytes| bytes > STAND_IN_OBJECT_BYTES)).map(|(k, v)| format!("{k} ({v} bytes)")).collect();
                    if !large.is_empty() {
                        refused.push(format!("large objects of compiled packages still reach the candidate in it (as arguments, results, local variables or named globals): {}. Operators on them still do the package's work; replace the objects themselves, and let a stand-in return something small of the right shape", large.join(", ")));
                    }
                    // Many small ones add up, and so do views of one array.
                    let sums = hook_left["total_bytes_of_each_compiled_package_seen"].clone();
                    let much: Vec<String> = sums.as_object().into_iter().flatten().filter(|(_, v)| v.as_f64().is_some_and(|bytes| bytes > STAND_IN_TOTAL_BYTES)).map(|(k, v)| format!("{k} ({v} bytes in all)")).collect();
                    if large.is_empty() && !much.is_empty() {
                        refused.push(format!("objects of compiled packages that together address much data still reach the candidate in it: {}. Every object seen is counted, so many calls with a small object each add up: use one small object for every call", much.join(", ")));
                    }
                    // Under the interpreter's lock the candidate's own Python
                    // cannot use more than one CPU. Counted between the
                    // candidate's entry and its return. More CPU time than
                    // wall time there is said and refuses nothing: a
                    // linear algebra library's threads spin for a moment
                    // after their last job, wherever that job was, and
                    // that costs a fixed amount of CPU which no limit
                    // tells from work.
                    let inside = |hook: &J, key: &str| hook["inside_the_candidate"][key].as_f64().unwrap_or(0.0);
                    let (cpu_left, wall_left) = (inside(hook_left, "cpu_seconds_of_all_threads"), inside(hook_left, "wall_seconds"));
                    let entries = hook_left["entered_from_outside"].as_u64().unwrap_or(0);
                    if let Some(text) = stand_in_threads_flag(cpu_left, wall_left, entries) {
                        flags.push("stand_in_more_cpu_than_wall", true, text);
                    }
                    let left = long_third_party(calls_left);
                    if wall > 0.0 && left > STAND_INS_LEAVE_CALLS * wall {
                        refused.push(format!("the candidate's code still spends {left:.3} s in long calls into compiled packages in it ({:.3} s in the original)", long_third_party(&monitored)));
                    }
                    if !uses_compiled {
                        flags.push("stand_ins_not_needed", false, "a stand-in workload is named and no use of a compiled package by the candidate was found; it was timed and is not used".to_string());
                    } else if refused.is_empty() {
                        removed = Some((1.0 - fraction).clamp(0.0, 1.0));
                        stand_in_fraction = Some(fraction.min(1.0));
                        if let Some((kind, text)) = stand_in_flag(fraction) {
                            flags.push(kind, true, text);
                        }
                    } else {
                        reasons.push(format!("the stand-in workload `{}` is not a stand-in and is not used: {}", declared.function, refused.join("; ")));
                    }
                    stand_ins = json!({
                        "declared": declared, "file": shown(file), "file_sha256": sha256_file(file)?, "measured": t,
                        "time_as_a_fraction_of_the_original": round(fraction, 4),
                        "refused_because": refused,
                        "share_of_the_time_the_stand_ins_removed": removed.map(|x| round(x, 4)),
                        "calls_into_compiled_code_left": calls_left,
                        // With the hook on, so slower than a plain run.
                        "inside_the_candidate": hook_left["inside_the_candidate"],
                        "largest_object_of_each_compiled_package_still_reaching_the_candidate_bytes": still,
                        "what_was_checked": "That it is another function or other arguments than the workload; that the candidate is called in it; that the objects of compiled packages that reach the candidate in it address little data, one by one and together (a view of an array counts the data it shows); and on Python 3.12 and later that the candidate's code makes no long calls into compiled packages in it. Not checked: that it does the rest of the work the workload does. More CPU time than wall time while the candidate runs in it is flagged and refuses nothing.",
                    });
                }
                // With stand-ins there is a second upper bound: all of the
                // candidate's own Python still runs in the stand-in
                // workload, so it is no more than that workload's time.
                let high = if uses_compiled {
                    (p - unseen_share).max(0.0).min(stand_in_fraction.unwrap_or(1.0))
                } else {
                    p
                };
                let low = if uses_compiled { removed.map(|x| (p - x).max(0.0).min(high)) } else { Some(p) };
                at_most = Some(high);
                at_least = low;
                rests_on_stand_ins = uses_compiled && removed.is_some();
                // The backstop: a compiled package is loaded in the process
                // and nothing showed the candidate using it. The tool does
                // not find every use.
                let loaded_unseen: Vec<String> = if uses_compiled {
                    Vec::new()
                } else {
                    let mut all: Vec<String> = [&profile, &b, &monitored].iter().flat_map(|phase| phase["compiled_packages_loaded"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from))).collect();
                    all.sort();
                    all.dedup();
                    all
                };
                unseen_packages = loaded_unseen.clone();
                let ceiling = |p: f64| if p < CEILING_NOT_SHOWN_ABOVE { J::from(round(1.0 / (1.0 - p), 2)) } else { J::Null };
                if uses_compiled {
                    flags.push("uses_compiled_package", false, format!("the candidate, or what it calls, uses compiled packages {packages:?}. The profiler counted {:.1}% of the time as the candidate's own Python, and that figure includes work done inside those packages, which a port would not make faster. It is not given as a finding", p * 100.0));
                }
                let holds_only_if: J = if loaded_unseen.is_empty() { J::Null } else {
                    J::from(format!("the candidate uses no compiled package, which {} and the tool has not checked", if c.uses_no_compiled_package.as_ref().is_some_and(|t| words(t) >= MIN_WORDS) { "the author states" } else { "nobody has stated" }))
                };
                candidate = json!({
                    "declared": c, "file_sha256": candidate_file.as_deref().map(sha256_file).transpose()?,
                    "line": f["line"], "calls": calls_made, "callers": f["callers"], "caller_names": f["caller_names"],
                    "cumulative_share_of_the_profile": round(if profiled > 0.0 { cumulative / profiled } else { 0.0 }, 4),
                    "made_of_seconds_in_the_profile": f["made_of_seconds"],
                    "uses_compiled_packages": packages,
                    "compiled_packages_loaded_and_not_seen_in_the_candidate": loaded_unseen,
                    "declared_to_use_no_compiled_package": c.uses_no_compiled_package,
                    "own_python_share_holds_only_if": holds_only_if,
                    "where_compiled_packages_were_seen": evidence,
                    // One figure when no compiled package is involved.
                    "own_python_share": if uses_compiled { J::Null } else { J::from(round(p, 4)) },
                    "own_python_share_bounds": if uses_compiled {
                        json!({
                            "at_most": round(high, 4),
                            "at_least": low.map(|x| round(x, 4)),
                            "as_the_profiler_counted_it": round(p, 4),
                            "long_calls_into_compiled_objects_the_profiler_does_not_see": round(unseen_share, 4),
                            "how": "The profiler's figure counts compiled work reached through operators, and through callables that are not built-in functions, as the candidate's Python. `at_most` is that figure less the long calls of that kind that `sys.monitoring` timed (Python 3.12 and later; nothing is taken off on older Pythons). Work done by operators is still inside it. With a stand-in workload `at_most` is also no more than that workload's share of the original's time, since all of the candidate's own Python still runs in it, and `at_least` is the profiler's figure less everything the stand-ins removed. Without a stand-in workload `at_least` is not known.",
                        })
                    } else { J::Null },
                    "ceiling": if uses_compiled {
                        json!({
                            "p": J::Null,
                            "holds_only_if": holds_only_if,
                            "the_whole_workload_could_be_at_most_this_many_times_faster": J::Null,
                            "bounds": low.map(|l| json!({"with_p_at_least": ceiling(l), "with_p_at_most": ceiling(high)})),
                            "note": "no single ceiling is given: the candidate's own Python cannot be separated from work inside compiled packages",
                            "meaning": "1 / (1 - p): the gain if the candidate's own Python cost nothing at all. A real port gains less.",
                        })
                    } else {
                        json!({
                            "p": round(p, 4),
                            "holds_only_if": holds_only_if,
                            "the_whole_workload_could_be_at_most_this_many_times_faster": ceiling(p),
                            "note": if p < CEILING_NOT_SHOWN_ABOVE { J::Null } else { J::from("nearly all of the time is this piece, so the formula gives no useful bound") },
                            "meaning": "1 / (1 - p): the gain if the candidate's own Python cost nothing at all. A real port gains less.",
                        })
                    },
                    "best_case": {
                        "holds_only_if": holds_only_if,
                        "workload_seconds_now": wall,
                        "workload_seconds_if_the_candidate_cost_nothing": wall * (1.0 - high),
                        "seconds_saved_per_run": wall * high,
                        "note": if uses_compiled { "from the upper bound of the candidate's share, so the true best case is no better than this and may be much worse" } else { "the candidate's own Python taken away entirely; a real port saves less" },
                    },
                    "seconds_per_call_estimate": per_call,
                    "largest_callees": f["largest_callees"],
                    "compiled_modules_imported_by_its_file": profile["compiled_modules_imported_by_the_candidate_file"],
                    "with_stand_ins": stand_ins,
                    "calls_into_compiled_code": monitored,
                    "boundary": b,
                });
            }
            many => reasons.push(format!("the file has {} functions named `{}` (lines {}); give the candidate's `line`", many.len(), c.function, many.iter().map(|f| f["line"].to_string()).collect::<Vec<_>>().join(", "))),
        }
    }

    // The split of the whole workload, corrected as far as it can be.
    let moved = moved.min(own);
    own -= moved;
    compiled += moved;
    let shares = [
        ("waiting", waiting),
        ("compiled_code", compiled),
        ("library_python", library),
        ("own_python", own),
    ];
    let top = shares
        .iter()
        .copied()
        .fold(("", 0.0), |a, b| if b.1 > a.1 { b } else { a });
    let classification = if top.0 == "own_python" && uses_compiled && top.1 >= DOMINANT {
        // The largest share holds work inside compiled packages.
        "own_python_and_compiled_packages_not_separated".to_string()
    } else if top.1 >= DOMINANT {
        format!("mostly_{}", top.0)
    } else {
        "mixed".to_string()
    };

    // The alternative in Python, measured the same way.
    let mut alternative = J::Null;
    let mut ratio = None;
    if let Some((plain, f)) = &tried {
        let t = summary(plain)?;
        busy_plain("the alternative", &t, &mut busy, &mut flags);
        let w = t["fastest_run"]["wall_seconds"].as_f64().unwrap_or(0.0);
        ratio = (wall > 0.0).then(|| w / wall);
        if w < MIN_WALL_SECONDS {
            flags.push("alternative_too_short", true, format!("the alternative ran for {:.1} milliseconds. That is too short to time well, so \"how many times faster\" is rough; that it is much faster is not in doubt", w * 1e3));
        }
        alternative = json!({
            "file": shown(f), "file_sha256": sha256_file(f)?, "measured": t,
            "time_as_a_fraction_of_the_original": ratio.map(|r| round(r, 4)),
            "times_faster": ratio.filter(|r| *r > 0.0).map(|r| round(1.0 / r, 2)),
        });
    }

    // The declared statements against the measurements.
    match &decl.language_or_algorithm {
        None => reasons.push("the question \"is the cost the language's or the algorithm's?\" is not answered (`language_or_algorithm`)".to_string()),
        Some(a) => {
            if words(&a.statement) < MIN_WORDS {
                reasons.push(format!("the answer to \"language or algorithm?\" is {:?}; say what the code does that costs the time, in at least {MIN_WORDS} words", a.statement));
            }
            match (a.better_algorithm_in_python.as_str(), &a.tried) {
                ("none_known" | "exists_untried", None) | ("tried", Some(_)) => {}
                ("tried", None) => reasons.push("`better_algorithm_in_python` is \"tried\" and no alternative is named for the tool to measure".to_string()),
                ("none_known" | "exists_untried", Some(_)) => reasons.push("an alternative is named and `better_algorithm_in_python` does not say \"tried\"".to_string()),
                (other, _) => reasons.push(format!("`better_algorithm_in_python` is {other:?}; it must be \"none_known\", \"exists_untried\" or \"tried\"")),
            }
        }
    }
    let algorithm_state = decl
        .language_or_algorithm
        .as_ref()
        .map(|a| a.better_algorithm_in_python.as_str());
    let mut need = J::Null;
    match &decl.decision {
        None => reasons.push("no decision is declared: `port`, `fix_algorithm_first` or `do_not_port`, with the reason".to_string()),
        Some(d) => {
            if words(&d.reason) < MIN_WORDS {
                reasons.push(format!("the decision's reason is {:?}; give one of at least {MIN_WORDS} words", d.reason));
            }
            for g in d.goals.iter().filter(|g| !GOALS.contains(&g.as_str())) {
                reasons.push(format!("unknown goal {g:?}; the goals are {GOALS:?}"));
            }
            match d.choice.as_str() {
                "port" => {
                    if decl.candidate.is_none() {
                        reasons.push("the decision is to port and no candidate piece is named".to_string());
                    }
                    if d.goals.is_empty() {
                        reasons.push(format!("the decision is to port and no goal is given; the goals are {GOALS:?}"));
                    }
                    if !d.goals.is_empty() && d.goals.iter().all(|g| g == "memory_safety") {
                        contradictions.push("the only goal is memory safety, and Python is already memory-safe: that is not a reason to port on its own".to_string());
                    }
                    if d.goals.iter().any(|g| g == "speed") {
                        let split = format!("The time is {:.0}% waiting, {:.0}% in compiled code, {:.0}% in library Python and {:.0}% in the project's own Python", waiting * 100.0, compiled * 100.0, library * 100.0, own * 100.0);
                        if let Some(p) = at_most.filter(|p| *p < MIN_SHARE_TO_PORT_FOR_SPEED) {
                            contradictions.push(format!(
                                "the decision is to port for speed, and the candidate's own Python is {}{:.1}% of the workload's time: were it to cost nothing, the workload would be at most {:.2} times faster. {split}",
                                if uses_compiled { "at most " } else { "" }, p * 100.0, 1.0 / (1.0 - p)
                            ));
                        } else if uses_compiled {
                            // The share could be anything up to the bound.
                            match at_least {
                                None => reasons.push("the decision is to port for speed, and the candidate uses a compiled package, so how much of its time is its own Python is not known: operators on the package's objects, and calls the instruments do not see, are counted as the candidate's Python. Show where the time is by another measurement: name `candidate.with_stand_ins`, the same workload with the package's work replaced by stand-ins of the same shape, and the tool will time it".to_string()),
                                Some(l) if l < MIN_SHARE_TO_PORT_FOR_SPEED => reasons.push(format!("the decision is to port for speed, and with the stand-ins the candidate's own Python is somewhere between {:.1}% and {:.1}% of the time: how much of what the stand-ins removed the profiler had counted as the candidate's Python is not known. That range reaches below the {:.0}% at which a port for speed is contradicted, so this tool cannot settle it; a line profiler in a scratch environment can", l * 100.0, at_most.unwrap_or(0.0) * 100.0, MIN_SHARE_TO_PORT_FOR_SPEED * 100.0)),
                                Some(_) => {}
                            }
                        } else if !unseen_packages.is_empty() {
                            // Loaded, and not seen in the candidate.
                            match decl.candidate.as_ref().and_then(|c| c.uses_no_compiled_package.as_ref()) {
                                Some(statement) if words(statement) >= MIN_WORDS => { rests_on_statement = true; flags.push("declared_to_use_no_compiled_package", false, format!("compiled packages {unseen_packages:?} are loaded in the process and the tool saw no use of them by the candidate. It does not find every use. The author declares, and the tool has not checked: {statement}")); }
                                _ => reasons.push(format!("the decision is to port for speed, compiled packages {unseen_packages:?} are loaded in the process, and the tool saw no use of them by the candidate. It does not find every use (an array reached only through an operator, from a place the tool does not look, is invisible), and if the candidate uses one its share is wrong. If it does, name a stand-in workload (`candidate.with_stand_ins`). If it does not, say so in `candidate.uses_no_compiled_package`, in at least {MIN_WORDS} words: what the candidate works on instead")),
                            }
                        }
                        if algorithm_state == Some("exists_untried") {
                            contradictions.push("the decision is to port for speed while the author says a better algorithm exists in Python and has not been tried. Try it first: it is cheaper than a port, and the port's gain has to be counted from there".to_string());
                        }
                        if let Some(r) = ratio.filter(|r| *r <= ALGORITHM_REMOVED_MOST) {
                            contradictions.push(format!("the decision is to port for speed, and the alternative in Python already takes {:.0}% of the original's time ({:.1} times faster). Make that change, then run this tool on the improved code and decide there", r * 100.0, 1.0 / r));
                        }
                        // What speed is needed, set against the best case.
                        match &d.need {
                            None => reasons.push("the decision is to port for speed and no need is declared (`decision.need`): say who needs it faster and why, with one number, either the time the workload must finish in (`workload_must_take_at_most_seconds`) or how often it runs (`runs_per_day`)".to_string()),
                            Some(n) => {
                                if words(&n.statement) < MIN_WORDS {
                                    reasons.push(format!("the need's statement is {:?}; say who needs the speed and why, in at least {MIN_WORDS} words", n.statement));
                                }
                                let best = at_most.map(|p| (wall * (1.0 - p), wall * p));
                                match (n.workload_must_take_at_most_seconds, n.runs_per_day) {
                                    (Some(limit), None) if limit > 0.0 => {
                                        if wall <= limit {
                                            contradictions.push(format!("the declared need is that the workload takes at most {limit} s, and it takes {wall:.3} s now: the need is already met"));
                                        } else if let Some((total, _)) = best.filter(|(total, _)| *total > limit) {
                                            contradictions.push(format!("the declared need is that the workload takes at most {limit} s. It takes {wall:.3} s, and if the candidate's own Python cost nothing at all it would still take {total:.3} s: no port of this piece can reach the need"));
                                        }
                                    }
                                    (None, Some(runs)) if runs > 0.0 => {
                                        if let Some((_, saved)) = best.filter(|(_, saved)| saved * runs < SMALL_SAVING_SECONDS_PER_DAY) {
                                            flags.push("small_saving", true, format!("by the declared {runs} runs a day, a port that made the candidate cost nothing would save {:.1} s a day", saved * runs));
                                        }
                                    }
                                    _ => reasons.push("the need must give exactly one number above zero: `workload_must_take_at_most_seconds` or `runs_per_day`".to_string()),
                                }
                                need = json!({
                                    "declared_by_the_author": n,
                                    "holds_only_if": candidate["own_python_share_holds_only_if"],
                                    "workload_seconds_now": wall,
                                    "workload_seconds_if_the_candidate_cost_nothing": best.map(|b| b.0),
                                    "seconds_saved_per_run_at_best": best.map(|b| b.1),
                                    "seconds_saved_per_day_at_best": n.runs_per_day.zip(best).map(|(runs, b)| runs * b.1),
                                });
                            }
                        }
                    } else if !d.goals.is_empty() {
                        flags.push("port_not_for_speed", false, "the port is not for speed. Time measurements neither support nor contradict the stated goals; the cost of the boundary still applies".to_string());
                    }
                }
                "fix_algorithm_first" => match (algorithm_state, ratio) {
                    (Some("exists_untried"), _) => {}
                    (Some("tried"), Some(r)) if r > 0.9 => contradictions.push(format!("the decision is to fix the algorithm first, and the alternative measured takes {:.0}% of the original's time: it is not faster", r * 100.0)),
                    (Some("tried"), _) => {}
                    _ => reasons.push("the decision is to fix the algorithm first, and `language_or_algorithm` does not say a better one exists".to_string()),
                },
                "do_not_port" => {}
                other => reasons.push(format!("unknown decision {other:?}; it must be \"port\", \"fix_algorithm_first\" or \"do_not_port\"")),
            }
        }
    }
    if !git.dirty.is_empty() {
        flags.push(
            "uncommitted_changes",
            false,
            "the working tree had uncommitted changes; the report does not describe the commit"
                .to_string(),
        );
    }

    let verdict = if !busy.is_empty() {
        "unreliable"
    } else if !reasons.is_empty() {
        "incomplete"
    } else if !contradictions.is_empty() {
        "contradicted"
    } else if rests_on_stand_ins
        && decl
            .decision
            .as_ref()
            .is_some_and(|d| d.choice == "port" && d.goals.iter().any(|g| g == "speed"))
    {
        // Nothing contradicts the port, and that rests on the author's
        // stand-ins: the lower bound of the candidate's share comes from
        // them, and the tool cannot check all that they replace.
        "consistent_if_the_stand_ins_are_faithful"
    } else if rests_on_statement {
        // The same, resting on a sentence: a compiled package is loaded,
        // the tool saw the candidate use none, it does not find every use,
        // and the author states that there is none.
        "consistent_on_the_authors_word"
    } else {
        "consistent"
    };
    let top_functions: Vec<J> = functions
        .iter()
        .take(12)
        .map(|f| {
            json!({
                "file": f["file"], "line": f["line"], "name": f["name"], "calls": f["calls"], "callers": f["callers"],
                "cumulative_share": round(if profiled > 0.0 { f["cumulative_seconds"].as_f64().unwrap_or(0.0) / profiled } else { 0.0 }, 4),
                "self_share": round(if profiled > 0.0 { f["self_seconds"].as_f64().unwrap_or(0.0) / profiled } else { 0.0 }, 4),
                "own_python_share_with_what_it_calls": round(if profiled > 0.0 { f["made_of_seconds"]["own_python"].as_f64().unwrap_or(0.0) / profiled } else { 0.0 }, 4),
            })
        })
        .collect();
    Ok(json!({
        "schema": REPORT_SCHEMA,
        "verdict": verdict,
        "exit_status": if verdict.starts_with("consistent") { 0 } else { 1 },
        "verdict_means": "`consistent`: no measurement contradicts the declared decision. It does not mean the decision is right. `contradicted`: a measurement plainly does. `incomplete`: something needed is missing. `consistent_if_the_stand_ins_are_faithful`: nothing contradicts a port for speed, and only because of the stand-in workload the author wrote: the candidate uses a compiled package, and how little of its time may be its own Python is known from the stand-ins alone. `consistent_on_the_authors_word`: nothing contradicts a port for speed, a compiled package is loaded in the process, the tool saw the candidate use none and cannot be sure, and the author states that it uses none; the candidate's share holds only if that is true. `unreliable`: the machine was busy while the measurements were made, so none of the figures below should be used; measure again.",
        "machine_was_busy": busy,
        "contradictions": contradictions,
        "reasons": reasons,
        "flags": flags.texts,
        "flag_kinds": flags.kinds,
        "not_measured": NOT_MEASURED,
        "git": {"commit": git.commit, "dirty": git.dirty},
        "declaration": {"file": shown(&path), "sha256": sha256_file(&path)?},
        "machine": {
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map_or(0, |n| n.get()),
            "python": plain["python"], "python_implementation": plain["implementation"],
            "interpreter": tool_version(&opts.python, &["--version"], here),
        },
        "workload": {
            "declared": decl.workload, "file": shown(&workload_file), "file_sha256": sha256_file(&workload_file)?,
            "project_root": shown(&root),
            "measured_with_nothing_attached": totals,
        },
        "where_the_time_goes": {
            "classification": classification,
            "shares": {"waiting": round(waiting, 4), "compiled_code": round(compiled, 4), "library_python": round(library, 4), "own_python": round(own, 4)},
            "own_python_includes_work_inside_compiled_packages": compiled_packages,
            // Loaded in the process, with nothing seen to use them: the
            // profiler would count their work as the project's own Python
            // all the same.
            "own_python_may_include_work_inside_loaded_packages": if uses_compiled { J::from(Vec::<String>::new()) } else { profile["compiled_packages_loaded"].clone() },
            "moved_from_own_python_to_compiled_code": round(moved, 4),
            "how": "Waiting is one minus CPU time over wall time in the fastest plain run. The rest is split in the proportions of one run under cProfile, with waiting first taken out of the compiled part, since a sleep or a read is a compiled call. A compiled call shorter than the limit below on average is counted with the Python code that makes it. Where the candidate uses a compiled package, the long calls into it that only `sys.monitoring` sees are moved from the project's own Python to compiled code; work reached through operators stays in the project's own Python, and the classification then says the two are not separated.",
            "short_compiled_call_seconds": profile["short_compiled_call_seconds"],
            "instrumented_run": {"wall_seconds": profile["wall_seconds"], "times_slower_than_plain": round(slowdown, 2), "seconds_by_where": profile["seconds_by_where"]},
            "own_python_functions": top_functions,
            "own_python_functions_seen": profile["own_python_functions_total"],
            "largest_outside_the_project": profile["largest_outside_the_project"],
        },
        "candidate": candidate,
        "language_or_algorithm": {"declared_by_the_author": decl.language_or_algorithm, "alternative_measured": alternative},
        "declared_decision": decl.decision,
        "need_for_speed": need,
        "limits_used": {
            "stand_ins_remove_little": STAND_INS_REMOVE_LITTLE, "stand_ins_leave_calls": STAND_INS_LEAVE_CALLS, "stand_in_cpu_over_wall": STAND_IN_CPU_OVER_WALL, "stand_in_surplus_per_entry_seconds": STAND_IN_SURPLUS_PER_ENTRY_SECONDS, "stand_in_object_bytes": STAND_IN_OBJECT_BYTES, "stand_in_total_bytes": STAND_IN_TOTAL_BYTES,
            "busy_calibration": BUSY_CALIBRATION, "busy_while_running": BUSY_WHILE_RUNNING, "not_watched_above_this_share_of_the_cpus": NOT_WATCHED_ABOVE_THIS_SHARE_OF_THE_CPUS, "small_saving_seconds_per_day": SMALL_SAVING_SECONDS_PER_DAY,
            "min_share_to_port_for_speed": MIN_SHARE_TO_PORT_FOR_SPEED, "algorithm_removed_most": ALGORITHM_REMOVED_MOST, "dominant": DOMINANT,
            "min_wall_seconds": MIN_WALL_SECONDS, "many_calls": MANY_CALLS, "small_call_seconds": SMALL_CALL_SECONDS, "many_objects": MANY_OBJECTS,
        },
    }))
}

/// The report as Markdown, for a person to read.
pub fn markdown(r: &J) -> String {
    let pct = |v: &J| format!("{:.1}%", v.as_f64().unwrap_or(0.0) * 100.0);
    let s = |v: &J| v.as_str().unwrap_or("").to_string();
    let w = &r["workload"];
    let t = &w["measured_with_nothing_attached"];
    let mut o = format!(
        "# Should this piece be ported?\n\nVerdict: **{}**. {}\n",
        s(&r["verdict"]),
        match r["verdict"].as_str() {
            Some("consistent") =>
                "No measurement contradicts the declared decision. That is not the same as the decision being right.",
            Some("contradicted") => "A measurement contradicts the declared decision.",
            Some("consistent_if_the_stand_ins_are_faithful") =>
                "No measurement contradicts the declared decision, and that rests on the stand-in workload the author wrote: the candidate uses a compiled package, and the tool cannot check all that the stand-ins replace. Read the stand-in workload before relying on this.",
            Some("consistent_on_the_authors_word") =>
                "No measurement contradicts the declared decision, and that rests on a sentence the author typed: a compiled package is loaded in this process, the tool saw the candidate use none, it does not find every use, and the author states there is none. If that is wrong, the candidate's share below is wrong.",
            Some("unreliable") =>
                "The machine was busy while this was measured. Do not use the figures below; measure again when it is quiet.",
            _ => "Something needed for a decision is missing.",
        }
    );
    for (title, key) in [
        ("The machine was busy", "machine_was_busy"),
        ("Contradicted by the measurements", "contradictions"),
        ("Missing", "reasons"),
        ("For a reader to look at", "flags"),
    ] {
        let items = r[key].as_array().cloned().unwrap_or_default();
        if !items.is_empty() {
            o.push_str(&format!("\n{title}:\n\n"));
            items
                .iter()
                .for_each(|i| o.push_str(&format!("- {}\n", s(i))));
        }
    }
    o.push_str(&format!(
        "\n## What was run\n\n- {}\n- `{}` in `{}`, arguments `{}`, {} times with nothing attached\n- Commit `{}`{}, {} {} with {} logical CPUs, {}\n- Wall time: fastest {:.3} s (median {:.3}, slowest {:.3}; they differ by {} of the median). CPU time of the fastest run: {:.3} s. Peak memory: {}\n",
        s(&w["declared"]["description"]),
        s(&w["declared"]["function"]),
        s(&w["file"]),
        w["declared"]["args"],
        w["declared"]["repeat"],
        &s(&r["git"]["commit"])[..12.min(s(&r["git"]["commit"]).len())],
        if r["git"]["dirty"].as_array().is_some_and(|d| !d.is_empty()) { " plus uncommitted changes" } else { "" },
        s(&r["machine"]["os"]),
        s(&r["machine"]["arch"]),
        r["machine"]["logical_cpus"],
        s(&r["machine"]["interpreter"]),
        t["wall_seconds"]["fastest"].as_f64().unwrap_or(0.0),
        t["wall_seconds"]["median"].as_f64().unwrap_or(0.0),
        t["wall_seconds"]["slowest"].as_f64().unwrap_or(0.0),
        pct(&t["wall_seconds"]["spread_of_median"]),
        t["fastest_run"]["cpu_seconds"].as_f64().unwrap_or(0.0),
        t["peak_memory_bytes"].as_f64().map_or("not known".to_string(), |b| format!("{:.0} MB", b / 1e6)),
    ));
    let g = &r["where_the_time_goes"];
    o.push_str(&format!(
        "\n## Where the time goes: `{}`\n\n| Waiting | Compiled code | Library Python | The project's own Python{} |\n|---|---|---|---|\n| {} | {} | {} | {} |\n\n{} The instrumented run took {} times as long as a plain one.\n",
        s(&g["classification"]),
        if g["own_python_includes_work_inside_compiled_packages"].as_array().is_some_and(|a| !a.is_empty()) { format!(", with work inside {} that cannot be separated from it", g["own_python_includes_work_inside_compiled_packages"]) } else if g["own_python_may_include_work_inside_loaded_packages"].as_array().is_some_and(|a| !a.is_empty()) { format!(", which may include work inside {}: the profiler counts a compiled package's work as the Python function that asked for it", g["own_python_may_include_work_inside_loaded_packages"]) } else { String::new() },
        pct(&g["shares"]["waiting"]),
        pct(&g["shares"]["compiled_code"]),
        pct(&g["shares"]["library_python"]),
        pct(&g["shares"]["own_python"]),
        s(&g["how"]),
        g["instrumented_run"]["times_slower_than_plain"],
    ));
    o.push_str("\nThe project's own functions, by time including what they call:\n\n| Function | File | Calls | Callers | With what it calls | Of that, own Python | Itself |\n|---|---|---|---|---|---|---|\n");
    for f in g["own_python_functions"].as_array().into_iter().flatten() {
        o.push_str(&format!(
            "| `{}` | {}:{} | {} | {} | {} | {} | {} |\n",
            s(&f["name"]),
            s(&f["file"]),
            f["line"],
            f["calls"],
            f["callers"],
            pct(&f["cumulative_share"]),
            pct(&f["own_python_share_with_what_it_calls"]),
            pct(&f["self_share"])
        ));
    }
    if g["own_python_includes_work_inside_compiled_packages"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        o.push_str(&format!("\nThese figures are as the profiler counted them. Work inside {} reached through operators, or through objects the profiler does not see as functions, is in the \"own Python\" columns.\n", g["own_python_includes_work_inside_compiled_packages"]));
    }
    o.push_str("\nThe largest outside the project:\n\n");
    for f in g["largest_outside_the_project"]
        .as_array()
        .into_iter()
        .flatten()
        .take(6)
    {
        o.push_str(&format!(
            "- `{}` ({}{}), {} calls, {:.3} s in the instrumented run\n",
            s(&f["name"]),
            s(&f["where"]).replace('_', " "),
            if f["long_calls"] == true {
                ", long calls"
            } else {
                ""
            },
            f["calls"],
            f["self_seconds"].as_f64().unwrap_or(0.0)
        ));
    }
    let c = &r["candidate"];
    if c["ran_outside_the_main_thread"] == true {
        o.push_str(&format!(
            "\n## The candidate: `{}` in `{}`\n\n- Called {} time(s), and never on the main thread, where the instruments are. Its share is not known and none is given.\n",
            s(&c["declared"]["function"]),
            s(&c["declared"]["file"]),
            c["calls"],
        ));
    } else if !c.is_null() {
        let b = &c["boundary"];
        let n = |v: &J| {
            v.as_f64()
                .map_or("not known".to_string(), |x| format!("{x:.0}"))
        };
        o.push_str(&format!(
            "\n## The candidate: `{}` in `{}`\n\n- Called {} times by {} caller(s): {}\n- With everything it calls it is {} of the time\n",
            s(&c["declared"]["function"]),
            s(&c["declared"]["file"]),
            c["calls"],
            c["callers"],
            c["caller_names"],
            pct(&c["cumulative_share_of_the_profile"]),
        ));
        let best = &c["best_case"];
        let bounds = &c["own_python_share_bounds"];
        // The same condition on every figure that depends on it.
        let cond = c["own_python_share_holds_only_if"]
            .as_str()
            .map_or(String::new(), |t| format!(", if {t}"));
        if bounds.is_null() {
            o.push_str(&format!(
                "- Its own Python, with the Python it calls, is **{}** of the time{cond}\n- **Ceiling: {}** (1 / (1 - p) with p = {}){cond}. A real port gains less.\n- Best case: the workload takes {:.3} s now and would take {:.3} s if the candidate's own Python cost nothing, {:.3} s saved per run{cond}\n",
                pct(&c["own_python_share"]),
                match c["ceiling"]["the_whole_workload_could_be_at_most_this_many_times_faster"].as_f64() {
                    Some(x) => format!("if that cost nothing, the whole workload would be at most {x} times faster"),
                    None => "nearly all of the time is this piece, so the formula gives no useful bound".to_string(),
                },
                c["ceiling"]["p"],
                best["workload_seconds_now"].as_f64().unwrap_or(0.0),
                best["workload_seconds_if_the_candidate_cost_nothing"].as_f64().unwrap_or(0.0),
                best["seconds_saved_per_run"].as_f64().unwrap_or(0.0),
            ));
        } else {
            o.push_str(&format!(
                "- **It uses compiled packages: {}.** How much of the time is its own Python cannot be separated from work done inside them, so no share and no ceiling are given as findings.\n- What is known: its own Python is **at most {}** of the time{}. The profiler counted {}; that figure includes compiled work reached through operators and through objects the profiler does not see as functions. Long calls of that kind timed with `sys.monitoring`: {}.\n- At best, and only if the upper bound were the truth: the workload takes {:.3} s now and would take {:.3} s, {:.3} s saved per run\n- Where the packages were seen: {}\n",
                c["uses_compiled_packages"],
                pct(&bounds["at_most"]),
                match bounds["at_least"].as_f64() {
                    Some(l) => format!(" and **at least about {:.1}%, if the stand-ins replace all of the package's work and nothing else**", l * 100.0),
                    None => ", and nothing is known about how little it may be: no stand-in workload was named".to_string(),
                },
                pct(&bounds["as_the_profiler_counted_it"]),
                if c["calls_into_compiled_code"]["available"] == true { pct(&bounds["long_calls_into_compiled_objects_the_profiler_does_not_see"]) } else { "not available on this Python (3.12 or later is needed)".to_string() },
                best["workload_seconds_now"].as_f64().unwrap_or(0.0),
                best["workload_seconds_if_the_candidate_cost_nothing"].as_f64().unwrap_or(0.0),
                best["seconds_saved_per_run"].as_f64().unwrap_or(0.0),
                c["where_compiled_packages_were_seen"],
            ));
            let st = &c["with_stand_ins"];
            if !st.is_null() {
                o.push_str(&format!(
                    "- The stand-in workload, written by the author and timed by the tool: {} `{}` took {} of the original's time (fastest run {:.3} s). {}\n",
                    s(&st["declared"]["description"]),
                    s(&st["declared"]["function"]),
                    pct(&st["time_as_a_fraction_of_the_original"]),
                    st["measured"]["wall_seconds"]["fastest"].as_f64().unwrap_or(0.0),
                    if st["refused_because"].as_array().is_some_and(|a| !a.is_empty()) {
                        format!("**It is not a stand-in and was not used:** {}.", st["refused_because"].as_array().into_iter().flatten().map(s).collect::<Vec<_>>().join("; "))
                    } else {
                        format!("{} The lower bound holds only if the stand-ins replace all of the package's work and nothing else.", s(&st["what_was_checked"]))
                    },
                ));
            }
        }
        let unseen = &c["compiled_packages_loaded_and_not_seen_in_the_candidate"];
        if unseen.as_array().is_some_and(|a| !a.is_empty()) {
            o.push_str(&format!(
                "- Compiled packages loaded in the process and not seen in the candidate: {unseen}. The tool does not find every use of such a package; if the candidate uses one, the share above is wrong. {}\n",
                match c["declared_to_use_no_compiled_package"].as_str() {
                    Some(statement) => format!("Declared by the author, not checked: {statement}"),
                    None => "The author has not said that it uses none.".to_string(),
                }
            ));
        }
        if c["calls_into_compiled_code"]["available"] != true {
            o.push_str("- Calls into compiled objects were not timed: that needs Python 3.12 or later. On this Python a compiled package is found by name, by the types of the objects crossing, of local variables and of named globals, and by its built-in functions only.\n");
        }
        o.push_str(&format!(
            "- Across the boundary: entered {} times from outside; a typical call takes in {} Python objects ({} bytes) and gives back {} ({} bytes); about {:.2} microseconds per call\n- First call: arguments {}, returns {}\n- Its file imports these compiled modules: {}\n",
            b["entered_from_outside"],
            n(&b["objects_in_per_call"]["median"]),
            n(&b["bytes_in_per_call"]["median"]),
            n(&b["objects_out_per_call"]["median"]),
            n(&b["bytes_out_per_call"]["median"]),
            c["seconds_per_call_estimate"].as_f64().unwrap_or(0.0) * 1e6,
            b["first_call"]["arguments"],
            b["first_call"]["returns"],
            c["compiled_modules_imported_by_its_file"],
        ));
    }
    let a = &r["language_or_algorithm"];
    o.push_str("\n## Language or algorithm?\n\n");
    match a["declared_by_the_author"].as_object() {
        None => o.push_str("Not answered.\n"),
        Some(d) => {
            o.push_str(&format!(
                "Declared by the author, not measured: {} Better algorithm in Python: `{}`.\n",
                s(&d["statement"]),
                s(&d["better_algorithm_in_python"])
            ));
            let m = &a["alternative_measured"];
            if !m.is_null() {
                o.push_str(&format!(
                    "\nThe alternative, measured by the tool in the same run: {} `{}`: fastest run {:.4} s against {:.3} s, **{} times faster**, {} of the original's time.\n",
                    s(&d["tried"]["description"]),
                    s(&d["tried"]["function"]),
                    m["measured"]["wall_seconds"]["fastest"].as_f64().unwrap_or(0.0),
                    t["wall_seconds"]["fastest"].as_f64().unwrap_or(0.0),
                    m["times_faster"],
                    pct(&m["time_as_a_fraction_of_the_original"]),
                ));
            }
        }
    }
    o.push_str("\n## The decision\n\n");
    match r["declared_decision"].as_object() {
        None => o.push_str("None declared.\n"),
        Some(d) => o.push_str(&format!(
            "Declared by the author: `{}`{}. {}\n",
            s(&d["choice"]),
            if d["goals"].as_array().is_some_and(|g| !g.is_empty()) {
                format!(", for {}", d["goals"])
            } else {
                String::new()
            },
            s(&d["reason"])
        )),
    }
    let need = &r["need_for_speed"];
    if !need.is_null() {
        let d = &need["declared_by_the_author"];
        o.push_str(&format!(
            "\nThe need, declared by the author, not measured: {} ({}).\n\nBeside it, measured: the workload takes {:.3} s; if the candidate's own Python cost nothing at all it would take {}, saving {} per run{}{}. A real port saves less.\n",
            s(&d["statement"]),
            match (d["workload_must_take_at_most_seconds"].as_f64(), d["runs_per_day"].as_f64()) {
                (Some(x), _) => format!("the workload must take at most {x} s"),
                (_, Some(x)) => format!("the workload runs {x} times a day"),
                _ => "no number given".to_string(),
            },
            need["workload_seconds_now"].as_f64().unwrap_or(0.0),
            need["workload_seconds_if_the_candidate_cost_nothing"].as_f64().map_or("not known".to_string(), |x| format!("{x:.3} s")),
            need["seconds_saved_per_run_at_best"].as_f64().map_or("not known".to_string(), |x| format!("{x:.3} s")),
            need["seconds_saved_per_day_at_best"].as_f64().map_or(String::new(), |x| format!(", {x:.0} s a day")),
            need["holds_only_if"].as_str().map_or(String::new(), |t| format!(", if {t}")),
        ));
    }
    o.push_str("\n## What this report does not measure\n\n");
    for line in r["not_measured"].as_array().into_iter().flatten() {
        o.push_str(&format!("- {}\n", s(line)));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_cpu_than_wall_in_a_stand_in_is_worded_by_how_much_per_entry() {
        // One thread: nothing to say.
        assert_eq!(stand_in_threads_flag(0.094, 0.094, 1), None);
        // What idle threads cost after a product.
        let idle = stand_in_threads_flag(0.312, 0.106, 1).unwrap();
        assert!(
            idle.contains(
                "(0.312 s of CPU in 0.106 s, 0.206 s more, 0.206 s for the candidate's 1 entry)"
            ) && idle.contains("This alone does not show work inside the candidate")
                && !idle.contains("probably"),
            "{idle}"
        );
        // Far more than that in one entry.
        let work = stand_in_threads_flag(24.851, 3.592, 1).unwrap();
        assert!(
            work.contains("21.259 s more, 21.259 s for the candidate's 1 entry")
                && work.contains("more than idle threads were measured to cost (at most 0.6 s per entry): the candidate is probably still doing threaded work in the stand-in")
                && !work.contains("This alone does not show"),
            "{work}"
        );
        // The same surplus over many entries is what idle threads cost.
        let many = stand_in_threads_flag(24.851, 3.592, 40).unwrap();
        assert!(
            many.contains("0.531 s for each of the candidate's 40 entries")
                && !many.contains("probably"),
            "{many}"
        );
        // The line itself.
        assert!(
            !stand_in_threads_flag(2.0, 1.0, 1)
                .unwrap()
                .contains("probably")
        );
        assert!(
            stand_in_threads_flag(2.01, 1.0, 1)
                .unwrap()
                .contains("probably")
        );
    }

    #[test]
    fn a_stand_in_workload_is_flagged_by_the_share_of_time_it_took() {
        // Figures given, not measured: this flag is judged by time.
        assert_eq!(stand_in_flag(0.40), None);
        assert_eq!(stand_in_flag(0.94), None);
        let (kind, text) = stand_in_flag(0.97).unwrap();
        assert_eq!(kind, "stand_ins_remove_little");
        assert!(text.starts_with("the stand-ins removed 3.0% of the workload's time. Either the package does almost no work here, or the stand-ins do not replace it"));
        assert_eq!(stand_in_flag(1.02).unwrap().0, "stand_ins_remove_little");
        let (kind, text) = stand_in_flag(1.20).unwrap();
        assert_eq!(kind, "stand_ins_slower");
        assert!(text.contains("took 120% of the original's time"));
    }

    #[test]
    fn a_run_that_used_half_the_cpus_or_more_is_not_judged_from_beside() {
        // Figures given, not measured.
        let run = |cpu: f64, waited: f64| json!({"calibration_before": 0.01, "calibration_after": 0.02, "wall_seconds": 1.0, "cpu_seconds": cpu, "seconds_ready_and_not_running": waited});
        let cpus = json!(8);
        // One thread that waited for a CPU for 30% of the run: disturbed.
        assert!(disturbed(&run(0.7, 0.3), &cpus));
        // Seven CPUs' worth of threads: they waited for each other.
        assert!(!disturbed(&run(7.1, 0.3), &cpus));
        // The loops before and after still count for such a run.
        let mut loud = run(7.1, 0.0);
        loud["calibration_after"] = json!(0.4);
        assert!(disturbed(&loud, &cpus));

        let phase = |used: f64| json!({"watched_from_outside": {"seconds": 2.0, "kept_off_the_cpu": 0.3}, "cpus_used_by_the_measured_process": used, "logical_cpus": 8});
        let (mut busy, mut flags) = (Vec::new(), Flags::default());
        watched("the plain runs", &phase(3.9), &mut busy, &mut flags);
        assert_eq!(busy.len(), 1);
        assert!(flags.texts.is_empty());
        busy.clear();
        watched("the plain runs", &phase(4.0), &mut busy, &mut flags);
        watched(
            "the run under the profiler",
            &phase(7.1),
            &mut busy,
            &mut flags,
        );
        assert!(busy.is_empty());
        // Said once.
        assert_eq!(flags.texts.len(), 1);
        assert!(flags.texts[0].starts_with("the workload itself used 4.0 CPUs of 8, so load inside the run could not be watched (first during the plain runs)"));
        // No watching process, nothing to say.
        let mut flags = Flags::default();
        watched(
            "x",
            &json!({"watched_from_outside": null, "cpus_used_by_the_measured_process": 7.0, "logical_cpus": 8}),
            &mut busy,
            &mut flags,
        );
        assert!(busy.is_empty() && flags.texts.is_empty());
    }

    #[test]
    fn a_machine_with_too_few_cpus_to_watch_a_run_says_so() {
        // Under four CPUs no process is started beside a run.
        let phase = |cpus: u64| json!({"watched_from_outside": null, "cpus_used_by_the_measured_process": 1.0, "logical_cpus": cpus});
        let (mut busy, mut flags) = (Vec::new(), Flags::default());
        watched("the plain runs", &phase(3), &mut busy, &mut flags);
        watched(
            "the run under the profiler",
            &phase(3),
            &mut busy,
            &mut flags,
        );
        assert!(busy.is_empty());
        // Said once.
        assert_eq!(flags.texts.len(), 1, "{:?}", flags.texts);
        assert!(flags.has("load_inside_the_run_not_watched_few_cpus"));
        assert!(
            flags.texts[0].starts_with("this machine has 3 CPUs, and with fewer than 4 no process is run beside the workload, so load inside a run was not watched"),
            "{}",
            flags.texts[0]
        );
        let on_linux = "the time the process spent ready and not running is still used";
        assert_eq!(
            flags.texts[0].contains(on_linux),
            cfg!(target_os = "linux"),
            "{}",
            flags.texts[0]
        );
        // With four it is started, and there is nothing to say.
        let mut flags = Flags::default();
        watched("the plain runs", &phase(4), &mut busy, &mut flags);
        assert!(flags.texts.is_empty());
    }
}
