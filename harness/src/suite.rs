//! Suite and case files. Language-neutral.
//!
//! A suite names the two sides and the case files. A case names a function
//! and its inputs. Neither can carry an expected output, a count or a
//! verdict: unknown fields are rejected, so there is nowhere to put one.

use crate::compare::{FloatRule, PathRule, Relax, Tolerance};
use crate::provenance::sha256_file;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use serde_json::value::RawValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const SUITE_SCHEMA: &str = "carcinify/diff/suite/1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub schema: String,
    pub original: SideSpec,
    pub candidate: SideSpec,
    /// Case files (JSON Lines), relative to the suite file.
    pub cases: Vec<String>,
    /// Directories of code both sides are meant to use: put on both sides'
    /// import path, exempt from the same-code rules, listed in the report.
    /// No function under test may be defined in one.
    #[serde(default)]
    pub shared: Vec<String>,
    /// Files of calls a measurement made: each must also be a case (see
    /// `measured`).
    #[serde(default)]
    pub measured: Vec<Measured>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measured {
    /// Relative to the suite file.
    pub file: String,
    /// The function every line calls, in place of the name the line gives:
    /// a measurement may call a private function that the cases reach under
    /// a public name.
    #[serde(default, rename = "fn")]
    pub func: Option<String>,
    /// The author's reason why the calls in this file that the value format
    /// cannot hold need not be compared. Without it they fail the run.
    #[serde(default)]
    pub not_comparable: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum SideSpec {
    /// A Python module imported from the given directories.
    Python { path: Vec<String>, module: String },
    /// A PyO3 crate, built by maturin for this run.
    Pyo3 {
        #[serde(rename = "crate")]
        crate_dir: String,
        module: String,
        #[serde(default)]
        features: Vec<String>,
        #[serde(default)]
        no_default_features: bool,
        /// Functions of the wheel that are allowed to be Python rather than
        /// compiled. The report shows them as `python`.
        #[serde(default)]
        python_functions: Vec<String>,
    },
}

impl Suite {
    pub fn load(path: &Path) -> Result<Suite, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read suite {}: {e}", path.display()))?;
        let suite: Suite = serde_json::from_str(&text)
            .map_err(|e| format!("bad suite {}: {e}", path.display()))?;
        if suite.schema != SUITE_SCHEMA {
            return Err(format!(
                "suite schema is {:?}, expected {SUITE_SCHEMA:?}",
                suite.schema
            ));
        }
        Ok(suite)
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Encoding {
    #[default]
    Plain,
    Typed,
}

/// A case's relaxations as written, kept for the report.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelaxDecl {
    /// Dicts whose key order is not compared: a path and a reason each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_order: Vec<PathDecl>,
    #[serde(default)]
    pub exception_message: bool,
    #[serde(default)]
    pub floats: Vec<FloatDecl>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PathDecl {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FloatDecl {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ulps: Option<u64>,
    /// Decimal text, read by Rust's correctly rounded parser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abs: Option<String>,
    pub reason: String,
}

impl RelaxDecl {
    fn build(&self) -> Result<Relax, String> {
        let mut floats = Vec::new();
        for f in &self.floats {
            let tolerance = match (&f.ulps, &f.abs) {
                (Some(n), None) => Tolerance::Ulps(*n),
                (None, Some(s)) => Tolerance::Abs(
                    s.parse()
                        .map_err(|_| format!("abs tolerance {s:?} is not a number"))?,
                ),
                _ => {
                    return Err(format!(
                        "tolerance on {} needs exactly one of \"ulps\" and \"abs\"",
                        f.path
                    ));
                }
            };
            floats.push(FloatRule::new(&f.path, tolerance, &f.reason)?);
        }
        Ok(Relax {
            key_order: self
                .key_order
                .iter()
                .map(|k| PathRule::new(&k.path, &k.reason))
                .collect::<Result<_, _>>()?,
            exception_message: self.exception_message,
            floats,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    id: String,
    #[serde(rename = "fn")]
    func: String,
    #[serde(default)]
    encoding: Encoding,
    args: Option<Box<RawValue>>,
    kwargs: Option<Box<RawValue>>,
    #[serde(default)]
    expect_raise: bool,
    #[serde(default)]
    relax: RelaxDecl,
}

#[derive(Debug)]
pub enum Input {
    /// Ordinary JSON, forwarded to both sides as the exact text in the case
    /// file. The harness does not read the numbers in it.
    Plain {
        args: Box<RawValue>,
        kwargs: Box<RawValue>,
    },
    /// Values in the wire format, which can also name data files and
    /// scratch directories.
    Typed {
        args: Vec<J>,
        kwargs: serde_json::Map<String, J>,
        dir: PathBuf,
    },
}

#[derive(Debug)]
pub struct Case {
    pub id: String,
    pub func: String,
    /// `file:line` of the case.
    pub source: String,
    pub expect_raise: bool,
    pub relax: Relax,
    pub relax_decl: RelaxDecl,
    pub input: Input,
}

/// A place in a typed input that the harness fills in itself.
pub enum Special<'a> {
    /// A private copy of a data file; the function gets its path.
    FilePath(&'a str),
    /// The contents of a data file; the function gets bytes.
    FileBytes(&'a str),
    /// A fresh, empty directory; the function gets its path.
    ScratchDir,
}

/// Checks a typed input and rebuilds it, asking `special` for each place the
/// harness has to fill in.
pub fn walk(j: &J, special: &mut dyn FnMut(Special) -> Result<J, String>) -> Result<J, String> {
    let m = j
        .as_object()
        .ok_or_else(|| format!("typed input must be a tagged object, got {j}"))?;
    let tag = m
        .get("t")
        .and_then(J::as_str)
        .ok_or_else(|| format!("typed input without a type tag: {j}"))?;
    let only = |allowed: &[&str]| match m.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("unexpected field {k:?} in {j}")),
        None => Ok(()),
    };
    match tag {
        "file_path" | "file_bytes" => {
            only(&["t", "path"])?;
            let p = m
                .get("path")
                .and_then(J::as_str)
                .ok_or_else(|| format!("{tag} needs a \"path\" string"))?;
            special(if tag == "file_path" {
                Special::FilePath(p)
            } else {
                Special::FileBytes(p)
            })
        }
        "scratch_dir" => {
            only(&["t"])?;
            special(Special::ScratchDir)
        }
        "list" | "tuple" | "set" => {
            only(&["t", "v"])?;
            let arr = m
                .get("v")
                .and_then(J::as_array)
                .ok_or_else(|| format!("{tag} needs an array \"v\""))?;
            let items = arr
                .iter()
                .map(|x| walk(x, special))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(serde_json::json!({"t": tag, "v": items}))
        }
        "map" => {
            only(&["t", "v"])?;
            let arr = m
                .get("v")
                .and_then(J::as_array)
                .ok_or("map needs an array \"v\" of pairs")?;
            let mut pairs = Vec::new();
            for pair in arr {
                let p = pair
                    .as_array()
                    .filter(|p| p.len() == 2)
                    .ok_or("map entry must be a [key, value] pair")?;
                pairs.push(J::Array(vec![walk(&p[0], special)?, walk(&p[1], special)?]));
            }
            Ok(serde_json::json!({"t": "map", "v": pairs}))
        }
        _ => {
            Value::decode(j)?;
            Ok(j.clone())
        }
    }
}

/// Every case from every case file, plus the SHA-256 of each file read:
/// the case files themselves and the data files they point at.
#[derive(Debug)]
pub struct Loaded {
    pub cases: Vec<Case>,
    pub inputs: BTreeMap<PathBuf, (&'static str, String)>,
}

pub fn load_cases(suite_dir: &Path, files: &[String]) -> Result<Loaded, String> {
    let mut cases = Vec::new();
    let mut inputs = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for rel in files {
        let path = suite_dir.join(rel);
        let path = path
            .canonicalize()
            .map_err(|e| format!("case file {}: {e}", path.display()))?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("case file {}: {e}", path.display()))?;
        inputs.insert(path.clone(), ("cases", sha256_file(&path)?));
        let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let source = format!("{rel}:{}", n + 1);
            let case = parse_case(line, &source, &dir, &mut inputs)?;
            if case.id.is_empty() || !ids.insert(case.id.clone()) {
                return Err(format!(
                    "{source}: case id {:?} is empty or used twice",
                    case.id
                ));
            }
            cases.push(case);
        }
    }
    Ok(Loaded { cases, inputs })
}

/// One line of a case file, `source` naming it. Data files a typed input
/// names are hashed into `inputs`.
pub fn parse_case(
    line: &str,
    source: &str,
    dir: &Path,
    inputs: &mut BTreeMap<PathBuf, (&'static str, String)>,
) -> Result<Case, String> {
    let raw: RawCase = serde_json::from_str(line).map_err(|e| format!("{source}: {e}"))?;
    let args = raw
        .args
        .unwrap_or(RawValue::from_string("[]".into()).map_err(|e| e.to_string())?);
    let kwargs = raw
        .kwargs
        .unwrap_or(RawValue::from_string("{}".into()).map_err(|e| e.to_string())?);
    if !args.get().starts_with('[') || !kwargs.get().starts_with('{') {
        return Err(format!(
            "{source}: \"args\" must be an array and \"kwargs\" an object"
        ));
    }
    let input = match raw.encoding {
        Encoding::Plain => Input::Plain { args, kwargs },
        Encoding::Typed => {
            let args: Vec<J> =
                serde_json::from_str(args.get()).map_err(|e| format!("{source}: {e}"))?;
            let kwargs: serde_json::Map<String, J> =
                serde_json::from_str(kwargs.get()).map_err(|e| format!("{source}: {e}"))?;
            // Check every value now and hash every data file now, so a
            // bad case stops the run before either side starts.
            let mut check = |s: Special| -> Result<J, String> {
                if let Special::FilePath(p) | Special::FileBytes(p) = s {
                    let file = dir.join(p);
                    let file = file
                        .canonicalize()
                        .map_err(|e| format!("data file {}: {e}", file.display()))?;
                    let hash = sha256_file(&file)?;
                    inputs.entry(file).or_insert(("data", hash));
                }
                Ok(J::Null)
            };
            for v in args.iter().chain(kwargs.values()) {
                walk(v, &mut check).map_err(|e| format!("{source}: {e}"))?;
            }
            Input::Typed {
                args,
                kwargs,
                dir: dir.to_path_buf(),
            }
        }
    };
    Ok(Case {
        id: raw.id,
        func: raw.func,
        source: source.to_string(),
        expect_raise: raw.expect_raise,
        relax: raw.relax.build().map_err(|e| format!("{source}: {e}"))?,
        relax_decl: raw.relax,
        input,
    })
}

/// The request for one side. Data files are copied into `scratch` first, so
/// a side that changes its file cannot change what the other side reads.
/// Both sides get the same paths: the harness empties `scratch` in between.
pub fn request(case: &Case, scratch: &Path) -> Result<String, String> {
    request_as(case, scratch, "call", &case.func)
}

/// The same with another operation, for another function of the module.
pub fn request_as(
    case: &Case,
    scratch: &Path,
    op: &'static str,
    func: &str,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct Call<'a> {
        op: &'static str,
        #[serde(rename = "fn")]
        func: &'a str,
        encoding: &'static str,
        args: &'a RawValue,
        kwargs: &'a RawValue,
    }
    let line = |encoding, args: &RawValue, kwargs: &RawValue| {
        serde_json::to_string(&Call {
            op,
            func,
            encoding,
            args,
            kwargs,
        })
        .map_err(|e| e.to_string())
    };
    match &case.input {
        Input::Plain { args, kwargs } => line("plain", args, kwargs),
        Input::Typed { args, kwargs, dir } => {
            let mut n = 0;
            let mut fill = |s: Special| -> Result<J, String> {
                n += 1;
                let utf8 = |p: PathBuf| {
                    p.to_str()
                        .map(str::to_string)
                        .ok_or_else(|| format!("path is not UTF-8: {}", p.display()))
                };
                let io = |e: std::io::Error| format!("scratch directory: {e}");
                match s {
                    Special::ScratchDir => {
                        let d = scratch.join(format!("dir{n}"));
                        std::fs::create_dir_all(&d).map_err(io)?;
                        Ok(serde_json::json!({"t": "str", "v": utf8(d)?}))
                    }
                    Special::FilePath(p) => {
                        let src = dir.join(p);
                        let d = scratch.join(format!("file{n}"));
                        std::fs::create_dir_all(&d).map_err(io)?;
                        let dest = d.join(src.file_name().ok_or("data file has no name")?);
                        std::fs::copy(&src, &dest).map_err(io)?;
                        Ok(serde_json::json!({"t": "str", "v": utf8(dest)?}))
                    }
                    Special::FileBytes(p) => {
                        let src = dir.join(p).canonicalize().map_err(io)?;
                        Ok(serde_json::json!({"t": "_bytes_file", "path": utf8(src)?}))
                    }
                }
            };
            let args = args
                .iter()
                .map(|v| walk(v, &mut fill))
                .collect::<Result<Vec<_>, _>>()?;
            let mut kw = serde_json::Map::new();
            for (k, v) in kwargs {
                kw.insert(k.clone(), walk(v, &mut fill)?);
            }
            let raw = |j: &J| serde_json::value::to_raw_value(j).map_err(|e| e.to_string());
            line("typed", &raw(&J::Array(args))?, &raw(&J::Object(kw))?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case_line(line: &str) -> Result<Loaded, String> {
        let dir = std::env::temp_dir().join(format!(
            "carcinify-diff-suite-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("c.jsonl"), line).unwrap();
        let out = load_cases(&dir, &["c.jsonl".to_string()]);
        std::fs::remove_dir_all(&dir).unwrap();
        out
    }

    #[test]
    fn a_case_cannot_carry_an_expected_value_or_a_typo() {
        assert!(case_line(r#"{"id":"a","fn":"f","args":[1]}"#).is_ok());
        for bad in [
            r#"{"id":"a","fn":"f","expected":1}"#,
            r#"{"id":"a","fn":"f","passed":true}"#,
            r#"{"id":"a","fn":"f","expect_rasie":true}"#,
            r#"{"id":"a","fn":"f","relax":{"keyorder":true}}"#,
            r#"{"id":"a","fn":"f","relax":{"key_order":true}}"#,
            r#"{"id":"a","fn":"f","relax":{"key_order":[{"path":"$"}]}}"#,
            r#"{"id":"a","fn":"f","relax":{"key_order":[{"path":"items","reason":"r"}]}}"#,
            r#"{"id":"a","fn":"f","relax":{"floats":[{"path":"$","ulps":1}]}}"#,
            r#"{"id":"a","fn":"f","relax":{"floats":[{"path":"$","ulps":1,"abs":"1","reason":"r"}]}}"#,
            r#"{"id":"a","fn":"f","relax":{"floats":[{"path":"$","abs":1e-9,"reason":"r"}]}}"#,
            r#"{"id":"a","fn":"f","args":{}}"#,
            r#"{"id":"","fn":"f"}"#,
            r#"{"id":"a","fn":"f","encoding":"typed","args":[1.5]}"#,
            r#"{"id":"a","fn":"f","encoding":"typed","args":[{"t":"file_path","path":"no-such-file"}]}"#,
        ] {
            assert!(case_line(bad).is_err(), "accepted {bad}");
        }
        let twice = "{\"id\":\"a\",\"fn\":\"f\"}\n{\"id\":\"a\",\"fn\":\"f\"}";
        assert!(case_line(twice).unwrap_err().contains("used twice"));
    }

    #[test]
    fn plain_inputs_reach_the_sides_as_written() {
        let text = r#"{"id":"a","fn":"f","args":[396.03999999999996, 1E400, 123456789012345678901234567890]}"#;
        let loaded = case_line(text).unwrap();
        let req = request(&loaded.cases[0], Path::new("/nonexistent")).unwrap();
        assert!(
            req.contains("[396.03999999999996, 1E400, 123456789012345678901234567890]"),
            "{req}"
        );
    }
}
