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

use crate::compare::compare;
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
    /// The scratch clone, with `--keep-work`, which keeps it. Otherwise it is
    /// removed when the run ends, and not named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scratch: Option<String>,
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
    /// `killed`, `survived`, `did_not_run` (it did not build or import), or
    /// `invalid`: its build or run wrote where a patch may not, or the
    /// original failed, so the result says nothing about the cases.
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
struct Scratch(PathBuf, bool);

impl Drop for Scratch {
    fn drop(&mut self) {
        if self.1 {
            eprintln!("carcinify diff: scratch clone kept at {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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

/// The paths of the clone that differ from the commit: changed, added,
/// deleted, untracked and ignored, each by itself (a rename is a deletion
/// and an addition), relative to the clone.
fn changed(clone: &Path) -> Result<Vec<String>, String> {
    let out = git(
        clone,
        &[
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--ignored",
        ],
    )?;
    Ok(out
        .split('\0')
        .filter_map(|e| e.get(3..))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect())
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

/// The failures in a mutant's run that are not the candidate's: only the
/// candidate is mutated, so the original failing (a time-out on a busy
/// machine, say) says nothing about the cases, and must not count as a kill.
fn not_the_candidates(cases: &[CaseReport]) -> Vec<String> {
    cases
        .iter()
        .flat_map(|c| c.errors.iter().map(move |e| (c, e)))
        .filter(|(_, e)| e.side != "candidate")
        .map(|(c, e)| format!("{} ({}: {:?})", c.id, e.side, e.kind))
        .collect()
}

fn name(p: &Path) -> String {
    p.file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned())
}

/// The rule for every path a patch changes, and every path its build and
/// run leave changed: inside the candidate, and nothing the comparison
/// reads. The candidate's ignored target directory passes.
struct Rule<'a> {
    root: &'a Path,
    short_dirs: String,
    allowed: Vec<PathBuf>,
    read: Vec<(PathBuf, &'static str)>,
}

impl Rule<'_> {
    fn refused(&self, path: &str) -> Option<String> {
        let full = self.root.join(path);
        if let Some((_, what)) = self.read.iter().find(|(p, _)| full.starts_with(p)) {
            return Some(format!("{path}, which the comparison reads ({what})"));
        }
        let inside = !Path::new(path)
            .components()
            .any(|c| c == Component::ParentDir)
            && self.allowed.iter().any(|d| full.starts_with(d));
        (!inside).then(|| {
            format!(
                "{path}, which is outside the candidate ({})",
                self.short_dirs
            )
        })
    }

    fn refused_in(&self, clone: &Path) -> Result<Vec<String>, String> {
        Ok(changed(clone)?
            .iter()
            .filter_map(|p| self.refused(p))
            .collect())
    }
}

/// Puts the clone back to the commit.
fn reset(clone: &Path, refused: &[String]) -> Result<(), String> {
    git(clone, &["checkout", "-q", "--", "."])?;
    git(clone, &["clean", "-fdq"])?;
    // Ignored files that should not be there go too; the candidate's
    // target directory stays, so that the next patch builds on it.
    for r in refused {
        let path = r.split(", which ").next().unwrap_or(r);
        git(clone, &["clean", "-fdxq", "--", path])?;
    }
    Ok(())
}

/// A clone of the repository at `commit`, in a scratch directory removed
/// when the run ends unless `keep_work`.
fn scratch_clone(root: &Path, commit: &str, keep_work: bool) -> Result<(Scratch, PathBuf), String> {
    sweep_leftovers();
    let scratch = Scratch(
        std::env::temp_dir().join(format!(
            "{PREFIX}{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        )),
        keep_work,
    );
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
    git(&clone, &["checkout", "-q", "--detach", commit])?;
    Ok((scratch, clone))
}

/// One patch's entry, from its run and from what its build and run changed
/// where a patch may not (`wrote`): which cases caught it and how, or why
/// the result says nothing about the cases.
fn patch_report(
    file: &Path,
    text: &str,
    ran: Result<crate::report::Report, String>,
    wrote: &[String],
    base: &crate::report::Report,
    base_original: &BTreeMap<&str, &CaseReport>,
) -> Result<Patch, String> {
    let original_changed = |c: &CaseReport| {
        let Some(now) = c.original_sha256.as_deref() else {
            return false;
        };
        let Some(was) = base_original.get(c.id.as_str()) else {
            return true;
        };
        match (&was.original_relaxed, &c.original_relaxed) {
            (Some((was, relax)), Some((now, _))) => compare(was, now, relax).is_err(),
            _ => was.original_sha256.as_deref() != Some(now),
        }
    };
    let mut p = Patch {
        file: name(file),
        sha256: crate::provenance::sha256_file(file)?,
        expected_to_survive: expected_to_survive(text),
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
            p.stopped_after_timeout = p.how.contains("timed out") && p.cases_run < base.cases.len();
            if p.not_identical > 0 {
                p.result = "killed";
            }
            let mut wrong = Vec::new();
            let original = not_the_candidates(&r.cases);
            if !original.is_empty() {
                wrong.push(format!(
                    "not the candidate alone failed, so the result says nothing: {}",
                    original.join(", ")
                ));
            }
            let changed: Vec<&str> = r
                .cases
                .iter()
                .filter(|c| original_changed(c))
                .map(|c| c.id.as_str())
                .collect();
            if !changed.is_empty() {
                wrong.push(format!(
                    "the original's output in {} changed from the unmutated run: something changed what it reads, or it does not give the same output twice",
                    changed.join(", ")
                ));
            }
            if !wrong.is_empty() {
                p.result = "invalid";
                p.error = Some(wrong.join("; "));
            }
        }
    }
    if !wrote.is_empty() {
        p.result = "invalid";
        p.error = Some(format!(
            "its build or run changed {}; the comparison did not read what the commit holds",
            wrote.join("; ")
        ));
    }
    Ok(p)
}

/// Why the run fails: each patch whose result is not the one it should
/// have.
fn reasons(patches: &[Patch]) -> Vec<String> {
    let mut reasons = Vec::new();
    for p in patches {
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
            ("invalid", _) => reasons.push(format!(
                "{}: invalid: {}",
                p.file,
                p.error.as_deref().unwrap_or("")
            )),
            _ => reasons.push(format!(
                "{}: did not run (it did not build or import), so it tests nothing",
                p.file
            )),
        }
    }
    reasons
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
    // Absolute, so that git in the repository and in the clone reads the
    // same files as are hashed here.
    let dir = &dir
        .canonicalize()
        .map_err(|e| format!("--mutants {}: {e}", dir.display()))?;
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
    let rule = Rule {
        root,
        short_dirs: allowed
            .iter()
            .map(|d| d.strip_prefix(root).unwrap_or(d).display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        allowed,
        read,
    };
    let (scratch, clone) = scratch_clone(root, &git_info.commit, opts.keep_work)?;
    // Every patch is checked before anything runs: it must apply to the
    // commit, and what it changes must pass the rule.
    let mut problems = Vec::new();
    for file in &files {
        let path = file.to_str().ok_or("patch path is not UTF-8")?;
        if let Err(e) = git(&clone, &["apply", path]) {
            problems.push(format!("{}: does not apply to {short}: {e}", name(file)));
            continue;
        }
        let wrong = rule.refused_in(&clone)?;
        problems.extend(wrong.iter().map(|w| format!("{}: changes {w}", name(file))));
        reset(&clone, &wrong)?;
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

    // What the original gave on each case, unmutated. Only the candidate is
    // patched, so the original must give the same in every patch's run: by
    // hash, or as the sides are compared where the case relaxes that.
    let base_original: BTreeMap<&str, &CaseReport> =
        base.cases.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut patches = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", name(file)))?;
        git(&clone, &["apply", file.to_str().unwrap_or_default()])?;
        let ran = crate::run(&run_opts(true));
        // The build or the run may have written where the patch could not.
        let wrote = rule.refused_in(&clone)?;
        reset(&clone, &wrote)?;
        patches.push(patch_report(
            file,
            &text,
            ran,
            &wrote,
            &base,
            &base_original,
        )?);
    }

    let reasons = reasons(&patches);
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
        scratch: opts.keep_work.then(|| scratch.0.display().to_string()),
        patches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::CaseError;

    #[test]
    fn only_the_candidate_failing_can_kill_a_mutant() {
        let mut inputs = BTreeMap::new();
        let case = crate::suite::parse_case(
            r#"{"id": "c", "fn": "f"}"#,
            "x:1",
            Path::new("/"),
            &mut inputs,
        )
        .unwrap();
        let failed = |side: &'static str, kind: ErrorKind| {
            let mut c = CaseReport::new(&case);
            c.errors.push(CaseError {
                side,
                kind,
                detail: String::new(),
            });
            c
        };
        assert!(not_the_candidates(&[failed("candidate", ErrorKind::Timeout)]).is_empty());
        assert_eq!(
            not_the_candidates(&[failed("original", ErrorKind::Timeout)]),
            ["c (original: Timeout)"]
        );
        assert_eq!(
            not_the_candidates(&[failed("both", ErrorKind::DidNotRaise)]).len(),
            1
        );
    }
}
