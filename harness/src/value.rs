//! The value model both sides report their outputs in.
//!
//! Language-neutral. Every value travels as a tagged JSON object and no
//! number that matters is ever a JSON number: floats are sixteen hex digits
//! of their IEEE-754 binary64 bit pattern, integers are decimal strings.
//! The harness therefore never rounds, widens or normalises anything.

use serde::Serialize;
use serde_json::Value as J;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub enum Value {
    None,
    Bool(bool),
    /// Canonical decimal text, any size.
    Int(String),
    /// IEEE-754 binary64 bit pattern.
    Float(u64),
    /// A string made only of Unicode scalar values.
    Str(String),
    /// A string holding at least one lone surrogate, as code points.
    StrUnits(Vec<u32>),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    /// Entries in the order the side iterated them.
    Map(Vec<(Value, Value)>),
    /// Unordered; duplicates are rejected on decode.
    Set(Vec<Value>),
}

fn obj<'a>(j: &'a J, allowed: &[&str]) -> Result<&'a serde_json::Map<String, J>, String> {
    let m = j
        .as_object()
        .ok_or_else(|| format!("expected a tagged object, got {j}"))?;
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("unexpected field {k:?} in {j}"));
        }
    }
    Ok(m)
}

fn field<'a>(m: &'a serde_json::Map<String, J>, name: &str) -> Result<&'a J, String> {
    m.get(name).ok_or_else(|| format!("missing field {name:?}"))
}

fn lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn canonical_int(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && s != "-0"
}

impl Value {
    /// Decodes one wire value, rejecting anything not in the format.
    pub fn decode(j: &J) -> Result<Value, String> {
        let tag = j
            .get("t")
            .and_then(J::as_str)
            .ok_or_else(|| format!("value without a type tag: {j}"))?;
        match tag {
            "none" => {
                obj(j, &["t"])?;
                Ok(Value::None)
            }
            "bool" => {
                let m = obj(j, &["t", "v"])?;
                field(m, "v")?
                    .as_bool()
                    .map(Value::Bool)
                    .ok_or_else(|| "bool value is not true/false".to_string())
            }
            "int" => {
                let m = obj(j, &["t", "v"])?;
                let s = field(m, "v")?
                    .as_str()
                    .ok_or("int value must be a decimal string")?;
                if !canonical_int(s) {
                    return Err(format!("int is not canonical decimal: {s:?}"));
                }
                Ok(Value::Int(s.to_string()))
            }
            "float" => {
                let m = obj(j, &["t", "bits"])?;
                let s = field(m, "bits")?
                    .as_str()
                    .ok_or("float bits must be a string")?;
                if s.len() != 16 || !lower_hex(s) {
                    return Err(format!(
                        "float bits must be 16 lower-case hex digits: {s:?}"
                    ));
                }
                Ok(Value::Float(
                    u64::from_str_radix(s, 16).map_err(|e| e.to_string())?,
                ))
            }
            "str" => {
                let m = obj(j, &["t", "v", "cp"])?;
                match (m.get("v"), m.get("cp")) {
                    (Some(v), None) => v
                        .as_str()
                        .map(|s| Value::Str(s.to_string()))
                        .ok_or_else(|| "str value must be a string".to_string()),
                    (None, Some(cp)) => {
                        let arr = cp.as_array().ok_or("str cp must be an array")?;
                        let mut units = Vec::with_capacity(arr.len());
                        for c in arr {
                            let n = c
                                .as_u64()
                                .filter(|n| *n <= 0x10FFFF)
                                .ok_or_else(|| format!("str cp entry is not a code point: {c}"))?;
                            units.push(n as u32);
                        }
                        // One string has one representation: code-point form
                        // is kept only when it is needed.
                        match units.iter().map(|u| char::from_u32(*u)).collect() {
                            Some(s) => Ok(Value::Str(s)),
                            None => Ok(Value::StrUnits(units)),
                        }
                    }
                    _ => Err("str needs exactly one of \"v\" and \"cp\"".to_string()),
                }
            }
            "bytes" => {
                let m = obj(j, &["t", "hex"])?;
                let s = field(m, "hex")?
                    .as_str()
                    .ok_or("bytes hex must be a string")?;
                if s.len() % 2 != 0 || !lower_hex(s) {
                    return Err("bytes hex must be lower-case hex of even length".to_string());
                }
                let b = s.as_bytes();
                let nib = |c: u8| {
                    if c.is_ascii_digit() {
                        c - b'0'
                    } else {
                        c - b'a' + 10
                    }
                };
                Ok(Value::Bytes(
                    b.chunks(2).map(|p| nib(p[0]) << 4 | nib(p[1])).collect(),
                ))
            }
            "list" | "tuple" | "set" => {
                let m = obj(j, &["t", "v"])?;
                let arr = field(m, "v")?
                    .as_array()
                    .ok_or_else(|| format!("{tag} value must be an array"))?;
                let items = arr
                    .iter()
                    .map(Value::decode)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(match tag {
                    "list" => Value::List(items),
                    "tuple" => Value::Tuple(items),
                    _ => {
                        let mut seen = BTreeSet::new();
                        for i in &items {
                            if !seen.insert(i.canon()) {
                                return Err("set holds the same element twice".to_string());
                            }
                        }
                        Value::Set(items)
                    }
                })
            }
            "map" => {
                let m = obj(j, &["t", "v"])?;
                let arr = field(m, "v")?
                    .as_array()
                    .ok_or("map value must be an array of pairs")?;
                let mut entries = Vec::with_capacity(arr.len());
                let mut seen = BTreeSet::new();
                for pair in arr {
                    let p = pair
                        .as_array()
                        .filter(|p| p.len() == 2)
                        .ok_or("map entry must be a [key, value] pair")?;
                    let k = Value::decode(&p[0])?;
                    if !seen.insert(k.canon()) {
                        return Err("map holds the same key twice".to_string());
                    }
                    entries.push((k, Value::decode(&p[1])?));
                }
                Ok(Value::Map(entries))
            }
            other => Err(format!("unknown value type {other:?}")),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::None => "none",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) | Value::StrUnits(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
        }
    }

    /// An unambiguous text form: two values have the same form exactly when
    /// they are identical (set elements sorted, everything else in order).
    /// Used to look up map keys and set elements and to digest outcomes.
    pub fn canon(&self) -> String {
        let mut s = String::new();
        self.canon_into(&mut s);
        s
    }

    fn canon_into(&self, out: &mut String) {
        use std::fmt::Write;
        let seq = |out: &mut String, open: char, items: &[Value], close: char| {
            out.push(open);
            for i in items {
                i.canon_into(out);
                out.push(',');
            }
            out.push(close);
        };
        match self {
            Value::None => out.push('N'),
            Value::Bool(b) => out.push(if *b { 'T' } else { 'F' }),
            Value::Int(s) => {
                let _ = write!(out, "i{s};");
            }
            Value::Float(b) => {
                let _ = write!(out, "f{b:016x}");
            }
            Value::Str(s) => {
                let _ = write!(out, "s{}:{s}", s.len());
            }
            Value::StrUnits(u) => {
                out.push('u');
                for c in u {
                    let _ = write!(out, "{c:x}.");
                }
                out.push(';');
            }
            Value::Bytes(b) => {
                let _ = write!(out, "b{}:", b.len());
                for x in b {
                    let _ = write!(out, "{x:02x}");
                }
            }
            Value::List(v) => seq(out, '[', v, ']'),
            Value::Tuple(v) => seq(out, '(', v, ')'),
            Value::Map(v) => {
                out.push('{');
                for (k, val) in v {
                    k.canon_into(out);
                    out.push('=');
                    val.canon_into(out);
                    out.push(',');
                }
                out.push('}');
            }
            Value::Set(v) => {
                let mut items: Vec<String> = v.iter().map(Value::canon).collect();
                items.sort();
                out.push('<');
                for i in items {
                    out.push_str(&i);
                    out.push(',');
                }
                out.push('>');
            }
        }
    }

    /// Code points of a string value, for locating a difference.
    pub fn code_points(&self) -> Option<Vec<u32>> {
        match self {
            Value::Str(s) => Some(s.chars().map(|c| c as u32).collect()),
            Value::StrUnits(u) => Some(u.clone()),
            _ => None,
        }
    }

    pub fn show(&self) -> Shown {
        const MAX: usize = 200;
        let escape = |cps: &mut dyn Iterator<Item = u32>| {
            let mut s = String::from("\"");
            for (n, c) in cps.enumerate() {
                if n == MAX {
                    s.push_str("...");
                    break;
                }
                match char::from_u32(c) {
                    Some(ch) if (' '..='~').contains(&ch) && ch != '"' && ch != '\\' => s.push(ch),
                    _ => s.push_str(&format!("\\u{{{c:04X}}}")),
                }
            }
            s.push('"');
            s
        };
        let (repr, bits) = match self {
            Value::None => ("none".to_string(), None),
            Value::Bool(b) => (b.to_string(), None),
            Value::Int(s) if s.len() > MAX => {
                (format!("{}... ({} digits)", &s[..MAX], s.len()), None)
            }
            Value::Int(s) => (s.clone(), None),
            // Rust prints the shortest decimal that reads back to the same
            // bits, so two different floats never share a repr.
            Value::Float(b) => (
                format!("{:?}", f64::from_bits(*b)),
                Some(format!("0x{b:016x}")),
            ),
            Value::Str(s) => (escape(&mut s.chars().map(|c| c as u32)), None),
            Value::StrUnits(u) => (escape(&mut u.iter().copied()), None),
            Value::Bytes(b) => {
                let mut s: String = b.iter().take(MAX).map(|x| format!("{x:02x}")).collect();
                if b.len() > MAX {
                    s.push_str("...");
                }
                (format!("{s} ({} bytes)", b.len()), None)
            }
            Value::List(v) => (format!("list of {} item(s)", v.len()), None),
            Value::Tuple(v) => (format!("tuple of {} item(s)", v.len()), None),
            Value::Map(v) => (format!("map of {} entr(y/ies)", v.len()), None),
            Value::Set(v) => (format!("set of {} element(s)", v.len()), None),
        };
        Shown {
            ty: self.type_name().to_string(),
            repr,
            bits,
        }
    }
}

/// How a value appears in the report.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Shown {
    #[serde(rename = "type")]
    pub ty: String,
    pub repr: String,
    /// Present for floats: the binary64 bit pattern.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_values_outside_the_format() {
        for bad in [
            json!(1.5),
            json!({"t": "float", "bits": "3FF0000000000000"}),
            json!({"t": "float", "bits": "3ff"}),
            json!({"t": "float", "v": 1.0}),
            json!({"t": "int", "v": 7}),
            json!({"t": "int", "v": "007"}),
            json!({"t": "int", "v": "-0"}),
            json!({"t": "int", "v": "+1"}),
            json!({"t": "bytes", "hex": "0G"}),
            json!({"t": "str", "v": "a", "cp": [97]}),
            json!({"t": "str", "cp": [1114112]}),
            json!({"t": "set", "v": [{"t": "none"}, {"t": "none"}]}),
            json!({"t": "map", "v": [[{"t": "none"}, {"t": "none"}], [{"t": "none"}, {"t": "bool", "v": true}]]}),
            json!({"t": "decimal", "v": "1.0"}),
            json!({"t": "none", "extra": 1}),
        ] {
            assert!(Value::decode(&bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn code_point_form_of_a_plain_string_is_the_same_string() {
        let a = Value::decode(&json!({"t": "str", "v": "ab"})).unwrap();
        let b = Value::decode(&json!({"t": "str", "cp": [97, 98]})).unwrap();
        assert_eq!(a.canon(), b.canon());
        let lone = Value::decode(&json!({"t": "str", "cp": [55296]})).unwrap();
        assert!(matches!(lone, Value::StrUnits(_)));
    }

    #[test]
    fn canon_tells_apart_what_looks_alike() {
        let forms: Vec<String> = [
            json!({"t": "int", "v": "1"}),
            json!({"t": "bool", "v": true}),
            json!({"t": "float", "bits": "3ff0000000000000"}),
            json!({"t": "str", "v": "1"}),
            json!({"t": "bytes", "hex": "31"}),
            json!({"t": "list", "v": [{"t": "int", "v": "1"}]}),
            json!({"t": "tuple", "v": [{"t": "int", "v": "1"}]}),
            json!({"t": "float", "bits": "0000000000000000"}),
            json!({"t": "float", "bits": "8000000000000000"}),
        ]
        .iter()
        .map(|j| Value::decode(j).unwrap().canon())
        .collect();
        let unique: BTreeSet<&String> = forms.iter().collect();
        assert_eq!(unique.len(), forms.len());
    }

    #[test]
    fn floats_are_shown_with_their_bits() {
        let v = Value::Float(0x4078c0a3d70a3d70).show();
        assert_eq!(v.repr, "396.03999999999996");
        assert_eq!(v.bits.as_deref(), Some("0x4078c0a3d70a3d70"));
    }
}
