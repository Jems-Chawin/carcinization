//! Measured inputs: every call a measurement made must also be a case, so
//! that a speed figure is never for an input the comparison did not cover.
//! The suite lists the files of recorded calls under `measured`; each line
//! is a case line, or names an argument the value format cannot hold.
//!
//! A recorded line and a case are the same input when the original's
//! signature binds them to the same arguments, compared as outputs are:
//! sets in no order, ints by every digit, dict key order counted.

use crate::side::{Reply, Side};
use crate::suite::{self, Case, Measured};
use crate::value::Value;
use serde::Serialize;
use serde_json::Value as J;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One listed file, read before either side starts.
pub struct File {
    pub entry: Measured,
    pub path: PathBuf,
    pub sha256: String,
    pub calls: Vec<Case>,
    pub unencodable: Vec<Unencodable>,
}

#[derive(Serialize, Clone)]
pub struct Unencodable {
    pub source: String,
    #[serde(rename = "fn")]
    pub func: String,
    /// The type the value format does not cover.
    pub holds: String,
}

#[derive(Serialize)]
pub struct Missing {
    pub source: String,
    #[serde(rename = "fn")]
    pub func: String,
    /// Why it could not be compared at all, when it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Serialize)]
pub struct FileReport {
    pub file: String,
    pub sha256: String,
    #[serde(rename = "fn", skip_serializing_if = "Option::is_none")]
    pub func: Option<String>,
    pub lines: usize,
    /// Lines found among the cases.
    pub compared: usize,
    pub missing: Vec<Missing>,
    pub unencodable: Vec<Unencodable>,
    /// The author's reason, from the suite, why `unencodable` may stand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_comparable: Option<String>,
    /// Set when the file holds no calls, which nothing else would show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<&'static str>,
}

/// Reads every listed file. A listed file that is not there stops the run.
pub fn load(
    suite_dir: &Path,
    entries: &[Measured],
    inputs: &mut BTreeMap<PathBuf, (&'static str, String)>,
) -> Result<Vec<File>, String> {
    let mut files = Vec::new();
    for entry in entries {
        let path = suite_dir.join(&entry.file);
        let path = path
            .canonicalize()
            .map_err(|e| format!("measured file {}: {e}", path.display()))?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("measured file {}: {e}", path.display()))?;
        let sha256 = crate::provenance::sha256_file(&path)?;
        inputs.insert(path.clone(), ("measured", sha256.clone()));
        let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        let (mut calls, mut unencodable) = (Vec::new(), Vec::new());
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let source = format!("{}:{}", entry.file, n + 1);
            let j: J = serde_json::from_str(line).map_err(|e| format!("{source}: {e}"))?;
            if let Some(holds) = j.get("unencodable") {
                let func = entry
                    .func
                    .clone()
                    .or_else(|| j["fn"].as_str().map(str::to_string));
                unencodable.push(Unencodable {
                    source,
                    func: func.unwrap_or_default(),
                    holds: holds.as_str().unwrap_or_default().to_string(),
                });
            } else {
                let mut call = suite::parse_case(line, &source, &dir, inputs)?;
                if let Some(f) = &entry.func {
                    call.func.clone_from(f);
                }
                calls.push(call);
            }
        }
        files.push(File {
            entry: entry.clone(),
            path,
            sha256,
            calls,
            unencodable,
        });
    }
    Ok(files)
}

/// The arguments the original binds a call to, in canonical form, or why
/// it could not bind them.
fn bound(
    original: &mut Side,
    call: &Case,
    scratch: &Path,
    timeout: Duration,
) -> Result<Result<String, String>, String> {
    let request = suite::request_as(call, scratch, "bind", &call.func)?;
    let reply = original.call(&request, timeout)?;
    let _ = std::fs::remove_dir_all(scratch);
    let line = match reply {
        Reply::Line(l) => l,
        Reply::Crashed(d) | Reply::Protocol(d) => {
            return Err(format!("the original, binding {}: {d}", call.source));
        }
        Reply::TimedOut => return Err(format!("the original timed out binding {}", call.source)),
    };
    let j: J = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    Ok(match j["status"].as_str() {
        Some("bound") => Ok(Value::decode(&j["value"])?.canon()),
        Some("no_such_function") => Err(format!("the original has no function {:?}", call.func)),
        _ => Err(j["detail"].as_str().unwrap_or("not bound").to_string()),
    })
}

/// Looks every measured call up among the cases. Runs on the original
/// alone, before any case.
pub fn check(
    files: &[File],
    root: &Path,
    cases: &[Case],
    original: &mut Side,
    scratch: &Path,
    timeout: Duration,
) -> Result<Vec<FileReport>, String> {
    let mut reports = Vec::new();
    if files.is_empty() {
        return Ok(reports);
    }
    let mut known = BTreeSet::new();
    for case in cases {
        // A case that cannot be bound fails when it runs, and says why there.
        if let Ok(key) = bound(original, case, scratch, timeout)? {
            known.insert((case.func.clone(), key));
        }
    }
    for f in files {
        let mut missing = Vec::new();
        for call in &f.calls {
            let why = match bound(original, call, scratch, timeout)? {
                Ok(key) if known.contains(&(call.func.clone(), key.clone())) => continue,
                Ok(_) => None,
                Err(why) => Some(why),
            };
            missing.push(Missing {
                source: call.source.clone(),
                func: call.func.clone(),
                why,
            });
        }
        reports.push(FileReport {
            file: f
                .path
                .strip_prefix(root)
                .unwrap_or(&f.path)
                .display()
                .to_string(),
            sha256: f.sha256.clone(),
            func: f.entry.func.clone(),
            lines: f.calls.len() + f.unencodable.len(),
            compared: f.calls.len() - missing.len(),
            missing,
            unencodable: f.unencodable.clone(),
            not_comparable: f.entry.not_comparable.clone(),
            warning: (f.calls.is_empty() && f.unencodable.is_empty()).then_some(
                "holds no calls: the recording never saw its candidate (a lambda, a partial or a wrong name), or the workload did not call it",
            ),
        });
    }
    Ok(reports)
}

/// Files of recorded calls in the repository that the suite does not list:
/// a recording that was made and then forgotten.
pub fn not_listed(root: &Path, files: &[File]) -> Result<Vec<String>, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.calls.jsonl",
        ])
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    let listed: BTreeSet<&Path> = files.iter().map(|f| f.path.as_path()).collect();
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let full = root.join(p);
            !listed.contains(full.canonicalize().as_deref().unwrap_or(&full))
        })
        .map(str::to_string)
        .collect())
}
