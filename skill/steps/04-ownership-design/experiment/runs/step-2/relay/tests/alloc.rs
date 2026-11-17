//! The hot path's allocation budget from `DESIGN.md`: 0 per tick after
//! warm-up. This file holds a single test so that nothing else allocates
//! while the count is taken.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use relay::Sim;

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

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

const BUDGET_PER_TICK: u64 = 0;

#[test]
fn tick_does_not_allocate_after_warm_up() {
    for (count, seed) in [(2, 1), (5, 2), (17, 99), (64, 123), (500, 4)] {
        let mut sim = Sim::new(count, seed);
        for _ in 0..1_000 {
            sim.tick();
        }
        let ticks = 100_000;
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..ticks {
            sim.tick();
        }
        let used = ALLOCATIONS.load(Ordering::Relaxed) - before;
        assert!(
            used <= BUDGET_PER_TICK * ticks,
            "count {count}: {used} allocations in {ticks} ticks"
        );
        // The counter itself works: building the state allocates.
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let state = sim.state();
        assert!(ALLOCATIONS.load(Ordering::Relaxed) > before);
        assert_eq!(state.len(), count * 6 + 4);
    }
}
