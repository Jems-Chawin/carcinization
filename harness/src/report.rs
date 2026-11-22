//! The report. Language-neutral.
//!
//! Every field is computed here from what the harness did or measured. The
//! verdict is derived from the case results at the moment of writing and
//! from nothing else.

use crate::Options;
use crate::compare::{Applied, Difference};
use crate::provenance::{Git, digest_of, sha256_bytes, sha256_file, tool_version, tracked_digest};
use crate::python::Built;
use crate::side::Side;
use crate::suite::{Case, RelaxDecl};
use serde::Serialize;
use serde_json::Value as J;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPORT_SCHEMA: &str = "carcinify/diff/report/1";
pub const MAX_APPLIED_LISTED: usize = 50;

#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    /// `pass`, `pass_with_relaxations` or `fail`. (`error` is written when
    /// the run could not happen.)
    pub verdict: Verdict,
    /// What the run comes to once `--strict` is taken into account: `pass`
    /// or `fail`. The exit status is 0 for `pass` and 1 for `fail`, and is
    /// given beside it, so a tool reading only this file can tell.
    pub outcome: FinalOutcome,
    pub exit_status: u8,
    pub reasons: Vec<String>,
    /// Whether `--strict` was given, which turns `pass_with_relaxations`
    /// into a failing exit status.
    pub strict: bool,
    pub hash_seed: HashSeed,
    pub summary: Summary,
    /// Each file of measured calls the suite lists, and which of its calls
    /// are not among the cases.
    pub measured: Vec<crate::measured::FileReport>,
    /// Files named `*.calls.jsonl` in the repository that the suite does
    /// not list under `measured`: a recording that may have been forgotten.
    pub calls_files_not_measured: Vec<String>,
    pub checks: Checks,
    pub git: GitReport,
    pub suite: FileHash,
    pub inputs: Vec<FileHash>,
    pub sides: Sides,
    pub tools: Tools,
    pub started_unix: u64,
    pub duration_ms: u128,
    pub cases: Vec<CaseReport>,
}

/// States that decide the verdict are enums, so that a misspelt one is a
/// compile error and every `match` on them is exhaustive. Their JSON is the
/// snake_case name.
macro_rules! states {
    ($(#[$doc:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
        }
    };
}

states! {
    /// How one case came out.
    CaseResult {
        Identical => "identical",
        IdenticalWithRelaxations => "identical_with_relaxations",
        Different => "different",
        Error => "error",
    }
}

states! {
    /// What the comparison found, before `--strict` is applied.
    Verdict {
        Pass => "pass",
        PassWithRelaxations => "pass_with_relaxations",
        Fail => "fail",
    }
}

states! {
    /// What the run comes to: the exit status in words.
    FinalOutcome {
        Pass => "pass",
        Fail => "fail",
    }
}

states! {
    /// Why a case could not be run on a side.
    ErrorKind {
        Crashed => "crashed",
        Timeout => "timeout",
        Raised => "raised",
        DidNotRaise => "did_not_raise",
        Unencodable => "unencodable",
        NoSuchFunction => "no_such_function",
        RefusedFunction => "refused_function",
        BadCall => "bad_call",
        Input => "input",
        Protocol => "protocol",
    }
}

impl Verdict {
    /// A pass that needed relaxations is a failure under `--strict`.
    pub fn outcome(self, strict: bool) -> FinalOutcome {
        match (self, strict) {
            (Verdict::Pass, _) | (Verdict::PassWithRelaxations, false) => FinalOutcome::Pass,
            (Verdict::PassWithRelaxations, true) | (Verdict::Fail, _) => FinalOutcome::Fail,
        }
    }
}

impl FinalOutcome {
    pub fn exit_status(self) -> u8 {
        match self {
            FinalOutcome::Pass => 0,
            FinalOutcome::Fail => 1,
        }
    }
}

#[derive(Serialize, Default)]
pub struct Summary {
    pub cases: usize,
    pub identical: usize,
    pub identical_with_relaxations: usize,
    pub different: usize,
    pub errors: usize,
}

/// The checks that stand between "the outputs matched" and "the port was
/// compared with the original".
#[derive(Serialize)]
pub struct Checks {
    pub sides_are_different_code: DistinctCheck,
    /// One entry per function the cases call.
    pub functions: Vec<FunctionCheck>,
    /// Each module's own public functions that no case called. Nothing in
    /// this report says anything about them.
    pub public_functions_not_called: NotCalled,
    /// Directories the suite declares as shared, and the files each side
    /// loaded from them. This code ran on both sides and was not compared.
    pub shared: Vec<SharedDir>,
    /// Processes each side's Python code started, seen by the audit hook.
    /// Recorded, not judged: the harness does not know what they ran.
    pub processes_started: Processes,
}

#[derive(Serialize)]
pub struct SharedDir {
    /// The shared file or directory.
    pub path: String,
    /// `original` or `candidate` when the entry is inside that side's
    /// source directory; absent when it stands on its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inside: Option<&'static str>,
    pub files_loaded: Vec<SharedFile>,
}

/// Python function calls made while a function under test ran, summed over
/// its cases, by where the called code lives.
#[derive(Serialize, Default, Clone, Copy)]
pub struct PythonCalls {
    /// In the side's own files (for a PyO3 side: Python files in the wheel).
    pub own: u64,
    /// In files the suite declares as shared.
    pub shared: u64,
    /// In the standard library and installed packages.
    pub other: u64,
}

#[derive(Serialize)]
pub struct SharedFile {
    pub path: String,
    pub sha256: String,
    /// `both`, `original` or `candidate`. A side that loaded a
    /// byte-identical copy of the file counts as having loaded it.
    pub loaded_by: &'static str,
    /// Copies of this file, elsewhere, that a side loaded in its place.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub copies: Vec<String>,
}

#[derive(Serialize)]
pub struct Processes {
    pub original: ProcessList,
    pub candidate: ProcessList,
}

#[derive(Serialize)]
pub struct ProcessList {
    pub count: u64,
    /// The first twenty, as the audit event and its first arguments.
    pub first: Vec<String>,
}

#[derive(Serialize)]
pub struct NotCalled {
    pub original: Vec<String>,
    pub candidate: Vec<String>,
}

#[derive(Serialize)]
pub struct DistinctCheck {
    /// Always `pass` here: a failure stops the run and gives an `error` report.
    pub result: &'static str,
    pub rules: [&'static str; 6],
}

#[derive(Serialize)]
pub struct FunctionCheck {
    #[serde(rename = "fn")]
    pub func: String,
    /// `pass` when at least one case made both sides return a value.
    /// Cases where both sides raise show that they reject the same inputs,
    /// not that the port does the work.
    pub compared_on_a_returned_value: &'static str,
    pub cases: usize,
    pub both_returned: usize,
    pub both_raised: usize,
    pub one_returned_one_raised: usize,
    pub failed_to_run: usize,
    pub identical: usize,
    pub identical_with_relaxations: usize,
    pub different: usize,
    /// `python` or `extension` (compiled): what the function is on each
    /// side. A `python` function in a wheel may still call compiled code;
    /// `extension` means the function itself is compiled.
    pub original_implementation: String,
    pub candidate_implementation: String,
    /// Python calls made while the function ran. `extension` with a count
    /// above zero means compiled code handed work to Python; counts under
    /// `shared` on both sides mean both ran the same code for that part.
    /// Absent when counting was switched off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_python_calls: Option<PythonCalls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_python_calls: Option<PythonCalls>,
    /// Cases in which the count could not be taken (the code under test
    /// replaced the profile function). Their calls are in neither total.
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub original_python_calls_uncounted_cases: u64,
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub candidate_python_calls_uncounted_cases: u64,
    /// The above in words, as printed.
    pub original_work: String,
    pub candidate_work: String,
    /// The file that defines the function on each side.
    pub original_defined_in: Vec<FileHash>,
    pub candidate_defined_in: Vec<FileHash>,
}

#[derive(Serialize)]
pub struct GitReport {
    pub commit: String,
    /// True when the run was allowed although the tree did not match the
    /// commit; the result then describes the tree, not the commit.
    pub dirty: bool,
    /// `git status --porcelain` lines.
    pub uncommitted: Vec<String>,
    /// Files the run imported, compiled or packaged that git does not
    /// track (ignored files included, which `git status` does not show).
    pub used_but_not_tracked: Vec<crate::commit::NotInCommit>,
    /// Each submodule and the commit it is at. The superproject's commit
    /// pins these, and they are given so a reader need not look them up.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub submodules: Vec<Submodule>,
}

#[derive(Serialize)]
pub struct Submodule {
    pub path: String,
    pub commit: String,
}

#[derive(Serialize)]
pub struct FileHash {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    pub sha256: String,
}

#[derive(Serialize)]
pub struct Sides {
    pub original: SideReport,
    pub candidate: SideReport,
}

#[derive(Serialize)]
pub struct SideReport {
    #[serde(flatten)]
    pub built: J,
    pub module_file: String,
    pub runtime: J,
    /// One digest over every file listed or counted below: the code that ran.
    pub implementation_sha256: String,
    /// Loaded files inside the repository or the freshly built wheel.
    pub files: Vec<FileHash>,
    /// How many loaded files are third-party: installed packages
    /// (site-packages, wherever the virtualenv is) and anything else outside
    /// the repository.
    pub external_files: usize,
    /// Those files, each with its SHA-256. They are part of
    /// `implementation_sha256` and are not required to be in the commit.
    pub third_party_files: Vec<FileHash>,
    /// PyO3 side: the tracked files of the crate's Cargo workspace
    /// (sources and Cargo.lock) as they were on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_source: Option<CrateSource>,
    /// How many times the process had to be started again after dying.
    pub restarts: u32,
}

#[derive(Serialize)]
pub struct CrateSource {
    pub dir: String,
    pub tracked_files: usize,
    pub sha256: String,
}

/// `PYTHONHASHSEED` for both sides: given with `--hash-seed`, or drawn by the
/// harness. Either way the report has the number, and `--hash-seed` with it
/// repeats the run.
#[derive(Serialize, Clone, Copy, Debug)]
pub struct HashSeed {
    pub value: u32,
    /// `given` or `drawn`.
    pub chosen: &'static str,
}

#[derive(Serialize)]
pub struct Tools {
    pub harness_version: &'static str,
    pub harness_sha256: String,
    pub driver_sha256: String,
    pub rustc: String,
    pub cargo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maturin: Option<String>,
    /// Cargo and rustup configuration outside the repository that applied
    /// to the build. No commit pins these.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cargo_config_outside_repository: Vec<FileHash>,
    /// Environment variables that change what Cargo builds, as they were.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub build_environment: BTreeMap<String, String>,
    pub os: &'static str,
    pub arch: &'static str,
}

#[derive(Serialize)]
pub struct CaseError {
    pub side: &'static str,
    /// `crashed`, `timeout`, `raised`, `did_not_raise`, `unencodable`,
    /// `no_such_function`, `input` or `protocol`.
    pub kind: ErrorKind,
    pub detail: String,
}

#[derive(Serialize)]
pub struct CaseReport {
    pub id: String,
    pub source: String,
    #[serde(rename = "fn")]
    pub func: String,
    /// `identical`, `identical_with_relaxations`, `different` or `error`.
    pub result: CaseResult,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub expect_raise: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relaxations_declared: Option<RelaxDecl>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relaxations_applied: Vec<Applied>,
    #[serde(skip_serializing_if = "is_zero")]
    pub relaxations_applied_total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_difference: Option<Difference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<CaseError>,
    /// What the comparison did by rule that a reader should know.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// `returned` or `raised`, when the side ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_outcome: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_outcome: Option<&'static str>,
    /// SHA-256 of each side's whole outcome in canonical form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_sha256: Option<String>,
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

fn uncounted(side: &Side, func: &str) -> u64 {
    side.python_calls_unavailable
        .get(func)
        .map_or(0, |(cases, _)| *cases)
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl CaseReport {
    /// Starts as `error`; only a completed comparison changes that.
    pub fn new(case: &Case) -> CaseReport {
        CaseReport {
            id: case.id.clone(),
            source: case.source.clone(),
            func: case.func.clone(),
            result: CaseResult::Error,
            expect_raise: case.expect_raise,
            relaxations_declared: (case.relax_decl != RelaxDecl::default())
                .then(|| case.relax_decl.clone()),
            relaxations_applied: Vec::new(),
            relaxations_applied_total: 0,
            first_difference: None,
            errors: Vec::new(),
            notes: Vec::new(),
            original_outcome: None,
            candidate_outcome: None,
            original_sha256: None,
            candidate_sha256: None,
        }
    }
}

pub struct Parts<'a> {
    pub opts: &'a Options,
    pub suite_path: &'a Path,
    pub git: &'a Git,
    pub inputs: &'a BTreeMap<PathBuf, (&'static str, String)>,
    pub original: (&'a Side, &'a Built),
    pub candidate: (&'a Side, &'a Built),
    pub cases: Vec<CaseReport>,
    pub measured: Vec<crate::measured::FileReport>,
    pub calls_files_not_measured: Vec<String>,
    pub shared: &'a [PathBuf],
    pub not_in_commit: Vec<crate::commit::NotInCommit>,
    pub cargo_config_outside_repository: Vec<FileHash>,
    pub duration: Duration,
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// A loaded file's name in the report: relative to the repository, or
/// `wheel:` plus its path in the wheel just built. `None` for anything else.
fn label(git: &Git, side: &Side, built: &Built, path: &Path) -> Option<String> {
    let wheel_root = built.wheel.as_ref().and_then(|_| built.load_roots.first());
    if let Some(inside) = wheel_root.and_then(|w| path.strip_prefix(w).ok()) {
        Some(format!("wheel:{}", inside.display()))
    } else if side.is_installed_package(path) {
        None
    } else if path.starts_with(&git.root) {
        Some(rel(&git.root, path))
    } else {
        None
    }
}

fn python_calls(side: &Side, func: &str) -> Option<PythonCalls> {
    let [own, shared, other] = *side.python_calls.get(func)?;
    Some(PythonCalls { own, shared, other })
}

/// One line saying what a function is on a side and where its work went,
/// for example `extension, 4 Python calls (4 in wheel)`.
fn work((side, built): (&Side, &Built), func: &str) -> String {
    let kind = side
        .implementation
        .get(func)
        .map_or("unknown", String::as_str);
    // Cases whose count could not be taken are said so, never left out
    // silently: a count that misses calls would read as a full one.
    let missing = side.python_calls_unavailable.get(func);
    let note = missing.map_or(String::new(), |(cases, why)| {
        format!("; not counted in {cases} case(s): {why}")
    });
    let Some(calls) = python_calls(side, func) else {
        return match missing {
            Some(_) => format!("{kind}, Python calls unknown{note}"),
            None => format!("{kind}, Python calls not counted"),
        };
    };
    let total = calls.own + calls.shared + calls.other;
    if total == 0 {
        return format!("{kind}, 0 Python calls{note}");
    }
    let own = if built.wheel.is_some() {
        "in wheel"
    } else {
        "own"
    };
    let parts: Vec<String> = [
        (calls.own, own),
        (calls.shared, "shared"),
        (calls.other, "in other packages"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, place)| format!("{n} {place}"))
    .collect();
    format!(
        "{kind}, {total} Python call{} ({}){note}",
        if total == 1 { "" } else { "s" },
        parts.join(", ")
    )
}

fn function_checks(p: &Parts) -> Result<Vec<FunctionCheck>, String> {
    let defined = |(side, built): (&Side, &Built), func: &str| -> Result<Vec<FileHash>, String> {
        let mut out = Vec::new();
        for f in side.defined_in.get(func).into_iter().flatten() {
            let path = Path::new(f);
            out.push(FileHash {
                path: label(p.git, side, built, path).unwrap_or_else(|| f.clone()),
                kind: None,
                sha256: sha256_file(path)?,
            });
        }
        Ok(out)
    };
    let implementation = |side: &Side, func: &str| {
        side.implementation
            .get(func)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    };
    let mut checks: Vec<FunctionCheck> = Vec::new();
    for c in &p.cases {
        let at = match checks.iter().position(|f| f.func == c.func) {
            Some(at) => at,
            None => {
                checks.push(FunctionCheck {
                    func: c.func.clone(),
                    compared_on_a_returned_value: "fail",
                    cases: 0,
                    both_returned: 0,
                    both_raised: 0,
                    one_returned_one_raised: 0,
                    failed_to_run: 0,
                    identical: 0,
                    identical_with_relaxations: 0,
                    different: 0,
                    original_implementation: implementation(p.original.0, &c.func),
                    candidate_implementation: implementation(p.candidate.0, &c.func),
                    original_python_calls: python_calls(p.original.0, &c.func),
                    candidate_python_calls: python_calls(p.candidate.0, &c.func),
                    original_python_calls_uncounted_cases: uncounted(p.original.0, &c.func),
                    candidate_python_calls_uncounted_cases: uncounted(p.candidate.0, &c.func),
                    original_work: work(p.original, &c.func),
                    candidate_work: work(p.candidate, &c.func),
                    original_defined_in: defined(p.original, &c.func)?,
                    candidate_defined_in: defined(p.candidate, &c.func)?,
                });
                checks.len() - 1
            }
        };
        let f = &mut checks[at];
        f.cases += 1;
        match c.result {
            CaseResult::Identical => f.identical += 1,
            CaseResult::IdenticalWithRelaxations => f.identical_with_relaxations += 1,
            CaseResult::Different => f.different += 1,
            CaseResult::Error => f.failed_to_run += 1,
        }
        if c.result != CaseResult::Error {
            match (c.original_outcome, c.candidate_outcome) {
                (Some("returned"), Some("returned")) => {
                    f.both_returned += 1;
                    f.compared_on_a_returned_value = "pass";
                }
                (Some("raised"), Some("raised")) => f.both_raised += 1,
                _ => f.one_returned_one_raised += 1,
            }
        }
    }
    Ok(checks)
}

fn side_report(git: &Git, side: &Side, built: &Built) -> Result<SideReport, String> {
    let wheel_root = built.wheel.as_ref().and_then(|_| built.load_roots.first());
    let mut files = Vec::new();
    let mut all = Vec::new();
    let mut external = 0;
    let mut third_party = Vec::new();
    for f in &side.files {
        let path = Path::new(f);
        let hash = sha256_file(path)?;
        let label = label(git, side, built, path);
        all.push((label.clone().unwrap_or_else(|| f.clone()), hash.clone()));
        match label {
            Some(path) => files.push(FileHash {
                path,
                kind: None,
                sha256: hash,
            }),
            None => {
                external += 1;
                let dir = side.package_dirs.iter().find(|d| path.starts_with(d));
                third_party.push(FileHash {
                    path: match dir.and_then(|d| path.strip_prefix(d).ok()) {
                        Some(inside) => format!("site-packages:{}", inside.display()),
                        None => f.clone(),
                    },
                    // `sitecustomize` and `usercustomize` are imported by
                    // the interpreter itself at start-up, before any code
                    // of the project. Debian and Ubuntu ship one. It runs
                    // in the measured process, so it is listed; it is not
                    // a package the project chose.
                    kind: matches!(
                        path.file_name().and_then(|n| n.to_str()),
                        Some("sitecustomize.py" | "usercustomize.py")
                    )
                    .then_some("interpreter_start_up"),
                    sha256: hash,
                });
            }
        }
    }
    let crate_source = match &built.workspace_dir {
        Some(dir) => {
            let (sha256, tracked_files) = tracked_digest(&git.root, dir)?;
            Some(CrateSource {
                dir: rel(&git.root, dir),
                tracked_files,
                sha256,
            })
        }
        None => None,
    };
    let mut built_json = serde_json::to_value(built).map_err(|e| e.to_string())?;
    if let (Some(m), false) = (built_json.as_object_mut(), built.source_dirs.is_empty()) {
        let dirs: Vec<String> = built
            .source_dirs
            .iter()
            .map(|d| rel(&git.root, d))
            .collect();
        m.insert("path".to_string(), dirs.into());
    }
    Ok(SideReport {
        built: built_json,
        module_file: side
            .hello
            .get("module_file")
            .and_then(J::as_str)
            .map(
                |f| match wheel_root.and_then(|w| Path::new(f).strip_prefix(w).ok()) {
                    Some(inside) => format!("wheel:{}", inside.display()),
                    None => rel(&git.root, Path::new(f)),
                },
            )
            .unwrap_or_default(),
        runtime: side.hello.get("python").cloned().unwrap_or(J::Null),
        implementation_sha256: digest_of(all),
        files,
        external_files: external,
        third_party_files: third_party,
        crate_source,
        restarts: side.restarts,
    })
}

fn shared_dirs(p: &Parts) -> Result<Vec<SharedDir>, String> {
    let mut entries = Vec::new();
    for entry in p.shared {
        // Shared is by content as well as by path: a side that loaded a
        // byte-identical copy of a shared file has loaded that file.
        let by_content = crate::distinct::contents(std::slice::from_ref(entry))?;
        // shared file -> (loaded by original, by candidate, copies)
        let mut loaded: BTreeMap<PathBuf, (bool, bool, Vec<String>)> = BTreeMap::new();
        for (is_original, (side, built)) in [(true, p.original), (false, p.candidate)] {
            for file in &side.files {
                let path = Path::new(file);
                let shared_file = if path.starts_with(entry) {
                    path.to_path_buf()
                } else if !by_content.is_empty() && path.metadata().is_ok_and(|m| m.len() > 0) {
                    match by_content.get(&sha256_file(path)?) {
                        Some(twin) => twin.clone(),
                        None => continue,
                    }
                } else {
                    continue;
                };
                let slot = loaded.entry(shared_file.clone()).or_default();
                if is_original {
                    slot.0 = true;
                } else {
                    slot.1 = true;
                }
                if shared_file != path {
                    slot.2
                        .push(label(p.git, side, built, path).unwrap_or_else(|| file.clone()));
                }
            }
        }
        let mut files_loaded = Vec::new();
        for (file, (by_original, by_candidate, copies)) in loaded {
            files_loaded.push(SharedFile {
                path: rel(&p.git.root, &file),
                sha256: sha256_file(&file)?,
                loaded_by: match (by_original, by_candidate) {
                    (true, true) => "both",
                    (true, false) => "original",
                    _ => "candidate",
                },
                copies,
            });
        }
        let inside = |built: &Built| built.load_roots.iter().any(|r| entry.starts_with(r));
        entries.push(SharedDir {
            path: rel(&p.git.root, entry),
            inside: if inside(p.original.1) {
                Some("original")
            } else if inside(p.candidate.1) {
                Some("candidate")
            } else {
                None
            },
            files_loaded,
        });
    }
    Ok(entries)
}

fn not_called(side: &Side) -> Vec<String> {
    let all = side.public_functions.iter();
    all.filter(|f| !side.defined_in.contains_key(*f))
        .cloned()
        .collect()
}

pub fn build(mut p: Parts) -> Result<Report, String> {
    let mut summary = Summary {
        cases: p.cases.len(),
        ..Summary::default()
    };
    for c in &p.cases {
        match c.result {
            CaseResult::Identical => summary.identical += 1,
            CaseResult::IdenticalWithRelaxations => summary.identical_with_relaxations += 1,
            CaseResult::Different => summary.different += 1,
            CaseResult::Error => summary.errors += 1,
        }
    }
    let mut reasons = Vec::new();
    if summary.cases == 0 {
        reasons.push("no cases were run".to_string());
    }
    if summary.different > 0 {
        reasons.push(format!(
            "{} case(s) gave different outputs",
            summary.different
        ));
    }
    if summary.errors > 0 {
        reasons.push(format!("{} case(s) failed to run", summary.errors));
    }
    let listed = |items: Vec<String>| {
        let more = items.len().saturating_sub(10);
        let mut text = items.into_iter().take(10).collect::<Vec<_>>().join(", ");
        if more > 0 {
            text += &format!(" and {more} more");
        }
        text
    };
    let missing: Vec<String> = p
        .measured
        .iter()
        .flat_map(|m| {
            m.missing
                .iter()
                .map(|x| format!("{} ({})", x.source, x.func))
        })
        .collect();
    if !missing.is_empty() {
        reasons.push(format!(
            "measured_input_not_compared: {} measured call(s) are not among the cases: {}",
            missing.len(),
            listed(missing)
        ));
    }
    let unwaived: Vec<String> = p
        .measured
        .iter()
        .filter(|m| m.not_comparable.is_none())
        .flat_map(|m| {
            m.unencodable
                .iter()
                .map(|u| format!("{} ({} holds a {})", u.source, u.func, u.holds))
        })
        .collect();
    if !unwaived.is_empty() {
        reasons.push(format!(
            "measured_input_not_encodable: {} measured call(s) hold a value the value format does not cover, and the suite gives no \"not_comparable\" reason: {}",
            unwaived.len(),
            listed(unwaived)
        ));
    }
    let functions = function_checks(&p)?;
    for f in functions
        .iter()
        .filter(|f| f.compared_on_a_returned_value == "fail")
    {
        reasons.push(format!(
            "function {:?} was never compared on a returned value: in each of its {} case(s) a side raised or failed to run",
            f.func, f.cases
        ));
    }
    let crate_dir = p
        .candidate
        .1
        .crate_dir
        .as_ref()
        .or(p.original.1.crate_dir.as_ref());
    let tool_dir = crate_dir.map_or(
        p.suite_path.parent().unwrap_or(Path::new("/")),
        PathBuf::as_path,
    );
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let verdict = match (reasons.is_empty(), summary.identical_with_relaxations) {
        (false, _) => Verdict::Fail,
        (true, 0) => Verdict::Pass,
        (true, _) => Verdict::PassWithRelaxations,
    };
    Ok(Report {
        schema: REPORT_SCHEMA,
        verdict,
        outcome: verdict.outcome(p.opts.strict),
        exit_status: verdict.outcome(p.opts.strict).exit_status(),
        reasons,
        strict: p.opts.strict,
        hash_seed: p.opts.hash_seed,
        measured: std::mem::take(&mut p.measured),
        calls_files_not_measured: std::mem::take(&mut p.calls_files_not_measured),
        summary,
        checks: Checks {
            sides_are_different_code: DistinctCheck {
                result: "pass",
                rules: crate::distinct::RULES,
            },
            functions,
            public_functions_not_called: NotCalled {
                original: not_called(p.original.0),
                candidate: not_called(p.candidate.0),
            },
            shared: shared_dirs(&p)?,
            processes_started: Processes {
                original: ProcessList {
                    count: p.original.0.process_count,
                    first: p.original.0.processes.clone(),
                },
                candidate: ProcessList {
                    count: p.candidate.0.process_count,
                    first: p.candidate.0.processes.clone(),
                },
            },
        },
        git: GitReport {
            commit: p.git.commit.clone(),
            dirty: !p.git.dirty.is_empty() || !p.not_in_commit.is_empty(),
            uncommitted: p.git.dirty.clone(),
            used_but_not_tracked: p.not_in_commit,
            submodules: p
                .git
                .submodules
                .iter()
                .map(|(path, commit)| Submodule {
                    path: path.clone(),
                    commit: commit.clone(),
                })
                .collect(),
        },
        suite: FileHash {
            path: rel(&p.git.root, p.suite_path),
            kind: None,
            sha256: sha256_file(p.suite_path)?,
        },
        inputs: p
            .inputs
            .iter()
            .map(|(path, (kind, sha256))| FileHash {
                path: rel(&p.git.root, path),
                kind: Some(kind),
                sha256: sha256.clone(),
            })
            .collect(),
        sides: Sides {
            original: side_report(p.git, p.original.0, p.original.1)?,
            candidate: side_report(p.git, p.candidate.0, p.candidate.1)?,
        },
        tools: Tools {
            harness_version: env!("CARGO_PKG_VERSION"),
            harness_sha256: sha256_file(&exe)?,
            driver_sha256: sha256_bytes(crate::python::DRIVER.as_bytes()),
            rustc: tool_version(Path::new("rustc"), &["--version"], tool_dir),
            cargo: tool_version(Path::new("cargo"), &["--version"], tool_dir),
            maturin: crate_dir.map(|_| tool_version(&p.opts.maturin, &["--version"], tool_dir)),
            cargo_config_outside_repository: p.cargo_config_outside_repository,
            build_environment: if crate_dir.is_some() {
                crate::commit::build_environment()
            } else {
                BTreeMap::new()
            },
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        },
        started_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
            .saturating_sub(p.duration.as_secs()),
        duration_ms: p.duration.as_millis(),
        cases: p.cases,
    })
}

/// Writes `value` as the report, replacing whatever was there.
pub fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("cannot write report {}: {e}", path.display()))
}
