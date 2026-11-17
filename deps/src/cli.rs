//! The command line of this tool, as a function the `carcinify` binary
//! calls.

use crate::report::{self, Options, REPORT_SCHEMA};
use std::path::PathBuf;
use std::process::ExitCode;

pub const USAGE: &str = "\
usage: carcinify deps <proposal.json> --report <report.json> [options]

Reads the imports of the Python files the proposal names, looks up on
crates.io every candidate and search term the proposal gives, and writes
what it found.

options:
  --markdown <path>      also write the report as Markdown
  --cache <dir>          where answers are kept      [target/carcinify-deps/cache]
  --replay               use only the cache; make no request
  --refresh              ask again even if the cache has a recent answer
  --python <path>        interpreter used to read the imports  [python3]
  --min-age-days <n>     flag a latest version younger than this  [14]

exit status: 0 the report is complete, 1 something is missing from it,
2 the run could not be carried out";

/// The `deps` subcommand: its arguments, without the program's name.
pub fn main(mut args: impl Iterator<Item = String>) -> ExitCode {
    let mut opts = Options {
        proposal: PathBuf::new(),
        cache: PathBuf::from("target/carcinify-deps/cache"),
        replay: false,
        refresh: false,
        python: PathBuf::from("python3"),
        min_age_days: 14,
    };
    let (mut out, mut md, mut proposal) = (None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--report" => out = args.next().map(PathBuf::from),
            "--markdown" => md = args.next().map(PathBuf::from),
            "--cache" => opts.cache = args.next().map(PathBuf::from).unwrap_or_default(),
            "--python" => opts.python = args.next().map(PathBuf::from).unwrap_or_default(),
            "--min-age-days" => {
                opts.min_age_days = args.next().and_then(|n| n.parse().ok()).unwrap_or(14)
            }
            "--replay" => opts.replay = true,
            "--refresh" => opts.refresh = true,
            flag if flag.starts_with('-') => {
                eprintln!("carcinify deps: unknown option {flag}\n\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => proposal = Some(PathBuf::from(arg)),
        }
    }
    let (Some(proposal), Some(out)) = (proposal, out) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    opts.proposal = proposal;
    let _ = std::fs::remove_file(&out);
    let write = |path: &PathBuf, text: String| {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    let r = match report::run(&opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("carcinify deps: {e}");
            let error = serde_json::json!({"schema": REPORT_SCHEMA, "verdict": "error", "exit_status": 2, "reasons": [e]});
            let _ = write(&out, error.to_string() + "\n");
            return ExitCode::from(2);
        }
    };
    let json = serde_json::to_string_pretty(&r).unwrap_or_default() + "\n";
    let written = write(&out, json).and_then(|()| {
        md.as_ref()
            .map_or(Ok(()), |p| write(p, report::markdown(&r)))
    });
    if let Err(e) = written {
        eprintln!("carcinify deps: {e}");
        return ExitCode::from(2);
    }
    for d in &r.dependencies {
        println!("{}: {}", d.import, d.outcome);
        for c in &d.candidates {
            let facts = &c.facts;
            match c.exists {
                true => println!(
                    "  {} [{}] {} released {}, {} downloads in 90 days (a weak signal)",
                    c.name,
                    c.classification,
                    facts["latest_version"].as_str().unwrap_or("?"),
                    facts["latest_published"].as_str().map_or("?", |d| &d[..10]),
                    facts["downloads_weak_signal"]["recent_90_days"]
                ),
                false => println!("  {} DOES NOT EXIST on crates.io", c.name),
            }
            for flag in &c.flags {
                println!("    flag: {flag}");
            }
        }
        for x in &d.dismissed {
            println!(
                "  dismissed by the proposal: {}{} ({})",
                x.krate.name,
                if x.plainly_alive {
                    " [PLAINLY ALIVE]"
                } else if !x.krate.exists {
                    " [does not exist]"
                } else {
                    ""
                },
                x.declared_reason
            );
        }
        if !d.not_examined.is_empty() {
            println!(
                "  found by search and not examined: {}",
                d.not_examined.join(", ")
            );
        }
        for flag in &d.flags {
            println!("  flag: {flag}");
        }
        println!("  also an option: keep it in Python");
    }
    for flag in &r.flags {
        println!("flag: {flag}");
    }
    let cached = r.source["answers_from_the_cache"].as_u64().unwrap_or(0);
    let how = match r.source["mode"].as_str() {
        Some("replay") => format!(
            "REPLAYED from the cache, no request made; oldest answer {}",
            r.source["oldest_cached_answer"].as_str().unwrap_or("?")
        ),
        Some("live") => "all fetched on this run".to_string(),
        _ => format!(
            "{cached} from the cache, oldest {}",
            r.source["oldest_cached_answer"].as_str().unwrap_or("?")
        ),
    };
    println!(
        "{} answers ({how}); report {} (generated {})",
        r.requests.len(),
        out.display(),
        r.generated_at
    );
    match r.verdict {
        "complete" => println!("COMPLETE"),
        _ => println!("INCOMPLETE: {}", r.reasons.join("; ")),
    }
    ExitCode::from(r.exit_status)
}
