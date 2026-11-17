# Ownership design: `fleet.py`

Written before `rust/core/src/designed.rs`. This is the design note the step
asks for, filled in for the example.

## References in the original

| Holder | Refers to | Direction | Used to change it? |
|---|---|---|---|
| `Unit.fleet` | the `Fleet` that contains it | up | yes: `fleet.transfers += 1`, `fleet.heat[i] += 1`, `fleet.energy += ...` |
| `Unit`, through `self.fleet.units` | every other `Unit` | sideways | yes: `other.fuel += 1` on the nearest one; all the others are only read |
| `Fleet.units` | each `Unit` | down | no (it only calls `unit.step()`) |

Mutable containers reachable from more than one place: `Fleet.units` and
`Fleet.heat`, both from the fleet and from every unit through its
back-reference.

## Hot path

- Entry point: `Fleet.tick`, which calls `Unit.step` for every unit in order.
- One step reads: position and fuel of every unit.
- One step writes: position and fuel of the stepping unit; fuel of one other
  unit; one cell of `heat`; `transfers`; `energy`.
- Needed during the same stretch of code: nothing. The search for the
  nearest unit reads all units and is finished before anything is written.
  All that has to survive from it is which unit and how far.
- What one tick has to create: nothing. Units and grid exist before it and
  after it; it returns nothing.
- **Allocation budget per tick, after warm-up: 0**, because a tick only
  changes numbers that already exist. Asserted in
  `rust/core/tests/allocations.rs`.

## Structure chosen

- Owner: `Fleet` owns everything. Units are plain data in its vectors.
- How parts refer to each other: by index into the fleet's vectors. A unit
  has no reference to the fleet.
- Where the hot-path logic lives: `Fleet::step(i)`. It needs all units, two
  of them mutably, and three other fleet fields, so it belongs to the fleet.
  The search is a free function over `&[Body]`.
- How the hot path gets what it needs at once: it does not need it at once.
  Read phase: `nearest(&self.bodies, me)` returns `(index, distance)`, two
  integers, and its shared borrow ends there. Write phase: `self.bodies[me]`
  and `self.bodies[other]` by index one after the other, then `heat`,
  `transfers`, `energy` as separate fields.
- Data split by use: `Body { x, y, fuel }` is `Copy` and is all the loop
  touches. Names are in a parallel `Vec<String>` that only `snapshot` reads.
- Behaviour that must stay exactly as it is: units step in index order and
  each sees the moves of those before it (no snapshot of the old state);
  the lowest index wins a distance tie; `energy` is added to once per step
  in that order, since floating-point addition is not associative.

## Rejected

| Alternative | Why not |
|---|---|
| `Unit` keeps a reference to the fleet (`&mut Fleet`, or a lifetime on `Unit`) | The fleet owns the units, so this is a value borrowing its own owner. It cannot be written. |
| `Rc<RefCell<Fleet>>` in every unit, `Rc<RefCell<Unit>>` in the fleet | It compiles and keeps the Python shape. Every access becomes a run-time borrow check that can panic, there is a reference cycle to break, and nothing here has more than one owner. |
| `unit.step(&others, ...)` on a clone of the unit list | This is `appeasement.rs`: correct results, and a copy of every unit, names included, for every step of every unit. Measured at 708 allocations per tick for 24 units. |
| Compute all moves from a snapshot, then apply them | Changes the results: in the original a unit sees the moves already made this tick. |
| `split_at_mut` to hold `&mut` to both units | Not needed. The two writes do not have to be held at the same time. |

## Changes during the port

None.
