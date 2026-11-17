# Report: `depot.py` ported to Rust

## Structure

- All of the port is in `src/depot.rs`, in the order of `depot.py`: `Rng`,
  `Parcel`, `Zone`, `Courier`, `Depot`, `run`. Every Python method has a
  Rust method of the same name doing the same job, except `Courier.move`,
  which is `move_on` (`move` is a reserved word). `src/lib.rs` only
  re-exports.
- `Parcel`, `Zone` and `Courier` are cloneable handles onto shared state
  (`Rc` with `Cell`/`RefCell` fields), so an object a caller keeps goes on
  showing the live state and `set_capacity`/`rest` change the real courier.
  `==` on handles is Python's `is`.
- Each handle is `{ depot, state }`. Inside the states, the references that
  would form a circle (parcel to zone, parcel to carrier, courier to zone,
  zone to neighbour) are `Weak` and the depot is not referenced, so nothing
  leaks; a handle holds its depot strongly, so kept objects stay readable
  after the caller drops the `Depot`, as in Python.
- No borrow is held across a call into other objects or into a listener;
  loops over a list that the Python copies (or does not change) run over a
  snapshot.

## Checked beyond the given tests

I compared the port with `depot.py` (temporary driver, removed afterwards)
on nine argument sets, printing the full report and kept parcels after
every step. The driver also submitted parcels with origin equal to
destination, negative weights and a weight of 2^62, set capacities to
negative values, and rested couriers and lowered capacity from the
listener. All output was identical.

## Unsure about / differences from the Python

- `Depot::new` panics with "at least three zones" where Python raises
  `ValueError`; `tests/callers.rs` needs `new` to return a `Depot`, not a
  `Result`. Out-of-range zone numbers likewise panic (Python: `IndexError`).
- Argument types narrow what can be passed: the seed is `u64` (Python
  masks any integer to 64 bits), zone numbers are `usize` (no negative
  indexing), `rest` takes `u64` (a negative value is a no-op in Python).
  Weights and capacities are `i64` and may be negative; sums of them
  (load, congestion, fatigue) are `i128`, so they cannot overflow as they
  would not in Python. `load()` and `congestion()` therefore return `i128`.
- The internal methods (`act`, `settle`, `offer`, `retire`, ...) are
  private; callers get the readers, `rest`, `set_capacity`, and the depot's
  `submit`, `step`, `on_delivered`, `hire`, `courier`, `zone`, `report`.
  `Zone::waiting()` is the count (`len(zone.waiting)`), as the test needs;
  the list is `waiting_parcels()`.
- `Depot` cannot be cloned and its driving methods take `&mut self`, so a
  listener cannot submit or step during a delivery (possible in Python,
  not used by `callers.py`). Handles are `Rc`-based, so not `Send`.

## Output of the three commands

```
$ cargo fmt --check
(exit 0)

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
(exit 0)

$ cargo test
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
     Running unittests src/lib.rs (target/debug/build/depot/6c2a01950f2b86dd/out/depot-6c2a01950f2b86dd)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/callers.rs (target/debug/build/depot/ab8707bbb5b1c09c/out/callers-ab8707bbb5b1c09c)

running 2 tests
test callers_keep_objects_and_change_a_courier ... ok
test a_run_with_nobody_watching ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s

   Doc-tests depot

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

(exit 0)
```
