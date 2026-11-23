//! The commit this crate is built from, for the report's tools block
//! (RUST-181): "unknown" outside a git checkout. Built again when HEAD, or
//! the branch it names, moves.

use std::process::Command;

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
}
