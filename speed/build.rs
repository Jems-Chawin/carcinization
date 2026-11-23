//! The commit this crate is built from, and its uncommitted changes, for the
//! report's tools block (RUST-181): "unknown" outside a git checkout. Built
//! again when HEAD, the branch it names, or a tracked file changes.

use std::io::Write;
use std::process::{Command, Stdio};

fn main() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=CARCINIFY_SOURCE_COMMIT={commit}");
    // The hash of git diff HEAD's output as it is, so that it can be checked
    // by hand: git diff HEAD | git hash-object --stdin.
    let diff = Command::new("git")
        .args(["diff", "--no-ext-diff", "--no-color", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success());
    let dirty = match diff {
        Some(d) if d.stdout.is_empty() => "clean".to_string(),
        Some(d) => hashed(&d.stdout).unwrap_or_else(|| "unknown".into()),
        None => "unknown".into(),
    };
    println!("cargo:rustc-env=CARCINIFY_SOURCE_DIRTY={dirty}");
    let branch = git(&["rev-parse", "--symbolic-full-name", "HEAD"]);
    let watched = [
        Some("HEAD".to_string()),
        branch,
        Some("packed-refs".to_string()),
    ];
    for path in watched.into_iter().flatten() {
        if let Some(file) = git(&["rev-parse", "--git-path", &path])
            && std::path::Path::new(&file).exists()
        {
            println!("cargo:rerun-if-changed={file}");
        }
    }
    if let (Some(top), Some(files)) = (git(&["rev-parse", "--show-toplevel"]), git(&["ls-files"])) {
        for file in files.lines() {
            println!("cargo:rerun-if-changed={top}/{file}");
        }
    }
}

/// git's own hash of the text, the same on every machine and toolchain.
fn hashed(text: &[u8]) -> Option<String> {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(text).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
