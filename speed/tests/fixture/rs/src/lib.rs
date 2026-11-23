use pyo3::prelude::*;

#[pymodule]
mod speed_port {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// How many threads have made their workspace.
    static WORKSPACES: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        /// Whether this thread has made its workspace: on its first nap, in
        /// 5 ms, as a real port's buffers take time to set up.
        static WORKSPACE: Cell<bool> = const { Cell::new(false) };
    }

    /// Sleeps ten times as long as the original: a port slow on purpose.
    #[pyfunction]
    fn nap(ms: u64) -> u64 {
        WORKSPACE.with(|made| {
            if !made.get() {
                std::thread::sleep(Duration::from_millis(5));
                WORKSPACES.fetch_add(1, Ordering::SeqCst);
                made.set(true);
            }
        });
        std::thread::sleep(Duration::from_millis(10 * ms));
        ms
    }

    /// How many threads have made their workspace so far.
    #[pyfunction]
    fn workspaces() -> usize {
        WORKSPACES.load(Ordering::SeqCst)
    }
}
