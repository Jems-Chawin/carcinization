# Worked example: a fleet that holds its units, and units that hold their fleet

`python/fleet.py` is the original: tankers on a grid. Each `Unit` keeps a
reference back to its `Fleet` and uses it to look at the other units, to
give fuel to one of them, and to write into the fleet's tallies. `Fleet.tick`
is the hot path.

There are two Rust ports of it in `rust/core/src/`, and both give exactly
the results of the original:

| | `appeasement.rs` | `designed.rs` |
|---|---|---|
| What it is | The counter-example, kept on purpose. The Python shape carried over, each borrow error answered with the smallest edit. Every such edit is marked `APPEASEMENT` with the error it silenced. | The port that follows `DESIGN.md`, the design note the step asks for. |
| `carcinify diff` against `fleet.py`, 35 cases | 35 identical, no relaxations | 35 identical, no relaxations |
| `cargo clippy -- -D warnings` | clean | clean |
| Allocations per tick, 24 units | **708** | **0** |
| Bytes requested per tick | 39,278 | 0 |

The last two rows are the point. Nothing in the first three tells the ports
apart.

## Read it in this order

1. `python/fleet.py`, noticing `self.fleet` in `Unit`.
2. `rust/core/src/appeasement.rs`: what happens if you port that as it is.
3. `DESIGN.md`: the inventory, the hot path, the structure chosen, what was
   rejected.
4. `rust/core/src/designed.rs`.
5. `rust/core/tests/allocations.rs`: the budget, as an assertion.

## Run it

The allocation numbers, from `rust/`:

```sh
cargo test --locked -- --nocapture
```

```
allocations per tick, 24 units, average over 100 ticks after 20 of warm-up:
  designed:         0.0 allocations,        0.0 bytes
  appeasement:    708.0 allocations,    39278.0 bytes
```

The test asserts a budget of zero for the designed port, with the reason in
a comment, and asserts that the counter-example stays costly, so that nobody
repairs it by accident.

Equivalence, from the repository root, with maturin 1.16 in `.venv` (see
`harness/README.md`):

```sh
cargo build --release
for port in appeasement designed; do
  target/release/carcinify diff skill/steps/04-ownership-design/example/suite-$port.json \
      --report target/carcinify-diff/fleet-$port.json \
      --python .venv/bin/python --maturin .venv/bin/maturin
done
```

Both exit 0 with `PASS`, 35 of 35 identical.

## What the example does not show

- Speed. The numbers are allocations, which is what the step asks to be
  budgeted. A benchmark from the Python caller is not part of this step, and
  this project has no step for it.
- A hot path that has to build output. There the budget is above zero and
  has to be argued; here it is zero and easy.
- `Rc<RefCell<..>>`. The counter-example shows cloning, collecting,
  `mem::take` and copy-out-and-write-back, which is what the borrow errors
  in this program lead to.
