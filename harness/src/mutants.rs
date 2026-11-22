//! `carcinify diff --mutants <dir>`: how strong the cases are, measured with
//! kept patches. Each `*.patch` in the directory is a deliberate bug in the
//! port. Every patch is applied on its own to a scratch clone of the commit,
//! never to the author's tree; the candidate is built and every case run,
//! and the report says which patches the cases caught (killed) and which
//! they did not (survived).
//!
//! Text before a patch's first `diff --git` line is its description, which
//! `git apply` ignores. A line there reading `Expected to survive: <reason>`
//! marks a patch that cannot change an answer (an equivalent mutant): its
//! survival is expected, and its being killed is reported.

use crate::report::{CaseReport, CaseResult, ErrorKind, HashSeed, Verdict};
use crate::suite::{SideSpec, Suite};
use crate::{Options, provenance};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub const SCHEMA: &str = "carcinify/diff/mutants/1";

#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    /// `pass` when every patch was killed, except those expected to survive,
    /// which survived; `fail` otherwise.
    pub verdict: &'static str,
    pub exit_status: u8,
    pub reasons: Vec<String>,
    /// The seed every run used: `--hash-seed` with it repeats the run.
    pub hash_seed: HashSeed,
    pub commit: String,
    pub suite: String,
    /// The unmutated suite in the scratch clone, which must pass first.
    pub base: Base,
    /// The scratch clone; removed when the run ends.
    pub scratch: String,
    pub patches: Vec<Patch>,
}

#[derive(Serialize)]
pub struct Base {
    pub cases: usize,
    pub identical: usize,
}

#[derive(Serialize, PartialEq, Debug)]
pub struct Patch {
    pub file: String,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_to_survive: Option<String>,
    /// `killed`, `survived`, or `did_not_run` (it did not build or import).
    pub result: &'static str,
    /// How the cases caught a killed patch: `different`, `panicked`,
    /// `crashed`, `timed out` or `failed` (another error on a side).
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub how: BTreeSet<&'static str>,
    pub cases_run: usize,
    pub not_identical: usize,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub not_identical_by_case_file: BTreeMap<String, usize>,
    /// The first few cases that caught it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub first_cases: Vec<String>,
    /// The run stopped at the first case that timed out.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stopped_after_timeout: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Removes the scratch clone, and the target directory inside it, however
/// the run ends short of the process being killed.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PREFIX: &str = "carcinify-mutants-";

/// Removes scratch clones left by runs that were stopped from outside (a
/// killed process cleans nothing up): those whose process is gone.
fn sweep_leftovers() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(pid) = name
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.split('-').next())
        else {
            continue;
        };
        let alive = Command::new("kill")
            .args(["-0", pid])
            .output()
            .map(|o| o.status.success());
        // Removed only when `kill -0` ran and said the process is gone.
        if matches!(alive, Ok(false)) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The directories a patch may change: the candidate's own code. For a PyO3
/// side that is its Cargo workspace (the crate and the crates beside it the
/// build pulls in), for a Python side its `path` directories.
fn candidate_dirs(suite: &Suite, suite_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = |rel: &str| {
        let p = suite_dir.join(rel);
        p.canonicalize()
            .map_err(|e| format!("candidate: {}: {e}", p.display()))
    };
    match &suite.candidate {
        SideSpec::Python { path, .. } => path.iter().map(|p| dir(p)).collect(),
        SideSpec::Pyo3 { crate_dir, .. } => {
            let workspace = crate::python::cargo_workspace("candidate", &dir(crate_dir)?)?;
            Ok(vec![workspace.canonicalize().map_err(|e| e.to_string())?])
        }
    }
}

/// What the comparison reads besides the candidate: the suite, the case and
/// data files, the original and the shared directories. A patch may not
/// change any of them, even where the candidate's directories hold them (a
/// Cargo workspace can be the whole repository).
fn read_by_comparison(
    suite: &Suite,
    suite_path: &Path,
    suite_dir: &Path,
) -> Result<Vec<(PathBuf, &'static str)>, String> {
    let dir = |rel: &String| suite_dir.join(rel).canonicalize().ok();
    let mut read = vec![(suite_path.to_path_buf(), "suite")];
    for (path, (role, _)) in crate::suite::load_cases(suite_dir, &suite.cases)?.inputs {
        read.push((
            path,
            if role == "cases" {
                "case file"
            } else {
                "data file"
            },
        ));
    }
    let original: Vec<PathBuf> = match &suite.original {
        SideSpec::Python { path, .. } => path.iter().filter_map(dir).collect(),
        SideSpec::Pyo3 { crate_dir, .. } => dir(crate_dir).into_iter().collect(),
    };
    read.extend(original.into_iter().map(|d| (d, "original")));
    read.extend(suite.shared.iter().filter_map(dir).map(|d| (d, "shared")));
    read.extend(
        suite
            .measured
            .iter()
            .filter_map(|m| dir(&m.file))
            .map(|f| (f, "measured file")),
    );
    Ok(read)
}

/// The paths a patch touches, relative to the repository, as git reads it.
fn touched(root: &Path, patch: &Path) -> Result<Vec<String>, String> {
    let patch = patch.to_str().ok_or("patch path is not UTF-8")?;
    let out = git(root, &["apply", "--numstat", "-z", patch])?;
    let mut paths = Vec::new();
    let mut fields = out.split('\0');
    while let Some(field) = fields.next() {
        let path = field.splitn(3, '\t').nth(2).unwrap_or("");
        if field.is_empty() {
            continue;
        } else if path.is_empty() {
            // A rename or copy: the old and the new path follow.
            paths.extend(fields.next().map(str::to_string));
            paths.extend(fields.next().map(str::to_string));
        } else {
            paths.push(path.to_string());
        }
    }
    Ok(paths)
}

fn expected_to_survive(text: &str) -> Option<String> {
    text.lines()
        .take_while(|l| !l.starts_with("diff --git "))
        .find_map(|l| l.strip_prefix("Expected to survive:"))
        .map(|reason| reason.trim().to_string())
}

/// How one case that was not identical caught the mutant.
fn how(case: &CaseReport) -> &'static str {
    let kinds: Vec<ErrorKind> = case.errors.iter().map(|e| e.kind).collect();
    let panicked = case.first_difference.as_ref().is_some_and(|d| {
        d.candidate
            .as_ref()
            .is_some_and(|s| s.repr.contains("PanicException") || s.ty.contains("PanicException"))
    }) || case
        .errors
        .iter()
        .any(|e| e.detail.contains("PanicException"));
    if kinds.contains(&ErrorKind::Timeout) {
        "timed out"
    } else if kinds.contains(&ErrorKind::Crashed) {
        "crashed"
    } else if panicked {
        "panicked"
    } else if case.result == CaseResult::Different {
        "different"
    } else {
        "failed"
    }
}

pub fn run(opts: &Options, dir: &Path) -> Result<Report, String> {
    if opts.allow_dirty {
        return Err(
            "--mutants applies each patch to the commit, so it cannot run with --allow-dirty"
                .to_string(),
        );
    }
    let suite_path = opts
        .suite
        .canonicalize()
        .map_err(|e| format!("suite {}: {e}", opts.suite.display()))?;
    let suite_dir = suite_path.parent().ok_or("suite has no directory")?;
    let suite = Suite::load(&suite_path)?;
    let git_info = provenance::git_info(suite_dir)?;
    let root = &git_info.root;
    let short = &git_info.commit[..12.min(git_info.commit.len())];
    if !git_info.dirty.is_empty() {
        return Err(format!(
            "the working tree has uncommitted changes, and the patches are applied to commit {short}:\n  {}\nCommit them first.",
            git_info.dirty.join("\n  ")
        ));
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("--mutants {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "patch"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("--mutants {}: no .patch file in it", dir.display()));
    }
    let allowed = candidate_dirs(&suite, suite_dir)?;
    let read = read_by_comparison(&suite, &suite_path, suite_dir)?;
    let name = |p: &Path| {
        p.file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned())
    };

    // Every patch is checked before anything runs: it may change only the
    // candidate's code, nothing else the comparison reads, and it must apply
    // to the commit.
    let mut problems = Vec::new();
    for file in &files {
        for path in touched(root, file).map_err(|e| format!("{}: {e}", name(file)))? {
            let inside = !Path::new(&path)
                .components()
                .any(|c| c == Component::ParentDir)
                && allowed.iter().any(|d| root.join(&path).starts_with(d));
            let full = root.join(&path);
            if let Some((_, what)) = read.iter().find(|(p, _)| full.starts_with(p)) {
                problems.push(format!(
                    "{}: changes {path}, which the comparison reads ({what})",
                    name(file)
                ));
            } else if !inside {
                problems.push(format!(
                    "{}: changes {path}, which is outside the candidate ({})",
                    name(file),
                    allowed
                        .iter()
                        .map(|d| d.strip_prefix(root).unwrap_or(d).display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    sweep_leftovers();
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "{PREFIX}{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    )));
    let clone = scratch.0.join("repo");
    let clone_str = clone.to_str().ok_or("temporary directory is not UTF-8")?;
    let root_str = root.to_str().ok_or("repository path is not UTF-8")?;
    git(
        root,
        &[
            "clone",
            "-q",
            "--shared",
            "--no-checkout",
            root_str,
            clone_str,
        ],
    )?;
    git(&clone, &["checkout", "-q", "--detach", &git_info.commit])?;
    for file in &files {
        let path = file.to_str().ok_or("patch path is not UTF-8")?;
        if let Err(e) = git(&clone, &["apply", "--check", path]) {
            problems.push(format!("{}: does not apply to {short}: {e}", name(file)));
        }
    }
    if !problems.is_empty() {
        return Err(format!(
            "the patches cannot be used as they are:\n  {}",
            problems.join("\n  ")
        ));
    }

    let clone_suite = clone.join(suite_path.strip_prefix(root).map_err(|e| e.to_string())?);
    let run_opts = |mutant: bool| Options {
        suite: clone_suite.clone(),
        report: PathBuf::new(),
        python: opts.python.clone(),
        maturin: opts.maturin.clone(),
        allow_dirty: mutant,
        strict: false,
        count_calls: false,
        timeout: opts.timeout,
        keep_work: false,
        hash_seed: opts.hash_seed,
        mutants: None,
        stop_at_first_timeout: mutant,
    };
    let base = crate::run(&run_opts(false)).map_err(|e| {
        format!("the unmutated suite could not run in a clean clone of {short}: {e}")
    })?;
    if !matches!(base.verdict, Verdict::Pass | Verdict::PassWithRelaxations) {
        return Err(format!(
            "the unmutated suite does not pass at {short}, so what a mutant changes could not be told: {}",
            base.reasons.join("; ")
        ));
    }

    let mut patches = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", name(file)))?;
        git(&clone, &["apply", file.to_str().unwrap_or_default()])?;
        let ran = crate::run(&run_opts(true));
        git(&clone, &["checkout", "-q", "--", "."])?;
        git(&clone, &["clean", "-fdq"])?;
        let mut p = Patch {
            file: name(file),
            sha256: crate::provenance::sha256_file(file)?,
            expected_to_survive: expected_to_survive(&text),
            result: "survived",
            how: BTreeSet::new(),
            cases_run: 0,
            not_identical: 0,
            not_identical_by_case_file: BTreeMap::new(),
            first_cases: Vec::new(),
            stopped_after_timeout: false,
            error: None,
        };
        match ran {
            Err(e) => {
                p.result = "did_not_run";
                let lines: Vec<&str> = e.lines().collect();
                p.error = Some(lines[lines.len().saturating_sub(12)..].join("\n"));
            }
            Ok(r) => {
                p.cases_run = r.cases.len();
                for c in r.cases.iter().filter(|c| {
                    !matches!(
                        c.result,
                        CaseResult::Identical | CaseResult::IdenticalWithRelaxations
                    )
                }) {
                    p.not_identical += 1;
                    let source = c.source.rsplit_once(':').map_or(c.source.as_str(), |s| s.0);
                    let source = source.rsplit('/').next().unwrap_or(source).to_string();
                    *p.not_identical_by_case_file.entry(source).or_default() += 1;
                    if p.first_cases.len() < 5 {
                        p.first_cases.push(c.id.clone());
                    }
                    p.how.insert(how(c));
                }
                p.stopped_after_timeout =
                    p.how.contains("timed out") && p.cases_run < base.cases.len();
                if p.not_identical > 0 {
                    p.result = "killed";
                }
            }
        }
        patches.push(p);
    }

    let mut reasons = Vec::new();
    for p in &patches {
        match (p.result, &p.expected_to_survive) {
            ("killed", None) | ("survived", Some(_)) => {}
            ("killed", Some(_)) => reasons.push(format!(
                "{}: killed, though it says it is expected to survive; its description is wrong",
                p.file
            )),
            ("survived", None) => reasons.push(format!(
                "{}: survived: no case tells it from the port",
                p.file
            )),
            _ => reasons.push(format!(
                "{}: did not run (it did not build or import), so it tests nothing",
                p.file
            )),
        }
    }
    let pass = reasons.is_empty();
    Ok(Report {
        schema: SCHEMA,
        verdict: if pass { "pass" } else { "fail" },
        exit_status: if pass { 0 } else { 1 },
        reasons,
        hash_seed: opts.hash_seed,
        commit: git_info.commit.clone(),
        suite: suite_path
            .strip_prefix(root)
            .unwrap_or(&suite_path)
            .display()
            .to_string(),
        base: Base {
            cases: base.cases.len(),
            identical: base.summary.identical,
        },
        scratch: scratch.0.display().to_string(),
        patches,
    })
}
