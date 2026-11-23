use pyo3::prelude::*;

#[pymodule]
mod speed_port {
    use super::*;

    /// Sleeps ten times as long as the original: a port slow on purpose.
    #[pyfunction]
    fn nap(ms: u64) -> u64 {
        std::thread::sleep(std::time::Duration::from_millis(10 * ms));
        ms
    }
}
