//! carcinify diff: runs a Python original and its Rust port on the same cases
//! and reports whether the outputs are identical. See the README beside
//! this crate for the design and its reasons.

pub mod cli;
pub mod commit;
pub mod compare;
pub mod distinct;
pub mod measured;
pub mod mutants;
pub mod provenance;
pub mod python;
pub mod report;
pub mod side;
pub mod suite;
pub mod value;

use compare::{Outcome, Raise, compare};
use report::{CaseError, CaseReport, CaseResult, ErrorKind, Report};
use serde_json::Value as J;
use side::{Reply, Side};
use std::path::{Path, PathBuf};
use std::time::Duration;
use suite::Case;
use value::Value;

pub struct Options {
    pub suite: PathBuf,
    pub report: PathBuf,
    pub python: PathBuf,
    pub maturin: PathBuf,
    pub allow_dirty: bool,
    /// Exit 1 when any case needed a relaxation.
    pub strict: bool,
    /// Count Python function calls made while each function under test runs.
    pub count_calls: bool,
    pub timeout: Duration,
    pub keep_work: bool,
    pub hash_seed: report::HashSeed,
    /// `--mutants`: the directory of kept patches to run instead of the
    /// suite alone (see `mutants`).
    pub mutants: Option<PathBuf>,
    /// Stop at the first case that times out. Set for each mutant's run: a
    /// hang has caught the mutant, and the cases after it would each wait
    /// out the limit too.
    pub stop_at_first_timeout: bool,
}

/// What a side made of one case: an outcome to compare, or a failure to run.
type Ran = Result<(Outcome, Option<String>), CaseError>;

fn run_side(
    side: &mut Side,
    case: &Case,
    scratch: &Path,
    timeout: Duration,
) -> Result<Ran, String> {
    let name = side.cfg.name;
    let fail = |kind: ErrorKind, detail: String| {
        Ok(Err(CaseError {
            side: name,
            kind,
            detail,
        }))
    };
    let _ = std::fs::remove_dir_all(scratch);
    let request = match suite::request(case, scratch) {
        Ok(r) => r,
        Err(e) => return fail(ErrorKind::Input, e),
    };
    let line = match side.call(&request, timeout)? {
        Reply::Line(l) => l,
        Reply::Crashed(detail) => return fail(ErrorKind::Crashed, detail),
        Reply::Protocol(detail) => return fail(ErrorKind::Protocol, detail),
        Reply::TimedOut => {
            return fail(
                ErrorKind::Timeout,
                format!(
                    "no answer within {}s; the process was killed. Counting Python calls slows code that is mostly Python by up to about nine times: if this side is only slow because it is being counted, pass --no-call-counts, or raise --timeout-secs",
                    timeout.as_secs()
                ),
            );
        }
    };
    let reply: J = match serde_json::from_str(&line) {
        Ok(j) => j,
        Err(e) => return fail(ErrorKind::Protocol, format!("reply is not JSON: {e}")),
    };
    let text = |key: &str| reply.get(key).and_then(J::as_str).unwrap_or("").to_string();
    side.absorb(&reply);
    if let Some(file) = reply.get("defined_in").and_then(J::as_str) {
        let files = side.defined_in.entry(case.func.clone()).or_default();
        files.insert(file.to_string());
        side.implementation
            .insert(case.func.clone(), text("implementation"));
    }
    let counted = reply.get("python_calls").filter(|c| c.is_object());
    if let Some(why) = counted
        .and_then(|c| c.get("unavailable"))
        .and_then(J::as_str)
    {
        let entry = side.python_calls_unavailable.entry(case.func.clone());
        entry.or_insert((0, why.to_string())).0 += 1;
    } else if let Some(counted) = counted {
        let sum = side.python_calls.entry(case.func.clone()).or_insert([0; 3]);
        for (slot, key) in ["own", "shared", "other"].iter().enumerate() {
            sum[slot] += counted.get(*key).and_then(J::as_u64).unwrap_or(0);
        }
    }
    match reply.get("status").and_then(J::as_str) {
        Some("returned") => match reply.get("value").map(Value::decode) {
            Some(Ok(v)) => Ok(Ok((Outcome::Returned(v), None))),
            Some(Err(e)) => fail(ErrorKind::Protocol, format!("bad value in reply: {e}")),
            None => fail(ErrorKind::Protocol, "reply has no value".to_string()),
        },
        Some("raised") => match reply.get("message").map(Value::decode) {
            Some(Ok(message)) => Ok(Ok((
                Outcome::Raised(Raise {
                    ty: text("type"),
                    class_name: text("class_name"),
                    own_class: reply.get("own_class").and_then(J::as_bool) == Some(true),
                    lineage: reply
                        .get("lineage")
                        .and_then(J::as_array)
                        .map(|l| l.iter().filter_map(J::as_str).map(str::to_string).collect())
                        .unwrap_or_default(),
                    message,
                    args: reply.get("args").and_then(|a| Value::decode(a).ok()),
                }),
                Some(text("traceback")),
            ))),
            _ => fail(
                ErrorKind::Protocol,
                "bad exception message in reply".to_string(),
            ),
        },
        Some("unencodable") => fail(ErrorKind::Unencodable, text("detail")),
        Some("bad_input") => fail(ErrorKind::Input, text("detail")),
        Some("bad_call") => fail(ErrorKind::BadCall, text("detail")),
        Some("refused") => fail(ErrorKind::RefusedFunction, text("detail")),
        // The name is a real function, but calling it would not compare
        // this side's port: the suite is set up wrongly, so nothing more
        // is run.
        Some("foreign" | "shared_function") => Err(format!("{name}: {}", text("detail"))),
        Some("no_such_function") => fail(
            ErrorKind::NoSuchFunction,
            format!("the module has no callable named {:?}", case.func),
        ),
        other => fail(
            ErrorKind::Protocol,
            format!("unknown reply status {other:?}"),
        ),
    }
}

fn run_case(
    case: &Case,
    original: &mut Side,
    candidate: &mut Side,
    scratch: &Path,
    timeout: Duration,
) -> Result<CaseReport, String> {
    let a = run_side(original, case, scratch, timeout)?;
    distinct::check_opened(original, "candidate")?;
    let b = run_side(candidate, case, scratch, timeout)?;
    distinct::check_opened(candidate, "original")?;
    let _ = std::fs::remove_dir_all(scratch);
    let mut out = CaseReport::new(case);
    let ((a, a_trace), (b, b_trace)) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            out.errors = [a.err(), b.err()].into_iter().flatten().collect();
            return Ok(out);
        }
    };
    let kind = |o: &Outcome| match o {
        Outcome::Returned(_) => "returned",
        Outcome::Raised(_) => "raised",
    };
    out.original_outcome = Some(kind(&a));
    out.candidate_outcome = Some(kind(&b));
    out.original_sha256 = Some(provenance::sha256_bytes(a.canon().as_bytes()));
    out.candidate_sha256 = Some(provenance::sha256_bytes(b.canon().as_bytes()));
    if out.relaxations_declared.is_some() {
        out.original_relaxed = Some((a.clone(), case.relax.clone()));
    }
    let raised = |o: &Outcome| matches!(o, Outcome::Raised(_));
    // Two sides failing the same way is agreement only when the case was
    // written to provoke it. Otherwise a broken case would count as a pass.
    if raised(&a) && raised(&b) && !case.expect_raise {
        for (side, trace) in [("original", &a_trace), ("candidate", &b_trace)] {
            out.errors.push(CaseError {
                side,
                kind: ErrorKind::Raised,
                detail: format!(
                    "raised, and the case does not set \"expect_raise\":\n{}",
                    trace.as_deref().unwrap_or("")
                ),
            });
        }
        return Ok(out);
    }
    if case.expect_raise && !raised(&a) && !raised(&b) {
        out.errors.push(CaseError {
            side: "both",
            kind: ErrorKind::DidNotRaise,
            detail: "the case sets \"expect_raise\" and neither side raised".to_string(),
        });
        return Ok(out);
    }
    match compare(&a, &b, &case.relax) {
        Ok(matched) => {
            out.notes = matched.notes;
            let applied = matched.applied;
            out.result = if applied.is_empty() {
                CaseResult::Identical
            } else {
                CaseResult::IdenticalWithRelaxations
            };
            out.relaxations_applied_total = applied.len();
            out.relaxations_applied = applied
                .into_iter()
                .take(report::MAX_APPLIED_LISTED)
                .collect();
        }
        Err(mut d) => {
            if d.kind == "outcome" {
                d.note = a_trace.or(b_trace);
            }
            out.result = CaseResult::Different;
            out.first_difference = Some(*d);
        }
    }
    Ok(out)
}

/// Runs a suite and returns the report. `Err` means the run could not be
/// carried out at all (bad suite, dirty tree, failed build).
pub fn run(opts: &Options) -> Result<Report, String> {
    let started = std::time::Instant::now();
    let suite_path = opts
        .suite
        .canonicalize()
        .map_err(|e| format!("suite {}: {e}", opts.suite.display()))?;
    let suite_dir = suite_path
        .parent()
        .ok_or("suite has no directory")?
        .to_path_buf();
    let suite = suite::Suite::load(&suite_path)?;
    let git = provenance::git_info(&suite_dir)?;
    if !git.dirty.is_empty() && !opts.allow_dirty {
        return Err(format!(
            "the working tree has uncommitted changes, so the result would not describe commit {}:\n  {}\nCommit them, or pass --allow-dirty to run anyway (the report will say so).",
            &git.commit[..12.min(git.commit.len())],
            git.dirty.join("\n  ")
        ));
    }
    let mut loaded = suite::load_cases(&suite_dir, &suite.cases)?;
    let measured = measured::load(&suite_dir, &suite.measured, &mut loaded.inputs)?;

    let work = std::env::temp_dir().join(format!(
        "carcinify-diff-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&work).map_err(|e| format!("cannot create {}: {e}", work.display()))?;
    let work = work.canonicalize().map_err(|e| e.to_string())?;
    let result = run_in(
        opts,
        &suite_path,
        &suite,
        &git,
        (loaded, measured),
        &work,
        started,
    );
    if opts.keep_work {
        eprintln!("carcinify diff: work directory kept at {}", work.display());
    } else {
        let _ = std::fs::remove_dir_all(&work);
    }
    result
}

fn refuse_outside_commit(
    opts: &Options,
    git: &provenance::Git,
    files: &[commit::NotInCommit],
) -> Result<(), String> {
    if files.is_empty() || opts.allow_dirty {
        return Ok(());
    }
    let list: Vec<String> = files
        .iter()
        .map(|f| format!("{} ({})", f.path, f.role))
        .collect();
    Err(format!(
        "the run used files that git does not track, so the result would not describe commit {}:\n  {}\nCommit them or remove them, or pass --allow-dirty to run anyway (the report will say so).",
        &git.commit[..12.min(git.commit.len())],
        list.join("\n  ")
    ))
}

/// Completes both sides' `init` as carcinify diff starts them: where the
/// other side's code lives, each side's import path, whether calls are
/// counted, and the shared files' hashes.
pub fn complete_inits(
    (original_cfg, original_built): (&mut side::SideConfig, &python::Built),
    (candidate_cfg, candidate_built): (&mut side::SideConfig, &python::Built),
    shared: &[PathBuf],
    count_calls: bool,
) -> Result<(), String> {
    // Each side is told where the other's code lives, so its audit hook can
    // report any attempt to read it.
    let roots = |b: &python::Built| {
        J::from(
            b.load_roots
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        )
    };
    original_cfg.init["other_roots"] = roots(candidate_built);
    candidate_cfg.init["other_roots"] = roots(original_built);
    // Import path of each side: its own directories, then shared
    // directories that stand on their own, then any directory of the other
    // side that holds a shared entry, so that `import pkg.util` finds the
    // other side's shared `pkg/util.py`. Everything in that directory which
    // is not shared stays off limits: loading or opening it stops the run.
    let side_roots = [
        original_built.load_roots.clone(),
        candidate_built.load_roots.clone(),
    ]
    .concat();
    for (cfg, own, other) in [
        (original_cfg, original_built, candidate_built),
        (candidate_cfg, candidate_built, original_built),
    ] {
        let standalone = shared
            .iter()
            .filter(|e| e.is_dir() && !side_roots.iter().any(|r| e.starts_with(r)));
        let lent = other
            .load_roots
            .iter()
            .filter(|r| shared.iter().any(|e| e.starts_with(r)));
        let path: Vec<String> = own
            .load_roots
            .iter()
            .chain(standalone)
            .chain(lent)
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        cfg.init["sys_path"] = J::from(path);
        cfg.init["count_calls"] = J::from(count_calls);
        // Shared is by content as well as by path, so the driver is given
        // the content of every shared file to recognise copies by.
        cfg.init["shared_hashes"] =
            J::from(distinct::contents(shared)?.into_keys().collect::<Vec<_>>());
    }
    Ok(())
}

fn run_in(
    opts: &Options,
    suite_path: &Path,
    suite: &suite::Suite,
    git: &provenance::Git,
    (loaded, measured): (suite::Loaded, Vec<measured::File>),
    work: &Path,
    started: std::time::Instant,
) -> Result<Report, String> {
    let suite_dir = suite_path.parent().unwrap_or(Path::new("/"));
    let tools = python::Tools {
        python: opts.python.clone(),
        maturin: opts.maturin.clone(),
        hash_seed: opts.hash_seed.value,
        recompile_crate: false,
    };
    let tracked = commit::tracked(git)?;
    let mut not_in_commit = Vec::new();
    let mut outside = Vec::new();
    let mut shared = Vec::new();
    for entry in &suite.shared {
        let path = suite_dir.join(entry);
        let path = path
            .canonicalize()
            .map_err(|e| format!("shared entry {}: {e}", path.display()))?;
        if !path.starts_with(&git.root) {
            return Err(format!(
                "shared entry {} must be inside the repository",
                path.display()
            ));
        }
        shared.push(path);
    }
    let mut prepared = Vec::new();
    for (name, spec) in [
        ("original", &suite.original),
        ("candidate", &suite.candidate),
    ] {
        let (cfg, built) =
            python::prepare(name, spec, suite_dir, &git.root, &shared, work, &tools)?;
        for dir in built
            .source_dirs
            .iter()
            .chain(&built.crate_dir)
            .chain(&built.workspace_dir)
        {
            if !dir.starts_with(&git.root) {
                return Err(format!(
                    "{name}: {} is outside the repository, so the commit would not describe it",
                    dir.display()
                ));
            }
        }
        let inputs = commit::build_inputs(git, &tracked, &built)?;
        not_in_commit.extend(inputs.not_in_commit);
        outside.extend(inputs.outside);
        refuse_outside_commit(opts, git, &not_in_commit)?;
        prepared.push((cfg, built));
    }
    let (mut candidate_cfg, candidate_built) = prepared.pop().ok_or("no candidate")?;
    let (mut original_cfg, original_built) = prepared.pop().ok_or("no original")?;
    distinct::check_layout(&original_built, &candidate_built, &shared)?;
    complete_inits(
        (&mut original_cfg, &original_built),
        (&mut candidate_cfg, &candidate_built),
        &shared,
        opts.count_calls,
    )?;
    let mut original = Side::start(original_cfg, opts.timeout)?;
    let mut candidate = Side::start(candidate_cfg, opts.timeout)?;
    // Installed packages are not checked against the commit, so project
    // code must not be allowed to pass for one.
    for (side, built) in [(&original, &original_built), (&candidate, &candidate_built)] {
        let dirs = built
            .source_dirs
            .iter()
            .chain(&built.crate_dir)
            .chain(&shared);
        if let Some(dir) = dirs.into_iter().find(|d| side.is_installed_package(d)) {
            return Err(format!(
                "{} is inside the interpreter's installed-package directory. Code under test, and code declared as shared, must be project files that git tracks, not an installed copy.",
                dir.display()
            ));
        }
    }
    // Once before any case runs, so a suite that names the same code twice
    // stops at once, and again at the end, when every import has happened.
    distinct::check(
        (&original, &original_built),
        (&candidate, &candidate_built),
        &shared,
    )?;

    let scratch = work.join("scratch");
    let calls_files_not_measured = measured::not_listed(&git.root, &measured)?;
    // Before any case: the measured calls are looked up among the cases.
    let measured = measured::check(
        &measured,
        &git.root,
        &loaded.cases,
        &mut original,
        &scratch,
        opts.timeout,
    )?;
    let mut cases = Vec::with_capacity(loaded.cases.len());
    for case in &loaded.cases {
        let ran = run_case(case, &mut original, &mut candidate, &scratch, opts.timeout)?;
        let timed_out = ran
            .errors
            .iter()
            .any(|e| e.kind == report::ErrorKind::Timeout);
        cases.push(ran);
        if timed_out && opts.stop_at_first_timeout {
            break;
        }
    }
    original.finish();
    candidate.finish();
    distinct::check(
        (&original, &original_built),
        (&candidate, &candidate_built),
        &shared,
    )?;
    not_in_commit.extend(commit::loaded(git, &tracked, &original));
    not_in_commit.extend(commit::loaded(git, &tracked, &candidate));
    refuse_outside_commit(opts, git, &not_in_commit)?;

    report::build(report::Parts {
        opts,
        suite_path,
        git,
        inputs: &loaded.inputs,
        original: (&original, &original_built),
        candidate: (&candidate, &candidate_built),
        cases,
        measured,
        calls_files_not_measured,
        shared: &shared,
        not_in_commit,
        cargo_config_outside_repository: outside,
        duration: started.elapsed(),
    })
}
