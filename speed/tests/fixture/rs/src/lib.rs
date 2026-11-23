use pyo3::prelude::*;

/// Counts the extension's Rust allocations, for carcinify speed's counted
/// wheel, which is never timed.
#[cfg(feature = "count_allocations")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    pub static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    pub static FREES: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub static LIVE: AtomicU64 = AtomicU64::new(0);

    pub struct Counting;

    // `realloc` counts as one allocation of the new size, as carcinify
    // structure counts it.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
            LIVE.fetch_add(layout.size() as u64, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            FREES.fetch_add(1, Relaxed);
            LIVE.fetch_sub(layout.size() as u64, Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Relaxed);
            BYTES.fetch_add(new_size as u64, Relaxed);
            LIVE.fetch_add(new_size as u64, Relaxed);
            LIVE.fetch_sub(layout.size() as u64, Relaxed);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;
}

#[pymodule]
mod speed_port {
    use super::*;
    use std::cell::Cell;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// The workspace fill makes on its first call, and keeps.
    static FILL_WORKSPACE: OnceLock<Box<[u8]>> = OnceLock::new();
    const FILL_WORKSPACE_BYTES: usize = 4096;

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

    /// The sum of 0..n, through n boxes held in one vector: n + 1
    /// allocations and 16n bytes, all freed by the end of the call. The
    /// first call also makes the workspace, and keeps it.
    #[pyfunction]
    fn fill(n: usize) -> usize {
        std::hint::black_box(FILL_WORKSPACE.get_or_init(|| vec![0u8; FILL_WORKSPACE_BYTES].into_boxed_slice()));
        let boxes: Vec<Box<usize>> = (0..n).map(Box::new).collect();
        std::hint::black_box(&boxes).iter().map(|b| **b).sum()
    }

    /// n, after making a Python tuple and a dict and dropping both: what a
    /// binding does on its way, which the caller is not left with.
    #[pyfunction]
    fn temp(py: Python<'_>, n: usize) -> PyResult<usize> {
        let tuple = pyo3::types::PyTuple::new(py, [n, n])?;
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("n", n)?;
        drop((tuple, dict));
        Ok(n)
    }

    /// Allocations, frees, bytes allocated and live bytes, so far.
    #[cfg(feature = "count_allocations")]
    #[pyfunction]
    fn _speed_allocations() -> (u64, u64, u64, u64) {
        use crate::counting::{ALLOCATIONS, BYTES, FREES, LIVE};
        let now = |c: &std::sync::atomic::AtomicU64| c.load(Ordering::Relaxed);
        (now(&ALLOCATIONS), now(&FREES), now(&BYTES), now(&LIVE))
    }

    /// How many threads have made their workspace so far.
    #[pyfunction]
    fn workspaces() -> usize {
        WORKSPACES.load(Ordering::SeqCst)
    }
}
