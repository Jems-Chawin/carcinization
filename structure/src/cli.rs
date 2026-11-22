//! The command line of this tool, as a function the `carcinify` binary
//! calls.

use crate::report::{self, Options, REPORT_SCHEMA};
use std::path::PathBuf;
use std::process::ExitCode;

pub const USAGE: &str = "\
usage: carcinify structure <declaration.json> --report <report.json> [--allow-dirty]
                         [--record-calls <file.calls.jsonl>]

Measures allocations on each declared hot path against its declared budget,
counts the constructs that hide an undesigned ownership in the functions
reachable from it, and writes the report.

exit status: 0 pass (possibly with flags), 1 a check failed, 2 the run could
not be carried out";

/// The `structure` subcommand: its arguments, without the program's name.
pub fn main(mut args: impl Iterator<Item = String>) -> ExitCode {
    let mut declaration = None;
    let mut out = None;
    let mut allow_dirty = false;
    let mut record_calls = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--report" => out = args.next().map(PathBuf::from),
            "--allow-dirty" => allow_dirty = true,
            "--record-calls" => record_calls = args.next().map(PathBuf::from),
            flag if flag.starts_with('-') => {
                eprintln!("carcinify structure: unknown option {flag}\n\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => declaration = Some(PathBuf::from(arg)),
        }
    }
    let (Some(declaration), Some(out)) = (declaration, out) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    // A report from an earlier run must not survive this one.
    let _ = std::fs::remove_file(&out);
    let write = |value: &serde_json::Value| {
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string_pretty(value).unwrap_or_default() + "\n";
        std::fs::write(&out, text).map_err(|e| format!("cannot write {}: {e}", out.display()))
    };
    let r = match report::run(&Options {
        declaration,
        allow_dirty,
        record_calls,
    }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("carcinify structure: {e}");
            let _ = write(
                &serde_json::json!({"schema": REPORT_SCHEMA, "verdict": "error", "exit_status": 2, "reasons": [e]}),
            );
            return ExitCode::from(2);
        }
    };
    if let Err(e) = serde_json::to_value(&r)
        .map_err(|e| e.to_string())
        .and_then(|v| write(&v))
    {
        eprintln!("carcinify structure: {e}");
        return ExitCode::from(2);
    }
    if !r.hot_paths.is_empty() {
        let f = &r.krate["features"];
        println!(
            "built with {} and features {}",
            if f["default_features"] == true {
                "default features"
            } else {
                "no default features"
            },
            f["features"]
        );
    }
    for h in &r.hot_paths {
        println!(
            "hot path {} ({})",
            h.entry,
            h.function.as_deref().unwrap_or("not found")
        );
        match &h.declared_driver {
            None => println!("  S1/S2  not measured: no driver declared"),
            Some(d) => {
                println!("  driver input: {}", d.input);
                println!(
                    "  driver one iteration: {}; {} to warm up, {} counted",
                    h.iteration.as_deref().unwrap_or(""),
                    d.warm_up,
                    d.iterations
                );
            }
        }
        for m in &h.measured {
            let verdict = match (&h.declared_budget, m.within_budget) {
                (Some(b), Some(ok)) => format!(
                    "budget {} and {}: {}",
                    b.allocations_per_iteration,
                    b.bytes_per_iteration,
                    if ok { "within" } else { "OVER" }
                ),
                _ => "NO BUDGET DECLARED".to_string(),
            };
            let built = match (&m.constructor, &m.input) {
                (Some(c), Some(i)) => format!("state {c}, input {i}"),
                (Some(c), None) => format!("built by {c}"),
                (None, Some(i)) => format!("input {i}"),
                (None, None) => "no state".to_string(),
            };
            println!(
                "  S1/S2  size {:?}, {built}: {} allocations, {} bytes per iteration; {verdict}",
                m.size, m.measured.allocations_per_iteration, m.measured.bytes_per_iteration
            );
        }
        for u in &h.measurement_support {
            println!(
                "  written by the author for this measurement: {} at {}:{}",
                u["function"].as_str().unwrap_or(""),
                u["file"].as_str().unwrap_or(""),
                u["line"]
            );
        }
        if h.allocations_differ_between_sizes == Some(true) {
            println!("  S1/S2  the number of allocations changes with the input size");
        } else if h.bytes_differ_between_sizes == Some(true) {
            println!(
                "  S1/S2  the bytes change with the input size; the number of allocations does not"
            );
        }
        println!(
            "  S3     {} copying construct(s) in {} reachable function(s)",
            h.copying_constructs,
            h.functions.len()
        );
        for f in h
            .functions
            .iter()
            .filter(|f| !f.copying.is_empty() || !f.allocating_macros.is_empty())
        {
            let hits: Vec<String> = f
                .copying
                .iter()
                .chain(&f.allocating_macros)
                .map(|c| format!("{} :{}", c.what, c.line))
                .collect();
            let called = f.calls_on_the_hot_path.map_or(String::new(), |n| {
                format!("; returns a new collection and is called at {n} place(s) on the hot path")
            });
            println!(
                "         {} ({}): {}{called}",
                f.function,
                f.file,
                hits.join(", ")
            );
        }
        for t in &h.fields_taken_out_of_self {
            let state = match (&t.justification, t.put_back) {
                (Some(why), _) => format!("justified: {why}"),
                (None, true) => "put back, NO JUSTIFICATION".to_string(),
                (None, false) => "not put back".to_string(),
            };
            println!(
                "  S4     {}: self.{} by {} at {}:{} ({state})",
                t.function, t.field, t.how, t.file, t.line
            );
        }
        for t in &h.threads {
            let state = t
                .justification
                .as_deref()
                .map_or("NO JUSTIFICATION".to_string(), |w| {
                    format!("justified: {w}")
                });
            println!(
                "  S5     {}: {} at {}:{} ({state})",
                t.function, t.hit.what, t.hit.file, t.hit.line
            );
        }
        for n in &h.not_followed {
            println!("  not followed: {} at {}:{}", n.what, n.file, n.line);
        }
        if !h.measured.is_empty() {
            let declared = match &h.declared_same_algorithm {
                Some(s) => format!(
                    "declared by the author: {} algorithm; {}",
                    if s.same { "same" } else { "a different" },
                    s.statement
                ),
                None => "the author has not declared whether the algorithm is the same".to_string(),
            };
            let times: Vec<String> = h
                .measured
                .iter()
                .map(|m| {
                    format!(
                        "{:.1} at {:?}",
                        m.measured.nanoseconds_per_iteration, m.size
                    )
                })
                .collect();
            println!(
                "  S9     ns per iteration from Rust: {} ({declared})",
                times.join(", ")
            );
        }
    }
    for s in &r.shared_ownership {
        let owner = s
            .declared_owner
            .as_deref()
            .map_or("NO OWNER STATED".to_string(), |o| format!("owner: {o}"));
        println!(
            "S7     {} in {} at {}:{} ({owner})",
            s.found.construct, s.found.item, s.found.file, s.found.line
        );
    }
    let unsafe_src = r.unsafe_code["in_src"].as_array().map_or(0, Vec::len);
    let lint = r.unsafe_code["crate_lint"]
        .as_str()
        .map_or("the crate does not forbid it".to_string(), |l| {
            format!("the crate has #![{l}(unsafe_code)]")
        });
    println!(
        "unsafe {unsafe_src} in src, {} in tests; {lint}",
        r.unsafe_code["in_tests"].as_array().map_or(0, Vec::len)
    );
    for u in r.unsafe_code["in_src"].as_array().into_iter().flatten() {
        println!(
            "       {} at {}:{}",
            u["what"].as_str().unwrap_or(""),
            u["file"].as_str().unwrap_or(""),
            u["line"]
        );
    }
    for flag in &r.flags {
        println!("flag   {flag}");
    }
    println!("not checked by this report:");
    for line in r.not_checked {
        println!("  - {line}");
    }
    println!(
        "commit {}{}",
        r.git["commit"].as_str().unwrap_or(""),
        if r.git["dirty"] == true {
            " plus uncommitted changes"
        } else {
            ""
        }
    );
    println!("report {}", out.display());
    match r.verdict {
        "pass" => println!("PASS"),
        "pass_with_flags" => println!(
            "PASS WITH FLAGS: {} thing(s) for a reader to look at",
            r.flags.len()
        ),
        _ => println!("FAIL: {}", r.reasons.join("; ")),
    }
    ExitCode::from(r.exit_status)
}
