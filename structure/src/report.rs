//! The declaration file, the checks, and the report.
//!
//! What the author declares (hot entry points, budgets, drivers,
//! justifications, owners, whether the algorithm is the same) is copied
//! into the report as declared. Everything else in the report is measured
//! or counted here.

use crate::analyze::{Crate, Hit, Shared};
use crate::probe::{Built, Driver, Measured, SUPPORT};
use carcinify_diff::provenance::{git_info, sha256_file, tool_version, tracked_digest};
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::path::{Path, PathBuf};

pub const DECLARATION_SCHEMA: &str = "carcinify/structure/declaration/2";
pub const REPORT_SCHEMA: &str = "carcinify/structure/report/2";
/// The lead's thresholds for S6, to be revisited after a first real port.
/// Times per iteration within this fraction of each other count as the
/// same, when asking whether two sizes were really two inputs.
pub const SAME_TIME: f64 = 0.10;
pub const MAX_FILE_LINES: usize = 1000;
pub const MAX_FUNCTION_LINES: usize = 150;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub schema: String,
    /// The crate's directory, relative to this file.
    #[serde(rename = "crate")]
    pub crate_dir: String,
    /// The design note the declarations come from; hashed into the report.
    pub design_note: Option<String>,
    /// Cargo features of the crate to measure with, as in the harness's
    /// suite, so that both reports describe the same build.
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub no_default_features: bool,
    pub hot_paths: Vec<HotPath>,
    /// For a crate with no hot path: the statement that it has none, and
    /// why. Without it an empty `hot_paths` fails the run.
    pub no_hot_path: Option<NoHotPath>,
    #[serde(default)]
    pub justifications: Vec<Justification>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoHotPath {
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HotPath {
    /// `module::Type::function`, or as much of the end of it as is unique.
    pub entry: String,
    pub budget: Option<Budget>,
    pub driver: Option<Driver>,
    /// The author's statement on whether the port uses the same algorithm
    /// as the original. It cannot be measured; it is recorded as declared.
    pub same_algorithm: Option<SameAlgorithm>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub allocations_per_iteration: f64,
    pub bytes_per_iteration: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SameAlgorithm {
    pub same: bool,
    pub statement: String,
}

/// A reason for one S4 or S5 hit, or the owner of one S7 construct.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Justification {
    /// `S4`, `S5` or `S7`.
    pub check: String,
    /// S4, S5: the function, as for `entry`. S7: the item the report names,
    /// such as `struct Node`.
    #[serde(rename = "where")]
    pub place: String,
    /// S4: the field. S5: text of the hit, or part of it. S7: the construct.
    pub construct: String,
    /// S4, S5: why it is needed. S7: who owns the data and why it is shared.
    pub reason: String,
}

#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    /// `pass`, `pass_with_flags` or `fail`.
    pub verdict: &'static str,
    pub exit_status: u8,
    /// Why it failed.
    pub reasons: Vec<String>,
    /// Things a reader should look at that do not fail the run.
    pub flags: Vec<String>,
    /// What this report cannot show, whatever its verdict.
    pub not_checked: [&'static str; 6],
    pub git: J,
    pub declaration: J,
    pub design_note: J,
    #[serde(rename = "crate")]
    pub krate: J,
    pub tools: J,
    pub hot_paths: Vec<HotPathReport>,
    pub shared_ownership: Vec<SharedReport>,
    #[serde(rename = "unsafe")]
    pub unsafe_code: J,
    pub size: J,
    pub unused_justifications: Vec<Justification>,
}

pub const NOT_CHECKED: [&str; 6] = [
    "Whether the structure is right. The counts find symptoms; a port can pass every one of them and still be badly built.",
    "A plain index that goes stale when elements are removed or reordered. It is an integer, it allocates nothing and uses none of the constructs counted here.",
    "Code the calls could not be followed into: closures and function values passed in, macros other than the standard ones, and anything outside the crate. Each such call is listed under `not_followed`.",
    "Calls are matched by name, not by type. A call through a trait or generic is followed to every function of that name, which can be more than really run; a function reached only through a name this tool does not see is missed.",
    "Whether the budget is the right one, whether the driver's input is representative, and whether the port uses the same algorithm as the original. Those are declared by the author and shown as declared.",
    "Speed. The nanoseconds per iteration are from one machine and one run, and say nothing about the time seen from the Python caller.",
];

#[derive(Serialize)]
pub struct HotPathReport {
    pub entry: String,
    /// The function the entry was resolved to.
    pub function: Option<String>,
    pub declared_budget: Option<Budget>,
    pub declared_driver: Option<Driver>,
    pub declared_same_algorithm: Option<SameAlgorithm>,
    /// The Rust the tool wrote from the driver: one iteration.
    pub iteration: Option<String>,
    /// One measurement per declared input size.
    pub measured: Vec<SizeReport>,
    /// `true` when every size is within budget, `false` when any is over,
    /// absent when there was nothing to compare.
    pub within_budget: Option<bool>,
    /// Whether allocations per iteration differ between the sizes.
    pub allocations_differ_between_sizes: Option<bool>,
    /// Whether bytes per iteration do.
    pub bytes_differ_between_sizes: Option<bool>,
    /// Functions of the crate's `measure_support` module the driver used.
    pub measurement_support: Vec<J>,
    /// S3: per function reachable from the entry.
    pub functions: Vec<FunctionReport>,
    pub copying_constructs: usize,
    /// Beside S3: the sum of `calls_on_the_hot_path` over `functions`.
    pub calls_to_functions_that_return_a_new_collection: usize,
    /// S4.
    pub fields_taken_out_of_self: Vec<TakeReport>,
    /// S5.
    pub threads: Vec<JustifiedHit>,
    pub not_followed: Vec<Hit>,
}

#[derive(Serialize)]
pub struct SizeReport {
    pub size: String,
    /// The Rust the tool wrote from the driver: what builds the state.
    pub constructor: Option<String>,
    /// And what builds the input, if the call takes one.
    pub input: Option<String>,
    #[serde(flatten)]
    pub measured: Measured,
    pub within_budget: Option<bool>,
}

#[derive(Serialize)]
pub struct FunctionReport {
    pub function: String,
    pub file: String,
    pub line: usize,
    pub lines: usize,
    pub copying: Vec<Hit>,
    pub allocating_macros: Vec<Hit>,
    /// For a function that returns an owned collection and copies or
    /// allocates: the calls to it written in the reachable functions. Its
    /// constructs are counted once in S3 and run at each of these.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls_on_the_hot_path: Option<usize>,
}

#[derive(Serialize)]
pub struct TakeReport {
    pub function: String,
    pub field: String,
    pub how: String,
    pub file: String,
    pub line: usize,
    /// True when the same function assigns the field again: the check-out
    /// and check-in that S4 is about.
    pub put_back: bool,
    pub justification: Option<String>,
}

#[derive(Serialize)]
pub struct JustifiedHit {
    pub function: String,
    #[serde(flatten)]
    pub hit: Hit,
    pub justification: Option<String>,
}

#[derive(Serialize)]
pub struct SharedReport {
    #[serde(flatten)]
    pub found: Shared,
    pub declared_owner: Option<String>,
}

pub struct Options {
    pub declaration: PathBuf,
    pub allow_dirty: bool,
}

fn matches_name(function: &str, declared: &str) -> bool {
    function == declared || function.ends_with(&format!("::{declared}"))
}

struct Package {
    name: String,
    /// The name the crate is used by in Rust.
    lib: String,
    target: PathBuf,
    workspace: PathBuf,
    /// The crates it depends on, by the names its code uses.
    dependencies: Vec<String>,
    /// The features it declares.
    features: Vec<String>,
}

fn package(crate_dir: &Path) -> Result<Package, String> {
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(crate_dir)
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{} is not a Cargo package: {}",
            crate_dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let json: J = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let manifest = crate_dir.join("Cargo.toml");
    let packages = json["packages"].as_array().cloned().unwrap_or_default();
    let package = packages
        .iter()
        .find(|p| {
            p["manifest_path"]
                .as_str()
                .is_some_and(|m| Path::new(m) == manifest)
        })
        .ok_or_else(|| format!("no package at {}", manifest.display()))?;
    let name = package["name"]
        .as_str()
        .ok_or("cargo metadata gave no package name")?
        .to_string();
    let targets = package["targets"].as_array().cloned().unwrap_or_default();
    let lib = targets
        .iter()
        .find(|t| {
            t["kind"].as_array().is_some_and(|k| {
                k.iter()
                    .any(|k| k.as_str().is_some_and(|k| k.contains("lib")))
            })
        })
        .and_then(|t| t["name"].as_str())
        .unwrap_or(&name)
        .replace('-', "_");
    let dependencies = package["dependencies"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|d| {
            d["rename"]
                .as_str()
                .or(d["name"].as_str())
                .map(|n| n.replace('-', "_"))
        })
        .collect();
    let features = package["features"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let target = json["target_directory"]
        .as_str()
        .ok_or("cargo metadata gave no target directory")?;
    let workspace = json["workspace_root"]
        .as_str()
        .ok_or("cargo metadata gave no workspace root")?;
    Ok(Package {
        // Named by the crate's place as well as its name: with one target
        // directory for everything (`CARGO_TARGET_DIR`, usual in CI), two
        // crates of one name must not measure in the same directory.
        target: Path::new(target).join("carcinify-structure").join(format!(
            "{name}-{}",
            &carcinify_diff::provenance::sha256_bytes(crate_dir.to_string_lossy().as_bytes())[..10]
        )),
        name,
        lib,
        workspace: PathBuf::from(workspace),
        features,
        dependencies,
    })
}

/// Runs every check and returns the report. `Err` means the run could not
/// be carried out (bad declaration, uncommitted changes, no such crate).
pub fn run(opts: &Options) -> Result<Report, String> {
    let path = opts
        .declaration
        .canonicalize()
        .map_err(|e| format!("{}: {e}", opts.declaration.display()))?;
    let here = path
        .parent()
        .ok_or("the declaration file has no directory")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decl: Declaration = serde_json::from_str(&text)
        .map_err(|e| format!("bad declaration {}: {e}", path.display()))?;
    if decl.schema != DECLARATION_SCHEMA {
        return Err(format!(
            "declaration schema is {:?}, expected {DECLARATION_SCHEMA:?}",
            decl.schema
        ));
    }
    let crate_dir = here
        .join(&decl.crate_dir)
        .canonicalize()
        .map_err(|e| format!("crate {}: {e}", decl.crate_dir))?;
    let git = git_info(here)?;
    if !git.dirty.is_empty() && !opts.allow_dirty {
        return Err(format!(
            "the working tree has uncommitted changes, so the report would not describe commit {}:\n  {}\nCommit them, or pass --allow-dirty to run anyway (the report will say so).",
            &git.commit[..12.min(git.commit.len())],
            git.dirty.join("\n  ")
        ));
    }
    let rel = |p: &Path| {
        p.strip_prefix(&git.root)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned()
    };
    let shown = rel(&crate_dir);
    let (mut krate, mut tests) = Crate::read(&crate_dir.join("src"), &format!("{shown}/src"))?;
    if crate_dir.join("tests").is_dir() {
        let (a, b) = Crate::read(&crate_dir.join("tests"), &format!("{shown}/tests"))?;
        tests
            .unsafe_uses
            .extend(a.unsafe_uses.into_iter().chain(b.unsafe_uses));
    }
    let pkg = package(&crate_dir)?;
    for feature in &decl.features {
        let ok = !feature.is_empty()
            && feature
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_+./".contains(c));
        if !ok {
            return Err(format!(
                "feature {feature:?} in the declaration is not a Cargo feature name"
            ));
        }
        if !pkg.features.contains(feature) {
            return Err(format!(
                "the declaration names the feature {feature:?}, and the crate has {:?}",
                pkg.features
            ));
        }
    }
    krate.resolve_calls(&pkg.dependencies);
    let package = pkg.name.clone();

    let mut reasons = Vec::new();
    let mut flags = Vec::new();
    let mut used = vec![false; decl.justifications.len()];
    let mut justify = |check: &str, matches: &dyn Fn(&Justification) -> bool| -> Option<String> {
        let found = decl
            .justifications
            .iter()
            .position(|j| j.check == check && !j.reason.trim().is_empty() && matches(j))?;
        used[found] = true;
        Some(decl.justifications[found].reason.clone())
    };
    match (&decl.no_hot_path, decl.hot_paths.is_empty()) {
        (Some(_), false) => return Err("the declaration names hot paths and also says there is none".to_string()),
        (Some(n), true) if !n.reason.trim().is_empty() => flags.push(format!("declared to have no hot path, so nothing was measured (S1, S2, S3 not applied). The stated reason: {}", n.reason)),
        (_, true) => reasons.push("no hot entry point is declared, so nothing was measured. A crate that has none says so with `no_hot_path` and a reason".to_string()),
        (None, false) => {}
    }

    // Resolve entries, then measure all drivers in one probe.
    let mut resolved = Vec::new();
    for hot in &decl.hot_paths {
        let found = krate.find(&hot.entry);
        match found.as_slice() {
            [one] => resolved.push(Some(*one)),
            [] => {
                return Err(format!(
                    "hot entry point {:?} matches no function of the crate",
                    hot.entry
                ));
            }
            many => {
                let names: Vec<&str> = many
                    .iter()
                    .map(|i| krate.functions[*i].name.as_str())
                    .collect();
                return Err(format!(
                    "hot entry point {:?} matches {}: write more of the path",
                    hot.entry,
                    names.join(", ")
                ));
            }
        }
    }
    // Every driver is checked and turned into Rust here; no text of the
    // declaration reaches the measuring program any other way.
    let mut jobs = Vec::new();
    // Code written for the measurement lives in one module of the crate,
    // so that a reviewer knows where to read it.
    let in_support = |f: &crate::analyze::Function| {
        f.name
            .strip_prefix(&format!("{SUPPORT}::"))
            .is_some_and(|rest| !rest.contains("::"))
    };
    let support_functions: Vec<String> = krate
        .functions
        .iter()
        .filter(|f| in_support(f))
        .map(|f| f.short.clone())
        .collect();
    let mut built: Vec<Option<Built>> = Vec::new();
    for (n, hot) in decl.hot_paths.iter().enumerate() {
        let Some(driver) = &hot.driver else {
            built.push(None);
            continue;
        };
        let f = &krate.functions[resolved[n].expect("resolved above")];
        let callable = krate
            .callable(f, &pkg.lib, &pkg.dependencies)
            .map_err(|e| format!("hot path {}: {e}", hot.entry))?;
        let entry = crate::probe::Entry {
            krate: &pkg.lib,
            name: &f.name,
            callable: &callable,
            short: &f.short,
            self_ty: f.self_ty.as_deref(),
            receiver: f.receiver,
            support: &support_functions,
        };
        let b = driver
            .build(&entry)
            .map_err(|e| format!("hot path {}: {e}", hot.entry))?;
        for (size, constructor, input) in &b.sizes {
            jobs.push(crate::probe::Job {
                entry: hot.entry.clone(),
                size: size.clone(),
                constructor: constructor.clone(),
                input: input.clone(),
                iteration: b.iteration.clone(),
                warm_up: driver.warm_up,
                iterations: driver.iterations,
            });
        }
        built.push(Some(b));
    }
    let mut probe = serde_json::json!({});
    let mut results = Vec::new().into_iter();
    if !jobs.is_empty() {
        let place = crate::probe::Place {
            repo_root: &git.root,
            crate_dir: &crate_dir,
            workspace_dir: &pkg.workspace,
            package: &package,
            target_dir: &pkg.target,
            features: &decl.features,
            no_default_features: decl.no_default_features,
        };
        let ran = crate::probe::run(&place, &jobs)?;

        probe = serde_json::json!({"command": ran.command, "rebuilt_from_clean": ran.rebuilt_from_clean, "dependency_versions": ran.lock_file});
        results = ran.results.into_iter();
    }

    let mut hot_paths = Vec::new();
    for (n, hot) in decl.hot_paths.iter().enumerate() {
        let entry = resolved[n];
        // S1, S2.
        if hot.driver.is_none() {
            reasons.push(format!("{}: no driver is declared, so allocations on this hot path were not measured (S1, S2)", hot.entry));
        }
        let budget = match &hot.budget {
            None => {
                reasons.push(format!(
                    "{}: no allocation budget is declared (S1, S2)",
                    hot.entry
                ));
                None
            }
            Some(b) if b.reason.trim().is_empty() => {
                reasons.push(format!(
                    "{}: the allocation budget has no reason (S1, S2)",
                    hot.entry
                ));
                None
            }
            Some(b) => Some(b),
        };
        let mut sizes = Vec::new();
        let mut iteration = None;
        let mut support_used = Vec::new();
        if let Some(b) = &built[n] {
            iteration = Some(b.iteration.clone());
            for (size, constructor, input) in &b.sizes {
                let m: Measured = results
                    .next()
                    .ok_or("the measuring program gave fewer results than asked for")?;
                // The budget is per iteration and has to hold at every size.
                let ok = budget.map(|b| {
                    m.allocations_per_iteration <= b.allocations_per_iteration
                        && m.bytes_per_iteration <= b.bytes_per_iteration
                });
                if let (Some(false), Some(b)) = (ok, budget) {
                    reasons.push(format!(
                        "{}: over budget at size {size:?}: {} allocations and {} bytes per iteration measured, {} and {} allowed (S1, S2)",
                        hot.entry, m.allocations_per_iteration, m.bytes_per_iteration, b.allocations_per_iteration, b.bytes_per_iteration
                    ));
                }
                sizes.push(SizeReport {
                    size: size.clone(),
                    constructor: constructor.clone(),
                    input: input.clone(),
                    measured: m,
                    within_budget: ok,
                });
            }
            for name in &b.support_used {
                let f = krate
                    .functions
                    .iter()
                    .find(|f| in_support(f) && &f.short == name)
                    .expect("checked when the driver was built");
                flags.push(format!("{}: measured on a state or input built by `{SUPPORT}::{name}` ({}:{}, {} lines), written by the author for this measurement. Read it: the numbers are for what it builds", hot.entry, f.file, f.line, f.lines));
                support_used.push(serde_json::json!({"function": format!("{SUPPORT}::{name}"), "file": f.file, "line": f.line, "lines": f.lines, "note": "written by the author for this measurement"}));
            }
            if let Some(reason) = hot
                .driver
                .as_ref()
                .and_then(|d| d.one_size_reason.as_ref())
                .filter(|_| sizes.len() == 1)
            {
                flags.push(format!("{}: measured at one input size, so growth with the input cannot be seen. The stated reason: {reason}", hot.entry));
            }
        }
        let within = if sizes.is_empty() || budget.is_none() {
            None
        } else {
            Some(sizes.iter().all(|s| s.within_budget == Some(true)))
        };
        // Which number changes between the sizes, if any.
        let changes = |get: &dyn Fn(&Measured) -> f64| {
            sizes.first().map(|first| {
                sizes
                    .iter()
                    .any(|s| get(&s.measured) != get(&first.measured))
            })
        };
        let differ = changes(&|m| m.allocations_per_iteration);
        let bytes_differ = changes(&|m| m.bytes_per_iteration);
        let shown = |get: &dyn Fn(&Measured) -> f64| {
            sizes
                .iter()
                .map(|s| format!("{} at {:?}", get(&s.measured), s.size))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let caution = "a budget that holds at these sizes says nothing about a larger one";
        // Sizes that measure the same in every respect may be the same
        // input: a constructor or support function that ignores its
        // argument. The time counts as the same within `SAME_TIME`.
        let times: Vec<f64> = sizes
            .iter()
            .map(|s| s.measured.nanoseconds_per_iteration)
            .collect();
        if measured_the_same(differ, bytes_differ, &times) {
            flags.push(format!(
                "{}: the {} sizes measured the same: allocations and bytes equal, and time per iteration within {:.0}% ({}). Check that the input really differs between them; a constructor or support function that ignores its argument gives this",
                hot.entry,
                sizes.len(),
                SAME_TIME * 100.0,
                sizes.iter().map(|s| format!("{:.1} ns at {:?}", s.measured.nanoseconds_per_iteration, s.size)).collect::<Vec<_>>().join(", ")
            ));
        }
        match (differ, bytes_differ) {
            (Some(true), _) => flags.push(format!("{}: allocations per iteration change with the input ({}); {caution}", hot.entry, shown(&|m| m.allocations_per_iteration))),
            (Some(false), Some(true)) => flags.push(format!("{}: bytes per iteration change with the input ({}) while the number of allocations does not; {caution}", hot.entry, shown(&|m| m.bytes_per_iteration))),
            _ => {}
        }
        if hot.same_algorithm.is_none() {
            flags.push(format!("{}: the author has not declared whether the port uses the same algorithm as the original (S9)", hot.entry));
        }
        let mut functions = Vec::new();
        let mut takes = Vec::new();
        let mut threads = Vec::new();
        let mut not_followed = Vec::new();
        let reachable = entry.map(|e| krate.reachable(e)).unwrap_or_default();
        let helper_calls = krate.call_sites_of_new_collections(&reachable);
        for index in reachable {
            let f = &krate.functions[index];
            functions.push(FunctionReport {
                function: f.name.clone(),
                file: f.file.clone(),
                line: f.line,
                lines: f.lines,
                copying: f.copying.clone(),
                allocating_macros: f.allocating_macros.clone(),
                calls_on_the_hot_path: helper_calls
                    .iter()
                    .find(|(helper, _)| *helper == index)
                    .map(|(_, sites)| *sites),
            });
            for t in &f.takes {
                let put_back = f.assigned_self_fields.contains(&t.field);
                let justification = justify("S4", &|j| {
                    matches_name(&f.name, &j.place) && j.construct == t.field
                });
                if put_back && justification.is_none() {
                    reasons.push(format!(
                        "{}: `self.{}` is taken out with {} and put back, at {}:{}, with no justification (S4)",
                        f.name, t.field, t.how, f.file, t.line
                    ));
                }
                takes.push(TakeReport {
                    function: f.name.clone(),
                    field: t.field.clone(),
                    how: t.how.clone(),
                    file: f.file.clone(),
                    line: t.line,
                    put_back,
                    justification,
                });
            }
            for hit in &f.threads {
                let justification = justify("S5", &|j| {
                    matches_name(&f.name, &j.place) && hit.what.contains(&j.construct)
                });
                if justification.is_none() {
                    reasons.push(format!("{}: {} at {}:{} starts a thread or pool on the hot path, with no justification (S5)", f.name, hit.what, hit.file, hit.line));
                }
                threads.push(JustifiedHit {
                    function: f.name.clone(),
                    hit: hit.clone(),
                    justification,
                });
            }
            not_followed.extend(f.not_followed.iter().cloned());
        }
        let listed: usize = functions
            .iter()
            .map(|f: &FunctionReport| f.copying.len() + f.allocating_macros.len())
            .sum();
        if listed == 0
            && sizes
                .iter()
                .any(|s| s.measured.allocations_per_iteration > 0.0)
        {
            flags.push(format!("{}: allocations were measured that no listed construct explains. They come from code this tool did not follow, or from a call that allocates without looking like a copy (`Vec::new` then `push`, `collect`, `Box::new`)", hot.entry));
        }
        if !not_followed.is_empty() {
            flags.push(format!("{}: {} call(s) on the hot path could not be followed; what they run was not counted", hot.entry, not_followed.len()));
        }
        hot_paths.push(HotPathReport {
            entry: hot.entry.clone(),
            function: entry.map(|e| krate.functions[e].name.clone()),
            declared_budget: hot.budget.clone(),
            declared_driver: hot.driver.clone(),
            declared_same_algorithm: hot.same_algorithm.clone(),
            iteration,
            measured: sizes,
            within_budget: within,
            allocations_differ_between_sizes: differ,
            bytes_differ_between_sizes: bytes_differ,
            measurement_support: support_used,
            copying_constructs: functions.iter().map(|f| f.copying.len()).sum(),
            calls_to_functions_that_return_a_new_collection: helper_calls
                .iter()
                .map(|(_, sites)| sites)
                .sum(),
            functions,
            fields_taken_out_of_self: takes,
            threads,
            not_followed,
        });
    }

    // S7, anywhere in the crate.
    let mut shared_ownership = Vec::new();
    for s in &krate.shared_ownership() {
        let owner = justify("S7", &|j| {
            (s.item == j.place
                || s.item.ends_with(&format!(" {}", j.place))
                || s.item.ends_with(&format!("::{}", j.place)))
                && j.construct == s.construct
        });
        if owner.is_none() {
            reasons.push(format!(
                "{} in {} at {}:{} has no stated owner (S7)",
                s.construct, s.item, s.file, s.line
            ));
        }
        shared_ownership.push(SharedReport {
            found: s.clone(),
            declared_owner: owner,
        });
    }
    // An allowance that matches nothing is stale, and could cover a hit
    // that appears later without anyone deciding it should.
    let unused: Vec<Justification> = decl
        .justifications
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(j, _)| j.clone())
        .collect();
    for j in &unused {
        reasons.push(format!(
            "the {} justification for {:?} in {:?} matches nothing in the crate: remove it",
            j.check, j.construct, j.place
        ));
    }

    // S6.
    let big_files: Vec<(&String, &usize)> = krate
        .file_lines
        .iter()
        .filter(|(f, n)| {
            **n - krate.test_lines.get(*f).copied().unwrap_or(0).min(**n) > MAX_FILE_LINES
        })
        .collect();
    let long_functions: Vec<&crate::analyze::Function> = krate
        .functions
        .iter()
        .filter(|f| f.lines > MAX_FUNCTION_LINES)
        .collect();
    for (file, lines) in &big_files {
        flags.push(format!(
            "{file} has {} lines outside its tests ({lines} with them), over {MAX_FILE_LINES} (S6)",
            **lines
                - krate
                    .test_lines
                    .get(*file)
                    .copied()
                    .unwrap_or(0)
                    .min(**lines)
        ));
    }
    for f in &long_functions {
        flags.push(format!(
            "{} has {} lines, over {MAX_FUNCTION_LINES}, at {}:{} (S6)",
            f.name, f.lines, f.file, f.line
        ));
    }
    let top = |map: &std::collections::BTreeMap<String, usize>| {
        let mut v: Vec<(&String, &usize)> = map.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        v.into_iter()
            .take(10)
            .map(|(k, n)| serde_json::json!({"name": k, "count": n}))
            .collect::<Vec<_>>()
    };
    let mut longest: Vec<&crate::analyze::Function> = krate.functions.iter().collect();
    longest.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.name.cmp(&b.name)));

    // unsafe.
    if !krate.unsafe_uses.is_empty() {
        flags.push(format!(
            "{} use(s) of `unsafe` in the crate's source; each is listed with its place",
            krate.unsafe_uses.len()
        ));
    }

    let verdict = if !reasons.is_empty() {
        "fail"
    } else if !flags.is_empty() {
        "pass_with_flags"
    } else {
        "pass"
    };
    let design_note = match &decl.design_note {
        Some(note) => {
            let p = here
                .join(note)
                .canonicalize()
                .map_err(|e| format!("design note {note}: {e}"))?;
            serde_json::json!({"path": rel(&p), "sha256": sha256_file(&p)?})
        }
        None => {
            flags.push("no design note is named in the declaration, so the budgets cannot be traced to one".to_string());
            J::Null
        }
    };
    let verdict = if verdict == "pass" && !flags.is_empty() {
        "pass_with_flags"
    } else {
        verdict
    };
    let (source_sha256, source_files) = tracked_digest(&git.root, &crate_dir)?;
    Ok(Report {
        schema: REPORT_SCHEMA,
        verdict,
        exit_status: if verdict == "fail" { 1 } else { 0 },
        reasons,
        flags,
        not_checked: NOT_CHECKED,
        git: serde_json::json!({"commit": git.commit, "dirty": !git.dirty.is_empty(), "uncommitted": git.dirty}),
        declaration: serde_json::json!({"path": rel(&path), "sha256": sha256_file(&path)?}),
        design_note,
        krate: serde_json::json!({"dir": shown, "package": package, "tracked_files": source_files, "sha256": source_sha256, "functions": krate.functions.len(),
            "features": {"default_features": !decl.no_default_features, "features": decl.features, "note": "S1, S2 and the time are of this build. The counts read from the source (S3 to S7, unsafe) are of all the code, whatever features it is behind."}}),
        tools: serde_json::json!({
            "carcinify_structure": env!("CARGO_PKG_VERSION"),
            "rustc": tool_version(Path::new("rustc"), &["--version"], &crate_dir),
            "cargo": tool_version(Path::new("cargo"), &["--version"], &crate_dir),
            "probe": probe,
        }),
        hot_paths,
        shared_ownership,
        unsafe_code: serde_json::json!({
            "crate_lint": krate.unsafe_lint,
            "in_src": krate.unsafe_uses,
            "in_tests": tests.unsafe_uses,
        }),
        size: serde_json::json!({
            "files_over_limit": big_files.iter().map(|(f, n)| serde_json::json!({"file": f, "lines": n})).collect::<Vec<_>>(),
            "functions_over_limit": long_functions.iter().map(|f| serde_json::json!({"function": f.name, "lines": f.lines})).collect::<Vec<_>>(),
            "limits": {"file_lines": MAX_FILE_LINES, "function_lines": MAX_FUNCTION_LINES},
            "files": krate.file_lines.iter().map(|(f, n)| { let t = krate.test_lines.get(f).copied().unwrap_or(0).min(*n); (f.clone(), serde_json::json!({"lines": n, "test_lines": t, "other_lines": n - t})) }).collect::<std::collections::BTreeMap<_, _>>(),
            "macros_that_may_define_functions": krate.item_macros,
            "measurement_support_module": {"name": SUPPORT, "functions": support_functions, "note": "code in the port written for the measurement; counted in S6, S7 and unsafe like any other, and in S3 to S5 only if a hot path reaches it"},
            "longest_functions": longest.iter().take(5).map(|f| serde_json::json!({"function": f.name, "lines": f.lines})).collect::<Vec<_>>(),
            "impl_blocks_per_type": top(&krate.impl_blocks),
            "fields_per_struct": top(&krate.struct_fields),
        }),
        unused_justifications: unused,
    })
}

/// Whether two or more sizes measured the same in every respect: neither
/// count changes between them and the times are within `SAME_TIME`.
fn measured_the_same(
    allocations_differ: Option<bool>,
    bytes_differ: Option<bool>,
    nanoseconds: &[f64],
) -> bool {
    let fastest = nanoseconds.iter().copied().fold(f64::MAX, f64::min);
    let slowest = nanoseconds.iter().copied().fold(0.0, f64::max);
    nanoseconds.len() >= 2
        && allocations_differ == Some(false)
        && bytes_differ == Some(false)
        && slowest <= fastest * (1.0 + SAME_TIME)
}

#[cfg(test)]
mod tests {
    use super::measured_the_same;

    #[test]
    fn sizes_measure_the_same_only_with_equal_counts_and_close_times() {
        let same = Some(false);
        assert!(measured_the_same(same, same, &[200.0, 210.0]));
        assert!(!measured_the_same(same, same, &[200.0, 300.0]));
        assert!(!measured_the_same(Some(true), same, &[200.0, 210.0]));
        assert!(!measured_the_same(same, Some(true), &[200.0, 210.0]));
        assert!(!measured_the_same(same, same, &[200.0]));
    }
}
