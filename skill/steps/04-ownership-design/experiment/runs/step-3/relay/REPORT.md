# Report: port of `relay.py` to Rust

## Structure

- `DESIGN.md` was written before any Rust (the ownership step in
  `step/STEP.md`); `src/lib.rs` follows it.
- `Sim` owns all state as parallel vectors indexed by node id: per-node
  `VecDeque<Packet>` queues, `loads`, `sent`, `delivered`, peer ids as an
  inline `[u32; 3]`, the `count*count` traffic table, and the labels kept
  apart as cold data. There is no node object: the `Node.hub`
  back-reference is gone and `Node.peers` became indices.
- `Node.pump` / `Node.pick` became `Sim::pump(nid)` / `Sim::pick(nid, dst)`.
  `Packet` is a 24-byte `Copy` value moved from queue to queue. Reads and
  writes are separated in time, so there are no clones, `collect`s or
  `mem::take` in anything reachable from `tick`.
- Order of events is that of the Python: clock, 3 injections, nodes pumped
  in index order in place, later nodes seeing earlier nodes' forwards.

## Checks

- `tests/golden.rs` passes unchanged (6 cases).
- `tests/alloc.rs` (new): counting global allocator; asserts exactly 0
  allocations and reallocations over 200 000 ticks after a 1 000-tick
  warm-up, for counts 2, 5, 17, 64, 500.
- Beyond the goldens, I compared `run` against `relay.py` on four more
  cases, (4, 3, 20000), (2, 2^64-1, 5000), (101, 0, 5000), (500, 7, 3000),
  with a temporary test that I then removed; all matched.
- Not measured: speed. No benchmark was run against the Python.

## Unsure about / worth knowing

- The first allocation measurement failed: 3 allocations in 200 000 ticks
  for count 17, from queues growing their ring buffer the first time they
  held a 5th packet. Fixed by reserving 32 slots per queue in `Sim::new`
  (longest queue seen in 300 000 Python ticks: 7). The budget stayed 0.
  Nothing in `relay.py` bounds queue length, so a queue that ever exceeds
  32 will reallocate once; that is not ruled out, only made very unlikely.
- Python integers do not overflow. Counters are `u64`; `state()` sums its
  weighted checksums in `i128` and panics if a value does not fit `i64`
  rather than returning something different from Python. Not reachable in
  any run I tried.
- Node ids are `u32`; `Sim::new` panics for `count < 2` or `count > u32::MAX`
  (the Python would misbehave or run out of memory there anyway).

## Output of the required commands

```
$ cargo fmt --check
exit status: 0

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.19s
exit status: 0

$ cargo test
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.08s
     Running unittests src/lib.rs (target/debug/build/relay/3921f1860ac6d7cc/out/relay-3921f1860ac6d7cc)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/alloc.rs (target/debug/build/relay/b400e3c0e1d09776/out/alloc-b400e3c0e1d09776)

running 1 test
test tick_allocates_nothing_after_warm_up ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.87s

     Running tests/golden.rs (target/debug/build/relay/a52a467e261e6678/out/golden-a52a467e261e6678)

running 6 tests
test count_2_seed_1_ticks_50 ... ok
test count_3_seed_1_ticks_0 ... ok
test count_5_seed_2_ticks_50 ... ok
test count_64_seed_123_ticks_300 ... ok
test count_32_seed_7_ticks_400 ... ok
test count_17_seed_99_ticks_1000 ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

   Doc-tests relay

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

exit status: 0
```
