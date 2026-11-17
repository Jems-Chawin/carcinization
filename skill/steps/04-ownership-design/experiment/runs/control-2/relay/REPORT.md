# Port of `relay.py` to Rust

## Structure

Everything is in `src/lib.rs`; the public interface (`Sim::new`, `Sim::tick`,
`Sim::state`, `run`) is unchanged.

- No object graph. Nodes are a `Vec<Node>` addressed by id; a node's peers are
  a fixed `[u32; 3]` plus a count, in the same order the Python builds them.
  There are no back references to the hub and no `Rc`/`RefCell`.
- Each queue is a `VecDeque<Packet>` (the Python's `list.pop(0)` is O(n)); a
  packet is 24 bytes and is moved between queues, never allocated per hop.
- `Node.load` lives in its own dense `Vec<i64>`, which is all `pick` reads.
- `traffic` is stored per (node, peer slot), `3 * count` cells, instead of the
  Python's `count * count` matrix. Only those cells can ever be non-zero;
  `state()` weights each by its position in the full matrix, so the result is
  the same.
- Labels are not stored; `state()` computes the character sum of
  `"relay-%04d"` directly (including ids with more than four digits).
- `tick` does not allocate once the queues have reached their working size,
  so memory is flat over a long run.

## Checks beyond the golden tests

I compared `run` against `relay.py` byte for byte on seven more cases (counts
2, 3, 4, 7, 100, 1000, 12345; up to 200,000 ticks; seed `2**64 - 1`
included): identical. The scratch files for that were removed afterwards.

Rough timing on this machine, 64 nodes: Python 3.4 s for 50,000 ticks
(about 69 us per tick), Rust release 2.1 s for 5,000,000 ticks (about 0.42 us
per tick), roughly 160 times faster. One run each, not a careful benchmark.

## Things I was unsure about

- Python integers are unbounded; the port uses 64-bit counters. The values
  returned by `state()` (latency, the weighted pid and traffic sums) would
  wrap where Python would keep growing. That needs on the order of 10^18,
  far beyond millions of ticks on realistic sizes, but it is a difference.
- `Sim::new` panics for `count < 2` (the documented precondition) and for
  `count` above `u32::MAX`. The Python happens to work for `count == 1` and
  divides by zero for `count == 0`.
- Seeds are `u64`, so the Python's `seed & MASK` has no counterpart; negative
  or larger Python seeds cannot be passed.

## Command output

```
$ cargo fmt --check
(no output, exit 0)

$ cargo clippy --all-targets -- -D warnings
    Checking relay v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
(exit 0)

$ cargo test
running 0 tests   (unit tests, src/lib.rs)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 6 tests   (tests/golden.rs)
test count_3_seed_1_ticks_0 ... ok
test count_2_seed_1_ticks_50 ... ok
test count_5_seed_2_ticks_50 ... ok
test count_64_seed_123_ticks_300 ... ok
test count_17_seed_99_ticks_1000 ... ok
test count_32_seed_7_ticks_400 ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 0 tests   (doc-tests)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
(exit 0)
```
