//! The allocation budget of `DESIGN.md`: `Sim::tick` allocates nothing after
//! warm-up. This file holds a single test so that no other test thread
//! allocates while the count is taken.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use relay::Sim;

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: every call is passed straight to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const WARM_UP: usize = 1_000;
const MEASURED: u64 = 100_000;
const BUDGET_PER_TICK: u64 = 0;

#[test]
fn tick_allocates_nothing_after_warm_up() {
    for (count, seed) in [(2, 1), (5, 2), (17, 99), (64, 123), (1000, 5)] {
        let mut sim = Sim::new(count, seed);
        for _ in 0..WARM_UP {
            sim.tick();
        }
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..MEASURED {
            sim.tick();
        }
        let used = ALLOCATIONS.load(Ordering::Relaxed) - before;
        assert_eq!(
            used,
            BUDGET_PER_TICK * MEASURED,
            "count {count} seed {seed}: {used} allocations in {MEASURED} ticks"
        );
    }
}
