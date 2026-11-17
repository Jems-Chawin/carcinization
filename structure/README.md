# carcinify structure

Says how a Rust port is built, by measuring and counting. The comparison
harness answers "does it behave the same". This answers a narrower question
about the result: on the hot path, what does it allocate, what does it copy,
and does it use the constructs that usually mean nobody decided who owns
what.

It exists because lints do not see this. In the first example of step four,
`clippy -D warnings` is clean on both ports, while one allocates 708 times
per tick and the other not at all: every clone in the first is needed for
that code to compile, so no lint calls it redundant.

## Use

```sh
cargo build --release -p carcinify
target/release/carcinify structure path/to/structure.json --report target/carcinify-structure/report.json
```

Exit status: `0` pass, possibly with flags; `1` a check failed; `2` the run
could not be carried out (bad declaration, uncommitted changes, an entry
point that matches no function, a driver that is refused or does not
compile, a port with no `Cargo.lock`).
`--allow-dirty` runs on a tree with uncommitted changes and the report says
so. Needs `git` and `cargo`. Nothing is installed.

## The declaration

What the author states goes in one file, kept with the port. It comes from
the design note of step four.

```json
{
  "schema": "carcinify/structure/declaration/2",
  "crate": "rust/core",
  "design_note": "DESIGN.md",
  "features": ["float-roundtrip"],
  "no_default_features": false,
  "hot_paths": [{
    "entry": "designed::Fleet::tick",
    "budget": {"allocations_per_iteration": 0, "bytes_per_iteration": 0,
               "reason": "a tick only changes numbers that already exist"},
    "driver": {"input": "8 and 24 units placed by seed 7",
               "construct": "fleet_core::designed::Fleet::new",
               "sizes": [{"label": "8 units", "args": [8, 7]},
                         {"label": "24 units", "args": [24, 7]}],
               "call": {"method": "tick"},
               "warm_up": 20, "iterations": 100},
    "same_algorithm": {"same": true, "statement": "both search every other unit, as the original does"}
  }],
  "justifications": [
    {"check": "S4", "where": "Fleet::tick", "construct": "heat", "reason": "..."},
    {"check": "S7", "where": "struct Node", "construct": "Rc<RefCell<..>>", "reason": "owned by ..."}
  ]
}
```

- `features`, `no_default_features`: the Cargo features of the crate to
  measure with, written as in the harness's suite, so that both reports
  describe the same build. Left out, the default features are built. The
  report and the screen say which were built. A name the crate does not
  declare stops the run. Each feature set has its own build directory and
  build record. S1, S2 and the time are of that build; the counts read
  from the source (S3 to S7, `unsafe`) are of all the code, whatever
  feature it is behind.
- `entry`: `module::Type::function`, or as much of the end of that as is
  unique in the crate.
- `budget`: allocations and bytes per iteration, with the reason. It has to
  hold at every size.
- `driver`: the workload. It is data, and holds no Rust:
  - `construct`: the path of the function that builds the state. It is
    either a function of the entry point's own type
    (`my_crate::designed::Fleet::new`) or a function in the crate's
    `measure_support` module (below). Left out for a free function that
    takes no state.
  - `sizes`: at least two, each a label and the constructor's `args`.
    Arguments are numbers, booleans and strings. Two sizes may not be the
    same input. One size is accepted when `one_size_reason` says why the
    input has no size; the report is then flagged.
  - `call`: one iteration, which is always the entry point itself.
    `{"method": "tick", "args": [..]}` for a method;
    `{"function": "my_crate::load", "state": "shared", "args": [..]}` for a
    free function, where `state` says how it takes the constructed value
    as its first argument: `exclusive` (`&mut state`), `shared` (`&state`)
    or `none`. The tool does not copy the name into the program: it writes
    the entry point's own path as it read it from the crate,
    `my_crate::designed::Fleet::tick(&mut state)`, so the state has to be
    a `Fleet` of that module whatever built it.
  - an **input beside the state**: a size may carry
    `"input": {"function": "numbers", "args": [1000]}`, a function of
    `measure_support` called once before the loop, and the call's `args`
    may then contain `{"input": "shared"}` (`&input`) or
    `{"input": "exclusive"}` (`&mut input`).
  - `warm_up` iterations run before counting, `iterations` are counted.
  - `input` says in words what the input is.

  The tool cannot invent a workload, so a hot path without a driver fails.

### The support module

Many honest hot paths cannot be driven by a constructor and a few numbers:
a function that takes a path, a slice, a struct. Code that builds such
input for the measurement goes in **one module at the root of the crate,
`measure_support`** (`src/measure_support.rs`, or `pub mod measure_support
{ .. }` in `lib.rs`), as public functions that take numbers, booleans and
strings.

```rust
#[doc(hidden)]
pub mod measure_support {
    pub fn ledger_file(records: usize) -> std::path::PathBuf { /* write it, return the path */ }
    pub fn numbers(count: u64) -> Vec<u64> { (0..count).collect() }
}
```

`&state` and `&input` coerce as usual, so a `PathBuf` drives a function
that takes `&Path`, and a `Vec<T>` one that takes `&[T]`.

- The report lists, under each hot path, every function of the module the
  driver used, with file, line and length, marked "written by the author
  for this measurement", and flags it. No function elsewhere in the crate
  can build state or input, apart from functions of the entry point's own
  type.
- The module is code in the port. It counts in S6, S7 and the `unsafe`
  list like any other code, and in S3 to S5 only if a hot path reaches it,
  which it should not.
- An input passed by value on every iteration is not supported: it would
  have to be cloned in the loop, and the clone would be counted.

**What the support module opens.** It is where a convenient input can be
written. The function called is fixed by the tool and the state's type
with it, so a support function cannot swap the code that is measured. It
can build a state on which that code has nothing to do: a fleet with no
units whatever size is asked for, a simulation already at rest, a file
that is empty. Nothing in the tool stops that. What a reviewer has: the
function is named and flagged in the report with its place, the counts are
printed per size, sizes that measure the same are flagged (a function that
ignores its size gives that, unless the timing noise exceeds 10%: on a
machine busy with other work the same input has been seen to time 19%
apart, and the flag was missed. That was with each size timed after the
other; the passes are now taken in turn, which spreads the load over the
sizes and has not been measured on a busy machine. The time of every size
is printed in every report, flag or no flag, on the S9 line), and the
allocations during warm-up are in the report. Read the
support functions as part of reading the report.

- `same_algorithm`: the author's statement, for S9. It cannot be measured
  and is printed as "declared by the author".
- `justifications`: a reason for one S4 or S5 hit, or the owner of one S7
  construct. One that matches nothing fails the run, so the list cannot go
  stale.
- A crate with no hot path has `"hot_paths": []` and
  `"no_hot_path": {"reason": "..."}`. The verdict is then at best
  `pass_with_flags`, and the reason is printed. Without it an empty list
  fails.

Unknown fields are rejected. The declared values are copied into the report
as declared; every other number in it is measured or counted by the tool.

## The checks

| | What | Fails when |
|---|---|---|
| S1, S2 | Allocations and bytes requested per iteration of each hot path, measured at each declared input size | over the budget at any size; no budget; no reason for the budget; no driver. Flagged when the counts differ between sizes; when allocations were measured and S3 lists nothing that explains them; and when the sizes measured the same (allocations and bytes equal, time per iteration within 10% of each other), which is what a constructor or support function that ignores its argument gives |
| S3 | `clone`, `cloned`, `collect`, `to_vec`, `to_owned`, `to_string` in each function reachable from the hot path, with file and line; `format!` and `vec!` beside them. A reachable function that returns an owned collection (`Vec`, `VecDeque`, `String`, a map or a set) and contains one of these is listed with the number of calls to it written in the reachable functions, `calls_on_the_hot_path` | never: it is a count for a reader, set beside S1 |
| S4 | A field of `self` taken out with `mem::take`, `mem::replace`, `mem::swap` or `Option::take` and assigned again in the same function. A field reached through `.borrow_mut()`, `.lock()` or `.write()` on a field of `self` counts as one (`self.inner.borrow_mut().queue`) | there is no justification |
| S5 | `thread::spawn`, `thread::scope`, `.spawn(..)`, a thread pool or runtime, reachable from the hot path | there is no justification |
| S6 | Files over 1,000 lines not counting their `#[cfg(test)]` modules (test lines are given beside), functions over 150; `impl` blocks per type, fields per struct | never: flagged |
| S7 | `Rc<RefCell<..>>`, `Arc<Mutex<..>>` (and `RwLock`, `Cell`), `Weak<..>`, anywhere in the crate; and `Rc<T>` or `Arc<T>` where `T` is a struct of the crate with a field whose type names `Cell`, `RefCell`, `Mutex`, `RwLock` or an atomic | no owner is stated |
| `unsafe` | Every `unsafe` block, function and impl with its place; whether the crate has `#![forbid(unsafe_code)]`; test code listed apart | never: flagged when the port's own source has any |
| S9 | Nanoseconds per iteration from the same run, and the author's declaration on the algorithm | never: a missing declaration is flagged |

The two kinds of check cover each other. An allocation count misses a
`mem::take` that is put back and an `Rc<RefCell<..>>`, which do not
allocate per iteration. The construct counts catch those by name and say
nothing of cost.

What the construct counts are and are not:

- S3 counts constructs, not copies made. A helper that returns a new
  vector is one construct however often it is called, so
  `calls_on_the_hot_path` is printed beside it, and the sum for a hot path
  is `calls_to_functions_that_return_a_new_collection`. That is the number
  of calls written, not how often they run: a call in a loop is one. Which
  function a call reaches is decided by name, as everywhere in this tool:
  a call on a field is counted for every type with a method of that name,
  so a helper can read more calls than it has. The helper is recognised by
  the return type itself, not one wrapped in `Option` or `Result`, boxed,
  in a tuple or behind an alias.
- S4 sees a field reached through a borrow or a lock only when it is
  written in one expression. `let mut this = self.0.borrow_mut();` followed
  by `mem::take(&mut this.queue)` is not seen. The borrow has to be
  directly on a field of `self`, with only `unwrap` or `expect` after:
  `try_borrow_mut`, `.lock().unwrap_or_else(..)` and a field two levels
  down (`self.a.b.borrow_mut()`) are not seen. The report names the field
  as `self.queue` although it is inside the cell.
- S7 counts every `Weak<..>`, shared or not, so a port with one owner and
  back-references has a count from those alone. It finds the types named
  in struct fields and inside function bodies, not those in a function's
  signature. `Rc<T>` is matched to `T` by the name of the type, and only a
  struct's own fields are read: cells one level further in, in a field's
  own type, are not followed, and neither is a type alias for the handle.
  Structs, not enums: an enum's fields are not read. A name the crate
  declares itself is not taken for a cell (a spreadsheet's `Cell`), so a
  crate that has its own `Cell` and also uses the standard one is not
  counted for either; and nothing inside `PhantomData` counts.

Verdict: `fail` if any check failed, `pass_with_flags` if there is
something for a reader to look at, else `pass`.

### How S1 and S2 are measured

The tool writes a small program with a global allocator that counts every
`alloc` and `realloc` and the bytes requested, process-wide. For each
driver and size it builds the state, runs `warm_up` iterations, then counts
over `iterations`. The same run times the iterations: five rounds over all
drivers and sizes in turn, best of five for each.
It is built in release mode in `target/carcinify-structure/` of the crate's
workspace. The command is in the report.

**No text from the declaration is compiled.** The program is written from
the checked pieces: a path made of names, literals, and the method or
function name. Under each hot path the screen and the report show the Rust
that was written (`fleet_core::designed::Fleet::new(24, 7)`,
`state.tick()`).

**The result line cannot be printed by anyone else in advance.** Each run
makes up a token; only lines that start with it are results, and there
must be exactly one per measurement, in order. Anything else on standard
output is ignored, and one line too many fails the run. The counters are
private to a module of the generated program.

**A declaration that does not compile is reported as that.** The program
is written from the declaration, so a wrong argument type or a private
function shows up as a compiler error in a file the author never saw. The
tool finds which declared piece the error is in and says so: the hot path,
the size, the field (`construct`, the size's `input`, or `call`), the Rust
it wrote from it, and the compiler's message.

**The build is of the commit.** Cargo decides by file time whether to
rebuild. Before building, the tool compares the sources with the record of
the last build (the harness's code for this: git tree ids on a clean tree)
and cleans the crate's packages, path dependencies included, when they
differ. `rebuilt_from_clean` in the report says whether it did.

**The dependency versions are the port's.** The port's `Cargo.lock` is
copied beside the probe. Cargo has to add the probe itself to it, so
`--locked` cannot be passed; after the build every package in the probe's
lock file must be in the port's with the same version and checksum, or the
run fails. A port with no lock file is not measured.

**The entry point is called by a public path.** The probe uses the crate
from outside, like an integration test. The tool works the path out from
the source:

- if every module on the way to the entry point is `pub`, the path it is
  written at;
- otherwise a path given by a `pub use`, of the item, of a module above
  it, or a glob (`mod inner; pub use inner::Sim;` gives `my_crate::Sim`);
- for a method written in `impl Trait for Type`, the fully qualified form,
  `<my_crate::Sim as my_crate::Step>::tick(&mut state)`, with `_` for each
  generic argument of the type (`<my_crate::Bag<_> as ..>`), and the
  trait's name as written when it comes from the standard library or a
  dependency.

Cases that cannot be written, each with a message that says so: an entry
point whose type has no public path at all; a method of a type from
another crate; a method that takes `self` by value. Not handled, and
failing with the compiler's message traced to `call`: a trait with generic
arguments of its own (`impl From<X> for Y`), and a private function or
method on a public path, since visibility of functions is not read. A
constructor has to be written through the type's own name: a type exported
only under another name (`pub use inner::Sim as Engine`) needs a support
function.

### What a declaration can still get wrong

Ways to a pass that this tool does not close. Each needs something a
reviewer can see in the declaration, the report or the port.

- **A budget set to fit.** The number and its reason are typed. The report
  prints both; step six says where a budget may be changed.
- **Sizes that are both small**, or a warm-up long enough that the work
  has run out before counting starts. Both are printed; allocations per
  iteration during warm-up are in the report for the second.
- **A justification for a construct that is wrong.** Printed beside its
  hit.
- **`no_hot_path` for a crate that has one.** Flagged with the reason.
- **A support function that builds a convenient input.** See "What the
  support module opens" above.
- **A port that behaves differently when measured**: one that looks at
  the name of the running program, or reads the probe's source for the
  token and prints result lines before exiting. That is fraud written in
  the port's own source, where `std::env`, `std::fs` and `process::exit`
  on a hot path are plain to see. The tool does not look for it.
- **`--allow-dirty`.** The report says the tree was not clean.

## How functions are found, and what is missed

Everything but S1 and S2 is read from the source with `syn`. Nothing is
type-checked. The rules, exactly:

- **What is read.** Every `.rs` file under the crate's `src/`. Items marked
  `#[cfg(test)]` or `#[test]`, and files that are a `#[cfg(test)] mod x;`,
  are left out of the counts; their `unsafe` is listed under tests. The
  module of a file comes from its path.
- **Which functions a call reaches.** By name:
  - `self.f()` inside an `impl` of `T` reaches the `f` of `T` in the same
    file, if there is one.
  - Any other `x.f()` reaches every method named `f` in the same file; if
    the file has none, every method named `f` in the crate. A call through
    a trait object or a generic is followed this way, to every
    implementation of that name. That can be more than really run.
  - `T::f()`, `Self::f()` and `module::f()` reach the `f` of that type or
    module.
  - `f()` reaches the free function `f` in the same file, else in the
    crate.
  - A name brought in by `use`, also under another name (`use
    helpers::dup as d`), is followed to the function it stands for.
- **Calls that are not written as calls** are followed the same way, to
  every implementation of the name in the crate:
  - an operator, to `add`, `sub`, `mul`, `div`, `rem`, `neg`, `not`, the
    bit operators, the `..._assign` forms, `eq` and `partial_cmp`;
  - `a[i]`, to `index` and `index_mut`; `*a`, to `deref` and `deref_mut`;
  - a `for` loop, to `into_iter` and `next`;
  - `.into()` and `.try_into()`, to `from` and `try_from`;
  - `.to_string()` and the formatting macros, to `fmt`;
  - the end of any scope, to every `drop` written in the crate (an `impl
    Drop`), which is counted as reachable from every hot path.

  Not followed: `?` (to `From::from` of the error), `Deref` applied
  silently on a method call, `AsRef`, `Borrow`, `PartialOrd` reached
  through `sort`, and anything the standard library calls back into
  (`Hash`, `Ord` inside a map).
- **Closures.** A closure written inside a reachable function is part of
  that function, and what is in it is counted there. A closure or function
  passed in as a value is not followed: the call through it is listed under
  `not_followed`, and the report is flagged.
- **Macros.** The arguments of the usual standard macros (`vec!`,
  `format!`, `println!`, `assert!` and the like) are read as expressions.
  Any other macro is listed under `not_followed` as not expanded.
- **Listed as not followed**, with file and line, and flagged:
  - a call into a dependency or another crate of the workspace, written
    out (`other::copy(..)`) or through `use`. Method calls on a
    dependency's types cannot be told from the crate's own and are not
    listed;
  - a call to a lower-case name that is no function the tool can see: one
    made by a macro, or brought in by a glob import. Macros invoked where
    items go are also listed under `size.macros_that_may_define_functions`.
- **Missed without a note:** the standard library; methods of types from
  dependencies; functions made by derives; a re-export under another name
  reached by a method call; a second function of the same name that the
  by-name rule wrongly leaves out because the same file has one.

When the count and the measurement disagree (a hot path that allocates
with no construct listed) the report is flagged: "allocations were
measured that no listed construct explains". The calls not followed are
the first place to look. The measurement does not depend on any of this: it runs the
code.

## What the report does not show

Printed with every report, before the verdict, and in `not_checked`:

- Whether the structure is right. Counts find symptoms.
- **A plain index that goes stale** when elements are removed or
  reordered. It is an integer: it allocates nothing and uses no counted
  construct. The second example of step four gives wrong results and
  passes every check here (`declarations/board-plain_index.json`).
- Code the calls could not be followed into.
- That a call matched by name is the one that runs.
- Whether the budget is right, the driver's input representative, or the
  algorithm the same. Those are declared.
- Speed as the caller sees it.

## Design choices

**A separate program from the harness.** The harness compares two sides
through Python; this reads one crate and runs it natively. They share the
code for the commit, the dirty check and hashes (this crate depends on the
harness's library for those), and nothing else.

**`syn`, not a text search.** A text search cannot tell a call in a
function from one in a comment, a string or a test, or say which function a
line is in. The experiment's `count.py` did it with brace matching; this
replaces it and agreed with it on all eight recorded ports at the size
the experiment measured.

**The probe is a generated program, not a test in the port.** The port
keeps no measuring code and no `unsafe` for an allocator, and the numbers
come from the tool. Step four still asks for an allocation test in the
port: that one runs in `cargo test` and guards the budget day to day; this
one is the audit of it.

**The driver is data.** It used to be two strings of Rust pasted into the
measuring program, and a `println!` in one of them could write the result.
A path, literals and a name are enough to drive a hot path, and none of
them can carry a statement. The cost is that a workload has to be
reachable through one constructor with plain arguments.

**By-name reachability, over-approximating.** Without types, the choice is
between missing an implementation and including too many. Including too
many makes the count an upper bound a reader can discount; missing one
would hide a clone.

**Dependencies:** `syn` and `proc-macro2` to parse and to give line
numbers; `serde` and `serde_json` for the declaration and the report; the
harness library. Reasons are in `Cargo.toml`.

## Tests

```sh
cargo test --locked -p carcinify-structure -p carcinify --lib --test structure
```

The files in `tests/` run the `carcinify` binary, so they are built as test
targets of the `carcinify` crate (see `carcinify/Cargo.toml`); the unit
tests are in this crate.

Unit tests in `src/analyze.rs` cover reachability (closures, trait objects,
generics, macros, two types of one name), those in `src/probe.rs` what a
driver may contain. `tests/cli.rs` runs the real
binary on small crates in temporary git repositories and on the
declarations kept in `skill/steps/06-structure-report/declarations/`.
