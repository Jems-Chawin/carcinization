//! `fleet_appeasement`: the `appeasement` port of `python/fleet.py`, reached through
//! PyO3 so that carcinify diff can compare it with the original.

use fleet_core::appeasement;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

#[pymodule]
mod fleet_appeasement {
    use super::*;

    /// Builds a fleet, runs it for `ticks` ticks and returns its state, in
    /// the shape `fleet.simulate` returns it.
    #[pyfunction]
    fn simulate(
        py: Python<'_>,
        count: usize,
        seed: u64,
        ticks: usize,
    ) -> PyResult<Bound<'_, PyDict>> {
        let state = py.detach(|| appeasement::simulate(count, seed, ticks));
        let units = PyList::empty(py);
        for (name, x, y, fuel) in state.units {
            let unit = PyList::empty(py);
            unit.append(name)?;
            unit.append(x)?;
            unit.append(y)?;
            unit.append(fuel)?;
            units.append(unit)?;
        }
        let out = PyDict::new(py);
        out.set_item("units", units)?;
        out.set_item("heat", state.heat)?;
        out.set_item("transfers", state.transfers)?;
        out.set_item("energy", state.energy)?;
        Ok(out)
    }
}
