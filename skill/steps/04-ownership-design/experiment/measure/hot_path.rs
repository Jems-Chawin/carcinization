//! Measurement, not part of any port. `measure.py` copies this file into a
//! run's `tests/` directory after the run has finished; no sub-agent sees it.
//!
//! It prints one line of JSON: heap allocations and bytes requested per tick
//! in steady state, and nanoseconds per tick. Counters are process-wide, so
//! a port that used threads would be counted too.

use relay::Sim;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size() as u64, Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        BYTES.fetch_add(new_size as u64, Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const NODES: usize = 32;
const SEED: u64 = 7;
const WARM_UP: usize = 2_000;
const COUNTED: u64 = 2_000;
const TIMED: u64 = 200_000;

#[test]
fn hot_path() {
    let mut sim = Sim::new(NODES, SEED);
    for _ in 0..WARM_UP {
        sim.tick();
    }
    let before = (ALLOCATIONS.load(Relaxed), BYTES.load(Relaxed));
    for _ in 0..COUNTED {
        sim.tick();
    }
    let allocations = ALLOCATIONS.load(Relaxed) - before.0;
    let bytes = BYTES.load(Relaxed) - before.1;

    // Best of five, to take the edge off scheduling noise.
    let mut best = f64::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..TIMED {
            sim.tick();
        }
        best = best.min(start.elapsed().as_nanos() as f64 / TIMED as f64);
    }
    println!(
        "MEASURED {{\"allocations_per_tick\": {:.3}, \"bytes_per_tick\": {:.1}, \"ns_per_tick\": {:.1}}}",
        allocations as f64 / COUNTED as f64,
        bytes as f64 / COUNTED as f64,
        best
    );
}
