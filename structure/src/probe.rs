//! Measures allocations per iteration of a hot path by running it.
//!
//! The declaration gives, for each hot entry point, a driver. A driver is
//! data, not code: the path of a constructor, literal arguments for it at
//! two or more input sizes, and the call that is one iteration. This module
//! writes the measuring program from those pieces. No text from the
//! declaration is pasted into it as Rust: a path has to look like a path,
//! an argument has to be a number, a boolean or a string, and nothing else
//! gets through.
//!
//! The program prints one result line for each driver and size. Each line
//! carries a token made up for this run, and exactly one line with it is
//! accepted per measurement, so output from the port itself cannot stand in
//! for a result.

use carcinify_diff::python::fresh::Build;
use serde::{Deserialize, Serialize};
use serde_json::Value as J;

use std::path::{Path, PathBuf};
use std::process::Command;

/// The one module of a port where code written for the measurement lives.
pub const SUPPORT: &str = "measure_support";

/// How to exercise one hot entry point, as declared in the design note.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Driver {
    /// The input, in words: what it is and why it is representative.
    pub input: String,
    /// The function that builds the state: a function of the entry point's
    /// own type (`crate_name::path::Type::new`), or one in the crate's
    /// `measure_support` module. Left out for a free function that takes no
    /// state.
    pub construct: Option<String>,
    /// The input sizes. At least two, so that growth with the input shows,
    /// unless `one_size_reason` says why the input has no size.
    pub sizes: Vec<Size>,
    /// Why the input has no size, when only one is given.
    pub one_size_reason: Option<String>,
    /// One iteration of the hot path.
    pub call: Call,
    /// Iterations run before counting starts.
    pub warm_up: u64,
    /// Iterations counted.
    pub iterations: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    /// The author's name for this size: `8 units`.
    pub label: String,
    /// The constructor's arguments at this size.
    #[serde(default)]
    pub args: Vec<J>,
    /// An input for the call, built once by a function of
    /// `measure_support`, for a hot path that takes one beside its state.
    pub input: Option<Support>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Support {
    /// The function's name in `measure_support`.
    pub function: String,
    #[serde(default)]
    pub args: Vec<J>,
    /// A comparison case of carcinify diff: its `args`, as the text in its
    /// file, are passed to the function as one `&str`, in place of `args`.
    pub case: Option<CaseRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRef {
    /// The case file, relative to the declaration.
    pub file: String,
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    /// The entry point, when it is a method: its name.
    pub method: Option<String>,
    /// The entry point, when it is a free function: its path.
    pub function: Option<String>,
    /// For a free function, how it takes the constructed value as its first
    /// argument: `exclusive` (`&mut state`), `shared` (`&state`) or `none`.
    pub state: Option<String>,
    /// Further arguments: numbers, booleans, strings, or
    /// `{"input": "shared"}` / `{"input": "exclusive"}` for the size's
    /// input, passed as `&input` or `&mut input`.
    #[serde(default)]
    pub args: Vec<J>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measured {
    pub allocations_per_iteration: f64,
    pub bytes_per_iteration: f64,
    /// The same, during the warm-up iterations. A hot path that allocates
    /// while warming up and not afterwards has settled, or has run out of
    /// work; this number lets a reader ask which.
    pub warm_up_allocations_per_iteration: f64,
    /// From the same run, release build, best of five passes, the passes
    /// taken in turn with those of the other sizes. One machine,
    /// no statistics.
    pub nanoseconds_per_iteration: f64,
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A path of identifiers that starts with the crate's own name. Keywords
/// that would reach somewhere else (`crate`, `self`, `super`, `std`) cannot
/// be the first segment, since the first segment must be the crate.
fn path(text: &str, krate: &str, what: &str) -> Result<String, String> {
    let segments: Vec<&str> = text.split("::").collect();
    if segments.len() < 2 || !segments.iter().all(|s| is_ident(s)) {
        return Err(format!(
            "{what} {text:?} is not a path of names separated by `::`"
        ));
    }
    if segments[0] != krate {
        return Err(format!(
            "{what} {text:?} must start with the crate's name, `{krate}`: the driver may only call into the crate being measured"
        ));
    }
    Ok(segments.join("::"))
}

/// A JSON number, boolean or string as a Rust literal.
fn literal(value: &J) -> Result<String, String> {
    match value {
        J::Bool(b) => Ok(b.to_string()),
        J::Number(n) if n.is_i64() || n.is_u64() => Ok(n.to_string()),
        J::Number(n) => Ok(format!("{:?}_f64", n.as_f64().unwrap_or(0.0))),
        // `{:?}` of a string is a Rust string literal with everything
        // escaped, so no character in it can end the literal.
        J::String(s) => Ok(format!("{s:?}")),
        other => Err(format!(
            "driver argument {other} is not a number, a boolean or a string. Richer input is built by a function in the crate's `{SUPPORT}` module"
        )),
    }
}

fn literals(values: &[J]) -> Result<String, String> {
    Ok(values
        .iter()
        .map(literal)
        .collect::<Result<Vec<_>, _>>()?
        .join(", "))
}

/// What the tool knows of the declared entry point from reading the crate.
pub struct Entry<'a> {
    /// The crate's name as Rust code uses it.
    pub krate: &'a str,
    /// `module::Type::function` or `module::function`, as analysed.
    pub name: &'a str,
    /// The entry point as an expression another crate can call, by a
    /// public path: `krate::Type::function`, or
    /// `<krate::Type as krate::Trait>::function`.
    pub callable: &'a str,
    pub short: &'a str,
    /// The type it is a method of, if any.
    pub self_ty: Option<&'a str>,
    /// `exclusive`, `shared` or `value` for a method; `None` for a function
    /// with no `self`.
    pub receiver: Option<&'a str>,
    /// Names of the functions in the crate's `measure_support` module.
    pub support: &'a [String],
}

/// A checked driver, as the Rust the tool writes from it.
#[derive(Debug, PartialEq)]
pub struct Built {
    /// Per size: label, the expression that builds the state (if any), the
    /// expression that builds the input (if any).
    pub sizes: Vec<(String, Option<String>, Option<String>)>,
    pub iteration: String,
    /// Functions of `measure_support` the driver uses.
    pub support_used: Vec<String>,
}

impl Driver {
    /// Checks the driver against the entry point and writes the Rust.
    ///
    /// The call is never taken from the declaration: it is the entry
    /// point's own path, as the tool read it from the crate, with the state
    /// as its first argument. So the state has to be of the entry point's
    /// type, whatever built it, and a function of the same name somewhere
    /// else cannot stand in.
    pub fn build(&self, e: &Entry) -> Result<Built, String> {
        let krate = e.krate;
        let support = format!("{krate}::{SUPPORT}::");
        let mut support_used: Vec<String> = Vec::new();
        let mut in_support = |name: &str, what: &str| -> Result<String, String> {
            if !is_ident(name) {
                return Err(format!("{what} {name:?} is not a name"));
            }
            if !e.support.iter().any(|f| f == name) {
                return Err(format!(
                    "{what} `{name}` is not a function of the module `{krate}::{SUPPORT}`, where code written for the measurement has to live"
                ));
            }
            if !support_used.iter().any(|f| f == name) {
                support_used.push(name.to_string());
            }
            Ok(format!("{support}{name}"))
        };

        match (self.sizes.len(), &self.one_size_reason) {
            (0, _) => return Err("the driver gives no input size".to_string()),
            (1, Some(reason)) if !reason.trim().is_empty() => {}
            (1, _) => return Err("the driver gives one input size; at least two are needed, so that growth with the input can be seen. If the input has no size, say why in `one_size_reason`".to_string()),
            (_, Some(_)) => return Err("the driver gives `one_size_reason` and more than one size".to_string()),
            _ => {}
        }
        if self.iterations == 0 {
            return Err("the driver counts 0 iterations".to_string());
        }

        // The state.
        let construct = match &self.construct {
            None => None,
            Some(text) => {
                let whole = path(text, krate, "the constructor (`construct`)")?;
                Some(match whole.strip_prefix(&support) {
                    Some(name) => in_support(name, "the constructor (`construct`)")?,
                    None => {
                        let owner = whole.rsplit("::").nth(1).unwrap_or("");
                        let is_type = owner.chars().next().is_some_and(char::is_uppercase);
                        if !is_type || e.self_ty.is_some_and(|t| t != owner) {
                            return Err(format!(
                                "the constructor (`construct`) {text:?} has to be a function of {}, or a function in `{krate}::{SUPPORT}`",
                                e.self_ty
                                    .map_or("a type of the crate".to_string(), |t| format!(
                                        "the entry point's own type `{t}`"
                                    ))
                            ));
                        }
                        whole
                    }
                })
            }
        };

        // The call's arguments.
        let mut args = Vec::new();
        for arg in &self.call.args {
            args.push(match arg.get("input").map(|i| i.as_str()) {
                Some(Some("shared")) if arg.as_object().is_some_and(|o| o.len() == 1) => "&input".to_string(),
                Some(Some("exclusive")) if arg.as_object().is_some_and(|o| o.len() == 1) => "&mut input".to_string(),
                Some(_) => return Err(format!("the call argument {arg} must be {{\"input\": \"shared\"}} or {{\"input\": \"exclusive\"}}")),
                None => literal(arg)?,
            });
        }
        // The sizes.
        let wants_input = self.call.args.iter().any(|a| a.get("input").is_some());
        let mut sizes = Vec::new();
        for (n, size) in self.sizes.iter().enumerate() {
            let same = |a: &Size| {
                a.args == size.args
                    && a.input.as_ref().map(|i| (&i.function, &i.args))
                        == size.input.as_ref().map(|i| (&i.function, &i.args))
            };
            if self.sizes[..n]
                .iter()
                .any(|s| s.label == size.label || same(s))
            {
                return Err(format!(
                    "the driver's size {:?} repeats the label or the arguments of another; each size has to be a different input",
                    size.label
                ));
            }
            let state = match &construct {
                Some(c) => Some(format!("{c}({})", literals(&size.args)?)),
                None if size.args.is_empty() => None,
                None => {
                    return Err(format!(
                        "size {:?} has arguments and the driver names no constructor to give them to",
                        size.label
                    ));
                }
            };
            let input = match &size.input {
                Some(i) => Some(format!(
                    "{}({})",
                    in_support(&i.function, "the input function")?,
                    literals(&i.args)?
                )),
                None => None,
            };
            if wants_input != input.is_some() {
                return Err(format!(
                    "size {:?}: the call {} an input and the size {} one",
                    size.label,
                    if wants_input {
                        "takes"
                    } else {
                        "does not take"
                    },
                    if input.is_some() {
                        "builds"
                    } else {
                        "does not build"
                    }
                ));
            }
            sizes.push((size.label.clone(), state, input));
        }

        // The call.
        let entry = e.callable.to_string();
        let written = format!("{krate}::{}", e.name);
        let state = match (&self.call.method, &self.call.function, e.self_ty) {
            (Some(method), None, Some(ty)) => {
                if !is_ident(method) {
                    return Err(format!("the driver's method {method:?} is not a name"));
                }
                if method != e.short {
                    return Err(format!(
                        "the driver calls the method `{method}`, which is not the declared entry point `{}`: the measurement has to be of the hot path itself",
                        e.short
                    ));
                }
                if construct.is_none() {
                    return Err(format!(
                        "the entry point is a method of `{ty}` and the driver names no constructor for one"
                    ));
                }
                match e.receiver {
                    Some("exclusive") => "&mut state",
                    Some("shared") => "&state",
                    _ => {
                        return Err(format!(
                            "the entry point `{}` takes `self` by value, so it cannot be called again and again on one state. Declare the function that loops over it as the hot path",
                            e.name
                        ));
                    }
                }
            }
            (Some(method), None, None) => {
                return Err(format!(
                    "the driver calls a method `{method}`, and the declared entry point `{}` is a free function: use `function`",
                    e.name
                ));
            }
            (None, Some(function), None) => {
                let function = path(function, krate, "the driver's function")?;
                if function != entry && function != written {
                    return Err(format!(
                        "the driver calls `{function}`, which is not the declared entry point `{entry}`"
                    ));
                }
                match (self.call.state.as_deref(), construct.is_some()) {
                    (Some("exclusive"), true) => "&mut state",
                    (Some("shared"), true) => "&state",
                    (Some("none"), false) => "",
                    (Some("none"), true) => {
                        return Err(
                            "the driver names a constructor and the call does not use the state"
                                .to_string(),
                        );
                    }
                    (Some("exclusive" | "shared"), false) => {
                        return Err(
                            "the call takes the state and the driver names no constructor"
                                .to_string(),
                        );
                    }
                    (other, _) => {
                        return Err(format!(
                            "the driver's `state` is {other:?}; it must be \"exclusive\", \"shared\" or \"none\""
                        ));
                    }
                }
            }
            (None, Some(function), Some(ty)) => {
                return Err(format!(
                    "the driver calls a function `{function}`, and the declared entry point is the method `{}` of `{ty}`: use `method`",
                    e.short
                ));
            }
            _ => {
                return Err(
                    "the driver's call needs exactly one of `method` and `function`".to_string(),
                );
            }
        };
        let all: Vec<&str> = std::iter::once(state)
            .chain(args.iter().map(String::as_str))
            .filter(|s| !s.is_empty())
            .collect();
        Ok(Built {
            sizes,
            iteration: format!("{entry}({})", all.join(", ")),
            support_used,
        })
    }
}

/// Everything in this module of the generated program is private to it.
/// The port is another crate and cannot name any of it.
const ALLOCATOR: &str = r#"
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    pub struct Counting;
    static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static BYTES: AtomicU64 = AtomicU64::new(0);

    // Process-wide, so threads the hot path starts are counted too.
    // `realloc` counts as one allocation of the new size.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Relaxed);
            BYTES.fetch_add(new_size as u64, Relaxed);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    pub fn now() -> (u64, u64) {
        (ALLOCATIONS.load(Relaxed), BYTES.load(Relaxed))
    }
}

#[global_allocator]
static GLOBAL: counting::Counting = counting::Counting;
"#;

/// One measurement to make.
pub struct Job {
    /// The hot path and size it belongs to, for messages.
    pub entry: String,
    pub size: String,
    /// The expression that builds the state, if there is one.
    pub constructor: Option<String>,
    /// The expression that builds the input, if there is one.
    pub input: Option<String>,
    pub iteration: String,
    pub warm_up: u64,
    pub iterations: u64,
}

/// The probe program's source, and for each line that carries a piece of a
/// declaration: the line number, the job, and the field it came from.
pub fn source(jobs: &[Job], token: &str) -> (String, Vec<(usize, usize, &'static str)>) {
    let mut out = String::from(
        "// Written by carcinify structure. Not part of the port.\n#![allow(unused, clippy::all)]\n",
    );
    out.push_str(ALLOCATOR);
    let mut map = Vec::new();
    for (n, job) in jobs.iter().enumerate() {
        let mut line = |out: &mut String, text: &str, field: Option<&'static str>| {
            if let Some(field) = field {
                map.push((out.lines().count() + 1, n, field));
            }
            out.push_str(text);
            out.push('\n');
        };
        let iteration = format!("        std::hint::black_box({});", job.iteration);
        let (warm, count) = (job.warm_up, job.iterations);
        line(
            &mut out,
            &format!("\nfn probe_{n}() -> (f64, f64, f64, Box<dyn FnMut() -> f64>) {{"),
            None,
        );
        line(
            &mut out,
            &format!(
                "    let mut state = {};",
                job.constructor.as_deref().unwrap_or("()")
            ),
            Some("construct"),
        );
        line(
            &mut out,
            &format!(
                "    let mut input = {};",
                job.input.as_deref().unwrap_or("()")
            ),
            Some("input"),
        );
        line(&mut out, "    let start = counting::now();", None);
        line(&mut out, &format!("    for _ in 0..{warm}u64 {{"), None);
        line(&mut out, &iteration, Some("call"));
        line(&mut out, "    }\n    let before = counting::now();", None);
        line(&mut out, &format!("    for _ in 0..{count}u64 {{"), None);
        line(&mut out, &iteration, Some("call"));
        line(
            &mut out,
            "    }\n    let after = counting::now();\n    // One timed pass. `main` calls it in turn with the other sizes.\n    let pass = move || {\n        let clock = std::time::Instant::now();",
            None,
        );
        line(
            &mut out,
            &format!("        for _ in 0..{count}u64 {{"),
            None,
        );
        line(&mut out, &format!("    {iteration}"), Some("call"));
        line(
            &mut out,
            &format!(
                "        }}\n        clock.elapsed().as_nanos() as f64 / {count} as f64\n    }};\n    (\n        (after.0 - before.0) as f64 / {count} as f64,\n        (after.1 - before.1) as f64 / {count} as f64,\n        (before.0 - start.0) as f64 / {} as f64,\n        Box::new(pass),\n    )\n}}",
                warm.max(1)
            ),
            None,
        );
    }
    // Allocations are counted one measurement after the other. The time is
    // taken in five rounds over all of them, best of five each, so that
    // whatever else the machine is doing falls on every size alike.
    out.push_str("\nfn main() {\n    let mut jobs = vec![\n");
    for n in 0..jobs.len() {
        out.push_str(&format!("        probe_{n}(),\n"));
    }
    out.push_str(&format!(
        "    ];\n    let mut best = vec![f64::MAX; jobs.len()];\n    for _ in 0..5 {{\n        for (n, job) in jobs.iter_mut().enumerate() {{\n            best[n] = best[n].min((job.3)());\n        }}\n    }}\n    for (n, job) in jobs.iter().enumerate() {{\n        println!(\n            \"{token} {{n}} {{{{\\\"allocations_per_iteration\\\": {{}}, \\\"bytes_per_iteration\\\": {{}}, \\\"warm_up_allocations_per_iteration\\\": {{}}, \\\"nanoseconds_per_iteration\\\": {{}}}}}}\",\n            job.0, job.1, job.2, best[n]\n        );\n    }}\n}}\n"
    ));
    (out, map)
}

/// Turns the compiler's complaint about the probe into one about the
/// declaration: which hot path, which field, and what the compiler said.
/// `None` when no error is in a line that came from a declaration.
fn explain(stderr: &str, jobs: &[Job], map: &[(usize, usize, &'static str)]) -> Option<String> {
    let lines: Vec<&str> = stderr.lines().collect();
    for (at, first) in lines.iter().enumerate() {
        if !first.starts_with("error") {
            continue;
        }
        let block: Vec<&str> = lines[at..]
            .iter()
            .copied()
            .take_while(|l| !l.trim().is_empty())
            .collect();
        let place = block
            .iter()
            .find_map(|l| l.trim_start().strip_prefix("--> "))?;
        let mut parts = place.rsplitn(3, ':');
        let (_column, line, file) = (
            parts.next()?,
            parts.next()?.parse::<usize>().ok()?,
            parts.next()?,
        );
        if !file.ends_with("src/main.rs") {
            continue;
        }
        let Some((_, job, field)) = map.iter().find(|(l, _, _)| *l == line) else {
            continue;
        };
        let j = &jobs[*job];
        let (field, written) = match *field {
            "construct" => (
                "`construct`, with the arguments of that size",
                j.constructor.clone().unwrap_or_default(),
            ),
            "input" => ("the size's `input`", j.input.clone().unwrap_or_default()),
            _ => ("`call`", j.iteration.clone()),
        };
        let said: Vec<&str> = block
            .iter()
            .copied()
            .filter(|l| !l.trim_start().starts_with("--> "))
            .take(12)
            .collect();
        return Some(format!(
            "the driver declared for hot path {} does not compile, at size {:?}. The fault is in {field}, from which the tool wrote:\n    {written}\nThe compiler says:\n  {}\nCorrect the declaration; the measuring program is written from it and is not yours to edit.",
            j.entry,
            j.size,
            said.join("\n  ")
        ));
    }
    None
}

/// `(name, version, checksum-or-source)` of every package in a lock file.
fn locked(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for block in text.split("[[package]]").skip(1) {
        let field = |key: &str| {
            let line = block
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("{key} = ")));
            line.and_then(|l| l.split('"').nth(1))
                .unwrap_or("")
                .to_string()
        };
        out.push((
            field("name"),
            field("version"),
            format!("{}{}", field("source"), field("checksum")),
        ));
    }
    out
}

pub struct Place<'a> {
    pub repo_root: &'a Path,
    pub crate_dir: &'a Path,
    pub workspace_dir: &'a Path,
    pub package: &'a str,
    pub target_dir: &'a Path,
    /// Cargo features of the port to build with.
    pub features: &'a [String],
    pub no_default_features: bool,
}

/// A name for a feature set that can be part of a directory name. One
/// build directory, and so one build record, per feature set: a build with
/// other features is another build.
pub fn feature_key(features: &[String], no_default_features: bool) -> String {
    let mut sorted: Vec<&str> = features.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let text = format!(
        "{}|{}",
        if no_default_features {
            "no-default"
        } else {
            "default"
        },
        sorted.join(",")
    );
    carcinify_diff::provenance::sha256_bytes(text.as_bytes())[..12].to_string()
}

/// What a run of the probe gives back besides the numbers.
pub struct Ran {
    pub results: Vec<Measured>,
    pub command: Vec<String>,
    /// True when the crate was cleaned first because its sources were not
    /// those of the last recorded build.
    pub rebuilt_from_clean: bool,
    /// `same`, or what differed, or that the port has no lock file.
    pub lock_file: String,
}

/// Builds and runs the probe. One result per job, in order.
pub fn run(place: &Place, jobs: &[Job]) -> Result<Ran, String> {
    let key = feature_key(place.features, place.no_default_features);
    let dir = place.target_dir.join(format!("probe-{key}"));
    std::fs::create_dir_all(dir.join("src"))
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let manifest = format!(
        "[package]\nname = \"carcinify-structure-probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{} = {{ path = {:?}, default-features = {}, features = {:?} }}\n\n[workspace]\n",
        place.package,
        place.crate_dir.to_string_lossy(),
        !place.no_default_features,
        place.features
    );
    // A token nobody could have known when the port or the declaration was
    // written: only the line that carries it is a result.
    let seed = format!(
        "{:?} {} {}",
        std::time::SystemTime::now(),
        std::process::id(),
        dir.display()
    );
    let token = format!(
        "MEASURED-{}",
        &carcinify_diff::provenance::sha256_bytes(seed.as_bytes())[..24]
    );
    let write = |path: PathBuf, text: &str| {
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    write(dir.join("Cargo.toml"), &manifest)?;
    let (program, map) = source(jobs, &token);
    write(dir.join("src/main.rs"), &program)?;

    // The probe starts from the port's lock file, so that it measures the
    // dependency versions the port is built and compared with.
    let lock_path = place.workspace_dir.join("Cargo.lock");
    let port_lock = std::fs::read_to_string(&lock_path).map_err(|_| {
        format!("the port has no {}. Commit one: the measurement has to use the dependency versions the port is built and compared with", lock_path.display())
    })?;
    write(dir.join("Cargo.lock"), &port_lock)?;

    // Cargo rebuilds by file time. The harness's build record goes by
    // content, and cleans the crate's packages when the sources are not
    // those of the last recorded build.
    let build_dir = place.target_dir.join(format!("build-{key}"));
    let record = Build {
        repo_root: place.repo_root,
        crate_dir: place.crate_dir,
        workspace_dir: place.workspace_dir,
        target_dir: &build_dir,
        local: std::cell::OnceCell::new(),
    };
    let rebuilt_from_clean = record.clean_if_inputs_changed()?;

    let manifest_path = dir.join("Cargo.toml").to_string_lossy().into_owned();
    let target = build_dir.to_string_lossy().into_owned();
    let base = [
        "run",
        "--release",
        "--quiet",
        "--manifest-path",
        &manifest_path,
        "--target-dir",
        &target,
    ];
    let mut last = String::new();
    // Offline first: the run should not need a network.
    for extra in [&["--offline"][..], &[][..]] {
        let command: Vec<String> = std::iter::once("cargo")
            .chain(base)
            .chain(extra.iter().copied())
            .map(str::to_string)
            .collect();
        // From the crate's directory, so its rust-toolchain.toml applies.
        let out = Command::new("cargo")
            .args(&command[1..])
            .current_dir(place.crate_dir)
            .output()
            .map_err(|e| format!("cannot run cargo: {e}"))?;
        if !out.status.success() {
            last = String::from_utf8_lossy(&out.stderr).into_owned();
            continue;
        }
        record.record()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = text
            .lines()
            .filter_map(|l| l.strip_prefix(&format!("{token} ")))
            .collect();
        if lines.len() != jobs.len() {
            return Err(format!(
                "the measuring program printed {} result line(s) for {} measurement(s); the result is not trusted",
                lines.len(),
                jobs.len()
            ));
        }
        let mut results = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let parsed = line
                .strip_prefix(&format!("{n} "))
                .and_then(|l| serde_json::from_str::<Measured>(l).ok());
            results.push(parsed.ok_or_else(|| format!("result line {n} of the measuring program is out of order or malformed; the result is not trusted"))?);
        }
        // The versions the probe was built with must be the port's.
        // Cargo adds the probe itself to the copied lock file, which is
        // why `--locked` cannot be passed. Anything else it added or
        // changed is a version the port is not built with.
        let probe_lock = std::fs::read_to_string(dir.join("Cargo.lock"))
            .map_err(|e| format!("cannot read the probe's Cargo.lock: {e}"))?;
        let port = locked(&port_lock);
        let strangers: Vec<String> = locked(&probe_lock)
            .into_iter()
            .filter(|p| p.0 != "carcinify-structure-probe" && !port.contains(p))
            .map(|p| format!("{} {}", p.0, p.1))
            .collect();
        if !strangers.is_empty() {
            return Err(format!(
                "the probe was built with dependency versions that are not in the port's Cargo.lock: {}. Update the port's lock file, or the numbers are for another set of dependencies",
                strangers.join(", ")
            ));
        }
        let lock_file = "every package the probe was built with is in the port's Cargo.lock, at the same version and checksum".to_string();
        return Ok(Ran {
            results,
            command,
            rebuilt_from_clean,
            lock_file,
        });
    }
    record.forget();
    if let Some(about_the_declaration) = explain(&last, jobs, &map) {
        return Err(about_the_declaration);
    }
    let tail: String = last
        .lines()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "the measuring program did not build or run, and the fault is not in a line written from the declaration: the port itself may not build.\n{tail}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn driver(construct: &str, call: J) -> Driver {
        serde_json::from_value(json!({"input": "x", "construct": construct, "sizes": [{"label": "small", "args": [8, 7]}, {"label": "large", "args": [24, 7]}], "call": call, "warm_up": 1, "iterations": 10})).unwrap()
    }

    fn fleet<'a>(support: &'a [String]) -> Entry<'a> {
        Entry {
            krate: "port",
            name: "designed::Fleet::tick",
            callable: "port::designed::Fleet::tick",
            short: "tick",
            self_ty: Some("Fleet"),
            receiver: Some("exclusive"),
            support,
        }
    }

    fn step<'a>(support: &'a [String]) -> Entry<'a> {
        Entry {
            krate: "port",
            name: "step",
            callable: "port::step",
            short: "step",
            self_ty: None,
            receiver: None,
            support,
        }
    }

    #[test]
    fn the_call_is_the_entry_point_by_its_own_path() {
        let built = driver("port::designed::Fleet::new", json!({"method": "tick"}))
            .build(&fleet(&[]))
            .unwrap();
        assert_eq!(
            built.sizes[0],
            (
                "small".to_string(),
                Some("port::designed::Fleet::new(8, 7)".to_string()),
                None
            )
        );
        // Not `state.tick()`: the function is named, so the state has to
        // be a `Fleet` of that module.
        assert_eq!(built.iteration, "port::designed::Fleet::tick(&mut state)");
        let f = driver(
            "port::Board::new",
            json!({"function": "port::step", "state": "exclusive", "args": [1.5, true, "a\"b"]}),
        );
        assert_eq!(
            f.build(&step(&[])).unwrap().iteration,
            "port::step(&mut state, 1.5_f64, true, \"a\\\"b\")"
        );
    }

    #[test]
    fn rust_text_does_not_get_through() {
        let refuse =
            |construct: &str, call: J| driver(construct, call).build(&fleet(&[])).unwrap_err();
        assert!(
            refuse(
                "{ println!(\"MEASURED 0 {}\"); port::Fleet::new }",
                json!({"method": "tick"})
            )
            .contains("is not a path")
        );
        assert!(
            refuse("port::Fleet::new(1, 2); println", json!({"method": "tick"}))
                .contains("is not a path")
        );
        assert!(
            refuse("std::process::exit", json!({"method": "tick"}))
                .contains("must start with the crate's name")
        );
        assert!(
            refuse("crate::counting::now", json!({"method": "tick"}))
                .contains("must start with the crate's name")
        );
        assert!(
            refuse(
                "port::Fleet::new",
                json!({"method": "tick(); println!(\"x\")"})
            )
            .contains("is not a name")
        );
        assert!(
            refuse(
                "port::Fleet::new",
                json!({"method": "tick", "args": [[1, 2]]})
            )
            .contains("not a number, a boolean or a string")
        );
        assert!(
            refuse(
                "port::Fleet::new",
                json!({"method": "tick", "args": [{"input": "shared; drop(x)"}]})
            )
            .contains("must be {\"input\": \"shared\"}")
        );
        let s = driver(
            "port::Fleet::new",
            json!({"method": "tick", "args": ["\"); std::process::exit(0); (\""]}),
        );
        assert_eq!(
            s.build(&fleet(&[])).unwrap().iteration,
            "port::designed::Fleet::tick(&mut state, \"\\\"); std::process::exit(0); (\\\"\")"
        );
    }

    #[test]
    fn the_state_comes_from_the_type_or_from_the_support_module() {
        let refuse = |construct: &str| {
            driver(construct, json!({"method": "tick"}))
                .build(&fleet(&["fleet_of".to_string()]))
                .unwrap_err()
        };
        // A free function elsewhere in the crate, and another type.
        assert!(refuse("port::designed::quiet").contains("has to be a function of the entry point's own type `Fleet`, or a function in `port::measure_support`"));
        assert!(
            refuse("port::Other::new")
                .contains("has to be a function of the entry point's own type `Fleet`")
        );
        assert!(
            refuse("port::measure_support::missing")
                .contains("is not a function of the module `port::measure_support`")
        );
        let built = driver("port::measure_support::fleet_of", json!({"method": "tick"}))
            .build(&fleet(&["fleet_of".to_string()]))
            .unwrap();
        assert_eq!(
            built.sizes[1].1.as_deref(),
            Some("port::measure_support::fleet_of(24, 7)")
        );
        assert_eq!(built.support_used, ["fleet_of"]);
    }

    #[test]
    fn the_iteration_has_to_be_the_entry_point() {
        let other = driver("port::Fleet::new", json!({"method": "len"}))
            .build(&fleet(&[]))
            .unwrap_err();
        assert!(
            other.contains("which is not the declared entry point `tick`"),
            "{other}"
        );
        let elsewhere = driver(
            "port::Board::new",
            json!({"function": "port::decoy::step", "state": "shared"}),
        )
        .build(&step(&[]))
        .unwrap_err();
        assert!(
            elsewhere.contains(
                "calls `port::decoy::step`, which is not the declared entry point `port::step`"
            ),
            "{elsewhere}"
        );
        let by_value = Entry {
            receiver: Some("value"),
            ..fleet(&[])
        };
        assert!(
            driver("port::Fleet::new", json!({"method": "tick"}))
                .build(&by_value)
                .unwrap_err()
                .contains("takes `self` by value")
        );
    }

    #[test]
    fn one_size_needs_a_reason_and_an_input_comes_from_support() {
        let mut one = driver("port::Fleet::new", json!({"method": "tick"}));
        one.sizes.pop();
        assert!(
            one.build(&fleet(&[]))
                .unwrap_err()
                .contains("say why in `one_size_reason`")
        );
        one.one_size_reason = Some("the constructor takes a seed and no size".to_string());
        assert_eq!(one.build(&fleet(&[])).unwrap().sizes.len(), 1);

        // A free function over a slice, at two lengths, with no state.
        let d: Driver = serde_json::from_value(json!({"input": "x", "sizes": [
            {"label": "10", "input": {"function": "numbers", "args": [10]}},
            {"label": "1000", "input": {"function": "numbers", "args": [1000]}}],
            "call": {"function": "port::step", "state": "none", "args": [{"input": "shared"}]}, "warm_up": 1, "iterations": 10})).unwrap();
        let support = ["numbers".to_string()];
        let built = d.build(&step(&support)).unwrap();
        assert_eq!(
            built.sizes[1],
            (
                "1000".to_string(),
                None,
                Some("port::measure_support::numbers(1000)".to_string())
            )
        );
        assert_eq!(built.iteration, "port::step(&input)");
    }

    #[test]
    fn a_compiler_error_is_traced_to_the_field_it_came_from() {
        let jobs = [Job {
            entry: "load".to_string(),
            size: "small".to_string(),
            constructor: None,
            input: None,
            iteration: "port::load(\"a.jsonl\")".to_string(),
            warm_up: 1,
            iterations: 2,
        }];
        let (program, map) = source(&jobs, "MEASURED-x");
        let call = map.iter().find(|m| m.2 == "call").unwrap().0;
        assert!(
            program
                .lines()
                .nth(call - 1)
                .unwrap()
                .contains("port::load(\"a.jsonl\")")
        );
        let stderr = format!(
            "error[E0308]: mismatched types\n  --> /t/probe/src/main.rs:{call}:40\n   |\n   | expected `&Path`, found `&str`\n\nerror: aborting due to 1 previous error\n"
        );
        let said = explain(&stderr, &jobs, &map).unwrap();
        assert!(said.contains("the driver declared for hot path load does not compile, at size \"small\". The fault is in `call`") && said.contains("expected `&Path`, found `&str`"), "{said}");
        assert_eq!(
            explain(
                "error[E0432]: unresolved import\n --> /p/src/lib.rs:1:5\n",
                &jobs,
                &map
            ),
            None
        );
    }

    #[test]
    fn lock_files_are_compared_package_by_package() {
        let text = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"registry+x\"\nchecksum = \"abc\"\n\n[[package]]\nname = \"port\"\nversion = \"0.1.0\"\n";
        assert_eq!(
            locked(text),
            [
                (
                    "a".to_string(),
                    "1.0.0".to_string(),
                    "registry+xabc".to_string()
                ),
                ("port".to_string(), "0.1.0".to_string(), String::new())
            ]
        );
    }
}
