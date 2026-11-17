//! The ledger store in plain Rust, with no Python in sight: the port of
//! `python/ledger.py`. The `py` crate next door exposes it to Python.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: i64,
    pub name: String,
    pub amount: f64,
}

#[derive(Debug)]
pub enum StoreError {
    Id,
    Name,
    Amount,
    Io(std::io::Error),
    Json(serde_json::Error),
}

// The three validation messages are word for word those of ledger.py: the
// harness compares exception messages too.
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Id => f.write_str("id must be an integer from 0 to 2**63 - 1"),
            StoreError::Name => f.write_str("name must be a string"),
            StoreError::Amount => f.write_str("amount must be a finite float"),
            StoreError::Io(e) => write!(f, "{e}"),
            StoreError::Json(e) => write!(f, "{e}"),
        }
    }
}

impl Record {
    /// Checks one record in the order ledger.py does: id, name, amount.
    /// `None` means the Python value was not of the required type.
    pub fn new(
        id: Option<i64>,
        name: Option<String>,
        amount: Option<f64>,
    ) -> Result<Record, StoreError> {
        let id = id.filter(|id| *id >= 0).ok_or(StoreError::Id)?;
        let name = name.ok_or(StoreError::Name)?;
        let amount = amount.filter(|a| a.is_finite()).ok_or(StoreError::Amount)?;
        Ok(Record { id, name, amount })
    }
}

pub fn save(records: &[Record], path: &Path) -> Result<(), StoreError> {
    let mut out = BufWriter::new(std::fs::File::create(path).map_err(StoreError::Io)?);
    for r in records {
        serde_json::to_writer(&mut out, r).map_err(StoreError::Json)?;
        out.write_all(b"\n").map_err(StoreError::Io)?;
    }
    out.flush().map_err(StoreError::Io)
}

pub fn load(path: &Path) -> Result<Vec<Record>, StoreError> {
    let file = BufReader::new(std::fs::File::open(path).map_err(StoreError::Io)?);
    let mut records = Vec::new();
    for line in file.lines() {
        let line = line.map_err(StoreError::Io)?;
        records.push(serde_json::from_str(&line).map_err(StoreError::Json)?);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"id": 3, "name": "freight", "amount": 396.03999999999996}"#;

    /// Holds the fix in place: with `float-roundtrip`, a float written by
    /// Python is read back to the same bits.
    #[cfg(feature = "float-roundtrip")]
    #[test]
    fn a_float_written_by_python_is_read_exactly() {
        let r: Record = serde_json::from_str(LINE).unwrap();
        assert_eq!(r.amount.to_bits(), 0x4078c0a3d70a3d70);
    }

    /// Keeps the example honest: without the feature the bug is really there.
    /// If serde_json ever changes its default, this fails and the example
    /// needs a new demonstration.
    #[cfg(not(feature = "float-roundtrip"))]
    #[test]
    fn the_default_build_reads_the_float_one_bit_off() {
        let r: Record = serde_json::from_str(LINE).unwrap();
        assert_eq!(r.amount.to_bits(), 0x4078c0a3d70a3d71);
    }

    #[test]
    fn records_are_checked_in_the_original_order() {
        assert!(matches!(Record::new(None, None, None), Err(StoreError::Id)));
        assert!(matches!(
            Record::new(Some(-1), Some(String::new()), Some(1.0)),
            Err(StoreError::Id)
        ));
        assert!(matches!(
            Record::new(Some(1), None, Some(f64::NAN)),
            Err(StoreError::Name)
        ));
        assert!(matches!(
            Record::new(Some(1), Some(String::new()), Some(f64::INFINITY)),
            Err(StoreError::Amount)
        ));
        assert!(matches!(
            Record::new(Some(1), Some(String::new()), None),
            Err(StoreError::Amount)
        ));
        assert!(Record::new(Some(i64::MAX), Some(String::new()), Some(-0.0)).is_ok());
    }
}
