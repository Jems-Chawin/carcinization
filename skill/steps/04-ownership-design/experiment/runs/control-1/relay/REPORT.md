# Port of `relay.py` to Rust

## Structure

Everything is in `src/lib.rs`; the public interface is unchanged.

- The Python object graph (hub, nodes, peers pointing at each other) is
  flattened into arrays owned by `Sim` and indexed by node id: `queues`,
  `peers`, `loads`, `sent`, `delivered`, `traffic`.
- Peers are node indices in a fixed three-slot array per node, built in the
  same order as `Hub.__init__` (next, previous, chord; no self, no duplicates).
- Loads are a dense vector of their own, since `pick` reads neighbours' loads.
- Each queue is a `VecDeque<Packet>`, so taking the head is O(1) where
  `list.pop(0)` is O(n). A packet is 24 bytes.
- `tick` skips nodes whose queue is empty (pumping one does nothing).
- Node labels are not stored; `state` computes the label's character sum
  from the node id.

## Checks beyond the golden tests

I compared `run` against `relay.py` on ten further inputs with a throwaway
binary (since deleted): counts 2, 3, 4, 7, 8, 65, 100, 129, 1000 and 12345
(five-digit labels), seeds including 0 and 2^64-1, up to 200,000 ticks. All
outputs were identical.

Speed, release build, 64 nodes: about 0.33 microseconds per tick, against
about 68 microseconds for the Python (roughly 200 times faster). Measured on
one machine with `time`, not a proper benchmark.

## Things I was unsure about

- Integer width. Python integers are unbounded; the port uses `i64`/`u64`.
  The weighted traffic sum in `state` is the first value that would
  overflow, and only after far longer runs than anything tested. In a
  release build it would wrap silently rather than fail.
- `Sim::new` panics if `count` does not fit in 32 bits (the traffic table
  would not fit in memory long before that). `count` of 0 or 1 is outside
  the contract and was not matched to the Python's behaviour.
- I tried a bitmap of non-empty queues so that `tick` could jump over idle
  nodes. It was about 25% faster at 1000 nodes but 60-75% slower at 64 and
  200, and a hybrid of the two was no better, so I removed it. If production
  networks are in the thousands of nodes and mostly idle, that is worth
  revisiting; the per-tick scan is O(count) as it stands.
- No benchmark or extra tests were added to the crate.

## Command output

```
$ cargo fmt --check; cargo clippy --all-targets -- -D warnings; cargo test
fmt exit 0
    Checking relay v0.1.0 (<scratch>/exp/control-1)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.89s
clippy exit 0
   Compiling relay v0.1.0 (<scratch>/exp/control-1)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.25s
     Running unittests src/lib.rs (target/debug/build/relay/3921f1860ac6d7cc/out/relay-3921f1860ac6d7cc)
     Running tests/golden.rs (target/debug/build/relay/a52a467e261e6678/out/golden-a52a467e261e6678)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 6 tests
   Doc-tests relay
test count_2_seed_1_ticks_50 ... ok
test count_32_seed_7_ticks_400 ... ok
test count_3_seed_1_ticks_0 ... ok
test count_5_seed_2_ticks_50 ... ok
test count_64_seed_123_ticks_300 ... ok
test count_17_seed_99_ticks_1000 ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

test exit 0
```
