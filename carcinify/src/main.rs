//! `carcinify`: one command for the tools that measure each step of moving
//! Python code to Rust behind PyO3. Each subcommand is a library crate; this
//! file chooses among them and adds nothing.

use std::process::ExitCode;

/// The subcommands, in the order of the skill's steps, each with one line
/// and its own usage text.
const COMMANDS: [(&str, &str, &str); 5] = [
    (
        "decide",
        "step 1: where a workload's time goes, and whether that contradicts a decision to port",
        carcinify_decide::cli::USAGE,
    ),
    (
        "deps",
        "step 2: what the code to be ported imports, and what crates.io has for it",
        carcinify_deps::cli::USAGE,
    ),
    (
        "diff",
        "step 3: the original and the port on the same cases, and whether the outputs are identical",
        carcinify_diff::cli::USAGE,
    ),
    (
        "structure",
        "step 6: what the port allocates on its hot path, and what it copies",
        carcinify_structure::cli::USAGE,
    ),
    (
        "speed",
        "step 7: the port against its original, timed from the Python caller",
        carcinify_speed::cli::USAGE,
    ),
];

fn overview() -> String {
    let mut text = String::from("usage: carcinify <command> [arguments]\n\ncommands:\n");
    for (name, line, _) in COMMANDS {
        text.push_str(&format!("  {name:<10} {line}\n"));
    }
    text.push_str(
        "\ncarcinify help <command>, or carcinify <command> --help, prints a command's arguments.\ncarcinify --version prints the version.",
    );
    text
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    let command = args.next();
    // `carcinify <command> --help` is `carcinify help <command>`.
    if let Some((_, _, usage)) = COMMANDS
        .iter()
        .find(|(name, _, _)| Some(*name) == command.as_deref())
        && matches!(args.peek().map(String::as_str), Some("--help" | "-h"))
    {
        println!("{usage}");
        return ExitCode::SUCCESS;
    }
    match command.as_deref() {
        Some("decide") => carcinify_decide::cli::main(args),
        Some("deps") => carcinify_deps::cli::main(args),
        Some("diff") => carcinify_diff::cli::main(args),
        Some("structure") => carcinify_structure::cli::main(args),
        Some("speed") => carcinify_speed::cli::main(args),
        Some("--version" | "-V" | "version") => {
            println!("carcinify {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("help" | "--help" | "-h") => match args.next() {
            None => {
                println!("{}", overview());
                ExitCode::SUCCESS
            }
            Some(command) => match COMMANDS.iter().find(|(name, _, _)| *name == command) {
                Some((_, _, usage)) => {
                    println!("{usage}");
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("carcinify: no command {command:?}\n\n{}", overview());
                    ExitCode::from(2)
                }
            },
        },
        // Nothing was asked for, so nothing was done: not a success.
        None => {
            eprintln!("{}", overview());
            ExitCode::from(2)
        }
        Some(other) => {
            eprintln!("carcinify: no command {other:?}\n\n{}", overview());
            ExitCode::from(2)
        }
    }
}
