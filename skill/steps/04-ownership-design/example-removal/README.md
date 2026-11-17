# Second example: an index is a reference the compiler does not check

The step's advice for a back-reference is to replace it with an index into
the owner's collection. That is right while the collection only grows. This
example is about what happens when it does not.

`python/board.py` is a task board. Tasks are added, finished tasks are
removed, and a task can hold a reference to the task that blocks it. It asks
`task.blocker in self.tasks`: is that very object still on the board?

| | `rust/core/src/plain_index.rs` | `rust/core/src/checked.rs` |
|---|---|---|
| What the reference became | an index into the task list | a handle: slot number and generation |
| After a task is removed | every later task moves up; the stored index names a different task, or none | the slot's generation is raised; the handle no longer matches anything |
| Compiles, clippy clean, never panics | yes | yes |
| Results | **wrong** for 20 of 20 seeds within 300 ticks, with no error | those of the original |
| `carcinify diff` against `board.py`, 31 cases | not exposed to Python; see below | 31 identical, no relaxations |

The counter-example is kept on purpose and marked as such. Nothing in the
language objects to it: the borrow checker has no opinion about an integer.

## Read it in this order

1. `python/board.py`: `Task.blocker`, `in self.tasks`, `self.tasks.remove`.
2. `rust/core/src/plain_index.rs`: the obvious port.
3. `DESIGN.md`: the three inventory questions that catch it (are elements
   removed, is anything compared by identity, does anything outlive its
   removal) and the alternatives.
4. `rust/core/src/checked.rs`.
5. The tests at the bottom of `rust/core/src/lib.rs`.

## Run it

From `rust/`:

```sh
cargo test --locked -- --nocapture
```

```
seed 7: first removal at tick 2, results differ from tick 4
of 20 seeds, the plain index gives wrong results after 300 ticks for 20
```

Three tests: the checked port gives values computed by `board.py`; the two
ports agree until the first removal and differ soon after, without a panic;
and no handle to a removed task ever resolves to the task that later took
its slot (the test checks that slots really were reused).

Equivalence of the checked port, from the repository root:

```sh
target/release/carcinify diff skill/steps/04-ownership-design/example-removal/suite.json \
    --report target/carcinify-diff/board.json \
    --python .venv/bin/python --maturin .venv/bin/maturin
```

It exits 0 with `PASS`, 31 of 31 identical.

## What this example does not show

- Reordering (a sort). The same handle design covers it, since `order` can
  be sorted without moving the slots; it is not exercised here.
- Objects handed to Python callers. Nothing outside `run` holds a task.
- An allocation budget. The first example is about that.
