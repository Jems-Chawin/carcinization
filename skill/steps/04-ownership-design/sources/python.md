# Ownership inventory: notes for Python sources

Python-specific help for parts 1 and 2 of `STEP.md`. Python is the only
source language the step has been used on.

## Where references hide in Python

Every Python name and attribute is a reference. The ones that matter are
those that let two places change the same object.

- **`self.x = x` in `__init__` where `x` is the creator or container.**
  `Unit(self, ...)` inside `Fleet.__init__`, `self.parent = parent`,
  `self.app = app`, `self.tree = tree`. This is the back-reference. Search
  for constructors that are passed `self`.
- **Attribute chains that go up and come down again:**
  `self.game.units`, `self.parent.children`, `self.fleet.heat[i] += 1`.
  Each one is a part reading or writing its container's other contents.
- **Lists and dicts handed over, not copied.** `self.items = items` stores
  the caller's list; both now change the same one. Likewise a default
  argument `def f(x, cache={})`, a class attribute that is a list or dict,
  and a module-level container. Look for containers assigned without
  `list(...)`, `dict(...)`, `.copy()` or a slice.
- **Objects stored in two containers**, such as a list in creation order and
  a dict by id holding the same objects. In Rust one of them owns the
  objects and the other holds indices or keys.
- **Closures and bound methods kept as callbacks** (`on_done=self.finish`):
  they hold `self`.
- **Iteration that changes what it iterates:** `for u in self.units:` whose
  body appends to, removes from or reorders `self.units`, or changes other
  elements of it. Python allows the second always and the first sometimes;
  note exactly which the code does, because the port has to do the same in
  the same order.

## Removal, identity, and objects that outlive their removal

These decide whether a plain index can stand in for a reference.

- **Removal and reordering.** Look for `list.remove`, `pop`, `del lst[i]`,
  a list rebuilt by a comprehension with a condition, `sort`, `reverse`,
  `insert`, and `del d[k]` or `d.pop(k)` on a dict. If any of them runs on a
  collection while another object refers to its elements, a position in that
  collection is not a stable name for an element.
- **Identity.** `a is b` and `a is not b` compare identity. So do `x in lst`,
  `lst.index(x)`, `lst.remove(x)` and `lst.count(x)` when the class of `x`
  defines no `__eq__`: they fall back to "the very same object". The same
  holds for objects used as dict keys or set members without `__hash__` and
  `__eq__`, and for `id(x)`. Check the class for `__eq__` before reading
  `in` as a comparison of values.
- **Life after removal.** An object removed from every collection is still
  alive while anything refers to it: `task.blocker.tid` works after the
  blocker has left the board. List what is read through such a reference;
  the port has to keep exactly that.

## What code outside the port holds

The port is one piece; its callers stay in Python.

- Find every place the piece's objects leave it: return values, arguments
  to callbacks, attributes other modules read (`board.tasks`), anything
  yielded.
- For each, find what the callers do: read attributes only; assign to them
  or call methods that change them; keep the object and look at it again
  later expecting to see changes made in the meantime; put it in their own
  dicts or sets; compare it with `is` or `==`.
  `grep -rn "\.tasks\b" --include="*.py" .` from the repository root, for
  each attribute and method of the piece, finds the callers.
- An object that callers only read can be handed out as a copy. One that
  they change, or watch, has identity across the boundary and needs a handle
  object on the Python side.

## Finding the hot path

Measure it; do not read it off the code.

```sh
python -m cProfile -o run.prof your_script.py   # a realistic workload
python -c "import pstats; pstats.Stats('run.prof').sort_stats('cumulative').print_stats(25)"
```

`cumulative` shows which function the time is under; `tottime` shows where
it is spent. `cProfile` slows code that makes many small calls, so the
proportions matter more than the seconds. A sampling profiler such as
`py-spy record` on the running program disturbs it less, if one is
installed. Record in the design note the command, the input, and the share
of time under the function you name as the hot path.

## Reading or writing?

For each reference, find the uses. It is a **write** if the code assigns to
an attribute or item through it (`other.fuel += 1`, `self.fleet.heat[i] = v`),
or calls a method that changes it (`append`, `pop`, `sort`, `update`,
`clear`, or any method of your own classes that assigns to `self`). A method
that only returns a value is a read. When unsure, read the method.

## Things that look alike and are not

- `a = b` never copies. `a = list(b)`, `b[:]` and `b.copy()` copy one level.
  Where the Python really does copy, the port needs a copy too; keep it and
  say so in the note.
- `x += 1` on an `int` attribute rebinds the attribute; `lst += [1]` changes
  the list in place for everyone who holds it.
- Integers do not overflow in Python. If a value can leave the range of the
  Rust type you choose, that is a difference in behaviour, and the
  comparison harness should have a case for it.
- Dicts keep insertion order, and code may depend on it.

## A quick way to get the list

```sh
grep -nE "self\.[a-z_]+ = (self|[a-z_]+)$|\(self[,)]" module.py   # stored references, constructors given self
grep -nE "self\.[a-z_]+\.[a-z_]+" module.py                       # reaching through an attribute
```

These find candidates. Read each one; the inventory is the result of the
reading, not of the search.
