//! Checks the allocation budget stated in `DESIGN.md`: `Sim::tick` allocates
//! nothing, except that a queue reallocates when it becomes longer than it
//! has ever been. This lives inside the crate so that it can see the queue
//! capacities and check that exception exactly instead of assuming a
//! warm-up length.
//!
//! Allocations are counted per thread, so other test threads do not disturb
//! the count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use crate::Sim;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

struct Counting;

fn bump() {
    // `try_with`: the allocator may be called while a thread is torn down.
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        bump();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocs() -> u64 {
    ALLOCS.with(Cell::get)
}

const WARM_UP: usize = 20_000;
const MEASURED: usize = 200_000;

/// Runs `MEASURED` ticks after `WARM_UP` and returns (allocations, ticks in
/// which a queue grew), asserting on every tick that the only allocations
/// are queue growths: one per queue whose capacity rose.
fn measure(count: usize, seed: u64) -> (u64, u64) {
    let mut sim = Sim::new(count, seed);
    for _ in 0..WARM_UP {
        sim.tick();
    }
    // Scratch for the check itself, allocated before counting starts.
    let mut caps: Vec<usize> = sim.queues.iter().map(|q| q.capacity()).collect();
    let mut total = 0;
    let mut growth_ticks = 0;
    for _ in 0..MEASURED {
        let before = allocs();
        sim.tick();
        let used = allocs() - before;
        let mut grown = 0;
        for (cap, queue) in caps.iter_mut().zip(&sim.queues) {
            if queue.capacity() != *cap {
                assert!(queue.capacity() > *cap);
                *cap = queue.capacity();
                grown += 1;
            }
        }
        assert_eq!(
            used, grown,
            "count={count} seed={seed}: a tick allocated {used} times with {grown} queue growths"
        );
        total += used;
        growth_ticks += u64::from(grown > 0);
    }
    (total, growth_ticks)
}

#[test]
fn tick_allocates_only_when_a_queue_reaches_a_new_high_water_mark() {
    for (count, seed) in [(2, 1), (5, 2), (17, 99), (64, 123), (500, 4242)] {
        let (total, growth_ticks) = measure(count, seed);
        println!(
            "count={count} seed={seed}: {total} allocations in {MEASURED} ticks, \
             all queue growth, in {growth_ticks} ticks"
        );
        // Growth is rare, not a per-tick cost: fewer than 1 tick in 10 000.
        assert!(growth_ticks * 10_000 < MEASURED as u64);
    }
}

/// The counter itself works: building a network allocates.
#[test]
fn counter_counts() {
    let before = allocs();
    let sim = Sim::new(8, 1);
    assert!(allocs() > before);
    drop(sim);
}
