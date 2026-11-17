//! What ran: commit, hashes, tool versions. Language-neutral.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_bytes(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot hash {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("cannot hash {}: {e}", path.display()))?;
        if n == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&buf[..n]);
    }
}

/// One digest for a set of `(label, sha256)` pairs, independent of order.
pub fn digest_of(mut entries: Vec<(String, String)>) -> String {
    entries.sort();
    let mut hasher = Sha256::new();
    for (label, hash) in entries {
        hasher.update(label.as_bytes());
        hasher.update([0]);
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    hex(&hasher.finalize())
}

fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

pub struct Git {
    pub root: PathBuf,
    pub commit: String,
    /// `git status --porcelain` lines, plus one line for each file that
    /// differs from the commit but is hidden from `git status`; empty when
    /// the tree matches the commit.
    pub dirty: Vec<String>,
    /// Path and commit of every submodule, nested ones included.
    pub submodules: Vec<(String, String)>,
}

/// Tracked files that `git status` has been told not to look at
/// (`git update-index --assume-unchanged` or `--skip-worktree`), as paths
/// relative to `root`. `pathspec` limits the listing.
pub fn hidden_from_status(root: &Path, pathspec: &str) -> Result<Vec<String>, String> {
    let listing = git(root, &["ls-files", "-v", "--", pathspec])?;
    let mut hidden = Vec::new();
    for line in String::from_utf8_lossy(&listing).lines() {
        // `git ls-files -v`: a lower-case tag means assume-unchanged, `S`
        // means skip-worktree.
        if let Some((tag, path)) = line.split_once(' ')
            && (tag == "S" || tag.chars().all(|c| c.is_ascii_lowercase()))
        {
            hidden.push(path.to_string());
        }
    }
    Ok(hidden)
}

pub fn git_info(dir: &Path) -> Result<Git, String> {
    let text = |b: Vec<u8>| String::from_utf8_lossy(&b).trim().to_string();
    let root = PathBuf::from(text(git(dir, &["rev-parse", "--show-toplevel"])?));
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let commit = text(git(&root, &["rev-parse", "HEAD"])?);
    let mut dirty: Vec<String> = text(git(&root, &["status", "--porcelain"])?)
        .lines()
        .map(str::to_string)
        .collect();
    // A file marked assume-unchanged or skip-worktree can be edited without
    // `git status` saying so. Such a file is compared with the commit here.
    for path in hidden_from_status(&root, ".")? {
        let on_disk = git(&root, &["hash-object", "--", &path])
            .map(text)
            .unwrap_or_default();
        let committed = git(&root, &["rev-parse", &format!("HEAD:{path}")])
            .map(text)
            .unwrap_or_default();
        if on_disk.is_empty() || on_disk != committed {
            dirty.push(format!(" M {path} (differs from the commit; hidden from git status by assume-unchanged or skip-worktree)"));
        }
    }
    let mut submodules = Vec::new();
    for line in text(git(&root, &["submodule", "status", "--recursive"])?).lines() {
        let mut parts = line
            .trim_start_matches(['+', '-', 'U', ' '])
            .split_whitespace();
        if let (Some(commit), Some(path)) = (parts.next(), parts.next()) {
            submodules.push((path.to_string(), commit.to_string()));
        }
    }
    Ok(Git {
        root,
        commit,
        dirty,
        submodules,
    })
}

/// Digest and count of the files git tracks under `dir`, as they are on disk.
pub fn tracked_digest(root: &Path, dir: &Path) -> Result<(String, usize), String> {
    let rel = dir
        .strip_prefix(root)
        .map_err(|_| format!("{} is outside the repository", dir.display()))?;
    let pathspec = if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().into_owned()
    };
    let out = git(
        root,
        &["ls-files", "-z", "--recurse-submodules", "--", &pathspec],
    )?;
    let mut entries = Vec::new();
    for name in out.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name).into_owned();
        let path = root.join(&name);
        if path.is_file() {
            entries.push((name, sha256_file(&path)?));
        }
    }
    let count = entries.len();
    Ok((digest_of(entries), count))
}

/// First line a tool prints for its version, or why it could not be asked.
pub fn tool_version(program: &Path, args: &[&str], cwd: &Path) -> String {
    match Command::new(program).args(args).current_dir(cwd).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
        Ok(out) => format!("unavailable: exit {}", out.status),
        Err(e) => format!("unavailable: {e}"),
    }
}
