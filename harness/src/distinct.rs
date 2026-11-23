//! Refuses to compare a piece of code with itself. Language-neutral.
//!
//! Two sides that are the same code always agree, and the report would then
//! be the strongest evidence the project has for a port that was never
//! compared. A suite that names one module twice, a candidate that imports
//! the original, a port that ships a copy of the original and calls it, and
//! a half-finished port that falls back to the original all look like this.
//! The run stops instead of passing.
//!
//! Code that both sides are meant to use goes under `shared` in the suite:
//! a directory of its own, or single files and sub-directories inside one
//! side's directory (a helper module of the package being ported, a data
//! file, a constants file that is the same on both sides). Shared entries
//! are exempt from these rules, listed in the report, and no function under
//! test may be defined in one. Everything else in a side's directory stays
//! off limits to the other side.

use crate::provenance::sha256_file;
use crate::python::Built;
use crate::side::Side;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What `check` enforces, as recorded in the report.
pub const RULES: [&str; 6] = [
    "the two sides' source directories are different and neither is inside the other",
    "a shared entry is not a side's whole source directory and does not contain one; a shared directory outside both sides holds no file with the same content as a side's own file",
    "no file loaded by one side lies in the other side's source directories, unless it is shared",
    "no file loaded by one side has the same content as a file in the other side's source directories, unless that content is shared",
    "no function under test is defined in a shared file, or by the same file or file content on both sides",
    "neither side's Python code opened a file in the other side's source directories, unless it is shared (seen by an audit hook; native code is not seen)",
];

const SHARED_HINT: &str = "If both sides are meant to use this file, list it under \"shared\" in the suite (a file or a directory; it may be inside one side's directory). No function under test may be defined in it.";

fn same(why: String) -> String {
    format!("the two sides are not different code: {why}")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() && entry.file_name() != "__pycache__" => files_under(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Content hash -> one file with that content, for every non-empty file
/// under the directories.
pub fn contents(dirs: &[PathBuf]) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut files = Vec::new();
    for dir in dirs {
        if dir.is_file() {
            files.push(dir.clone());
        } else {
            files_under(dir, &mut files);
        }
    }
    let mut map = BTreeMap::new();
    for file in files {
        if file.metadata().is_ok_and(|m| m.len() > 0) {
            map.entry(sha256_file(&file)?).or_insert(file);
        }
    }
    Ok(map)
}

/// Checks that need nothing from a running side: how the directories lie.
pub fn check_layout(original: &Built, candidate: &Built, shared: &[PathBuf]) -> Result<(), String> {
    for a in &original.load_roots {
        for b in &candidate.load_roots {
            if a == b {
                return Err(same(format!(
                    "both sides name the same source directory, {}. The original and the candidate must each have a directory of their own; code both are meant to use goes in a directory listed under \"shared\" in the suite, not in either side's path.",
                    a.display()
                )));
            }
            let (inner, outer, which) = if a.starts_with(b) {
                (a, b, ("original", "candidate"))
            } else if b.starts_with(a) {
                (b, a, ("candidate", "original"))
            } else {
                continue;
            };
            return Err(same(format!(
                "the {} side's directory {} is inside the {} side's directory {}, so the outer side can import the inner one's code. Put the two in sibling directories (for example original/ and candidate/); code both need goes in a third directory listed under \"shared\" in the suite.",
                which.0,
                inner.display(),
                which.1,
                outer.display()
            )));
        }
    }
    let roots = [original.load_roots.clone(), candidate.load_roots.clone()].concat();
    let mut own = contents(&roots)?;
    own.retain(|_, file| !shared.iter().any(|e| file.starts_with(e)));
    for entry in shared {
        for (name, built) in [("original", original), ("candidate", candidate)] {
            if let Some(root) = built.load_roots.iter().find(|r| r.starts_with(entry)) {
                return Err(same(format!(
                    "the shared entry {} is, or contains, the {name} side's whole source directory {}, which would hand that side's implementation to the other. Name only the files or sub-directories inside it that both sides are meant to use.",
                    entry.display(),
                    root.display()
                )));
            }
        }
        // A shared directory of its own must not be a place to park one
        // side's code. (An entry inside a side's directory is that side's
        // code by definition; what it may hold is limited by the rule that
        // no function under test is defined in it.)
        if roots.iter().any(|r| entry.starts_with(r)) {
            continue;
        }
        for (hash, file) in contents(std::slice::from_ref(entry))? {
            if let Some(twin) = own.get(&hash) {
                return Err(same(format!(
                    "{} in a shared directory has the same content as {} in a side's own source directory, so one side's code is available to both through the shared directory. If the file is meant to be the same on both sides, list {} under \"shared\" instead.",
                    file.display(),
                    twin.display(),
                    twin.display()
                )));
            }
        }
    }
    Ok(())
}

/// The message for a file one side opened in the other's directories.
pub fn check_opened(side: &Side, other: &str) -> Result<(), String> {
    // Name the source file rather than its bytecode cache when both were opened.
    let source = side
        .opened_other
        .iter()
        .find(|f| !f.contains("__pycache__"));
    match source.or(side.opened_other.iter().next()) {
        None => Ok(()),
        Some(file) => Err(same(format!(
            "the {} side opened {file}, which is in the {other} side's source directory (seen by the audit hook: an import, an exec of its source, or a plain read). {SHARED_HINT}",
            side.cfg.name
        ))),
    }
}

/// Checks on what the two sides loaded and ran. Called before the first
/// case and again after the last, when every import has happened.
pub fn check(
    original: (&Side, &Built),
    candidate: (&Side, &Built),
    shared: &[PathBuf],
) -> Result<(), String> {
    check_opened(original.0, "candidate")?;
    check_opened(candidate.0, "original")?;
    let in_shared = |file: &str| shared.iter().any(|d| Path::new(file).starts_with(d));
    // Content that is declared shared is shared wherever a copy of it sits
    // (a constants file kept identical on both sides, or the copy of a
    // shared file that maturin put in the wheel).
    let shared_content = contents(shared)?;
    for ((a, _), (b, b_built)) in [(candidate, original), (original, candidate)] {
        let mut theirs = contents(&b_built.load_roots)?;
        theirs.retain(|hash, _| !shared_content.contains_key(hash));
        for file in &a.files {
            if in_shared(file) {
                continue;
            }
            if b_built
                .load_roots
                .iter()
                .any(|r| Path::new(file).starts_with(r))
            {
                return Err(same(format!(
                    "the {} side loaded {file}, which belongs to the {} side. {SHARED_HINT}",
                    a.cfg.name, b.cfg.name
                )));
            }
            if !Path::new(file).metadata().is_ok_and(|m| m.len() > 0) {
                continue;
            }
            if let Some(twin) = theirs.get(&sha256_file(Path::new(file))?) {
                return Err(same(format!(
                    "{1}, loaded by the {0} side, has the same content as {2} in the {3} side's source directory: one side holds a copy of the other's code. {SHARED_HINT}",
                    a.cfg.name,
                    file,
                    twin.display(),
                    b.cfg.name
                )));
            }
        }
    }
    let (a, b) = (original.0, candidate.0);
    for (func, a_files) in &a.defined_in {
        for a_file in a_files {
            for b_file in b.defined_in.get(func).into_iter().flatten() {
                if a_file == b_file
                    || sha256_file(Path::new(a_file))? == sha256_file(Path::new(b_file))?
                {
                    return Err(same(format!(
                        "function {func:?} is defined by {a_file} on the original side and by {b_file} on the candidate side, and the two are the same file or have the same content"
                    )));
                }
            }
        }
    }
    Ok(())
}
