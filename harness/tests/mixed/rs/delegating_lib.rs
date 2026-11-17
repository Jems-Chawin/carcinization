// Swapped in for src/lib.rs by the test of the auditor's N8: the compiled
// function hands the call to a copy of the original shipped in the wheel.
use pyo3::prelude::*;

pyo3::create_exception!(port_rs, StoreError, pyo3::exceptions::PyValueError);

#[pymodule]
mod _native {
    use super::*;

    #[pyfunction]
    fn scale<'py>(py: Python<'py>, x: i64) -> PyResult<Bound<'py, PyAny>> {
        py.import("port_rs._impl")?.getattr("scale")?.call1((x,))
    }

    #[pyfunction]
    fn check(n: i64) -> PyResult<i64> {
        if n < 0 {
            return Err(StoreError::new_err(("negative", n)));
        }
        Ok(n)
    }
}
