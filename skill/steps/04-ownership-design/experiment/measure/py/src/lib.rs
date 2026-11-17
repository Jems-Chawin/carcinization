use pyo3::prelude::*;

#[pymodule]
mod relay_rs {
    use super::*;

    /// `run(count, seed, ticks)` of `relay.py`.
    #[pyfunction]
    fn run(py: Python<'_>, count: usize, seed: u64, ticks: usize) -> Vec<i64> {
        py.detach(|| relay::run(count, seed, ticks))
    }
}
