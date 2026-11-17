//! Is what ran what the commit holds?
//!
//! `git status` answers for tracked and untracked files but says nothing
//! about ignored ones, and an ignored file can be imported, compiled in or
//! packaged all the same. So every file the run is known to have used is
//! looked up in the list of tracked files instead.

use crate::provenance::{Git, sha256_file};
use crate::python::Built;
use crate::report::FileHash;
use crate::side::Side;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A file that took part in the run and is not in the commit.
#[derive(Debug, Serialize)]
pub struct NotInCommit {
    pub path: String,
    pub role: String,
}

pub fn tracked(git: &Git) -> Result<BTreeSet<PathBuf>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(&git.root)
        // Files inside a submodule are tracked, by the submodule; the
        // superproject lists a submodule as one entry unless asked.
        .args(["ls-files", "-z", "--recurse-submodules"])
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|n| !n.is_empty())
        .map(|n| git.root.join(String::from_utf8_lossy(n).as_ref()))
        .collect())
}

fn rel(git: &Git, p: &Path) -> String {
    p.strip_prefix(&git.root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// Files a side imported from inside the repository that git does not track.
///
/// Installed packages are left out even when the virtualenv is inside the
/// repository: they are third-party code, hashed and listed in the report,
/// and no project commit could hold them. Only files physically under the
/// interpreter's package directories count as installed. A project package
/// installed in editable mode stays in the source tree, so it is checked
/// here like any other project file.
pub fn loaded(git: &Git, tracked: &BTreeSet<PathBuf>, side: &Side) -> Vec<NotInCommit> {
    side.files
        .iter()
        .map(Path::new)
        .filter(|p| {
            p.starts_with(&git.root) && !side.is_installed_package(p) && !tracked.contains(*p)
        })
        .map(|p| NotInCommit {
            path: rel(git, p),
            role: format!("imported by the {} side", side.cfg.name),
        })
        .collect()
}

fn walk(dir: &Path, skip: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if skip(&path) {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, skip, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Every source file that went into the libraries this build produced.
///
/// Cargo writes one dep-info file beside each final library
/// (`<target>/release/lib<name>.d`) and rewrites it on every build; it
/// lists the sources of the crate and of the path dependencies in its
/// workspace. The per-compilation files under `deps/` are not read: those
/// of earlier builds stay there, and would name files that are no longer
/// part of the crate.
pub(crate) fn dep_info_paths(target_dir: &Path) -> Result<BTreeSet<PathBuf>, String> {
    // `release/` directly, or under a target triple when cross-compiling.
    let mut profile_dirs = vec![target_dir.join("release")];
    for entry in std::fs::read_dir(target_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        profile_dirs.push(entry.path().join("release"));
    }
    let mut files = Vec::new();
    for dir in profile_dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.path().extension().is_some_and(|x| x == "d") {
                files.push(entry.path());
            }
        }
    }
    if files.is_empty() {
        return Err(format!(
            "no dep-info file beside the built library under {}, so the sources of the build cannot be checked against the commit",
            target_dir.display()
        ));
    }
    let mut paths = BTreeSet::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines() {
            let Some((_, deps)) = line.split_once(": ") else {
                continue;
            };
            // Spaces inside a path are written as "\ ".
            let mut current = String::new();
            let mut chars = deps.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\\' && chars.peek() == Some(&' ') {
                    current.push(' ');
                    chars.next();
                } else if c == ' ' {
                    paths.insert(PathBuf::from(std::mem::take(&mut current)));
                } else {
                    current.push(c);
                }
            }
            paths.insert(PathBuf::from(current));
        }
    }
    Ok(paths)
}

/// What the build of a PyO3 side used that the commit does not hold, and
/// what it may have used from outside the repository.
pub struct BuildInputs {
    pub not_in_commit: Vec<NotInCommit>,
    /// Cargo and rustup configuration above the repository or in Cargo's
    /// home: it applies to the build and no commit can pin it.
    pub outside: Vec<FileHash>,
}

const CONFIG_NAMES: [&str; 4] = [
    ".cargo/config.toml",
    ".cargo/config",
    "rust-toolchain.toml",
    "rust-toolchain",
];

pub fn build_inputs(
    git: &Git,
    tracked: &BTreeSet<PathBuf>,
    built: &Built,
) -> Result<BuildInputs, String> {
    let mut found: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut outside = Vec::new();
    let (Some(crate_dir), Some(workspace), Some(target_dir)) =
        (&built.crate_dir, &built.workspace_dir, &built.target_dir)
    else {
        return Ok(BuildInputs {
            not_in_commit: Vec::new(),
            outside,
        });
    };
    let mut check = |path: PathBuf, role: &str| {
        if path.is_file()
            && path.starts_with(&git.root)
            && !path.starts_with(target_dir)
            && !tracked.contains(&path)
        {
            found.entry(path).or_insert(role.to_string());
        }
    };

    // 1. Files Cargo, rustup and maturin read by name.
    for dir in [crate_dir, workspace] {
        for name in ["Cargo.toml", "Cargo.lock", "pyproject.toml", "build.rs"] {
            check(dir.join(name), "read by the build");
        }
    }
    let mut outside_files = Vec::new();
    for dir in crate_dir.ancestors() {
        for name in CONFIG_NAMES {
            if dir.starts_with(&git.root) {
                check(dir.join(name), "configures the build");
            } else if dir.join(name).is_file() {
                outside_files.push(dir.join(name));
            }
        }
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    for name in ["config.toml", "config"] {
        if let Some(file) = cargo_home
            .as_ref()
            .map(|h| h.join(name))
            .filter(|f| f.is_file())
        {
            outside_files.push(file);
        }
    }
    outside_files.sort();
    outside_files.dedup();
    for file in outside_files {
        outside.push(FileHash {
            path: file.to_string_lossy().into_owned(),
            kind: None,
            sha256: sha256_file(&file)?,
        });
    }

    // 2. Every source file rustc read, from the dep-info of this build.
    //    One route to a false stop is left: a library that was renamed
    //    leaves its old dep-info file behind. The message covers it.
    let stale_hint = format!(
        "read by the build: compiled into the crate, or an input of its build script, according to the dep-info beside the built library; if the library was renamed since an earlier build, an old record may name it: delete {} and run again",
        target_dir.display()
    );
    for path in dep_info_paths(target_dir)? {
        let path = if path.is_absolute() {
            path
        } else {
            workspace.join(path)
        };
        // Cargo writes a path as the source spelt it, `..` and all
        // (`src/../../shared/schema.json`). The list of tracked files holds
        // real paths, so the path is resolved before it is looked up.
        let path = path.canonicalize().unwrap_or(path);
        check(path, &stale_hint);
    }

    // 3. Files maturin copied into the wheel. Anything in the wheel whose
    //    content equals an untracked or ignored file of the workspace came
    //    from that file.
    let wheel_root = built.load_roots.first().ok_or("no unpacked wheel")?;
    let mut wheel_files = Vec::new();
    walk(
        wheel_root,
        &|p| p.extension().is_some_and(|x| x == "dist-info"),
        &mut wheel_files,
    );
    let mut wheel_hashes = BTreeSet::new();
    for file in wheel_files
        .iter()
        .filter(|f| f.metadata().is_ok_and(|m| m.len() > 0))
    {
        wheel_hashes.insert(sha256_file(file)?);
    }
    let others = Command::new("git")
        .arg("-C")
        .arg(&git.root)
        .args(["ls-files", "-z", "--others", "--"])
        .arg(workspace)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    for name in others.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let path = git.root.join(String::from_utf8_lossy(name).as_ref());
        let skipped = path.strip_prefix(workspace).is_ok_and(|p| {
            p.components().any(|c| {
                ["target", ".venv", "venv"]
                    .iter()
                    .any(|s| c.as_os_str() == *s)
            })
        });
        if skipped || !path.is_file() || path.metadata().is_ok_and(|m| m.len() == 0) {
            continue;
        }
        if wheel_hashes.contains(&sha256_file(&path)?) {
            found
                .entry(path)
                .or_insert("packaged into the wheel".to_string());
        }
    }

    let not_in_commit = found
        .into_iter()
        .map(|(p, role)| NotInCommit {
            path: rel(git, &p),
            role,
        })
        .collect();
    Ok(BuildInputs {
        not_in_commit,
        outside,
    })
}

/// Environment variables that change what Cargo builds. They are recorded,
/// not refused: the same suite may legitimately need one.
pub fn build_environment() -> BTreeMap<String, String> {
    const NAMES: [&str; 8] = [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "RUSTC",
        "RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_TARGET",
        "RUSTUP_TOOLCHAIN",
    ];
    std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .filter(|(k, _)| {
            NAMES.contains(&k.as_str())
                || k.starts_with("PYO3_")
                || k.starts_with("CARGO_PROFILE_")
                || k.starts_with("MATURIN_")
        })
        .collect()
}
