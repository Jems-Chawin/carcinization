//! `board_rs`: the checked port of `python/board.py`, reached through PyO3
//! so that carcinify diff can compare it with the original. (The plain-index
//! counter-example is not exposed; `cargo test` in `core` shows it failing.)

use board_core::{Snapshot, checked};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

fn to_python(py: Python<'_>, state: Snapshot) -> PyResult<Bound<'_, PyDict>> {
    let done = PyList::empty(py);
    for (tid, clock) in state.done {
        done.append(PyList::new(py, [tid, clock])?)?;
    }
    let open = PyList::empty(py);
    for (tid, work, blocker) in state.open {
        let row = PyList::empty(py);
        row.append(tid)?;
        row.append(work)?;
        row.append(blocker)?;
        open.append(row)?;
    }
    let out = PyDict::new(py);
    out.set_item("done", done)?;
    out.set_item("open", open)?;
    Ok(out)
}

#[pymodule]
mod board_rs {
    use super::*;

    /// `board.run(seed, ticks)`, by the checked port.
    #[pyfunction]
    fn run(py: Python<'_>, seed: u64, ticks: usize) -> PyResult<Bound<'_, PyDict>> {
        let state = py.detach(|| checked::run(seed, ticks));
        to_python(py, state)
    }
}
