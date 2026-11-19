# Step 4: Decide who owns what, before writing any Rust

Do this after the comparison harness exists for the code you are porting and
before the first line of the port. It takes less time than the port and it
decides how the port turns out.

## Why this step exists

Many source languages let any object hold a reference to any other and
change it at any time. Rust does not: at any moment a value has one owner,
and it can be reached through many shared references or one exclusive
reference, never both. A program written without that rule usually breaks it
in many places, most often like this: a small object holds a reference to
the large object that contains it (a unit holds its game, a node holds its
tree, a handler holds the app) and reaches everything else through it.

Carried over as it is, that structure does not compile, and each error has a
small local fix:

- clone the value, or the whole collection, so the borrow ends;
- `collect()` an iterator into a `Vec` so the thing iterated is released;
- take a field out of `self` with `std::mem::take`, use it, put it back;
- copy an item out, change the copy, write it back;
- wrap things in `Rc<RefCell<..>>` or `Arc<Mutex<..>>`.

Each of these compiles, and given the structure each one is necessary, so
the default lints do not flag it. The result can match the original bit for
bit and pass every comparison, and still copy data on every iteration that
the original only looked at. The errors were never about the lines they
pointed at. They were the compiler saying that nobody had decided who owns
what. This step makes that decision first and on purpose.

The step does not claim to prevent this pattern. This project has not seen
an agent produce it; what the step has been observed to do is leave a
decision and a measurement that a reviewer can check. Only Python sources
have been tried. The evidence for each claim is in `EVIDENCE.md`.

## What you hand over

A checklist. The parts below say how to produce each.

- The design note, committed before the port, and its later updates.
- The port, passing the comparison harness with no relaxations.
- The allocation test with its budget, and the construct counts.
- The list of objects that cross the boundary, as copies or as handles, for
  the next step.

## What to do

### 1. Inventory the references

Read the code being ported and write down, for each kind of object:

- what it holds a reference to (not a copy of);
- for each such reference, whether it is ever used to **change** the thing
  referred to, or only to read it;
- which references point **up or sideways**: from a part to its container,
  or between siblings. Mark these. They are where the trouble is.
- which mutable containers are reachable from more than one place.

Then three questions that decide whether an index will do (part 3):

- **Are elements removed or reordered while something refers to them?**
- **Is anything compared by identity** ("is this the same object", "is this
  object in that collection") and not by value?
- **Does anything outlive its removal?** In a garbage-collected language an
  object taken out of a collection lives on while something refers to it,
  and can still be read.

And one about the edge of the port:

- **Which objects does code outside the port hold references to, and what
  does it do with them:** only read them, change them, or expect to see
  changes made elsewhere? The port is usually one piece of a program whose
  other pieces stay as they are.

The notes for your source language (in `sources/`) say how to find these.
Keep the inventory short: a table with one row per reference is enough.

### 2. Find the hot path by measuring, and what it touches at once

The hot path is where the time goes, and that is a measurement, not a
reading. Take it from the profile made when the port was decided on (step 1
of the skill). If there is none, make one now: run the original on a
realistic workload under the source language's profiler and note which
functions hold most of the time. `sources/` says how. Write in the design
note how the hot path was identified: which profile, on what input, and the
share of time. If you had to judge it by reading because nothing can be run,
say so; a design built around the wrong loop gets a budget for the wrong
loop.

For one iteration of the hot path, list:

- what it reads;
- what it writes;
- which of those it needs **during the same stretch of code**. "Reads every
  unit while changing one unit" and "appends to a list it is iterating over"
  are the facts that decide the design.

Then ask what an iteration has to **create**. Often nothing: it changes
state that already exists. That gives the allocation budget for the hot
path, often zero after warm-up. If its job is to build output, the budget is
what the output needs and no more. Write the number down with its reason.

### 3. Choose the ownership structure

Decide, and be able to say in one sentence each:

- **Who owns each piece of data.** One owner. Usually the large object owns
  the collections, and the small objects become plain data inside them.
- **How a part refers to another part.** Not by a reference. By one of
  these, chosen from the answers to the three questions in part 1:
  - **A plain index** into the owner's collection, when elements are never
    removed or reordered while something refers to them. It is the cheapest
    choice and it is only safe then. The compiler does not check an index:
    after a removal a held index is still a valid number and now names a
    different element, with no error. That is the bug a reference would
    have prevented, and nothing reports it.
  - **A generation-checked handle** (a slot number plus the slot's
    generation, raised on every removal), when elements come and go and
    slots are reused. A handle to a removed element stops matching for
    good, so "is it still there" is one comparison.
  - **A stable id** (a key that is never reused) looked up in a map or a
    sorted list, when identity has to survive reordering or be shown to the
    outside.
  - Where the source compares by identity, the port compares handles or
    ids, never positions and never field values.
  - Where the source reads something from an object after its removal, a
    handle keeps nothing alive, so the value has to be kept some other way.
    If the value never changes (an id), copy it when the reference is made.
    If it can change, that copy would be out of date: copy it when the
    element is removed, or keep the removed element until nothing refers to
    it. Give the comparison harness a case that changes such a field, then
    removes the element, then reads it; a wrong choice shows there.
  The second example (`example-removal/`) shows a plain index going wrong
  and a handle that does not.
- **Where the logic lives.** A function that needs the whole container
  belongs to the container (or is a free function taking the fields it
  needs), not to the part. `unit.step()` reaching up into its fleet becomes
  `fleet.step(i)`.
- **How the hot path gets what it needs at once.** In rough order of
  preference:
  - Separate reading from writing in time. Read with shared access, keep
    only small values (an index, a number, a `Copy` struct), let that borrow
    end, then write.
  - Borrow separate fields separately. The compiler allows `&self.a` and
    `&mut self.b` together when it can see both fields, which it can inside
    one function or when the fields are passed as separate arguments. It
    cannot see through a method that takes all of `self`.
  - Split the data by how it is used: what the hot loop reads on every
    iteration apart from what it never touches (names, descriptions,
    anything owning heap memory). Copying three integers is free; cloning a
    struct with a `String` in it allocates.
  - Two items of one collection mutably: indices used one after the other,
    or `split_at_mut`, or `get_disjoint_mut`.
  - Buffers the loop needs are owned by the long-lived struct and reused,
    not created per iteration.
  - Shared ownership (`Rc`, `Arc`) and interior mutability (`RefCell`,
    `Mutex`) are for data that really has several owners or several
    threads. If you choose them, the design note says which owners and why
    nothing simpler fits. They are not a way to keep the source's shape.
- **What the outside is given.** The answer to the last inventory question
  limits the design. If callers that stay in the source language hold an
  object of the piece being ported, they must still have something to hold
  after the port, and what they do with it must still work:
  - if they only read it, the port can hand out a copy or a read-only view
    at the boundary, and inside be arranged however the hot path needs;
  - if they change it, or expect to see changes made by others through it,
    then it has identity across the boundary. Inside, the data may still
    live in the owner's collections; what the caller holds is a small
    object carrying a handle or stable id (never a plain index) whose
    methods go to the owner. Folding such objects into arrays with nothing
    left to hold changes behaviour the caller can observe.
  Decide here which objects cross the boundary and whether they are copies
  or handles, and write it in the note. Building that boundary (the classes
  the binding exposes, how they keep the owner alive, what crosses in bulk)
  is the next step of the skill, the port through PyO3, which is not
  written yet; this step hands it the list.

Keep observable behaviour exactly as it is, including the order in which
things happen. If the original updates items in place and later items see
earlier changes, the port does too. Ownership design changes who holds the
data, not what the program computes.

### 4. Write the design note, and commit it before any Rust

Write a short note next to the port (use `design-note-template.md`). It
holds the inventory, how the hot path was found and its budget, the
structure chosen, what crosses the boundary, and the alternatives you
rejected with one line each on why. The allocation budget is stated in the
note, as a number with its reason, before the port is written: afterwards it
is checked against the measured count, and a budget written after the
measurement checks nothing. Half a page to a page.

**Commit the note on its own, before the first Rust file exists.** Updating
it later is expected, and each update is its own change. The order is then
in the history, where a reviewer can see that the design came first, and
not only in your account of it.

Its purpose is that the decisions exist somewhere other than in the code, so
a reviewer can check the code against them and you can check yourself.
Writing "we clone the list each iteration because ..." in a note is harder
than typing `.clone()` at an error. That is the point of writing it.

### 5. While porting: treat repeated borrow errors as a design signal

One borrow error is a slip. Fix it. When you find yourself doing any of the
following, stop editing and go back to the note:

- a second or third borrow error in the same function or about the same
  field;
- reaching for `.clone()`, `.collect()`, `mem::take`, a copy-out-and-write-
  back, `Rc<RefCell<..>>` or `Arc<Mutex<..>>` **in order to make an error
  go away**, on the hot path, when the note did not call for it;
- adding a parameter that hands a function a copy of something it used to
  reach by reference.

Ask which decision in the note is wrong or missing: who owns this, where
this function should live, which data is being held across a write. Change
the design, update the note, then change the code. A clone is fine when the
program needs a second copy that will live its own life. It is the wrong
answer to "the compiler will not let me".

### 6. Measure the hot path

Equivalence is checked by the comparison harness. Structure is checked by
two measures, and each catches what the other cannot:

| Measure | Catches | Does not catch |
|---|---|---|
| **Allocations and bytes per iteration** of the hot path, against the budget | clones of anything that owns heap memory, `collect`, `to_vec`, `to_string`, buffers created per iteration | a `mem::take` of a field that is put back (it moves, it does not allocate); `Rc<RefCell<..>>` and `Arc<Mutex<..>>` (built once, then only counted and borrowed); a clone of a `Copy`-like struct |
| **A count of constructs** in the functions reachable from the hot path: `clone`, `collect`, `to_vec`, `to_owned`, `to_string`, `mem::take`, `mem::replace`, `Rc`, `RefCell`, `Arc`, `Mutex` | all of the above by name, including the ones that do not allocate | how much any of them costs; a copy made some other way |

For the first: a test with a counting global allocator, about twenty lines
and no dependency, that runs the hot path after a warm-up and asserts the
budget from the note. It needs `unsafe impl GlobalAlloc`, so put it in an
integration test file (`tests/allocations.rs`), not in `src/`: a test file
is its own crate, and the port itself can stay free of `unsafe`. For the
second, each construct that remains has a reason you can state.

Neither measure checks an index. That is what the three questions in part 1
are for, and a test that removes elements and then uses what referred to
them.

If the count is over the budget, the design is not finished. Do not raise
the budget to fit the code unless you can argue the new number as you argued
the first.
