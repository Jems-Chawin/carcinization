# Report: `relay.py` -> Rust

## Structure

Design decided first, in `DESIGN.md` (ownership inventory, hot path, budget).

- `Sim` owns everything. A node is an index into parallel columns
  (`load`, `sent`, `queues`, `peers`, `delivered`, `labels`); the Python's
  `Node.hub` back-reference and `Node.peers` object references are gone.
  Peers are a fixed `[u32; 3]` plus a length, in the Python's peer order.
- `Packet` is a 24-byte `Copy` struct moved by value between per-node
  `VecDeque`s (FIFO, O(1) pop instead of `list.pop(0)`).
- `Node.pump` became `Sim::pump(&mut self, i)`; `Node.pick` became a free
  function over the node's peers and the `load` slice, returning an index.
  Reads and writes are sequential, so there is no `Rc`, `RefCell`, clone,
  scratch buffer or `split_at_mut`.
- Order of events is the Python's: clock, inject, nodes pumped in index
  order with later nodes seeing earlier forwards in the same tick.

## Measurements

- Allocation budget for `tick` after warm-up: 0 (stated in `DESIGN.md` before
  the port). `tests/alloc.rs` (counting global allocator) measures 0
  allocations over 100,000 ticks after 1,000 warm-up ticks, for counts
  2, 5, 17, 64 and 500.
- `clone` / `to_vec` / `to_owned` / `to_string` / `mem::take` /
  `mem::replace` in `src/lib.rs`: 0. `collect`: 2, both in `Sim::new`.

## Unsure about / worth knowing

- "0 allocations" holds once every queue has reached its largest length. A
  queue that exceeds its capacity (16 initially) grows by doubling; that is
  rare and amortised but not impossible later in a long run. Nothing in the
  program bounds queue length, in Python either.
- Integer range: Python integers are unbounded. Hot-path counters are `u64`.
  `state()` returns `i64` (fixed by the interface); its weighted sums are
  computed in `i128` and the conversion panics instead of wrapping. The
  traffic checksum is the first value that could get there on a very long
  run with a large network. No golden case covers that.
- `Sim::new` asserts `count >= 2` and that `count` fits in `u32`; Python
  with `count` 0 or 1 fails differently (modulo by zero / `None` peer).
- Speed was not benchmarked against the Python; only the golden outputs and
  the allocation count were checked.
- `tests/golden.rs`, `Cargo.toml` untouched; no dependencies.

## Command output

```
$ cargo fmt --check
(no output, exit 0)

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
(exit 0)

$ cargo test
     Running unittests src/lib.rs
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/alloc.rs
test tick_does_not_allocate_after_warm_up ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.36s
     Running tests/golden.rs
test count_2_seed_1_ticks_50 ... ok
test count_3_seed_1_ticks_0 ... ok
test count_5_seed_2_ticks_50 ... ok
test count_32_seed_7_ticks_400 ... ok
test count_64_seed_123_ticks_300 ... ok
test count_17_seed_99_ticks_1000 ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests relay
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
