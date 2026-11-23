//! `carcinify speed --compare`: two reports of one commit, each from its
//! own platform, side by side (design §7). It judges nothing: its flags
//! say where to look before a figure is quoted for another platform.

use serde_json::Value as J;
use std::process::ExitCode;

/// How far apart, max ÷ min, the two median ratios of a target may be
/// before it is flagged platform_dependent. A first value, and arbitrary
/// (design §7).
const APART: f64 = 1.25;

/// `--compare <A.json> <B.json>`: the comparison on standard output, exit
/// 0, flags or not; exit 2 when the reports cannot be compared. An error in
/// the arguments themselves is returned, for the usage.
pub fn main(args: impl Iterator<Item = String>) -> Result<ExitCode, String> {
    let paths: Vec<String> = args.collect();
    let [a, b] = paths.as_slice() else {
        return Err("--compare needs two reports".into());
    };
    match read(a).and_then(|a| compare(&a, &read(b)?)) {
        Ok(text) => {
            print!("{text}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("carcinify speed: {e}");
            Ok(ExitCode::from(2))
        }
    }
}

fn read(path: &str) -> Result<J, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let report: J = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    if report["schema"] != "carcinify/speed/report/1" {
        return Err(format!(
            "{path} is not a carcinify speed report (its schema is {})",
            report["schema"]
        ));
    }
    Ok(report)
}

/// The two reports side by side, every figure with its platform; or why
/// they cannot be compared: other code on each side is not a platform
/// difference.
fn compare(a: &J, b: &J) -> Result<String, String> {
    let differ: Vec<String> = [
        ("commit", "/run/commit"),
        ("dirty state (dirty_sha256)", "/run/dirty_sha256"),
        ("declaration (declaration_sha256)", "/declaration_sha256"),
        ("suite (suite_sha256)", "/run/suite_sha256"),
        // RUST-181: other measuring code is not a platform difference.
        (
            "measuring code (tools.embedded_sha256)",
            "/tools/embedded_sha256",
        ),
        (
            "carcinify commit (tools.carcinify_commit)",
            "/tools/carcinify_commit",
        ),
        (
            "carcinify's uncommitted changes (tools.carcinify_dirty)",
            "/tools/carcinify_dirty",
        ),
    ]
    .iter()
    .filter_map(|(what, at)| {
        let (x, y) = (&a.pointer(at), &b.pointer(at));
        (x != y).then(|| format!("the {what} is {} in A and {} in B", shown(x), shown(y)))
    })
    .collect();
    if !differ.is_empty() {
        return Err(format!(
            "the two reports are of different code, so their difference would not be the platforms': {}. Compare two runs of one commit, one on each platform",
            differ.join("; ")
        ));
    }
    let (la, lb) = (label(a, "A"), label(b, "B"));
    let mut out = String::new();
    let mut flags = Vec::new();
    for (r, l, which) in [(a, &la, "A"), (b, &lb, "B")] {
        let verdict = text(&r["verdict"]);
        out += &format!(
            "{l}: {}; {}; {}; carcinify {}; verdict {verdict}\n",
            text(&r["machine"]["cpu"]),
            interpreter(r),
            build(r),
            text(&r["tools"]["harness_version"]),
        );
        // So that "none" is never printed under a report that did not pass.
        if !matches!(verdict, "pass" | "pass_with_flags") {
            flags.push(format!("verdict: {which} {verdict}"));
        }
    }
    if interpreter(a) != interpreter(b) {
        flags.push(format!(
            "interpreters_differ: {} on {la}; {} on {lb}",
            interpreter(a),
            interpreter(b)
        ));
    }
    // RUST-187: other rustc flags make two builds, whatever the target CPU.
    let pyo3 = |r: &J| r["build"]["sides"]["candidate"]["kind"] == "pyo3";
    let builds_differ = (pyo3(a) || pyo3(b)) && rustflags(a) != rustflags(b);
    if builds_differ {
        flags.push(format!(
            "builds_differ: {} on {la}; {} on {lb}",
            rustflags(a),
            rustflags(b)
        ));
    }
    let targets = |r: &J| r["targets"].as_array().cloned().unwrap_or_default();
    let (ta, tb) = (targets(a), targets(b));
    let mut names: Vec<&str> = ta.iter().filter_map(|t| t["name"].as_str()).collect();
    for t in &tb {
        let name = t["name"].as_str().unwrap_or_default();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let find = |ts: &[J], name: &str| ts.iter().find(|t| t["name"] == name).cloned();
    for name in names {
        out += &format!("\n{name}\n");
        let (fa, fb) = (find(&ta, name), find(&tb, name));
        let (ra, rb) = (
            figures(&mut out, &la, fa.as_ref()),
            figures(&mut out, &lb, fb.as_ref()),
        );
        let (Some((ma, loa, hia)), Some((mb, lob, hib))) = (ra, rb) else {
            continue;
        };
        let apart = ma.max(mb) / ma.min(mb);
        let line = format!(
            "{} on {la} and {} on {lb}, {} apart",
            number(ma),
            number(mb),
            apart_text(apart)
        );
        if apart > APART {
            // With two builds, the platform cannot be named as the cause.
            let (what, flag) = if builds_differ {
                (
                    "builds and platforms both differ",
                    "builds_and_platforms_differ",
                )
            } else {
                ("platform_dependent", "platform_dependent")
            };
            out += &format!(
                "  {what}: {line}, over {APART}; per pair {} to {} on {la} and {} to {} on {lb}; {} on {la} and {} on {lb}\n",
                number(loa),
                number(hia),
                number(lob),
                number(hib),
                target(a),
                target(b)
            );
            flags.push(format!("{flag}: {name}"));
        } else {
            out += &format!("  {line}, within {APART}\n");
        }
    }
    out += "\nflags:\n";
    if flags.is_empty() {
        flags.push("none".into());
    }
    for flag in flags {
        out += &format!("  {flag}\n");
    }
    Ok(out)
}

/// A report's platform, by which every figure from it is printed.
fn label(r: &J, which: &str) -> String {
    format!(
        "{} {} ({which})",
        text(&r["machine"]["os"]),
        text(&r["tools"]["arch"])
    )
}

/// Prints the target's line for one platform, and gives its median ratio
/// and the lowest and highest pair's when it was timed.
fn figures(out: &mut String, label: &str, t: Option<&J>) -> Option<(f64, f64, f64)> {
    let Some(t) = t else {
        *out += &format!("  {label}: not in this report\n");
        return None;
    };
    if let Some(why) = t["failed"].as_str() {
        *out += &format!("  {label}: failed: {}\n", why.lines().next().unwrap_or(""));
        return None;
    }
    let r = &t["ratio"];
    let (Some(m), Some(lo), Some(hi)) =
        (r["median"].as_f64(), r["low"].as_f64(), r["high"].as_f64())
    else {
        *out += &format!("  {label}: not measured: no pair of runs was kept\n");
        return None;
    };
    let time = |side: &str| t[side]["median"].as_f64().map_or("unknown".into(), time);
    *out += &format!(
        "  {label}: original {}, port {} per call; ratio original ÷ port {}, per pair {} to {}\n",
        time("original"),
        time("port"),
        number(m),
        number(lo),
        number(hi)
    );
    Some((m, lo, hi))
}

fn interpreter(r: &J) -> String {
    let p = &r["python"];
    let threads = if p["gil"] == false {
        ", free-threaded"
    } else {
        ""
    };
    format!(
        "Python {} {}{threads}, NumPy {}",
        text(&p["version"]),
        text(&p["implementation"]),
        p["numpy"].as_str().unwrap_or("none")
    )
}

/// RUST-151: what the port was built for, so that a build difference
/// reads as one before the platform is blamed.
fn build(r: &J) -> String {
    match r["build"]["sides"]["candidate"]["kind"].as_str() {
        Some("pyo3") => format!("the port built for {}", target(r)),
        _ => "a Python port".into(),
    }
}

/// The build's target CPU and feature count, or that the port is Python.
fn target(r: &J) -> String {
    let side = &r["build"]["sides"]["candidate"];
    if side["kind"] != "pyo3" {
        return "a Python port".into();
    }
    format!(
        "target CPU {}, {} target features, {}",
        text(&side["target"]["target_cpu"]),
        text(&side["target"]["target_features"]),
        rustflags(r)
    )
}

/// RUST-187: the rustc flags the port was built with, as the build
/// recorded them from its environment.
fn rustflags(r: &J) -> String {
    let env = &r["tools"]["build_environment"];
    let set: Vec<String> = [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
    ]
    .iter()
    .filter_map(|name| env[*name].as_str().map(|v| format!("{name}={v:?}")))
    .collect();
    if set.is_empty() {
        "no RUSTFLAGS".into()
    } else {
        set.join(", ")
    }
}

fn text(j: &J) -> &str {
    j.as_str().unwrap_or("unknown")
}

fn shown(j: &Option<&J>) -> String {
    j.map_or("nothing".into(), J::to_string)
}

/// Three significant figures, as a ratio is read, at every size (RUST-182).
fn number(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return x.to_string();
    }
    let digits = (2 - x.abs().log10().floor() as i32).max(0) as usize;
    format!("{x:.digits$}")
}

/// How far apart two ratios are: 3 decimals, and more where 3 would print
/// the threshold itself, so that a value is never shown on the wrong side
/// of it (RUST-182).
fn apart_text(x: f64) -> String {
    (3..=9)
        .map(|digits| format!("{x:.digits$}"))
        .find(|s| x == APART || s.parse::<f64>() != Ok(APART))
        .unwrap_or_else(|| x.to_string())
}

fn time(seconds: f64) -> String {
    let (scale, unit) = [(1.0, "s"), (1e-3, "ms"), (1e-6, "µs")]
        .into_iter()
        .find(|(scale, _)| seconds >= *scale)
        .unwrap_or((1e-9, "ns"));
    format!("{} {unit}", number(seconds / scale))
}
