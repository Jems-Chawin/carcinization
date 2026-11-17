use pyo3::prelude::*;

pyo3::create_exception!(port_rs, StoreError, pyo3::exceptions::PyValueError);

#[pymodule]
mod _native {
    use super::*;

    #[pyfunction]
    fn scale(x: i64) -> i64 {
        x * 3
    }

    #[pyfunction]
    fn check(n: i64) -> PyResult<i64> {
        if n < 0 {
            return Err(StoreError::new_err(("negative", n)));
        }
        Ok(n)
    }
}
