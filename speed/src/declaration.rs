//! The declaration both steps read, and the checks its targets must pass
//! before anything is built (design §3).

use carcinify_diff::suite::{self, Case, Suite};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "carcinify/speed/declaration/1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub schema: String,
    /// The harness's suite, relative to this file.
    pub suite: String,
    /// Runs per side and target.
    pub runs: usize,
    pub targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: String,
    /// A direct target: a case of the suite.
    #[serde(default)]
    pub case: Option<CaseRef>,
    /// A caller target: a function that calls the real caller.
    #[serde(default)]
    pub caller: Option<Caller>,
    pub calls_per_run: usize,
}

/// A Python function that calls the real caller, which reaches the
/// compared function through the attribute `replace` (design §1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caller {
    /// Relative to the suite's directory, as its case files are.
    pub file: String,
    pub function: String,
    /// The compared function the caller reaches.
    #[serde(rename = "fn")]
    pub func: String,
    /// `module.attribute`, where each side's function is put.
    pub replace: String,
}

/// What a target times.
pub enum Timed {
    /// A case: an index into the suite's cases.
    Case(usize),
    /// A caller, in this file.
    Caller(PathBuf),
}

/// A case of the suite: its file, as the suite lists it, and its id.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRef {
    pub file: String,
    pub id: String,
}

pub struct Loaded {
    pub declaration: Declaration,
    pub suite_path: PathBuf,
    pub suite: Suite,
    /// The suite's cases.
    pub cases: Vec<Case>,
    /// What each target times, in the declaration's order; `None` for a
    /// target that was refused.
    pub timed: Vec<Option<Timed>>,
    /// Why each refused target was refused.
    pub refused: Vec<String>,
}

/// Reads the declaration and its suite, and checks each target. An error is
/// a declaration or suite that cannot be used at all; a refused target is
/// listed in `refused`.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read declaration {}: {e}", path.display()))?;
    let declaration: Declaration = serde_json::from_str(&text)
        .map_err(|e| format!("bad declaration {}: {e}", path.display()))?;
    if declaration.schema != SCHEMA {
        return Err(format!(
            "declaration schema is {:?}, expected {SCHEMA:?}",
            declaration.schema
        ));
    }
    if declaration.runs == 0 || declaration.targets.iter().any(|t| t.calls_per_run == 0) {
        return Err("\"runs\" and every \"calls_per_run\" must be at least 1".into());
    }
    let suite_path = path
        .parent()
        .unwrap_or(Path::new("."))
        .join(&declaration.suite);
    let suite = Suite::load(&suite_path)?;
    let suite_dir = suite_path.parent().unwrap_or(Path::new("/"));
    let loaded = suite::load_cases(suite_dir, &suite.cases)?;
    let canonical = |rel: &str| suite_dir.join(rel).canonicalize().ok();
    let suite_files: Vec<_> = suite.cases.iter().filter_map(|c| canonical(c)).collect();
    let cases = loaded.cases;
    let mut timed = Vec::new();
    let mut refused = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for t in &declaration.targets {
        let checked = if !names.insert(t.name.as_str()) {
            Err("the name is used twice".to_string())
        } else {
            match (&t.case, &t.caller) {
                (Some(case), None) => direct(case, &cases, &suite_files, &canonical),
                (None, Some(caller)) => match canonical(&caller.file) {
                    Some(file) if file.is_file() => Ok(Timed::Caller(file)),
                    _ => Err(format!("the caller's file {:?} is not there", caller.file)),
                },
                _ => Err("a target names a case or a caller, one of them".to_string()),
            }
        };
        timed.push(
            checked
                .map_err(|why| refused.push(format!("target {:?}: {why}", t.name)))
                .ok(),
        );
    }
    Ok(Loaded {
        declaration,
        suite_path,
        suite,
        cases,
        timed,
        refused,
    })
}

/// A direct target's case, if it is one diff compares and its call returns.
/// Two targets may time the same case.
fn direct(
    case: &CaseRef,
    cases: &[Case],
    suite_files: &[PathBuf],
    canonical: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<Timed, String> {
    let file = canonical(&case.file);
    if file.as_ref().is_none_or(|f| !suite_files.contains(f)) {
        return Err(format!(
            "{:?} is not one of the suite's case files, so no diff run compares its cases",
            case.file
        ));
    }
    match cases.iter().position(|c| c.id == case.id) {
        Some(i) if canonical(case_file(&cases[i])) != file => Err(format!(
            "{:?} has no case {:?}; it is in {}",
            case.file, case.id, cases[i].source
        )),
        None => Err(format!("{:?} has no case {:?}", case.file, case.id)),
        Some(i) if cases[i].expect_raise => Err(format!(
            "case {:?} sets expect_raise: a call that raises is not timed",
            case.id
        )),
        Some(i) => Ok(Timed::Case(i)),
    }
}

/// The case file a case came from, as the suite lists it.
fn case_file(c: &Case) -> &str {
    c.source.rsplit_once(':').map_or("", |(file, _)| file)
}
