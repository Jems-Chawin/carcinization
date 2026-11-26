//! Exact comparison of two outcomes. Language-neutral.
//!
//! Identical means identical: floats by bit pattern, integers by their
//! digits, strings by code points, bytes byte for byte, container types and
//! map order included. The only ways to loosen that are the relaxations a
//! case names for itself, and every use of one is returned to the caller so
//! it lands in the report.

use crate::value::{Dtype, Elem, Shown, Value};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// What one side did with a case.
#[derive(Debug, Clone)]
pub enum Outcome {
    Returned(Value),
    Raised(Raise),
}

/// An exception, as the side reported it.
#[derive(Debug, Clone)]
pub struct Raise {
    /// `module.ClassName`.
    pub ty: String,
    pub class_name: String,
    /// The class is defined by the side's own code, not by the language or
    /// a third-party package.
    pub own_class: bool,
    /// The class and its bases, most derived first, without `object`: what
    /// an `except` clause could name to catch it. A class the side defines
    /// itself appears as `own:ClassName`, any other by full name.
    pub lineage: Vec<String>,
    /// `str(e)`.
    pub message: Value,
    /// `e.args`, or `None` if they hold something the value format does not
    /// cover.
    pub args: Option<Value>,
}

impl Outcome {
    pub fn canon(&self) -> String {
        match self {
            Outcome::Returned(v) => format!("returned {}", v.canon()),
            Outcome::Raised(r) => format!(
                "raised {} {} {}",
                r.ty,
                r.message.canon(),
                r.args.as_ref().map_or("?".to_string(), Value::canon)
            ),
        }
    }

    fn show(&self) -> Shown {
        match self {
            Outcome::Returned(v) => {
                let s = v.show();
                Shown {
                    ty: "returned".to_string(),
                    repr: format!("{} {}", s.ty, s.repr),
                    bits: s.bits,
                }
            }
            Outcome::Raised(r) => Shown {
                ty: "raised".to_string(),
                repr: format!("{}: {}", r.ty, r.message.show().repr),
                bits: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tolerance {
    /// Units in the last place between two finite floats.
    Ulps(u64),
    /// Absolute difference between two finite floats.
    Abs(f64),
}

#[derive(Debug, Clone, PartialEq)]
enum PatSeg {
    Any,
    Index(usize),
    Key(String),
}

/// One float tolerance: where it applies, how much, and why.
#[derive(Debug, Clone)]
pub struct FloatRule {
    pattern: Vec<PatSeg>,
    pub path: String,
    pub tolerance: Tolerance,
    pub reason: String,
}

impl FloatRule {
    /// `path` is `$` followed by `[3]`, `["key"]` or `[*]` segments.
    pub fn new(path: &str, tolerance: Tolerance, reason: &str) -> Result<FloatRule, String> {
        if reason.trim().is_empty() {
            return Err(format!("tolerance on {path} needs a reason"));
        }
        if let Tolerance::Abs(e) = tolerance
            && (!e.is_finite() || e < 0.0)
        {
            return Err(format!(
                "tolerance on {path}: abs must be finite and not negative"
            ));
        }
        let bad = || format!("bad tolerance path {path:?}: use $ then [n], [\"key\"] or [*]");
        let mut rest = path.strip_prefix('$').ok_or_else(bad)?;
        let mut pattern = Vec::new();
        while !rest.is_empty() {
            rest = rest.strip_prefix('[').ok_or_else(bad)?;
            if let Some(r) = rest.strip_prefix("*]") {
                pattern.push(PatSeg::Any);
                rest = r;
            } else if rest.starts_with('"') {
                // A JSON string; find its end by letting the JSON parser read it.
                let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<String>();
                let key = stream.next().and_then(Result::ok).ok_or_else(bad)?;
                rest = rest[stream.byte_offset()..]
                    .strip_prefix(']')
                    .ok_or_else(bad)?;
                pattern.push(PatSeg::Key(key));
            } else {
                let end = rest.find(']').ok_or_else(bad)?;
                pattern.push(PatSeg::Index(rest[..end].parse().map_err(|_| bad())?));
                rest = &rest[end + 1..];
            }
        }
        Ok(FloatRule {
            pattern,
            path: path.to_string(),
            tolerance,
            reason: reason.to_string(),
        })
    }

    fn matches(&self, path: &[Seg]) -> bool {
        self.pattern.len() == path.len()
            && self.pattern.iter().zip(path).all(|(p, s)| match (p, s) {
                (PatSeg::Any, _) => true,
                (PatSeg::Index(a), Seg::Index(b)) => a == b,
                (PatSeg::Key(a), Seg::Key(b)) => a == b,
                _ => false,
            })
    }

    fn allows(&self, a: u64, b: u64) -> bool {
        let (x, y) = (f64::from_bits(a), f64::from_bits(b));
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        match self.tolerance {
            Tolerance::Abs(eps) => (x - y).abs() <= eps,
            Tolerance::Ulps(n) => ulps_apart(a, b) <= n as u128,
        }
    }

    /// How far apart two floats the rule allows are, in its unit.
    fn deviation(&self, a: u64, b: u64) -> f64 {
        match self.tolerance {
            Tolerance::Abs(_) => (f64::from_bits(a) - f64::from_bits(b)).abs(),
            Tolerance::Ulps(_) => ulps_apart(a, b) as f64,
        }
    }

    fn relaxation(&self) -> &'static str {
        match self.tolerance {
            Tolerance::Ulps(_) => "float_ulps",
            Tolerance::Abs(_) => "float_abs",
        }
    }
}

/// Binary64 floats between two bit patterns, counted on the number line;
/// -0.0 and 0.0 are one step apart.
fn ulps_apart(a: u64, b: u64) -> u128 {
    let rank = |bits: u64| {
        let mag = (bits & !(1 << 63)) as i128;
        if bits >> 63 == 1 { -mag - 1 } else { mag }
    };
    (rank(a) - rank(b)).unsigned_abs()
}

impl FloatRule {}

/// A relaxation that applies to the value at one path: where, and why.
#[derive(Debug, Clone)]
pub struct PathRule {
    rule: FloatRule,
}

impl PathRule {
    /// `path` is written like a float tolerance's: `$`, then `[n]`,
    /// `["key"]` or `[*]` segments. It names the dict itself.
    pub fn new(path: &str, reason: &str) -> Result<PathRule, String> {
        Ok(PathRule {
            rule: FloatRule::new(path, Tolerance::Ulps(0), reason)?,
        })
    }
}

/// The relaxations one case opts into. The default is none.
#[derive(Debug, Clone, Default)]
pub struct Relax {
    /// Dicts whose key order is not compared, each named by its path.
    pub key_order: Vec<PathRule>,
    pub exception_message: bool,
    pub floats: Vec<FloatRule>,
}

#[derive(Debug, Clone)]
enum Seg {
    Index(usize),
    Key(String),
    /// A map key that is not a plain string, in canonical form.
    OtherKey(String),
    /// Inside a set, which has no positions.
    SetElement,
}

fn render(path: &[Seg]) -> String {
    let mut s = String::from("$");
    for seg in path {
        match seg {
            Seg::Index(i) => s.push_str(&format!("[{i}]")),
            Seg::Key(k) => s.push_str(&format!("[{}]", serde_json::Value::from(k.as_str()))),
            Seg::OtherKey(k) => s.push_str(&format!("[key {k}]")),
            Seg::SetElement => s.push_str("[set element]"),
        }
    }
    s
}

/// The first place two outcomes differ.
#[derive(Debug, Clone, Serialize)]
pub struct Difference {
    pub path: String,
    /// `outcome`, `type`, `value`, `length`, `missing_key`, `extra_key`,
    /// `key_order`, `set_element`, `ndarray_dtype`, `ndarray_shape`,
    /// `exception_type`, `exception_bases`, `exception_message` or
    /// `exception_args`; or `unsupported_relaxation`, which is an error of
    /// the case and not a difference.
    pub kind: &'static str,
    pub original: Option<Shown>,
    pub candidate: Option<Shown>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The simple fields of the innermost map around the difference, as the
    /// original gave them: which record this is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<std::collections::BTreeMap<String, String>>,
}

/// One place where a named relaxation let a difference through.
#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub relaxation: &'static str,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub original: Shown,
    pub candidate: Shown,
}

/// What one float rule allowed inside one array: a summary, since an array
/// can need it at every element.
#[derive(Debug, Clone, Serialize)]
pub struct ArrayRelaxed {
    pub relaxation: &'static str,
    /// The array's path.
    pub path: String,
    pub rule: String,
    pub reason: String,
    pub elements: usize,
    /// How many elements needed the rule.
    pub needed: usize,
    /// The largest deviation, in the rule's unit: ulps, or the absolute
    /// difference.
    pub largest_deviation: String,
    pub largest_at: String,
    /// The first places that needed it, with both values.
    pub first: Vec<Applied>,
    #[serde(skip)]
    largest: f64,
}

/// How many places an array summary lists.
const ARRAY_PLACES_LISTED: usize = 5;

type Diff = Box<Difference>;

struct Comparer<'a> {
    relax: &'a Relax,
    path: Vec<Seg>,
    applied: Vec<Applied>,
    arrays: Vec<ArrayRelaxed>,
}

/// What a successful comparison had to allow.
#[derive(Debug, Default)]
pub struct Matched {
    /// Relaxations the case declared and the comparison needed, outside
    /// arrays. Empty, with `arrays` empty too, means bit-for-bit identical.
    pub applied: Vec<Applied>,
    /// Float rules that elements of arrays needed, one entry per array and
    /// rule.
    pub arrays: Vec<ArrayRelaxed>,
    /// Things the harness does by rule, which a reader should know about.
    pub notes: Vec<String>,
}

/// Compares two outcomes.
pub fn compare(original: &Outcome, candidate: &Outcome, relax: &Relax) -> Result<Matched, Diff> {
    let mut c = Comparer {
        relax,
        path: Vec::new(),
        applied: Vec::new(),
        arrays: Vec::new(),
    };
    let mut notes = Vec::new();
    match (original, candidate) {
        (Outcome::Returned(a), Outcome::Returned(b)) => c.values(a, b)?,
        (Outcome::Raised(a), Outcome::Raised(b)) => {
            // A port's own exception classes live in the port's module, so
            // their full names can never match the original's. When both
            // classes are the sides' own, the class name is what is compared.
            let by_name = a.own_class && b.own_class;
            let same = if by_name {
                a.class_name == b.class_name
            } else {
                a.ty == b.ty
            };
            if !same {
                // The classes each one inherits from, so the reader sees
                // what a caller's `except` clauses would make of the two: a
                // Rust panic, for one, arrives as a BaseException, which
                // `except Exception` does not catch.
                let line = |r: &Raise| {
                    r.lineage
                        .iter()
                        .map(|c| c.strip_prefix("own:").unwrap_or(c))
                        .collect::<Vec<_>>()
                        .join(" < ")
                };
                let mut note = format!("original is {}; candidate is {}", line(a), line(b));
                if a.own_class != b.own_class {
                    note.push_str("; one class is defined by its side's own code and the other is not, so the full names are compared");
                }
                return Err(c.diff(
                    "exception_type",
                    Some(original.show()),
                    Some(candidate.show()),
                    Some(note),
                ));
            }
            if by_name {
                // Same name is not enough: `class StoreError(ValueError)`
                // and `class StoreError(Exception)` are caught by different
                // handlers. The whole line of base classes must agree.
                let chain = |r: &Raise| {
                    r.lineage
                        .iter()
                        .map(|c| c.strip_prefix("own:").unwrap_or(c))
                        .collect::<Vec<_>>()
                        .join(" < ")
                };
                if a.lineage != b.lineage || a.lineage.is_empty() {
                    let show = |r: &Raise| Shown {
                        ty: "exception class".to_string(),
                        repr: chain(r),
                        bits: None,
                    };
                    let note = "the classes have the same name but not the same base classes, so an `except` clause that catches one does not catch the other".to_string();
                    return Err(c.diff(
                        "exception_bases",
                        Some(show(a)),
                        Some(show(b)),
                        Some(note),
                    ));
                }
                if a.ty != b.ty {
                    notes.push(format!(
                        "exception type compared by class name and base classes: {} and {} are each defined by their own side's code, so the module part was ignored; both are {}",
                        a.ty,
                        b.ty,
                        chain(a)
                    ));
                }
            }
            let relaxed = |c: &mut Comparer, x: &Value, y: &Value| {
                c.applied.push(Applied {
                    relaxation: "exception_message",
                    path: "$".to_string(),
                    rule: None,
                    reason: None,
                    original: x.show(),
                    candidate: y.show(),
                })
            };
            if a.message.canon() != b.message.canon() {
                if !relax.exception_message {
                    return Err(c.diff(
                        "exception_message",
                        Some(a.message.show()),
                        Some(b.message.show()),
                        None,
                    ));
                }
                relaxed(&mut c, &a.message, &b.message);
            } else {
                match (&a.args, &b.args) {
                    (Some(x), Some(y)) if x.canon() == y.canon() => {}
                    _ if relax.exception_message => {
                        let unknown =
                            Value::Str("(arguments outside the value format)".to_string());
                        let (x, y) = (
                            a.args.as_ref().unwrap_or(&unknown),
                            b.args.as_ref().unwrap_or(&unknown),
                        );
                        relaxed(&mut c, x, y);
                    }
                    (Some(x), Some(y)) => {
                        let note = "the messages are equal but e.args are not".to_string();
                        return Err(c.diff(
                            "exception_args",
                            Some(x.show()),
                            Some(y.show()),
                            Some(note),
                        ));
                    }
                    (x, y) => {
                        let note = "an exception's args hold a value the value format does not cover, so they cannot be compared; \"exception_message\" in the case's relax compares the type only".to_string();
                        return Err(c.diff(
                            "exception_args",
                            x.as_ref().map(Value::show),
                            y.as_ref().map(Value::show),
                            Some(note),
                        ));
                    }
                }
            }
        }
        _ => {
            return Err(c.diff(
                "outcome",
                Some(original.show()),
                Some(candidate.show()),
                None,
            ));
        }
    }
    Ok(Matched {
        applied: c.applied,
        arrays: c.arrays,
        notes,
    })
}

impl Comparer<'_> {
    fn diff(
        &self,
        kind: &'static str,
        original: Option<Shown>,
        candidate: Option<Shown>,
        note: Option<String>,
    ) -> Diff {
        Box::new(Difference {
            path: render(&self.path),
            kind,
            original,
            candidate,
            note,
            record: None,
        })
    }

    fn value_diff(&self, kind: &'static str, a: &Value, b: &Value, note: Option<String>) -> Diff {
        self.diff(kind, Some(a.show()), Some(b.show()), note)
    }

    fn values(&mut self, a: &Value, b: &Value) -> Result<(), Diff> {
        match (a, b) {
            (Value::None, Value::None) => Ok(()),
            (Value::Bool(x), Value::Bool(y)) if x == y => Ok(()),
            (Value::Int(x), Value::Int(y)) if x == y => Ok(()),
            (Value::Bytes(x), Value::Bytes(y)) if x == y => Ok(()),
            (Value::Bytes(x), Value::Bytes(y)) => {
                let at = x
                    .iter()
                    .zip(y)
                    .position(|(p, q)| p != q)
                    .unwrap_or(x.len().min(y.len()));
                let note = format!(
                    "first differing byte at offset {at}; lengths {} and {}",
                    x.len(),
                    y.len()
                );
                Err(self.value_diff("value", a, b, Some(note)))
            }
            (Value::Float(x), Value::Float(y)) => self.floats(*x, *y, a, b),
            (Value::Str(_) | Value::StrUnits(_), Value::Str(_) | Value::StrUnits(_)) => {
                self.strings(a, b)
            }
            (Value::List(x), Value::List(y)) | (Value::Tuple(x), Value::Tuple(y)) => {
                for (i, (p, q)) in x.iter().zip(y).enumerate() {
                    self.path.push(Seg::Index(i));
                    self.values(p, q)?;
                    self.path.pop();
                }
                if x.len() != y.len() {
                    return Err(self.value_diff("length", a, b, None));
                }
                Ok(())
            }
            (Value::Map(x), Value::Map(y)) => self.maps(x, y),
            (Value::Set(x), Value::Set(y)) => self.sets(x, y),
            (
                Value::Array {
                    dtype: da,
                    shape: sa,
                    data: x,
                },
                Value::Array {
                    dtype: db,
                    shape: sb,
                    data: y,
                },
            ) => {
                if da != db {
                    return Err(self.value_diff("ndarray_dtype", a, b, None));
                }
                if sa != sb {
                    return Err(self.value_diff("ndarray_shape", a, b, None));
                }
                self.array(*da, sa, x, y)
            }
            _ if a.type_name() != b.type_name() => Err(self.value_diff("type", a, b, None)),
            _ => Err(self.value_diff("value", a, b, None)),
        }
    }

    fn floats(&mut self, x: u64, y: u64, a: &Value, b: &Value) -> Result<(), Diff> {
        if x == y {
            return Ok(());
        }
        let relax = self.relax;
        if let Some(rule) = relax
            .floats
            .iter()
            .find(|r| r.matches(&self.path) && r.allows(x, y))
        {
            self.applied.push(Applied {
                relaxation: rule.relaxation(),
                path: render(&self.path),
                rule: Some(rule.path.clone()),
                reason: Some(rule.reason.clone()),
                original: a.show(),
                candidate: b.show(),
            });
            return Ok(());
        }
        Err(self.value_diff("value", a, b, None))
    }

    /// Elements of two arrays of the same dtype and shape, in C order. Each
    /// element's path is the array's, then one index per dimension.
    fn array(&mut self, dtype: Dtype, shape: &[usize], x: &[u8], y: &[u8]) -> Result<(), Diff> {
        let base = self.path.len();
        let at = render(&self.path);
        let n = x.len() / dtype.size();
        // By rule, in the order first needed.
        let mut used: Vec<(usize, ArrayRelaxed)> = Vec::new();
        let mut index = vec![0; shape.len()];
        for i in 0..n {
            let mut rest = i;
            for (k, d) in shape.iter().enumerate().rev() {
                index[k] = rest % d;
                rest /= d;
            }
            self.path.extend(index.iter().map(|k| Seg::Index(*k)));
            let r = self.element(Elem::at(dtype, x, i), Elem::at(dtype, y, i), &mut used);
            self.path.truncate(base);
            r?;
        }
        self.arrays.extend(used.into_iter().map(|(_, mut s)| {
            s.path = at.clone();
            s.elements = n;
            s
        }));
        Ok(())
    }

    fn element(
        &mut self,
        p: Elem,
        q: Elem,
        used: &mut Vec<(usize, ArrayRelaxed)>,
    ) -> Result<(), Diff> {
        let relax = self.relax;
        if let Elem::F4(_) = p
            && let Some(r) = relax
                .floats
                .iter()
                .find(|r| matches!(r.tolerance, Tolerance::Ulps(_)) && r.matches(&self.path))
        {
            let note = format!(
                "the rule on {} gives ulps for a float32 element; ulps on float32 is not supported; use abs",
                r.path
            );
            return Err(self.diff("unsupported_relaxation", None, None, Some(note)));
        }
        if p == q {
            return Ok(());
        }
        // Every float32 is a binary64 exactly, so an abs rule compares them
        // as such; their bits were compared above as float32.
        let wide = |e: Elem| match e {
            Elem::F8(b) => Some(b),
            Elem::F4(b) => Some(f64::from(f32::from_bits(b)).to_bits()),
            Elem::Int(_) | Elem::Bool(_) => None,
        };
        let differs = |c: &Self| c.diff("value", Some(p.show()), Some(q.show()), None);
        let (Some(x), Some(y)) = (wide(p), wide(q)) else {
            return Err(differs(self));
        };
        let Some(k) = relax
            .floats
            .iter()
            .position(|r| r.matches(&self.path) && r.allows(x, y))
        else {
            return Err(differs(self));
        };
        let rule = &relax.floats[k];
        let place = render(&self.path);
        let i = match used.iter().position(|(j, _)| *j == k) {
            Some(i) => i,
            None => {
                used.push((
                    k,
                    ArrayRelaxed {
                        relaxation: rule.relaxation(),
                        path: String::new(),
                        rule: rule.path.clone(),
                        reason: rule.reason.clone(),
                        elements: 0,
                        needed: 0,
                        largest_deviation: String::new(),
                        largest_at: String::new(),
                        first: Vec::new(),
                        largest: -1.0,
                    },
                ));
                used.len() - 1
            }
        };
        let s = &mut used[i].1;
        s.needed += 1;
        let dev = rule.deviation(x, y);
        if dev > s.largest {
            s.largest = dev;
            s.largest_deviation = match rule.tolerance {
                Tolerance::Ulps(_) => format!("{dev}"),
                Tolerance::Abs(_) => format!("{dev:?}"),
            };
            s.largest_at = place.clone();
        }
        if s.first.len() < ARRAY_PLACES_LISTED {
            s.first.push(Applied {
                relaxation: rule.relaxation(),
                path: place,
                rule: None,
                reason: None,
                original: p.show(),
                candidate: q.show(),
            });
        }
        Ok(())
    }

    fn strings(&mut self, a: &Value, b: &Value) -> Result<(), Diff> {
        let (x, y) = (
            a.code_points().unwrap_or_default(),
            b.code_points().unwrap_or_default(),
        );
        if x == y {
            return Ok(());
        }
        let at = x
            .iter()
            .zip(&y)
            .position(|(p, q)| p != q)
            .unwrap_or(x.len().min(y.len()));
        let cp = |v: &[u32]| {
            v.get(at)
                .map_or("end of string".to_string(), |c| format!("U+{c:04X}"))
        };
        let note = format!(
            "first differing code point at index {at}: {} and {}; lengths {} and {} code points",
            cp(&x),
            cp(&y),
            x.len(),
            y.len()
        );
        Err(self.value_diff("value", a, b, Some(note)))
    }

    fn maps(&mut self, x: &[(Value, Value)], y: &[(Value, Value)]) -> Result<(), Diff> {
        let xk: Vec<String> = x.iter().map(|(k, _)| k.canon()).collect();
        let yk: Vec<String> = y.iter().map(|(k, _)| k.canon()).collect();
        let seg = |k: &Value, canon: &str| match k {
            Value::Str(s) => Seg::Key(s.clone()),
            _ => Seg::OtherKey(canon.to_string()),
        };
        // Each side's keys indexed once, so a large dict costs one lookup
        // per key and not a scan of the other side.
        let x_at: HashMap<&str, usize> = xk
            .iter()
            .enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();
        let y_at: HashMap<&str, usize> = yk
            .iter()
            .enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();
        for (i, k) in xk.iter().enumerate() {
            if !y_at.contains_key(k.as_str()) {
                self.path.push(seg(&x[i].0, k));
                return Err(self.diff(
                    "missing_key",
                    Some(x[i].1.show()),
                    None,
                    Some("the candidate has no such key".to_string()),
                ));
            }
        }
        for (i, k) in yk.iter().enumerate() {
            if !x_at.contains_key(k.as_str()) {
                self.path.push(seg(&y[i].0, k));
                return Err(self.diff(
                    "extra_key",
                    None,
                    Some(y[i].1.show()),
                    Some("the original has no such key".to_string()),
                ));
            }
        }
        if let Some(i) = xk.iter().zip(&yk).position(|(p, q)| p != q) {
            let relax = self.relax;
            let Some(allowed) = relax.key_order.iter().find(|r| r.rule.matches(&self.path)) else {
                let note = format!("same keys in a different order; first differs at position {i}");
                return Err(self.diff(
                    "key_order",
                    Some(x[i].0.show()),
                    Some(y[i].0.show()),
                    Some(note),
                ));
            };
            self.applied.push(Applied {
                relaxation: "key_order",
                path: render(&self.path),
                rule: Some(allowed.rule.path.clone()),
                reason: Some(allowed.rule.reason.clone()),
                original: x[i].0.show(),
                candidate: y[i].0.show(),
            });
        }
        for (i, k) in xk.iter().enumerate() {
            let j = y_at.get(k.as_str()).copied().unwrap_or(i);
            self.path.push(seg(&x[i].0, k));
            self.values(&x[i].1, &y[j].1).map_err(|mut d| {
                if d.record.is_none() {
                    let simple = |v: &Value| {
                        !matches!(
                            v,
                            Value::List(_) | Value::Tuple(_) | Value::Map(_) | Value::Set(_)
                        )
                    };
                    let fields = x.iter().filter(|(k, v)| simple(k) && simple(v)).take(8);
                    d.record = Some(
                        fields
                            .map(|(k, v)| (k.show().repr, v.show().repr))
                            .collect(),
                    );
                }
                d
            })?;
            self.path.pop();
        }
        Ok(())
    }

    fn sets(&mut self, x: &[Value], y: &[Value]) -> Result<(), Diff> {
        let xc: Vec<String> = x.iter().map(Value::canon).collect();
        let yc: Vec<String> = y.iter().map(Value::canon).collect();
        self.path.push(Seg::SetElement);
        let x_has: HashSet<&str> = xc.iter().map(String::as_str).collect();
        let y_has: HashSet<&str> = yc.iter().map(String::as_str).collect();
        if let Some(i) = xc.iter().position(|c| !y_has.contains(c.as_str())) {
            return Err(self.diff(
                "set_element",
                Some(x[i].show()),
                None,
                Some("only the original has this element".to_string()),
            ));
        }
        if let Some(i) = yc.iter().position(|c| !x_has.contains(c.as_str())) {
            return Err(self.diff(
                "set_element",
                None,
                Some(y[i].show()),
                Some("only the candidate has this element".to_string()),
            ));
        }
        self.path.pop();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Dtype;

    fn f(x: f64) -> Value {
        Value::Float(x.to_bits())
    }
    fn s(x: &str) -> Value {
        Value::Str(x.to_string())
    }
    fn int(x: &str) -> Value {
        Value::Int(x.to_string())
    }
    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(entries.iter().map(|(k, v)| (s(k), v.clone())).collect())
    }
    /// Most tests only care about the relaxations that were applied.
    fn compare(a: &Outcome, b: &Outcome, relax: &Relax) -> Result<Vec<Applied>, Diff> {
        super::compare(a, b, relax).map(|m| m.applied)
    }
    fn strict(a: &Value, b: &Value) -> Result<Vec<Applied>, Diff> {
        compare(
            &Outcome::Returned(a.clone()),
            &Outcome::Returned(b.clone()),
            &Relax::default(),
        )
    }
    fn differs(a: &Value, b: &Value) -> Difference {
        *strict(a, b).expect_err("expected a difference")
    }

    #[test]
    fn negative_zero_is_not_zero() {
        let d = differs(&f(-0.0), &f(0.0));
        assert_eq!(d.kind, "value");
        assert_eq!(d.original.unwrap().bits.unwrap(), "0x8000000000000000");
        assert_eq!(d.candidate.unwrap().bits.unwrap(), "0x0000000000000000");
    }

    #[test]
    fn nan_equals_only_the_same_nan() {
        let quiet = Value::Float(0x7ff8000000000000);
        let payload = Value::Float(0x7ff8000000000001);
        let negative = Value::Float(0xfff8000000000000);
        assert!(strict(&quiet, &quiet).unwrap().is_empty());
        assert_eq!(differs(&quiet, &payload).kind, "value");
        assert_eq!(differs(&quiet, &negative).kind, "value");
        assert_eq!(differs(&quiet, &f(0.0)).kind, "value");
    }

    #[test]
    fn infinity_is_exact() {
        assert!(
            strict(&f(f64::INFINITY), &f(f64::INFINITY))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            differs(&f(f64::INFINITY), &f(f64::NEG_INFINITY)).kind,
            "value"
        );
        assert_eq!(differs(&f(f64::INFINITY), &f(f64::MAX)).kind, "value");
    }

    #[test]
    fn one_bit_apart_is_a_difference() {
        let d = differs(
            &Value::Float(0x4078c0a3d70a3d70),
            &Value::Float(0x4078c0a3d70a3d71),
        );
        assert_eq!(d.original.unwrap().repr, "396.03999999999996");
        assert_eq!(d.candidate.unwrap().repr, "396.04");
    }

    #[test]
    fn integers_beyond_64_bits_keep_every_digit() {
        let big = "340282366920938463463374607431768211457"; // 2**128 + 1
        assert!(strict(&int(big), &int(big)).unwrap().is_empty());
        let d = differs(&int(big), &int("340282366920938463463374607431768211456"));
        assert_eq!(d.kind, "value");
        // What a port that went through f64 would hand back.
        assert_eq!(differs(&int(big), &f(3.402823669209385e38)).kind, "type");
    }

    #[test]
    fn types_do_not_blur() {
        assert_eq!(differs(&int("1"), &Value::Bool(true)).kind, "type");
        assert_eq!(differs(&int("1"), &f(1.0)).kind, "type");
        assert_eq!(differs(&s("a"), &Value::Bytes(vec![b'a'])).kind, "type");
        assert_eq!(
            differs(&Value::List(vec![]), &Value::Tuple(vec![])).kind,
            "type"
        );
        assert_eq!(differs(&Value::None, &Value::Bool(false)).kind, "type");
    }

    #[test]
    fn key_order_matters_until_a_case_says_otherwise() {
        let a = map(&[("x", int("1")), ("y", int("2"))]);
        let b = map(&[("y", int("2")), ("x", int("1"))]);
        assert_eq!(differs(&a, &b).kind, "key_order");

        let relax = Relax {
            key_order: vec![PathRule::new("$", "test").unwrap()],
            ..Relax::default()
        };
        let applied = compare(
            &Outcome::Returned(a.clone()),
            &Outcome::Returned(b.clone()),
            &relax,
        )
        .unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].relaxation, "key_order");
        assert_eq!(applied[0].reason.as_deref(), Some("test"));

        // The relaxation is for the dict it names and no other: the same two
        // dicts one level down are still compared in order.
        let (outer_a, outer_b) = (Value::List(vec![a.clone()]), Value::List(vec![b]));
        let d = *compare(
            &Outcome::Returned(outer_a.clone()),
            &Outcome::Returned(outer_b.clone()),
            &relax,
        )
        .unwrap_err();
        assert_eq!((d.kind, d.path.as_str()), ("key_order", "$[0]"));
        let inner = Relax {
            key_order: vec![PathRule::new("$[*]", "test").unwrap()],
            ..Relax::default()
        };
        assert_eq!(
            compare(
                &Outcome::Returned(outer_a),
                &Outcome::Returned(outer_b),
                &inner
            )
            .unwrap()
            .len(),
            1
        );

        // Ignoring order never hides a different value.
        let c = map(&[("y", int("3")), ("x", int("1"))]);
        let d = *compare(&Outcome::Returned(a), &Outcome::Returned(c), &relax).unwrap_err();
        assert_eq!((d.kind, d.path.as_str()), ("value", "$[\"y\"]"));
    }

    #[test]
    fn missing_and_extra_keys_are_named() {
        let a = map(&[("x", int("1")), ("y", int("2"))]);
        let d = differs(&a, &map(&[("x", int("1"))]));
        assert_eq!((d.kind, d.path.as_str()), ("missing_key", "$[\"y\"]"));
        let d = differs(&map(&[("x", int("1"))]), &a);
        assert_eq!((d.kind, d.path.as_str()), ("extra_key", "$[\"y\"]"));
    }

    #[test]
    fn nfc_and_nfd_are_different_strings() {
        let d = differs(&s("caf\u{e9}"), &s("cafe\u{301}"));
        assert_eq!(d.kind, "value");
        let note = d.note.unwrap();
        assert!(note.contains("index 3: U+00E9 and U+0065"), "{note}");
        assert!(note.contains("lengths 4 and 5"), "{note}");
    }

    #[test]
    fn bytes_and_lengths() {
        assert_eq!(
            differs(&Value::Bytes(vec![1, 2]), &Value::Bytes(vec![1, 3])).kind,
            "value"
        );
        let d = differs(
            &Value::List(vec![int("1")]),
            &Value::List(vec![int("1"), int("2")]),
        );
        assert_eq!((d.kind, d.path.as_str()), ("length", "$"));
    }

    #[test]
    fn the_path_names_the_record() {
        let rec = |amount: u64| map(&[("id", int("7")), ("amount", Value::Float(amount))]);
        let a = Value::List(vec![rec(1), rec(0x4078c0a3d70a3d70)]);
        let b = Value::List(vec![rec(1), rec(0x4078c0a3d70a3d71)]);
        let d = differs(&a, &b);
        assert_eq!(d.path, "$[1][\"amount\"]");
        assert_eq!(d.record.unwrap()["\"id\""], "7");
    }

    #[test]
    fn sets_ignore_order_and_nothing_else() {
        let a = Value::Set(vec![f(0.0), int("1")]);
        assert!(
            strict(&a, &Value::Set(vec![int("1"), f(0.0)]))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            differs(&a, &Value::Set(vec![int("1"), f(-0.0)])).kind,
            "set_element"
        );
    }

    #[test]
    fn a_tolerance_applies_only_where_named_and_is_reported() {
        let rec = |amount: u64| {
            Value::List(vec![map(&[
                ("amount", Value::Float(amount)),
                ("rate", Value::Float(amount)),
            ])])
        };
        let (a, b) = (rec(0x4078c0a3d70a3d70), rec(0x4078c0a3d70a3d71));
        let rule = FloatRule::new("$[*][\"amount\"]", Tolerance::Ulps(1), "test").unwrap();
        let relax = Relax {
            floats: vec![rule],
            ..Relax::default()
        };
        // "rate" has the same one-bit difference and no rule, so it still fails.
        let d = *compare(&Outcome::Returned(a.clone()), &Outcome::Returned(b), &relax).unwrap_err();
        assert_eq!(d.path, "$[0][\"rate\"]");

        let b = Value::List(vec![map(&[
            ("amount", Value::Float(0x4078c0a3d70a3d71)),
            ("rate", Value::Float(0x4078c0a3d70a3d70)),
        ])]);
        let applied =
            compare(&Outcome::Returned(a.clone()), &Outcome::Returned(b), &relax).unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].path, "$[0][\"amount\"]");
        assert_eq!(
            applied[0].original.bits.as_deref(),
            Some("0x4078c0a3d70a3d70")
        );

        // Two bits apart is outside one ulp.
        let b = Value::List(vec![map(&[
            ("amount", Value::Float(0x4078c0a3d70a3d72)),
            ("rate", Value::Float(0x4078c0a3d70a3d70)),
        ])]);
        assert!(compare(&Outcome::Returned(a), &Outcome::Returned(b), &relax).is_err());
    }

    #[test]
    fn a_tolerance_never_covers_nan_or_infinity() {
        let rule = FloatRule::new("$", Tolerance::Abs(f64::MAX), "test").unwrap();
        let relax = Relax {
            floats: vec![rule],
            ..Relax::default()
        };
        let run =
            |a: Value, b: Value| compare(&Outcome::Returned(a), &Outcome::Returned(b), &relax);
        assert!(
            run(
                Value::Float(0x7ff8000000000000),
                Value::Float(0x7ff8000000000001)
            )
            .is_err()
        );
        assert!(run(f(f64::INFINITY), f(f64::MAX)).is_err());
        assert_eq!(run(f(1.0), f(2.0)).unwrap().len(), 1);
    }

    #[test]
    fn tolerance_paths_are_parsed_strictly() {
        assert!(FloatRule::new("$[0][\"a]b\"][*]", Tolerance::Ulps(1), "r").is_ok());
        for bad in [
            "", "amount", "$.amount", "$[", "$[x]", "$[\"a\"", "$[*]tail",
        ] {
            assert!(
                FloatRule::new(bad, Tolerance::Ulps(1), "r").is_err(),
                "accepted {bad:?}"
            );
        }
        assert!(FloatRule::new("$", Tolerance::Ulps(1), " ").is_err());
        assert!(FloatRule::new("$", Tolerance::Abs(f64::NAN), "r").is_err());
        assert!(FloatRule::new("$", Tolerance::Abs(-1.0), "r").is_err());
    }

    #[test]
    fn ulps_count_across_zero() {
        let rule = FloatRule::new("$", Tolerance::Ulps(1), "r").unwrap();
        assert!(rule.allows((-0.0f64).to_bits(), 0.0f64.to_bits()));
        assert!(!rule.allows((-0.0f64).to_bits(), 1)); // two steps: -0.0, 0.0, 5e-324
    }

    #[test]
    fn exceptions_compare_by_type_and_message() {
        let raised = |ty: &str, msg: &str| raise(ty, false, Some(Value::Tuple(vec![s(msg)])), msg);
        let none = Relax::default();
        assert!(
            compare(
                &raised("builtins.ValueError", "bad"),
                &raised("builtins.ValueError", "bad"),
                &none
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            compare(
                &raised("builtins.ValueError", "bad"),
                &raised("builtins.TypeError", "bad"),
                &none
            )
            .unwrap_err()
            .kind,
            "exception_type"
        );
        assert_eq!(
            compare(
                &raised("builtins.ValueError", "bad"),
                &raised("builtins.ValueError", "worse"),
                &none
            )
            .unwrap_err()
            .kind,
            "exception_message"
        );
        let relax = Relax {
            exception_message: true,
            ..Relax::default()
        };
        let applied = compare(
            &raised("builtins.ValueError", "bad"),
            &raised("builtins.ValueError", "worse"),
            &relax,
        )
        .unwrap();
        assert_eq!(applied[0].relaxation, "exception_message");
        assert_eq!(
            compare(
                &raised("builtins.ValueError", "bad"),
                &Outcome::Returned(Value::None),
                &relax
            )
            .unwrap_err()
            .kind,
            "outcome"
        );
    }

    fn raise(ty: &str, own_class: bool, args: Option<Value>, msg: &str) -> Outcome {
        Outcome::Raised(Raise {
            ty: ty.to_string(),
            class_name: ty.rsplit('.').next().unwrap_or(ty).to_string(),
            own_class,
            lineage: vec![
                if own_class {
                    format!("own:{}", ty.rsplit('.').next().unwrap_or(ty))
                } else {
                    ty.to_string()
                },
                "builtins.Exception".to_string(),
                "builtins.BaseException".to_string(),
            ],
            message: s(msg),
            args,
        })
    }

    #[test]
    fn a_sides_own_exception_class_is_compared_by_class_name() {
        let none = Relax::default();
        let args = || Some(Value::Tuple(vec![s("m")]));
        let m = super::compare(
            &raise("orig.StoreError", true, args(), "m"),
            &raise("port.StoreError", true, args(), "m"),
            &none,
        )
        .unwrap();
        assert!(m.applied.is_empty());
        assert!(
            m.notes[0].contains("orig.StoreError and port.StoreError"),
            "{}",
            m.notes[0]
        );
        // Same full name: nothing to note.
        let m = super::compare(
            &raise("builtins.KeyError", false, args(), "m"),
            &raise("builtins.KeyError", false, args(), "m"),
            &none,
        )
        .unwrap();
        assert!(m.notes.is_empty());
        // A different class name is still a difference.
        let d = compare(
            &raise("orig.StoreError", true, args(), "m"),
            &raise("port.Corrupt", true, args(), "m"),
            &none,
        )
        .unwrap_err();
        assert_eq!(d.kind, "exception_type");
        // The rule is for own classes only: the same name from two foreign
        // modules, or own on one side only, does not match.
        for (a, b) in [(false, false), (true, false), (false, true)] {
            let d = compare(
                &raise("x.Error", a, args(), "m"),
                &raise("y.Error", b, args(), "m"),
                &none,
            )
            .unwrap_err();
            assert_eq!(d.kind, "exception_type");
        }
    }

    #[test]
    fn exception_args_are_compared_exactly() {
        let none = Relax::default();
        let with = |args: Option<Value>| raise("builtins.ValueError", false, args, "1");
        let int_arg = || Some(Value::Tuple(vec![Value::Int("1".to_string())]));
        let str_arg = || Some(Value::Tuple(vec![s("1")]));
        // str(e) is "1" for both; the arguments are not the same.
        let d = compare(&with(int_arg()), &with(str_arg()), &none).unwrap_err();
        assert_eq!(d.kind, "exception_args");
        assert!(
            compare(&with(int_arg()), &with(int_arg()), &none)
                .unwrap()
                .is_empty()
        );
        // Arguments that could not be encoded are never assumed equal.
        assert_eq!(
            compare(&with(None), &with(None), &none).unwrap_err().kind,
            "exception_args"
        );
        assert_eq!(
            compare(&with(int_arg()), &with(None), &none)
                .unwrap_err()
                .kind,
            "exception_args"
        );
        // Opting out of the message opts out of the arguments, and says so.
        let relax = Relax {
            exception_message: true,
            ..Relax::default()
        };
        let applied = compare(&with(int_arg()), &with(str_arg()), &relax).unwrap();
        assert_eq!(applied[0].relaxation, "exception_message");
        assert_eq!(compare(&with(None), &with(None), &relax).unwrap().len(), 1);
    }

    #[test]
    fn own_exception_classes_must_have_the_same_bases() {
        let none = Relax::default();
        let class = |module: &str, lineage: &[&str]| {
            Outcome::Raised(Raise {
                ty: format!("{module}.StoreError"),
                class_name: "StoreError".to_string(),
                own_class: true,
                lineage: lineage.iter().map(|c| c.to_string()).collect(),
                message: s("m"),
                args: Some(Value::Tuple(vec![s("m")])),
            })
        };
        let value_error = [
            "own:StoreError",
            "builtins.ValueError",
            "builtins.Exception",
            "builtins.BaseException",
        ];
        let exception = [
            "own:StoreError",
            "builtins.Exception",
            "builtins.BaseException",
        ];
        let m = super::compare(
            &class("orig", &value_error),
            &class("port", &value_error),
            &none,
        )
        .unwrap();
        assert!(
            m.notes[0].contains("StoreError < builtins.ValueError < builtins.Exception"),
            "{}",
            m.notes[0]
        );
        let d = compare(
            &class("orig", &value_error),
            &class("port", &exception),
            &none,
        )
        .unwrap_err();
        assert_eq!(d.kind, "exception_bases");
        assert_eq!(
            d.original.unwrap().repr,
            "StoreError < builtins.ValueError < builtins.Exception < builtins.BaseException"
        );
        assert_eq!(
            d.candidate.unwrap().repr,
            "StoreError < builtins.Exception < builtins.BaseException"
        );
        // An intermediate class of the side's own counts by name too.
        let via = [
            "own:StoreError",
            "own:LedgerError",
            "builtins.Exception",
            "builtins.BaseException",
        ];
        assert!(compare(&class("orig", &via), &class("port", &via), &none).is_ok());
        assert_eq!(
            compare(&class("orig", &via), &class("port", &exception), &none)
                .unwrap_err()
                .kind,
            "exception_bases"
        );
        // A side that sent no class line at all is never taken as agreeing.
        assert_eq!(
            compare(&class("orig", &[]), &class("port", &[]), &none)
                .unwrap_err()
                .kind,
            "exception_bases"
        );
    }

    #[test]
    fn large_dicts_and_sets_are_compared_by_lookup() {
        // 60,000 keys; with a scan per key this took many seconds.
        let key = |i: usize| s(&format!("k{i}"));
        let a = Value::Map((0..60_000).map(|i| (key(i), int("1"))).collect());
        let mut entries: Vec<(Value, Value)> = (0..60_000).map(|i| (key(i), int("1"))).collect();
        assert!(strict(&a, &Value::Map(entries.clone())).unwrap().is_empty());
        entries[59_999].1 = int("2");
        assert_eq!(
            differs(&a, &Value::Map(entries.clone())).path,
            "$[\"k59999\"]"
        );
        entries.reverse();
        assert_eq!(differs(&a, &Value::Map(entries)).kind, "key_order");
        let x = Value::Set((0..60_000).map(key).collect());
        let y = Value::Set((1..60_001).rev().map(key).collect());
        assert_eq!(differs(&x, &y).kind, "set_element");
    }

    // NumPy arrays.

    fn arr(dtype: Dtype, shape: &[usize], data: Vec<u8>) -> Value {
        Value::Array {
            dtype,
            shape: shape.to_vec(),
            data,
        }
    }
    fn f8s(xs: &[f64]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }
    fn f4bits(xs: &[u32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }
    fn with(rules: &[(&str, Tolerance)]) -> Relax {
        Relax {
            floats: rules
                .iter()
                .map(|(p, t)| FloatRule::new(p, t.clone(), "test").unwrap())
                .collect(),
            ..Relax::default()
        }
    }
    fn matched(a: &Value, b: &Value, relax: &Relax) -> Result<Matched, Diff> {
        super::compare(
            &Outcome::Returned(a.clone()),
            &Outcome::Returned(b.clone()),
            relax,
        )
    }
    /// One ulp up, for a positive float.
    fn up(x: f64, n: u64) -> f64 {
        f64::from_bits(x.to_bits() + n)
    }

    #[test]
    fn arrays_compare_dtype_then_shape_then_each_element() {
        let a = arr(Dtype::F8, &[2], f8s(&[1.0, 2.0]));
        assert!(strict(&a, &a).unwrap().is_empty());

        // float32 against float64 is never converted.
        let b = arr(
            Dtype::F4,
            &[2],
            f4bits(&[1.0f32.to_bits(), 2.0f32.to_bits()]),
        );
        let d = differs(&a, &b);
        assert_eq!(d.kind, "ndarray_dtype");
        assert_eq!(
            d.original.unwrap().repr,
            "<f8 shape (2,): [1.0, 2.0] (2 elements)"
        );
        assert_eq!(
            d.candidate.unwrap().repr,
            "<f4 shape (2,): [1.0, 2.0] (2 elements)"
        );

        let d = differs(&a, &arr(Dtype::F8, &[1, 2], f8s(&[1.0, 2.0])));
        assert_eq!((d.kind, d.path.as_str()), ("ndarray_shape", "$"));

        let d = differs(&a, &arr(Dtype::F8, &[2], f8s(&[1.0, 3.0])));
        assert_eq!((d.kind, d.path.as_str()), ("value", "$[1]"));
        assert_eq!(d.candidate.unwrap().repr, "3.0");

        // A path into a 2-d array names the row, then the column.
        let m = arr(Dtype::F8, &[2, 3], f8s(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]));
        let n = arr(Dtype::F8, &[2, 3], f8s(&[0.0, 1.0, 2.0, 3.0, 9.0, 5.0]));
        let d = differs(
            &Value::Tuple(vec![f(0.0), m]),
            &Value::Tuple(vec![f(0.0), n]),
        );
        assert_eq!(d.path, "$[1][1][1]");
    }

    #[test]
    fn an_array_is_never_equal_to_a_list() {
        let a = arr(Dtype::F8, &[1], f8s(&[1.0]));
        assert_eq!(differs(&a, &Value::List(vec![f(1.0)])).kind, "type");
        assert_eq!(differs(&Value::Tuple(vec![f(1.0)]), &a).kind, "type");
    }

    #[test]
    fn integers_and_bools_in_arrays_are_exact() {
        let a = arr(
            Dtype::I8,
            &[2],
            [1i64, -5].iter().flat_map(|x| x.to_le_bytes()).collect(),
        );
        let b = arr(
            Dtype::I8,
            &[2],
            [1i64, -6].iter().flat_map(|x| x.to_le_bytes()).collect(),
        );
        let d = *matched(&a, &b, &with(&[("$[*]", Tolerance::Abs(10.0))])).unwrap_err();
        assert_eq!((d.kind, d.path.as_str()), ("value", "$[1]"));
        assert_eq!(d.original.unwrap().repr, "-5");
        let d = differs(
            &arr(Dtype::B1, &[2], vec![1, 0]),
            &arr(Dtype::B1, &[2], vec![1, 1]),
        );
        assert_eq!((d.kind, d.path.as_str()), ("value", "$[1]"));
    }

    #[test]
    fn float32_elements_compare_as_their_own_bits() {
        let nan = |bits: u32| arr(Dtype::F4, &[1], f4bits(&[bits]));
        assert!(
            strict(&nan(0x7fc0_0000), &nan(0x7fc0_0000))
                .unwrap()
                .is_empty()
        );
        // Widened to binary64, these two NaNs could become one.
        let d = differs(&nan(0x7fc0_0000), &nan(0x7fc0_0001));
        assert_eq!(d.kind, "value");
        let o = d.original.unwrap();
        assert_eq!(
            (o.ty.as_str(), o.bits.as_deref()),
            ("float32", Some("0x7fc00000"))
        );
    }

    #[test]
    fn negative_zero_in_an_array_needs_a_rule_and_any_abs_covers_it() {
        let (a, b) = (
            arr(Dtype::F8, &[1], f8s(&[-0.0])),
            arr(Dtype::F8, &[1], f8s(&[0.0])),
        );
        assert_eq!(differs(&a, &b).kind, "value");
        assert!(matched(&a, &b, &with(&[("$[*]", Tolerance::Ulps(0))])).is_err());
        let m = matched(&a, &b, &with(&[("$[*]", Tolerance::Abs(0.0))])).unwrap();
        assert_eq!(m.arrays[0].needed, 1);
    }

    #[test]
    fn a_rule_inside_an_array_is_summarised_per_array_and_rule() {
        let x: Vec<f64> = (1..=10).map(f64::from).collect();
        let mut y = x.clone();
        y[3] = up(y[3], 1);
        y[5] = up(y[5], 2);
        y[7] = up(y[7], 1);
        let (a, b) = (
            arr(Dtype::F8, &[10], f8s(&x)),
            arr(Dtype::F8, &[10], f8s(&y)),
        );
        let m = matched(&a, &b, &with(&[("$[*]", Tolerance::Ulps(2))])).unwrap();
        // Each element is not listed on its own.
        assert!(m.applied.is_empty());
        let s = &m.arrays[..];
        assert_eq!(s.len(), 1);
        assert_eq!(
            (
                s[0].relaxation,
                s[0].path.as_str(),
                s[0].rule.as_str(),
                s[0].reason.as_str()
            ),
            ("float_ulps", "$", "$[*]", "test")
        );
        assert_eq!((s[0].elements, s[0].needed), (10, 3));
        assert_eq!(
            (s[0].largest_deviation.as_str(), s[0].largest_at.as_str()),
            ("2", "$[5]")
        );
        let firsts: Vec<&str> = s[0].first.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(firsts, ["$[3]", "$[5]", "$[7]"]);
        assert_eq!(
            s[0].first[0].candidate.bits.as_deref(),
            Some("0x4010000000000001")
        );

        // Only the first five places are listed; the count is all of them.
        let all: Vec<f64> = x.iter().map(|v| up(*v, 1)).collect();
        let m = matched(
            &a,
            &arr(Dtype::F8, &[10], f8s(&all)),
            &with(&[("$[*]", Tolerance::Ulps(1))]),
        )
        .unwrap();
        assert_eq!((m.arrays[0].needed, m.arrays[0].first.len()), (10, 5));
    }

    #[test]
    fn nodes_and_weights_each_get_their_own_rule_and_summary() {
        let pair = |x: &[f64], w: &[f64]| {
            Value::Tuple(vec![
                arr(Dtype::F8, &[2], f8s(x)),
                arr(Dtype::F8, &[2], f8s(w)),
            ])
        };
        let a = pair(&[-0.5, 0.5], &[1e-30, 1.0]);
        let b = pair(&[-0.5 - 1e-16, 0.5], &[up(1e-30, 3), 1.0]);
        let relax = with(&[
            ("$[0][*]", Tolerance::Abs(1e-15)),
            ("$[1][*]", Tolerance::Ulps(4)),
        ]);
        let m = matched(&a, &b, &relax).unwrap();
        let got: Vec<(&str, &str, usize)> = m
            .arrays
            .iter()
            .map(|s| (s.relaxation, s.path.as_str(), s.needed))
            .collect();
        assert_eq!(got, [("float_abs", "$[0]", 1), ("float_ulps", "$[1]", 1)]);
        assert_eq!(m.arrays[1].largest_deviation, "3");
        // The same tolerance in absolute terms says nothing about tiny weights.
        assert!(matched(&a, &pair(&[-0.5, 0.5], &[2e-30, 1.0]), &relax).is_err());
    }

    #[test]
    fn a_rule_one_level_short_leaves_the_elements_strict() {
        let a = arr(Dtype::F8, &[2, 2], f8s(&[1.0, 2.0, 3.0, 4.0]));
        let b = arr(Dtype::F8, &[2, 2], f8s(&[1.0, 2.0, 3.0, up(4.0, 1)]));
        let d = *matched(&a, &b, &with(&[("$[*]", Tolerance::Ulps(9))])).unwrap_err();
        assert_eq!((d.kind, d.path.as_str()), ("value", "$[1][1]"));
        assert!(matched(&a, &b, &with(&[("$[*][*]", Tolerance::Ulps(9))])).is_ok());
    }

    #[test]
    fn float32_takes_abs_and_refuses_ulps() {
        let one = 1.0f32.to_bits();
        let (a, b) = (
            arr(Dtype::F4, &[2], f4bits(&[one, one])),
            arr(Dtype::F4, &[2], f4bits(&[one, one + 1])),
        );
        let m = matched(&a, &b, &with(&[("$[*]", Tolerance::Abs(1e-6))])).unwrap();
        assert_eq!(
            (m.arrays[0].relaxation, m.arrays[0].needed),
            ("float_abs", 1)
        );
        assert!(matched(&a, &b, &with(&[("$[*]", Tolerance::Abs(1e-8))])).is_err());
        // An ulps rule that reaches a float32 element is an error of the
        // case, even where the elements are equal: its figure would count
        // binary64 steps.
        for b in [&a, &b] {
            let d = *matched(&a, b, &with(&[("$[*]", Tolerance::Ulps(1))])).unwrap_err();
            assert_eq!(
                (d.kind, d.path.as_str()),
                ("unsupported_relaxation", "$[0]")
            );
            assert!(
                d.note
                    .as_deref()
                    .unwrap()
                    .contains("ulps on float32 is not supported; use abs"),
                "{:?}",
                d.note
            );
        }
    }
}
