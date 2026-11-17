The run could not write `REPORT.md` (the file write was refused for a sub-agent). This is its final message, unchanged.

The port is done and passes all three commands, but `REPORT.md` is not written: the harness refused the write (subagents may not create report files), and I did not work around it. The content it would have held is below, so you can save it as `/private/tmp/w7/depot-1/REPORT.md`.

## Structure

- **Layout:** everything is in `/private/tmp/w7/depot-1/src/lib.rs`, in the order of `depot.py`: constants, `Rng`, `Parcel`, `Zone`, `Courier`, `Depot`, `run`. Every Python method is there under the same name, except `Courier.move`, which is `r#move` because `move` is a Rust keyword.
- **Shared objects:** each class is two structs. `ParcelState`, `ZoneState`, `CourierState` and `DepotState` hold the attributes behind `Rc` with `Cell`/`RefCell`. `Parcel`, `Zone`, `Courier` and `Depot` are handles carrying that state plus `depot`, and the methods are on them.
- **Aliasing:** cloning a handle gives another name for the same object. A kept handle keeps showing current state, and `rest` / `set_capacity` act on the real courier, including from inside a delivery listener.
- **No reference cycles:** states name zones by number, a parcel holds its carrier through a `Weak`, and only handles point back at the depot. A kept handle keeps the depot alive, as in Python, and memory is freed once all are gone.
- **Visibility:** only what `tests/callers.rs` uses, plus read-only getters and `run`, is `pub`. The simulation's own methods (`act`, `post`, `retire` and the like) are private.
- **Numbers:** `Rng` is `u64` with wrapping arithmetic, which is the Python `& MASK`. Weights, capacities and rest times are `i64`. `load`, `congestion` and `fatigue` are `i128`, so any `i64` weights match Python's integers without overflow. `//` is `div_euclid`.

## Checking

Beyond the given tests, I compared the port with `depot.py` on 60 random configurations (3 to 40 zones, 0 to 40 couriers, 0 to 900 steps). That covered `run` and a harsher scenario than `callers.py`:

- origin equal to destination;
- zero, negative and `i64::MIN`/`MAX` weights;
- a listener that rests couriers and sets their capacity to zero or below.

All output was identical. The temporary files for this were removed.

## Unsure about

- **Errors:** where Python raises, the port panics. That covers `Depot::new` with fewer than three zones ("at least three zones") and an out-of-range zone number in `zone` or `submit`. `callers.rs` fixes `Depot::new` to return a `Depot`, not a `Result`.
- **Argument types:** `callers.rs` fixes `usize` zones and couriers and a `u64` seed. Python's negative indices, seeds outside 64 bits and weights beyond `i64` cannot be passed.
- **Listeners:** a listener gets `&Parcel` and `&Courier` and cannot call back into the depot, because `step`, `submit` and `on_delivered` take `&mut self`. A Python listener could; `callers.py` does not.
- **Threads:** the objects are `Rc`-based, so they are single-threaded (not `Send`).
- **`zone.waiting()`:** it returns the count, `len(zone.waiting)`, because that is what `callers.rs` asks for.

## Output of the three commands

```
$ cargo fmt --check
(no output, exit 0)

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
(exit 0)

$ cargo test
     Running unittests src/lib.rs
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/callers.rs
test callers_keep_objects_and_change_a_courier ... ok
test a_run_with_nobody_watching ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   Doc-tests depot
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`tests/callers.rs`, `tests/golden/` and `Cargo.toml` are unchanged, with no dependencies added.
