//! The value model both sides report their outputs in.
//!
//! Language-neutral. Every value travels as a tagged JSON object and no
//! number that matters is ever a JSON number: floats are sixteen hex digits
//! of their IEEE-754 binary64 bit pattern, integers are decimal strings.
//! The harness therefore never rounds, widens or normalises anything.
//! A NumPy array travels as its dtype, its shape and its elements' bytes in
//! C order, little-endian.

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
    /// A NumPy array: its elements' bytes in C order, little-endian, of
    /// length `shape`'s product times the dtype's size.
    Array {
        dtype: Dtype,
        shape: Vec<usize>,
        data: Vec<u8>,
    },
}

/// The array element types the format has. Others are refused by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dtype {
    F8,
    F4,
    I8,
    I4,
    B1,
}

impl Dtype {
    /// The name on the wire, NumPy's `dtype.str`.
    pub fn wire(self) -> &'static str {
        match self {
            Dtype::F8 => "<f8",
            Dtype::F4 => "<f4",
            Dtype::I8 => "<i8",
            Dtype::I4 => "<i4",
            Dtype::B1 => "|b1",
        }
    }

    fn parse(s: &str) -> Option<Dtype> {
        [Dtype::F8, Dtype::F4, Dtype::I8, Dtype::I4, Dtype::B1]
            .into_iter()
            .find(|d| d.wire() == s)
    }

    pub fn size(self) -> usize {
        match self {
            Dtype::F8 | Dtype::I8 => 8,
            Dtype::F4 | Dtype::I4 => 4,
            Dtype::B1 => 1,
        }
    }
}

/// One element of an array, read from its bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Elem {
    F8(u64),
    F4(u32),
    Int(i64),
    Bool(bool),
}

impl Elem {
    /// Element `i` (in C order) of an array's data.
    pub fn at(dtype: Dtype, data: &[u8], i: usize) -> Elem {
        let b = &data[i * dtype.size()..(i + 1) * dtype.size()];
        let n = |w: usize| {
            let mut a = [0u8; 8];
            a[..w].copy_from_slice(b);
            u64::from_le_bytes(a)
        };
        match dtype {
            Dtype::F8 => Elem::F8(n(8)),
            Dtype::F4 => Elem::F4(n(4) as u32),
            Dtype::I8 => Elem::Int(n(8) as i64),
            Dtype::I4 => Elem::Int(n(4) as u32 as i32 as i64),
            Dtype::B1 => Elem::Bool(b[0] == 1),
        }
    }

    pub fn show(self) -> Shown {
        let (ty, repr, bits) = match self {
            Elem::F8(b) => return Value::Float(b).show(),
            // Like f64's, f32's Debug is the shortest text that reads back
            // to the same bits.
            Elem::F4(b) => (
                "float32",
                format!("{:?}", f32::from_bits(b)),
                Some(format!("0x{b:08x}")),
            ),
            Elem::Int(n) => ("int", n.to_string(), None),
            Elem::Bool(b) => ("bool", b.to_string(), None),
        };
        Shown {
            ty: ty.to_string(),
            repr,
            bits,
        }
    }
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

fn hex_bytes(s: &str, what: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) || !lower_hex(s) {
        return Err(format!("{what} must be lower-case hex of even length"));
    }
    let nib = |c: u8| {
        if c.is_ascii_digit() {
            c - b'0'
        } else {
            c - b'a' + 10
        }
    };
    Ok(s.as_bytes()
        .chunks(2)
        .map(|p| nib(p[0]) << 4 | nib(p[1]))
        .collect())
}

fn canonical_int(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && s != "-0"
}

/// The fields of an `ndarray` wire value, checked against each other.
fn decode_array(m: &serde_json::Map<String, J>) -> Result<Value, String> {
    let name = field(m, "dtype")?
        .as_str()
        .ok_or("ndarray dtype must be a string")?;
    let dtype = Dtype::parse(name).ok_or_else(|| {
        format!("ndarray dtype {name:?} is not one of <f8, <f4, <i8, <i4 and |b1")
    })?;
    let shape = field(m, "shape")?
        .as_array()
        .ok_or("ndarray shape must be an array")?
        .iter()
        .map(|n| n.as_u64().and_then(|n| usize::try_from(n).ok()))
        .collect::<Option<Vec<usize>>>()
        .ok_or("ndarray shape must hold non-negative integers")?;
    let s = field(m, "hex")?
        .as_str()
        .ok_or("ndarray hex must be a string")?;
    let data = hex_bytes(s, "ndarray hex")?;
    let want = shape
        .iter()
        .try_fold(dtype.size(), |n, d| n.checked_mul(*d))
        .ok_or("ndarray shape has more elements than can be counted")?;
    if data.len() != want {
        return Err(format!(
            "ndarray of shape {shape:?} and dtype {name} needs {want} bytes, got {}",
            data.len()
        ));
    }
    if dtype == Dtype::B1 && data.iter().any(|b| *b > 1) {
        return Err("bool ndarray holds a byte other than 0 and 1".to_string());
    }
    Ok(Value::Array { dtype, shape, data })
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
                Ok(Value::Bytes(hex_bytes(s, "bytes hex")?))
            }
            "ndarray" => decode_array(obj(j, &["t", "dtype", "shape", "hex"])?),
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
            Value::Array { .. } => "ndarray",
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
            Value::Array { dtype, shape, data } => {
                let _ = write!(out, "a{}{shape:?}:", dtype.wire());
                for x in data {
                    let _ = write!(out, "{x:02x}");
                }
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
            Value::Array { dtype, shape, data } => {
                const SHOWN: usize = 8;
                let n = data.len() / dtype.size();
                let mut items: Vec<String> = (0..n.min(SHOWN))
                    .map(|i| Elem::at(*dtype, data, i).show().repr)
                    .collect();
                if n > SHOWN {
                    items.push("...".to_string());
                }
                let dims: Vec<String> = shape.iter().map(usize::to_string).collect();
                let dims = match dims.len() {
                    1 => format!("({},)", dims[0]),
                    _ => format!("({})", dims.join(", ")),
                };
                let unit = if n == 1 { "element" } else { "elements" };
                (
                    format!(
                        "{} shape {dims}: [{}] ({n} {unit})",
                        dtype.wire(),
                        items.join(", ")
                    ),
                    None,
                )
            }
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

    fn array(dtype: &str, shape: J, hex: &str) -> J {
        json!({"t": "ndarray", "dtype": dtype, "shape": shape, "hex": hex})
    }

    #[test]
    fn an_array_decodes_with_its_dtype_shape_and_bytes() {
        // 1.0 and 2.0 as little-endian binary64, in a 1 x 2 array.
        let v = Value::decode(&array(
            "<f8",
            json!([1, 2]),
            "000000000000f03f0000000000000040",
        ))
        .unwrap();
        let Value::Array { dtype, shape, data } = &v else {
            panic!("not an array: {v:?}");
        };
        assert_eq!(
            (*dtype, shape.as_slice(), data.len()),
            (Dtype::F8, &[1, 2][..], 16)
        );
        assert_eq!(v.type_name(), "ndarray");
        // Every dtype the format has, and a 0-d array (one element) and an
        // empty one (none).
        for (dtype, shape, hex) in [
            ("<f4", json!([1]), "0000803f"),
            ("<i8", json!([]), "ffffffffffffffff"),
            ("<i4", json!([0, 3]), ""),
            ("|b1", json!([2]), "0100"),
        ] {
            let j = array(dtype, shape, hex);
            assert!(Value::decode(&j).is_ok(), "refused {j}");
        }
    }

    #[test]
    fn an_array_outside_the_format_is_refused() {
        for bad in [
            // one byte short, and one too many
            array("<f8", json!([2]), "000000000000f03f00000000000000"),
            array("<f8", json!([1]), "000000000000f03f00"),
            // a shape whose element count overflows
            array("<f8", json!([4294967296u64, 4294967296u64, 2]), ""),
            // a dtype not in the format, and big-endian, which the encoder
            // never writes
            array("<c16", json!([0]), ""),
            array(">f8", json!([1]), "3ff0000000000000"),
            array("float64", json!([0]), ""),
            // a bool byte other than 0 and 1
            array("|b1", json!([1]), "02"),
            // shapes that are not lists of non-negative integers
            array("<f8", json!([-1]), ""),
            array("<f8", json!(2), "0000000000000000"),
            array("<f8", json!([1.5]), ""),
            json!({"t": "ndarray", "dtype": "<f8", "shape": [0]}),
            json!({"t": "ndarray", "dtype": "<f8", "shape": [0], "hex": "", "order": "C"}),
        ] {
            assert!(Value::decode(&bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn arrays_that_differ_in_dtype_shape_or_bytes_have_different_canonical_forms() {
        let forms: Vec<String> = [
            array("<f8", json!([1]), "000000000000f03f"),
            array("<i8", json!([1]), "000000000000f03f"),
            array("<f8", json!([1, 1]), "000000000000f03f"),
            array("<f8", json!([]), "000000000000f03f"),
            array("<f8", json!([1]), "000000000000f0bf"),
            array("<f4", json!([2]), "000000000000f03f"),
            json!({"t": "list", "v": [{"t": "float", "bits": "3ff0000000000000"}]}),
        ]
        .iter()
        .map(|j| Value::decode(j).unwrap().canon())
        .collect();
        let unique: BTreeSet<&String> = forms.iter().collect();
        assert_eq!(unique.len(), forms.len());
    }

    #[test]
    fn an_array_is_shown_by_dtype_shape_and_its_first_eight_elements() {
        let hex: String = (1..=10u64)
            .flat_map(|i| (i as f64).to_le_bytes())
            .map(|b| format!("{b:02x}"))
            .collect();
        let v = Value::decode(&array("<f8", json!([10]), &hex)).unwrap();
        let s = v.show();
        assert_eq!(s.ty, "ndarray");
        assert_eq!(
            s.repr,
            "<f8 shape (10,): [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, ...] (10 elements)"
        );
        let v = Value::decode(&array("|b1", json!([1, 2]), "0100")).unwrap();
        assert_eq!(
            v.show().repr,
            "|b1 shape (1, 2): [true, false] (2 elements)"
        );
        let v = Value::decode(&array("<i4", json!([]), "ffffffff")).unwrap();
        assert_eq!(v.show().repr, "<i4 shape (): [-1] (1 element)");
    }

    #[test]
    fn floats_are_shown_with_their_bits() {
        let v = Value::Float(0x4078c0a3d70a3d70).show();
        assert_eq!(v.repr, "396.03999999999996");
        assert_eq!(v.bits.as_deref(), Some("0x4078c0a3d70a3d70"));
    }
}
