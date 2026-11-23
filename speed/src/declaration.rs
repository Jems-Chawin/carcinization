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
    pub case: CaseRef,
    pub calls_per_run: usize,
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
    /// Each target's case, an index into `cases`, in the declaration's
    /// order; `None` for a target that was refused.
    pub target_cases: Vec<Option<usize>>,
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
    let mut target_cases = Vec::new();
    let mut refused = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for t in &declaration.targets {
        let refuse = |why: String| format!("target {:?}: {why}", t.name);
        let file = canonical(&t.case.file);
        let why = if !names.insert(t.name.as_str()) {
            Some("the name is used twice".to_string())
        } else if file.as_ref().is_none_or(|f| !suite_files.contains(f)) {
            Some(format!(
                "{:?} is not one of the suite's case files, so no diff run compares its cases",
                t.case.file
            ))
        } else {
            match cases.iter().find(|c| c.id == t.case.id) {
                Some(c) if canonical(case_file(c)) != file => Some(format!(
                    "{:?} has no case {:?}; it is in {}",
                    t.case.file, t.case.id, c.source
                )),
                None => Some(format!("{:?} has no case {:?}", t.case.file, t.case.id)),
                Some(c) if c.expect_raise => Some(format!(
                    "case {:?} sets expect_raise: a call that raises is not timed",
                    t.case.id
                )),
                Some(_) => None,
            }
        };
        // Two targets may time the same case.
        target_cases.push(match why {
            Some(why) => {
                refused.push(refuse(why));
                None
            }
            None => cases.iter().position(|c| c.id == t.case.id),
        });
    }
    Ok(Loaded {
        declaration,
        suite_path,
        suite,
        cases,
        target_cases,
        refused,
    })
}

/// The case file a case came from, as the suite lists it.
fn case_file(c: &Case) -> &str {
    c.source.rsplit_once(':').map_or("", |(file, _)| file)
}
