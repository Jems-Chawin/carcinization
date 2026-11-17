# Port of `relay.py` to Rust

## Structure

Everything is in `src/lib.rs`; the public interface (`Sim::new`, `Sim::tick`,
`Sim::state`, `run`) is unchanged and there are no dependencies.

- Nodes are indices into parallel vectors (`load`, `queues`, `links`, `sent`,
  `delivered`) instead of objects pointing at each other and at the hub.
- Each queue is a `VecDeque<Packet>`, so taking the head is O(1) (the Python
  `list.pop(0)` is O(queue length)). A packet is 24 bytes.
- Peers are a fixed array of at most 3 node ids per node, in the Python's
  peer order (next, previous, chord; self and duplicates skipped).
- The `count * count` traffic matrix is not stored. Only links can carry
  traffic, so a counter is kept per link and `state()` computes the same
  weighted sum from them. Memory is O(count), not O(count^2).
- A bitset marks the nodes whose queue is not empty, and `tick` visits only
  those, in ascending order. The word is re-read after each pump, so a node
  made busy by an earlier node in the same tick is still pumped in that tick,
  as in the Python.
- The label is not stored; `state()` computes the character sum of
  `"relay-%04d" % nid` directly (including ids with more than 4 digits).

## Unsure about / worth knowing

- Counters are `u64` and `state()` casts to `i64`. Python integers are
  unbounded, so the results would differ only past 2^63, which the latency
  and traffic sums do not reach in any realistic run.
- `Sim::new` panics if `count < 2` (the documented precondition; the Python
  misbehaves there too) or if `count` does not fit in `u32`.
- Beyond the golden tests I compared a hash of `run` against `relay.py` for 8
  more configurations (counts 2, 3, 4, 64, 65, 129, 257, 10007; seeds
  including 0 and 2^64-1; up to 20000 ticks): all identical. That check used
  a throwaway example which is not kept in the crate.
- Speed, measured once on this machine with a release build: 5,000,000 ticks
  of a 1000-node network in 3.7 s (about 0.74 microseconds per tick). I did
  not time the Python for the same run.

## Command output

```
$ cargo fmt --check
(exit 0)
```

```
$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
(exit 0)
```

```
$ cargo test
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.07s
     Running unittests src/lib.rs (target/debug/build/relay/3921f1860ac6d7cc/out/relay-3921f1860ac6d7cc)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/golden.rs (target/debug/build/relay/a52a467e261e6678/out/golden-a52a467e261e6678)

running 6 tests
test count_2_seed_1_ticks_50 ... ok
test count_3_seed_1_ticks_0 ... ok
test count_5_seed_2_ticks_50 ... ok
test count_32_seed_7_ticks_400 ... ok
test count_64_seed_123_ticks_300 ... ok
test count_17_seed_99_ticks_1000 ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests relay

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

(exit 0)
```

