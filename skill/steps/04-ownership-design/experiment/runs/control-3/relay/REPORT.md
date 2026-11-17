# Port of `relay.py` to Rust

## Structure

Everything is in `src/lib.rs`; the public interface (`Sim::new`, `Sim::tick`,
`Sim::state`, `run`) is unchanged.

- Nodes are indices. Their fields live in parallel vectors owned by `Sim`
  (`queues`, `peers`, `load`, `sent`, `delivered`), so there are no
  back-references to the hub and no `Rc`/`RefCell`. A tick allocates only
  when a queue outgrows its buffer.
- Queues are `VecDeque<Packet>` (the Python `pop(0)` is O(n)); a packet is
  24 bytes.
- Peers are a fixed `[u32; 3]` plus a length, in the Python's append order.
- `traffic` is stored per (node, peer slot), `3 * count` counters, instead of
  the `count * count` matrix. A node only ever sends to its peers, so the
  rest of the matrix is always zero; `state` weights each counter by its
  matrix position, giving the same number.
- A bitset marks the non-empty queues, and `tick` visits only those nodes, in
  ascending order. The word is re-read after every pump, because a pump can
  make a later node busy and that node must still be pumped in the same tick.
- The label is not stored; `state` formats it to compute the character sum.

## Checks beyond the golden file

`tests/golden.rs` only goes up to 64 nodes, which is exactly one word of the
bitset, so it does not exercise the multi-word path. I added `tests/wide.rs`:
10 more cases computed with `relay.py` (65 to 4097 nodes, up to 30000 ticks,
plus seeds 0, 2^64-1 and one above 2^63), compared by length and checksum.

Rough timing on this machine (release build, one run each, seed 1):

| nodes | Python      | Rust        |
|-------|-------------|-------------|
| 64    | ~72 us/tick | ~0.7 us/tick |
| 4096  | ~900 us/tick | ~1.0 us/tick |

## Unsure about

- Integer width. Python integers do not overflow; `state` returns `i64`. The
  counters and the two weighted sums (`(k+1) * pid` over a queue, and the
  traffic sum) are `i64` and would overflow on a long enough run: the traffic
  sum grows by roughly `count^2 / 2` per forwarded packet, so e.g. 4096 nodes
  reach `i64::MAX` after about 10^12 forwards. In a debug build that panics,
  in release it wraps. Nothing in the interface lets the result be wider, so
  I left it.
- `Sim::new` panics for `count < 2` (the Python misbehaves there too: count 0
  divides by zero in `below`, count 1 has a node with no peers) and for
  `count > u32::MAX`.
- Seeds: the Python masks the seed to 64 bits; the `u64` parameter already
  does that, so negative or larger Python seeds have no Rust counterpart.

## Command output

```
$ cargo fmt --check
(exit 0)
```

```
$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
(exit 0)
```

```
$ cargo test
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/lib.rs (target/debug/build/relay/3921f1860ac6d7cc/out/relay-3921f1860ac6d7cc)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/golden.rs (target/debug/build/relay/a52a467e261e6678/out/golden-a52a467e261e6678)

running 6 tests
test count_5_seed_2_ticks_50 ... ok
test count_2_seed_1_ticks_50 ... ok
test count_3_seed_1_ticks_0 ... ok
test count_32_seed_7_ticks_400 ... ok
test count_64_seed_123_ticks_300 ... ok
test count_17_seed_99_ticks_1000 ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/wide.rs (target/debug/build/relay/3d682111f4041af5/out/wide-3d682111f4041af5)

running 1 test
test wide_networks_match_python ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.67s

   Doc-tests relay

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

(exit 0)
```

