The run could not write `REPORT.md` (the file write was refused for a sub-agent). This is its final message, unchanged.

The port is done and all three checks pass, but `REPORT.md` is not written: the harness refused the file write ("subagents should return findings as text"). I did not work around that, so the report content is below instead and you will need to save it to `/private/tmp/w7/depot-3/REPORT.md` yourself.

## Structure

Everything is in `/private/tmp/w7/depot-3/src/lib.rs`, in the same order as `depot.py`: constants, `Rng`, `Parcel`, `Zone`, `Courier`, `Depot`, `run`. Every method of the original is there under the same name, except `Courier.move`, which is `move_on` because `move` is a Rust keyword.

- **Two types per class.** `XState` holds the attributes, with the changing ones in `Cell` or `RefCell`. `X` (`Parcel`, `Zone`, `Courier`, `Depot`) is an `Rc` handle carrying the methods; cloning it is another name for the same object, and `==` is Python's `is`.
- **No `Rc` cycles.** Ownership goes one way: the depot owns zones, couriers and open parcels; a zone owns its queue and the couriers in it; a courier owns its bag. Every reference back or sideways (`.depot`, `parcel.dest`, `parcel.carrier`, `courier.zone`, `zone.neighbours`) is `Weak`.
- **Kept objects stay usable.** A handle given to a caller also holds a strong reference to the depot, so it works after the caller drops the `Depot`, as in Python.
- **Listeners can touch anything they hold.** No `RefCell` borrow is held while a delivery listener runs.
- **Visibility.** Only what `tests/callers.rs` uses is `pub`, plus `Zone::id()`; the other methods are private.

## Checked beyond the given tests

I ran a temporary differential test against `depot.py` on 400 random cases (3–12 zones, 0–20 couriers, random 64-bit seeds, 0–400 steps), then removed it. Half were plain `run`. The other half used a caller that:

- submits parcels with origin equal to destination and with zero or negative weights;
- sets capacities to zero and negative values;
- calls `rest` with negative steps from the listener;
- reads kept parcels after dropping the depot.

All 400 matched.

## Unsure about / differences

- **Integer width.** Python integers are unbounded. Weights, capacity, load, congestion, fatigue and resting are `i64`, with `//` ported as `div_euclid` so negatives round the same way; ids, clock and counters are `u64`. Weights around 2^61 or more would overflow (panic in debug, wrap in release) where Python carries on.
- **Errors.** Fewer than three zones panics with "at least three zones" where Python raises `ValueError`, because the test file fixes `Depot::new` as returning `Depot`. An out-of-range zone number panics where Python raises `IndexError`. Zone numbers are `usize`, so Python's negative indexing has no counterpart.
- **Listener capturing a kept handle.** A listener that captures a `Courier`, `Zone` or `Parcel` forms a cycle through the depot that is never freed. Python's collector handles that; `Rc` does not. The test's listener only captures channel senders.
- **No re-entry into the depot.** `Depot` is not `Clone`, so a listener cannot call `submit` or `step`; in Python it could.

## Output of the checks

```
$ cargo fmt --check
(no output, exit 0)

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
(exit 0)

$ cargo test
     Running unittests src/lib.rs
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/callers.rs
test callers_keep_objects_and_change_a_courier ... ok
test a_run_with_nobody_watching ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
   Doc-tests depot
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`tests/callers.rs`, `tests/golden/` and `Cargo.toml` are unchanged.
