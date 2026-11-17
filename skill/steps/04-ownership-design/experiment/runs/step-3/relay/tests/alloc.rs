//! The hot path's allocation budget from `DESIGN.md`: after warm-up,
//! `Sim::tick` allocates nothing.
//!
//! This file holds a single test so that nothing else runs on its thread;
//! the counter is per thread, so the test harness's own threads do not
//! disturb it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use relay::Sim;

thread_local! {
    // `const` initialisation and no destructor: safe to touch from inside
    // the allocator.
    static EVENTS: Cell<u64> = const { Cell::new(0) };
}

struct Counting;

fn record() {
    EVENTS.with(|events| events.set(events.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn events() -> u64 {
    EVENTS.with(Cell::get)
}

const BUDGET_PER_TICK: u64 = 0;
const WARM_UP: usize = 1_000;
const MEASURED: u64 = 200_000;

#[test]
fn tick_allocates_nothing_after_warm_up() {
    for (count, seed) in [(2, 1), (5, 2), (17, 99), (64, 123), (500, 7)] {
        let mut sim = Sim::new(count, seed);
        for _ in 0..WARM_UP {
            sim.tick();
        }
        let before = events();
        for _ in 0..MEASURED {
            sim.tick();
        }
        let used = events() - before;
        assert_eq!(
            used,
            BUDGET_PER_TICK * MEASURED,
            "count {count}, seed {seed}: allocations in {MEASURED} ticks"
        );
        // The counter does count: building the state vector allocates.
        let before = events();
        let state = sim.state();
        assert!(events() > before);
        assert_eq!(state.len(), count * 6 + 4);
    }
}
