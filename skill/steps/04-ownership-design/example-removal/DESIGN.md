# Ownership design: `board.py`

Written before `rust/core/src/checked.rs`.

## References in the original

| Holder | Refers to | Direction | Used to change it? |
|---|---|---|---|
| `Board.tasks` | each `Task` on the board | down | yes: `task.work -= 1`; tasks are **removed** from it |
| `Task.blocker` | another `Task` | sideways | no. It is used for two things: `task.blocker in self.tasks` (is that very object still on the board?) and, in `snapshot`, `task.blocker.tid` |

Mutable containers reachable from more than one place: none.

**Are elements removed or reordered while something refers to them?** Yes.
`self.tasks.remove(task)` runs every tick, and a removed task may still be
some other task's `blocker`. Every later task moves up one place.

**Is anything compared by identity?** Yes: `task.blocker in self.tasks`.
`Task` defines no `__eq__`, so `in` asks whether that very object is in the
list.

**Does anything outlive its removal?** Yes. A removed task stays alive for
as long as another task's `blocker` refers to it, and `snapshot` reads its
`tid` then.

**Objects that code outside the port holds:** none. `run` returns lists of
integers.

## Hot path

- How it was identified: this example has one loop, `Board.tick`; there was
  nothing to profile. (For real code, see part 2 of the step.)
- One tick reads: for each task, whether its blocker is on the board.
- One tick writes: `work` of each unblocked task; removes finished tasks;
  appends to `done`.
- What one tick has to create: the new tasks, and one `done` entry per
  finished task. So the budget is not zero: storage for tasks is reused once
  the board has reached its working size, and `done` grows by design.
- **Allocation budget per tick:** only the growth of `done`, amortised. Not
  asserted here; this example is about indices, not allocations.

## Structure chosen

- Owner: `Board` owns the tasks, in slots that are reused.
- How a task refers to its blocker: a `Handle`, which is a slot number and
  the slot's generation when the handle was made. Removing a task raises the
  generation, so the handle stops matching and can never match whatever is
  put in the slot later. "Is my blocker still on the board?" is
  `self.get(handle).is_some()`.
- The blocker's `tid` is copied into the task when the reference is made,
  because Python can read it after the blocker has gone and a handle keeps
  nothing alive.
- Order: the Python list's order (oldest first, survivors keep their place)
  is kept in `order: Vec<Handle>`, separate from the slots.

## Rejected

| Alternative | Why not |
|---|---|
| `blocker: Option<usize>`, an index into the task list | This is `plain_index.rs`. Removal moves every later task up, so the index names another task or none. No error, wrong results: 20 of 20 seeds within 300 ticks. |
| A slot number with no generation | Survives the shift, not the reuse: when the slot is given to a new task, the old handle names the newcomer. |
| A stable id (`tid`) and a search of the list | Correct. `in` becomes a scan per task per tick, as in the Python; fine for a short list, and the simplest choice when the list is short or the check is rare. |
| A stable id and a table of live ids that never shrinks | Correct and fast; the table grows with every task ever made. |
| `Rc<Task>` in the list and `Weak<Task>` as the blocker | Says what the Python says, and `Weak::upgrade` answers "is it alive". But "alive" would then mean "something still holds it", not "on the board", and `work` would need a `Cell`. |

## Changes during the port

None.
