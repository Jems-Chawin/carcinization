//! Makes sure the library that gets packaged was compiled from the sources
//! that are on disk now.
//!
//! Cargo decides whether to recompile by comparing file times. A source
//! file put in place with a time older than the last build (`cp -p`,
//! `rsync -t`, an archive unpacked over the tree, clocks that disagree on a
//! shared directory) is not recompiled, and the previous library would be
//! run under the new commit's name. Only file content is decided by time;
//! a changed toolchain, feature set or `RUSTFLAGS` Cargo notices itself.
//!
//! So the harness keeps its own record of the content. After each
//! successful build it writes into the target directory a digest of the
//! sources: the Cargo workspace and every path dependency outside it.
//! Before the next build it compares. If the digest differs, or there is
//! no record, every package built from local sources is cleaned first,
//! path dependencies included, which makes Cargo compile them again.
//! Registry dependencies stay built, so this costs seconds, and an
//! unchanged tree is not rebuilt at all.
//!
//! The digest does not come from the previous build's dep-info, which
//! would miss a file that build never saw. For a directory with nothing
//! uncommitted it is the git tree id, which names the content exactly.
//! With uncommitted changes (`--allow-dirty`) the files themselves are
//! hashed: every tracked file as it is on disk and every untracked file
//! git does not ignore.
//!
//! One record per target directory, and there is one target directory per
//! feature set, so each feature set is tracked on its own.

use crate::provenance::{digest_of, sha256_file, tracked_digest};
use serde_json::Value as J;
use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::process::Command;

const RECORD: &str = "carcinify-diff-build-inputs.sha256";

pub struct Build<'a> {
    pub repo_root: &'a Path,
    pub crate_dir: &'a Path,
    pub workspace_dir: &'a Path,
    pub target_dir: &'a Path,
    /// Name and directory of every package built from local sources, from
    /// `cargo metadata`, asked once.
    pub local: OnceCell<Result<Vec<(String, PathBuf)>, String>>,
}

fn git(root: &Path, args: &[&str], dir: &Path) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .arg(dir)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

fn files_outside_repo(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        match entry.file_type() {
            Ok(t) if t.is_dir() && entry.file_name() != "target" && entry.file_name() != ".git" => {
                files_outside_repo(&entry.path(), out)
            }
            Ok(t) if t.is_file() => out.push(entry.path()),
            _ => {}
        }
    }
}

impl Build<'_> {
    /// Packages whose `source` is null in `cargo metadata`: the workspace's
    /// members and every path dependency, wherever it lives.
    fn local_packages(&self) -> Result<&Vec<(String, PathBuf)>, String> {
        let found = self.local.get_or_init(|| {
            let out = Command::new("cargo")
                .args(["metadata", "--format-version", "1", "--locked"])
                .current_dir(self.crate_dir)
                .output()
                .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "cargo metadata failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            let json: J =
                serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
            let mut local = Vec::new();
            for p in json
                .get("packages")
                .and_then(J::as_array)
                .into_iter()
                .flatten()
            {
                let name = p.get("name").and_then(J::as_str);
                let manifest = p.get("manifest_path").and_then(J::as_str).map(Path::new);
                if let (true, Some(name), Some(dir)) = (
                    p.get("source").is_some_and(J::is_null),
                    name,
                    manifest.and_then(Path::parent),
                ) {
                    local.push((
                        name.to_string(),
                        dir.canonicalize().unwrap_or(dir.to_path_buf()),
                    ));
                }
            }
            if local.is_empty() {
                return Err("cargo metadata lists no local package".to_string());
            }
            Ok(local)
        });
        found.as_ref().map_err(String::clone)
    }

    /// A digest of the content of every local source directory.
    fn digest(&self) -> Result<String, String> {
        let mut dirs = vec![self.workspace_dir.to_path_buf()];
        for (_, dir) in self.local_packages()? {
            if !dirs.iter().any(|d| dir.starts_with(d)) {
                dirs.push(dir.clone());
            }
        }
        let covered_dirs = dirs.clone();
        let mut entries = Vec::new();
        for dir in dirs {
            let label = dir.to_string_lossy().into_owned();
            let Ok(rel) = dir.strip_prefix(self.repo_root) else {
                // A path dependency outside the repository: no git to ask.
                let mut files = Vec::new();
                files_outside_repo(&dir, &mut files);
                let hashed = files
                    .iter()
                    .map(|f| Ok((f.to_string_lossy().into_owned(), sha256_file(f)?)));
                entries.push((
                    label,
                    digest_of(hashed.collect::<Result<Vec<_>, String>>()?),
                ));
                continue;
            };
            let hidden =
                crate::provenance::hidden_from_status(self.repo_root, &dir.to_string_lossy())?;
            if hidden.is_empty()
                && git(self.repo_root, &["status", "--porcelain", "--"], &dir)?.is_empty()
            {
                // Nothing uncommitted here: the tree id is the content.
                let spec = if rel.as_os_str().is_empty() {
                    "HEAD^{tree}".to_string()
                } else {
                    format!("HEAD:{}", rel.display())
                };
                let out = Command::new("git")
                    .arg("-C")
                    .arg(self.repo_root)
                    .args(["rev-parse", &spec])
                    .output()
                    .map_err(|e| format!("cannot run git: {e}"))?;
                if !out.status.success() {
                    return Err(format!(
                        "git rev-parse {spec} failed: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
                entries.push((
                    label,
                    format!("tree {}", String::from_utf8_lossy(&out.stdout).trim()),
                ));
            } else {
                // Uncommitted changes: hash what is on disk.
                let mut files = vec![(
                    "tracked".to_string(),
                    tracked_digest(self.repo_root, &dir)?.0,
                )];
                let untracked = git(
                    self.repo_root,
                    &["ls-files", "-z", "--others", "--exclude-standard", "--"],
                    &dir,
                )?;
                for name in untracked.split(|b| *b == 0).filter(|n| !n.is_empty()) {
                    let name = String::from_utf8_lossy(name).into_owned();
                    let path = self.repo_root.join(&name);
                    if path.is_file() {
                        files.push((name, sha256_file(&path)?));
                    }
                }
                entries.push((label, format!("files {}", digest_of(files))));
            }
        }
        // A file compiled in from outside those directories (a schema or
        // test vectors at the repository root, reached with `..`). The last
        // build's dep-info names them. That list cannot be missing a file
        // that matters: a new outside file can only come in through a
        // change to a source file inside, and that changes a digest above.
        for path in crate::commit::dep_info_paths(self.target_dir).unwrap_or_default() {
            let path = if path.is_absolute() {
                path
            } else {
                self.workspace_dir.join(path)
            };
            let path = path.canonicalize().unwrap_or(path);
            let covered = path.starts_with(self.target_dir)
                || covered_dirs.iter().any(|d| path.starts_with(d));
            if !covered && path.is_file() {
                entries.push((
                    path.to_string_lossy().into_owned(),
                    format!("file {}", sha256_file(&path)?),
                ));
            }
        }
        Ok(digest_of(entries))
    }

    /// Call before building. Returns true when the local packages were
    /// cleaned, which happens when the sources are not those of the last
    /// recorded build.
    pub fn clean_if_inputs_changed(&self) -> Result<bool, String> {
        let record = self.target_dir.join(RECORD);
        if !self.target_dir.exists() {
            return Ok(false); // nothing built here yet, so nothing stale
        }
        let now = self.digest();
        if let (Ok(now), Ok(then)) = (&now, std::fs::read_to_string(&record))
            && then.trim() == now
        {
            return Ok(false);
        }
        // The record goes first: if anything below fails half way, the next
        // run must not find a record that vouches for what is there.
        self.forget();
        if !self.clean_local_packages() {
            std::fs::remove_dir_all(self.target_dir)
                .map_err(|e| format!("cannot clear {}: {e}", self.target_dir.display()))?;
        }
        Ok(true)
    }

    /// `cargo clean -p` for every local package, path dependencies
    /// included. False if that could not be done; the caller then removes
    /// the whole directory.
    fn clean_local_packages(&self) -> bool {
        let Ok(local) = self.local_packages() else {
            return false;
        };
        let mut clean = Command::new("cargo");
        clean
            .arg("clean")
            .arg("--release")
            .arg("--target-dir")
            .arg(self.target_dir)
            .current_dir(self.crate_dir);
        for (name, _) in local {
            clean.args(["-p", name]);
        }
        clean.output().is_ok_and(|o| o.status.success())
    }

    /// Call after a successful build: what is in the target directory now
    /// was built from the sources as they are now.
    pub fn record(&self) -> Result<(), String> {
        let record = self.target_dir.join(RECORD);
        std::fs::write(&record, self.digest()?)
            .map_err(|e| format!("cannot write {}: {e}", record.display()))
    }

    /// Call when a build fails: nothing vouches for the directory any more.
    pub fn forget(&self) {
        let _ = std::fs::remove_file(self.target_dir.join(RECORD));
    }
}
