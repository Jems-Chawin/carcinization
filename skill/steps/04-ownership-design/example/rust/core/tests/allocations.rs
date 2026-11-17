//! Puts a number on the difference between the two ports: heap allocations
//! and bytes requested per tick of the hot path, counted by an allocator
//! that wraps the system one. No tool or dependency is needed, and the
//! budget is an assertion, so `cargo test` fails if the hot path starts
//! allocating.
//!
//! One test function, so nothing else allocates on this thread while it
//! counts (the counters are per thread in any case).

use fleet_core::{appeasement, designed};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|c| c.set(c.get() + 1));
        BYTES.with(|c| c.set(c.get() + layout.size() as u64));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|c| c.set(c.get() + 1));
        BYTES.with(|c| c.set(c.get() + new_size as u64));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocations and bytes requested while `work` runs, on this thread.
fn count(work: impl FnOnce()) -> (u64, u64) {
    let before = (ALLOCATIONS.get(), BYTES.get());
    work();
    (ALLOCATIONS.get() - before.0, BYTES.get() - before.1)
}

const UNITS: usize = 24;
const SEED: u64 = 7;
const WARM_UP: usize = 20;
const TICKS: u64 = 100;

#[test]
fn allocations_per_tick_of_the_hot_path() {
    let mut designed = designed::Fleet::new(UNITS, SEED);
    let mut appeasement = appeasement::Fleet::new(UNITS, SEED);
    for _ in 0..WARM_UP {
        designed.tick();
        appeasement.tick();
    }
    let (designed_allocations, designed_bytes) = count(|| {
        for _ in 0..TICKS {
            designed.tick();
        }
    });
    let (appeasement_allocations, appeasement_bytes) = count(|| {
        for _ in 0..TICKS {
            appeasement.tick();
        }
    });
    // Same work was done: the two fleets are in the same state.
    assert_eq!(designed.snapshot(), appeasement.snapshot());

    println!(
        "allocations per tick, {UNITS} units, average over {TICKS} ticks after {WARM_UP} of warm-up:"
    );
    println!(
        "  designed:    {:>8.1} allocations, {:>10.1} bytes",
        designed_allocations as f64 / TICKS as f64,
        designed_bytes as f64 / TICKS as f64
    );
    println!(
        "  appeasement: {:>8.1} allocations, {:>10.1} bytes",
        appeasement_allocations as f64 / TICKS as f64,
        appeasement_bytes as f64 / TICKS as f64
    );

    // THE BUDGET for the designed port: zero.
    //
    // A tick moves units that already exist on a grid that already exists.
    // It creates nothing that outlives it and returns nothing, so there is
    // nothing it needs the heap for. (A hot path whose job is to build
    // output would have a budget above zero, argued the same way.)
    assert_eq!(designed_allocations, 0, "the designed hot path allocated");
    assert_eq!(designed_bytes, 0);

    // The counter-example must stay a counter-example. Per tick it clones
    // every unit once per unit (a Vec and a name each), and more besides:
    // with 24 units that is over 24 * 24 allocations. If this assertion
    // fails, someone has repaired the module, and the example no longer
    // shows what it is for.
    assert!(
        appeasement_allocations / TICKS > (UNITS * UNITS) as u64,
        "the counter-example allocates {} times per tick; it is meant to be costly",
        appeasement_allocations / TICKS
    );
}
