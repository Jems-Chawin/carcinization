//! The `carcinify` command itself: what it prints and how it exits before
//! any tool runs, and that every test file of a tool is built.

use std::path::Path;
use std::process::Command;

fn carcinify(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_carcinify"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const COMMANDS: [&str; 5] = ["decide", "deps", "diff", "structure", "speed"];

#[test]
fn the_command_says_what_it_is_and_exits_as_documented() {
    let (code, out, _) = carcinify(&["--version"]);
    assert_eq!(
        (code, out.trim()),
        (0, concat!("carcinify ", env!("CARGO_PKG_VERSION")))
    );

    // The five commands, in the order of the skill's steps, one line each.
    let (code, help, _) = carcinify(&["help"]);
    assert_eq!(code, 0);
    let at: Vec<usize> = COMMANDS
        .iter()
        .map(|c| {
            help.find(&format!("\n  {c} "))
                .unwrap_or_else(|| panic!("{c} is missing:\n{help}"))
        })
        .collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{help}");
    for (step, command) in ["step 1", "step 2", "step 3", "step 6", "step 7"]
        .iter()
        .zip(COMMANDS)
    {
        let line = help
            .lines()
            .find(|l| l.trim_start().starts_with(command))
            .unwrap();
        assert!(line.contains(step), "{line}");
    }
    assert_eq!(carcinify(&["--help"]), (0, help.clone(), String::new()));

    // Nothing asked for: the same text on standard error, and not a success.
    let (code, out, err) = carcinify(&[]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert_eq!(err, help);

    for command in COMMANDS {
        // Three ways to ask for a command's arguments, one answer.
        let (code, usage, _) = carcinify(&["help", command]);
        assert_eq!(code, 0);
        assert!(
            usage.starts_with(&format!("usage: carcinify {command} ")),
            "{usage}"
        );
        assert_eq!(
            carcinify(&[command, "--help"]),
            (0, usage.clone(), String::new())
        );
        assert_eq!(
            carcinify(&[command, "-h"]),
            (0, usage.clone(), String::new())
        );
        // A command with nothing to work on is not a success.
        assert_eq!(carcinify(&[command]).0, 2);
    }

    for unknown in [&["carcinize"][..], &["help", "carcinize"][..]] {
        let (code, out, err) = carcinify(unknown);
        assert_eq!((code, out.as_str()), (2, ""), "{unknown:?}");
        assert!(
            err.starts_with("carcinify: no command \"carcinize\""),
            "{err}"
        );
    }
}

/// The tests that run the binary live in each tool's `tests/` directory and
/// are built only if `carcinify/Cargo.toml` lists them: the tools' own
/// crates have `autotests = false`. A file that is not listed would never
/// run, and nothing else would say so.
#[test]
fn every_test_file_of_a_tool_is_built() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(here.join("Cargo.toml")).unwrap();
    let listed: Vec<String> = manifest
        .lines()
        .filter_map(|l| l.trim().strip_prefix("path = \""))
        .filter_map(|l| l.strip_suffix('"'))
        .map(String::from)
        .collect();
    let mut missing = Vec::new();
    let mut found = 0;
    for tool in ["harness", "structure", "deps", "decide", "speed"] {
        let dir = here.join("..").join(tool).join("tests");
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_file() && name.ends_with(".rs") {
                found += 1;
                let path = format!("../{tool}/tests/{name}");
                if !listed.contains(&path) {
                    missing.push(path);
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "test files that no [[test]] in carcinify/Cargo.toml builds, so they never run: {missing:?}"
    );
    // And the check itself looked at something.
    assert!(found >= 6, "only {found} test files found");
}
