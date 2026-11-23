# The counting allocator

What step 7 asks the port's crate to carry, so that `carcinify speed build`
can count what a call costs in Rust allocations. Copy it as it is. It is
the one in `speed/tests/fixture/rs/src/lib.rs`, which speed's tests build
and count, with one `use` line added to the export so that it stands
alone; if the two ever differ otherwise, the fixture is right.

It is built only into the counted wheel, which speed build makes for
itself and never times. The wheel that is timed is built without it, and
speed refuses a timed wheel built with it.

## 1. The feature, in the crate's `Cargo.toml`

```toml
[features]
# carcinify speed's counted wheel: a counting allocator and its export.
count_allocations = []
```

## 2. The allocator, at the top of `src/lib.rs`

```rust
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
```

## 3. The export, in the `#[pymodule]`

```rust
    /// Allocations, frees, bytes allocated and live bytes, so far.
    #[cfg(feature = "count_allocations")]
    #[pyfunction]
    fn _speed_allocations() -> (u64, u64, u64, u64) {
        use crate::counting::{ALLOCATIONS, BYTES, FREES, LIVE};
        use std::sync::atomic::Ordering;
        let now = |c: &std::sync::atomic::AtomicU64| c.load(Ordering::Relaxed);
        (now(&ALLOCATIONS), now(&FREES), now(&BYTES), now(&LIVE))
    }
```

The name and the order of the four values are what speed reads. Inside a
`#[pymodule] mod`, as in the fixture, the function is exported as it
stands. A module written as `#[pymodule] fn` adds it there instead, under
the same `#[cfg]` (this form has been compiled and built, not yet counted
by speed):

```rust
    #[cfg(feature = "count_allocations")]
    m.add_function(wrap_pyfunction!(_speed_allocations, m)?)?;
```

## 4. A crate that sets its own allocator

A crate can have only one `#[global_allocator]`, so a crate that already
sets its own (mimalloc or jemalloc, say) fails to build with the feature:
"cannot define multiple global allocators". Its own steps aside in the
counted wheel, in one of two ways:

- Put `#[cfg(not(feature = "count_allocations"))]` on its static. The
  counted wheel then counts over `System`. The counts are those of the
  requests the code makes, whichever allocator serves them.
- Or wrap it: write its attribute as
  `#[cfg_attr(not(feature = "count_allocations"), global_allocator)]`, and
  put the static in place of `System` in `Counting`'s three calls, as in
  `unsafe { crate::OWN.alloc(layout) }`, dropping `System` from the `use`
  line.

Either way the timed wheel keeps the crate's own allocator.

Without the feature, speed times the port as before, and each target is
flagged `counts_not_measured`, naming what to add.
