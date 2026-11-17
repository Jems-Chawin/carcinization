//! The command line of this tool, as a function the `carcinify` binary
//! calls.

use crate::report::{self, Options, REPORT_SCHEMA};
use std::path::PathBuf;
use std::process::ExitCode;

pub const USAGE: &str = "\
usage: carcinify decide <declaration.json> --report <report.json> [--markdown <report.md>]
                      [--python <interpreter>] [--allow-dirty]

Runs the declared workload of a Python project, several times with nothing
attached and then under a profiler, and writes where the time goes, what the
candidate piece holds of it, and whether the measurements contradict the
declared decision.

exit status: 0 nothing contradicts the decision; 1 a measurement contradicts
it, something needed is missing, or the machine was too busy to measure on;
2 the run could not be carried out";

/// The `decide` subcommand: its arguments, without the program's name.
pub fn main(mut args: impl Iterator<Item = String>) -> ExitCode {
    let (mut declaration, mut out, mut md) = (None, None, None);
    let mut python = PathBuf::from("python3");
    let mut allow_dirty = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--report" => out = args.next().map(PathBuf::from),
            "--markdown" => md = args.next().map(PathBuf::from),
            "--python" => python = args.next().map(PathBuf::from).unwrap_or(python),
            "--allow-dirty" => allow_dirty = true,
            flag if flag.starts_with('-') => {
                eprintln!("carcinify decide: unknown option {flag}\n\n{USAGE}");
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
    if let Some(md) = &md {
        let _ = std::fs::remove_file(md);
    }
    let write = |path: &PathBuf, text: String| {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    let pretty = |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default() + "\n";
    let r = match report::run(&Options {
        declaration,
        python,
        allow_dirty,
    }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("carcinify decide: {e}");
            let _ = write(
                &out,
                pretty(
                    &serde_json::json!({"schema": REPORT_SCHEMA, "verdict": "error", "exit_status": 2, "reasons": [e]}),
                ),
            );
            return ExitCode::from(2);
        }
    };
    let written = write(&out, pretty(&r)).and_then(|()| {
        md.as_ref()
            .map_or(Ok(()), |md| write(md, report::markdown(&r)))
    });
    if let Err(e) = written {
        eprintln!("carcinify decide: {e}");
        return ExitCode::from(2);
    }
    let t = &r["workload"]["measured_with_nothing_attached"];
    let g = &r["where_the_time_goes"];
    let pct = |v: &serde_json::Value| format!("{:.1}%", v.as_f64().unwrap_or(0.0) * 100.0);
    println!(
        "workload {} in {}, arguments {}: fastest of {4} runs {3:.3} s (they differ by {5} of the median)",
        r["workload"]["declared"]["function"].as_str().unwrap_or(""),
        r["workload"]["file"].as_str().unwrap_or(""),
        r["workload"]["declared"]["args"],
        t["wall_seconds"]["fastest"].as_f64().unwrap_or(0.0),
        r["workload"]["declared"]["repeat"],
        pct(&t["wall_seconds"]["spread_of_median"])
    );
    println!(
        "time: {} waiting, {} compiled code, {} library Python, {} the project's own Python{} ({})",
        pct(&g["shares"]["waiting"]),
        pct(&g["shares"]["compiled_code"]),
        pct(&g["shares"]["library_python"]),
        pct(&g["shares"]["own_python"]),
        if g["own_python_includes_work_inside_compiled_packages"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            " with work inside compiled packages that cannot be separated from it"
        } else if g["own_python_may_include_work_inside_loaded_packages"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            ", which may include work inside the compiled packages that are loaded"
        } else {
            ""
        },
        g["classification"].as_str().unwrap_or("")
    );
    let c = &r["candidate"];
    if c["ran_outside_the_main_thread"] == true {
        println!(
            "candidate {}: never ran on the main thread; its share is not known",
            c["declared"]["function"].as_str().unwrap_or("")
        );
    } else if !c.is_null() {
        let bounds = &c["own_python_share_bounds"];
        if bounds.is_null() {
            println!(
                "candidate {}: own Python {} of the time{}; {}; entered {} times",
                c["declared"]["function"].as_str().unwrap_or(""),
                pct(&c["own_python_share"]),
                c["own_python_share_holds_only_if"]
                    .as_str()
                    .map_or(String::new(), |t| format!(", if {t}")),
                match c["ceiling"]["the_whole_workload_could_be_at_most_this_many_times_faster"]
                    .as_f64()
                {
                    Some(x) => format!(
                        "were that to cost nothing, the workload would be at most {x} times faster"
                    ),
                    None => "nearly all of the time is this piece".to_string(),
                },
                c["boundary"]["entered_from_outside"]
            );
        } else {
            println!(
                "candidate {}: uses compiled packages {}; its own Python cannot be separated from their work: at most {} of the time, at least {}; entered {} times",
                c["declared"]["function"].as_str().unwrap_or(""),
                c["uses_compiled_packages"],
                pct(&bounds["at_most"]),
                bounds["at_least"]
                    .as_f64()
                    .map_or("not known".to_string(), |l| format!(
                        "about {:.1}% if the stand-ins replace all of the package's work",
                        l * 100.0
                    )),
                c["boundary"]["entered_from_outside"]
            );
        }
    }
    let m = &r["language_or_algorithm"]["alternative_measured"];
    if !m.is_null() {
        println!("alternative in Python: {} times faster", m["times_faster"]);
    }
    match r["declared_decision"]["choice"].as_str() {
        Some(choice) => println!("decision declared by the author: {choice}"),
        None => println!("no decision declared"),
    }
    for (label, key) in [
        ("BUSY", "machine_was_busy"),
        ("flag", "flags"),
        ("missing", "reasons"),
        ("CONTRADICTED", "contradictions"),
    ] {
        for item in r[key].as_array().into_iter().flatten() {
            println!("{label}: {}", item.as_str().unwrap_or(""));
        }
    }
    println!("report {}", out.display());
    match r["verdict"].as_str() {
        Some("consistent") => println!("CONSISTENT: no measurement contradicts the decision"),
        Some("contradicted") => println!("CONTRADICTED"),
        Some("consistent_if_the_stand_ins_are_faithful") => println!(
            "CONSISTENT ONLY IF THE STAND-INS ARE FAITHFUL: the candidate uses a compiled package, and nothing but the author's stand-in workload shows that its time is its own Python"
        ),
        Some("consistent_on_the_authors_word") => println!(
            "CONSISTENT ON THE AUTHOR'S WORD: a compiled package is loaded, the tool saw the candidate use none and cannot be sure, and the author states that it uses none"
        ),
        Some("unreliable") => println!(
            "UNRELIABLE: the machine was busy while this was measured; measure again when it is quiet"
        ),
        _ => println!("INCOMPLETE"),
    }
    ExitCode::from(r["exit_status"].as_u64().unwrap_or(1) as u8)
}
