//! `ledger_rs`: the Rust port of `python/ledger.py`, reached through PyO3.
//!
//! Free-threaded Python: the functions keep no state between calls and share
//! nothing, so the module is left at PyO3's default of declaring itself
//! thread-safe. It is not an abi3 build: the harness builds a wheel for the
//! one interpreter it is about to run.

use ledger_core::{self as store, Record, StoreError};
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyFloat, PyInt, PyList, PyString};
use std::path::PathBuf;

fn to_py_err(e: StoreError) -> PyErr {
    match e {
        StoreError::Io(io) => PyOSError::new_err(io.to_string()),
        other => PyValueError::new_err(other.to_string()),
    }
}

fn from_python(records: &Bound<'_, PyList>) -> PyResult<Vec<Record>> {
    let mut out = Vec::with_capacity(records.len());
    for item in records.iter() {
        // A missing key raises KeyError, as `record["id"]` does in Python.
        let id = item.get_item("id")?;
        let name = item.get_item("name")?;
        let amount = item.get_item("amount")?;
        out.push(
            Record::new(
                id.cast_exact::<PyInt>().ok().and_then(|i| i.extract().ok()),
                name.cast_exact::<PyString>()
                    .ok()
                    .and_then(|s| s.extract().ok()),
                amount.cast_exact::<PyFloat>().ok().map(|f| f.value()),
            )
            .map_err(to_py_err)?,
        );
    }
    Ok(out)
}

fn to_python<'py>(py: Python<'py>, records: &[Record]) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for r in records {
        let d = PyDict::new(py);
        d.set_item("id", r.id)?;
        d.set_item("name", &r.name)?;
        d.set_item("amount", r.amount)?;
        list.append(d)?;
    }
    Ok(list)
}

#[pymodule]
mod ledger_rs {
    use super::*;

    /// Writes the records to `path`. Nothing is written if any record is bad.
    #[pyfunction]
    fn save(py: Python<'_>, records: &Bound<'_, PyList>, path: PathBuf) -> PyResult<()> {
        let records = from_python(records)?;
        // File work needs nothing from the interpreter, so let it go.
        py.detach(|| store::save(&records, &path))
            .map_err(to_py_err)
    }

    /// Reads the records back from `path`.
    #[pyfunction]
    fn load(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyList>> {
        let records = py.detach(|| store::load(&path)).map_err(to_py_err)?;
        to_python(py, &records)
    }

    /// Saves the records into `directory` and loads them again.
    #[pyfunction]
    fn roundtrip<'py>(
        py: Python<'py>,
        records: &Bound<'py, PyList>,
        directory: PathBuf,
    ) -> PyResult<Bound<'py, PyList>> {
        let records = from_python(records)?;
        let path = directory.join("ledger.jsonl");
        let loaded = py
            .detach(|| store::save(&records, &path).and_then(|()| store::load(&path)))
            .map_err(to_py_err)?;
        to_python(py, &loaded)
    }

    /// The amount of the first record.
    ///
    /// A bug kept on purpose, for `suite-panic.json`. The Python raises
    /// `IndexError` for an empty list. This indexes without checking, so an
    /// empty list is a Rust panic, which PyO3 turns into
    /// `pyo3_runtime.PanicException`. That class derives from
    /// `BaseException`, so a caller's `except Exception` does not catch it.
    #[pyfunction]
    fn first_amount(records: &Bound<'_, PyList>) -> PyResult<f64> {
        let records = from_python(records)?;
        Ok(records[0].amount)
    }
}
