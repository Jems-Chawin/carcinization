//! The command line of this tool, as a function the `carcinify` binary
//! calls.

use crate::report::{CaseResult, HashSeed, Verdict};
use crate::{Options, report, run};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

pub const USAGE: &str = "\
usage: carcinify diff <suite.json> --report <report.json> [options]

Runs both sides of a suite on every case, compares the outputs exactly and
writes the report.

options:
  --python <path>       interpreter for both sides  [$CARCINIFY_PYTHON, else python3]
  --maturin <path>      maturin for PyO3 sides      [$CARCINIFY_MATURIN, else maturin]
  --timeout-secs <n>    limit per case and side     [120]
  --allow-dirty         run although the tree does not match the commit;
                        the report records it
  --strict              exit 1 if any case needed a relaxation
  --no-call-counts      do not count Python calls made by each function
                        (the count slows sides that are mostly Python)
  --keep-work           keep the temporary build and log directory
  --hash-seed <n>       PYTHONHASHSEED for both sides, 0 to 4294967295
                        [drawn at random; the report records it either way]

exit status: 0 every case identical (with --strict: and none through a
relaxation), 1 any difference, any case that failed to run, a function never
compared on a returned value, or no cases, 2 the run could not be carried out";

fn options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let env =
        |name: &str, default: &str| PathBuf::from(std::env::var_os(name).unwrap_or(default.into()));
    let mut suite = None;
    let mut report = None;
    let mut opts = Options {
        suite: PathBuf::new(),
        report: PathBuf::new(),
        python: env("CARCINIFY_PYTHON", "python3"),
        maturin: env("CARCINIFY_MATURIN", "maturin"),
        allow_dirty: false,
        strict: false,
        count_calls: true,
        timeout: Duration::from_secs(120),
        keep_work: false,
        hash_seed: HashSeed {
            value: drawn_seed(),
            chosen: "drawn",
        },
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--report" => report = Some(PathBuf::from(value()?)),
            "--python" => opts.python = PathBuf::from(value()?),
            "--maturin" => opts.maturin = PathBuf::from(value()?),
            "--timeout-secs" => {
                let secs: u64 = value()?
                    .parse()
                    .map_err(|_| "--timeout-secs needs a whole number".to_string())?;
                opts.timeout = Duration::from_secs(secs.max(1));
            }
            "--allow-dirty" => opts.allow_dirty = true,
            "--strict" => opts.strict = true,
            "--no-call-counts" => opts.count_calls = false,
            "--keep-work" => opts.keep_work = true,
            "--hash-seed" => {
                let value = value()?.parse().map_err(|_| {
                    "--hash-seed needs a whole number from 0 to 4294967295".to_string()
                })?;
                opts.hash_seed = HashSeed {
                    value,
                    chosen: "given",
                };
            }
            "-h" | "--help" => return Err(String::new()),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ if suite.is_none() => suite = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    opts.suite = suite.ok_or("no suite file given")?;
    opts.report = report.ok_or("--report is required")?;
    // A relative interpreter path with a directory in it must survive the
    // harness changing directory for the build.
    for tool in [&mut opts.python, &mut opts.maturin] {
        if tool.components().count() > 1 && tool.is_relative() {
            *tool = std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(&*tool);
        }
    }
    Ok(opts)
}

/// A seed for a run that names none. The keys of std's `RandomState` are
/// random for each process, so one hash of them is a random number without a
/// dependency.
fn drawn_seed() -> u32 {
    use std::hash::{BuildHasher, RandomState};
    RandomState::new().hash_one(std::process::id()) as u32
}

/// The `diff` subcommand: its arguments, without the program's name.
pub fn main(args: impl Iterator<Item = String>) -> ExitCode {
    let opts = match options(args) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("carcinify diff: {e}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    // Whatever happens next, a report from an earlier run must not survive it.
    let _ = std::fs::remove_file(&opts.report);
    let outcome = run(&opts).and_then(|r| report::write(&opts.report, &r).map(|()| r));
    let r = match outcome {
        Ok(r) => r,
        Err(e) => {
            eprintln!("carcinify diff: {e}");
            let error = serde_json::json!({"schema": report::REPORT_SCHEMA, "verdict": "error", "outcome": "error", "exit_status": 2, "reasons": [e]});
            if let Err(e) = report::write(&opts.report, &error) {
                eprintln!("carcinify diff: {e}");
            }
            return ExitCode::from(2);
        }
    };
    for c in r
        .cases
        .iter()
        .filter(|c| matches!(c.result, CaseResult::Different | CaseResult::Error))
        .take(20)
    {
        println!(
            "{} {} ({})",
            c.result.as_str().to_uppercase(),
            c.id,
            c.source
        );
        if let Some(d) = &c.first_difference {
            println!("  at {}: {}", d.path, d.kind);
            for (side, shown) in [("original ", &d.original), ("candidate", &d.candidate)] {
                match shown {
                    Some(s) => println!(
                        "    {side}  {} {}{}",
                        s.ty,
                        s.repr,
                        s.bits
                            .as_ref()
                            .map(|b| format!("  bits {b}"))
                            .unwrap_or_default()
                    ),
                    None => println!("    {side}  (absent)"),
                }
            }
            if let Some(record) = &d.record {
                let fields: Vec<String> = record.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                println!("    in the record  {}", fields.join(", "));
            }
            if let Some(note) = d.note.as_ref().filter(|_| d.kind != "outcome") {
                println!("    {note}");
            }
        }
        for e in &c.errors {
            println!(
                "  {} {}: {}",
                e.side,
                e.kind.as_str(),
                e.detail.lines().last().unwrap_or("")
            );
        }
    }
    let s = &r.summary;
    println!(
        "{:<28} {:>5} {:>9} {:>7} {:>9} {:>6}  on a returned value",
        "function", "cases", "identical", "relaxed", "different", "failed"
    );
    for f in &r.checks.functions {
        println!(
            "{:<28} {:>5} {:>9} {:>7} {:>9} {:>6}  {} ({} returned, {} raised)",
            f.func,
            f.cases,
            f.identical,
            f.identical_with_relaxations,
            f.different,
            f.failed_to_run,
            f.compared_on_a_returned_value,
            f.both_returned,
            f.both_raised
        );
        // Where the work was done: what the function is, and how many
        // Python calls were made while it ran.
        println!("    original:  {}", f.original_work);
        println!("    candidate: {}", f.candidate_work);
    }
    let nc = &r.checks.public_functions_not_called;
    for (side, names) in [("original", &nc.original), ("candidate", &nc.candidate)] {
        if !names.is_empty() {
            println!("not called by any case ({side}): {}", names.join(", "));
        }
    }
    for entry in &r.checks.shared {
        let by = |who: &str| -> Vec<&str> {
            let files = entry.files_loaded.iter().filter(|f| f.loaded_by == who);
            files.map(|f| f.path.as_str()).collect()
        };
        let copies: Vec<&str> = entry
            .files_loaded
            .iter()
            .flat_map(|f| f.copies.iter().map(String::as_str))
            .collect();
        let copies = if copies.is_empty() {
            String::new()
        } else {
            format!(
                " (also as the identical cop{} {})",
                if copies.len() == 1 { "y" } else { "ies" },
                copies.join(", ")
            )
        };
        let both = by("both");
        println!(
            "shared, run by both sides and not compared ({}): {}",
            entry.path,
            if both.is_empty() {
                "nothing loaded by both".to_string()
            } else {
                format!("{}{copies}", both.join(", "))
            }
        );
        for who in ["original", "candidate"] {
            if !by(who).is_empty() {
                println!("    loaded by the {who} side only: {}", by(who).join(", "));
            }
        }
    }
    let started = &r.checks.processes_started;
    for (side, list) in [
        ("original", &started.original),
        ("candidate", &started.candidate),
    ] {
        if list.count > 0 {
            println!(
                "processes started by the {side} side: {} (listed in the report)",
                list.count
            );
        }
    }
    println!(
        "{} cases: {} identical, {} identical with relaxations, {} different, {} failed to run",
        s.cases, s.identical, s.identical_with_relaxations, s.different, s.errors
    );
    println!(
        "commit {}{}",
        r.git.commit,
        if r.git.dirty {
            " plus changes that are not in it"
        } else {
            ""
        }
    );
    println!(
        "hash seed {} ({}); --hash-seed {} repeats the run",
        r.hash_seed.value, r.hash_seed.chosen, r.hash_seed.value
    );
    println!("report {}", opts.report.display());
    let headline = match r.verdict {
        Verdict::Pass => "PASS".to_string(),
        Verdict::PassWithRelaxations => format!(
            "PASS WITH RELAXATIONS: {} of {} case(s) were identical only under a relaxation they declared{}",
            s.identical_with_relaxations,
            s.cases,
            if opts.strict {
                "; --strict makes this a failure"
            } else {
                ""
            }
        ),
        Verdict::Fail => format!("FAIL: {}", r.reasons.join("; ")),
    };
    println!("{headline}");
    ExitCode::from(r.exit_status)
}
