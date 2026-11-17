# Step 6: Report how the port is built

Do this when the port passes the comparison harness. The harness says the
port behaves like the original. This step says how it is built, for the one
failure the harness cannot see: code that is equivalent and compiles
cleanly, and copies data, checks fields out of `self`, or shares state
through `Rc<RefCell<..>>`, because who owns what was never decided.

Lints do not see it either. Each such construct is needed for that
structure to compile, so none is redundant. It has to be measured and
counted, against numbers stated beforehand. The design note of step four
states them; this step holds the port to them.

How far each claim here is supported is in `EVIDENCE.md`. In short: the
checks have been run on two hand-written examples, eight small
agent-written ports and one larger crate, all in this repository, and on
nothing else yet.

## What you hand over

- `structure.json`, the declaration, next to the port.
- The report written by `carcinify structure`, with the commit it describes.
- For each hot path: the time per iteration from the caller's side, with
  and without the port, and the author's statement on the algorithm.
- A sentence on each flag the report raised: what it is and why it stands.

## What to do

### 1. Write the declaration from the design note

`structure.json` names the crate, the design note, and for each hot entry
point (the same ones the design note names):

- the **budget** from the note: allocations and bytes per iteration, with
  the reason. Do not set it here by looking at the code. If the note has no
  budget, go back to step four; a budget chosen after measuring checks
  nothing.
- a **driver**: the workload the budget is about. Which constructor
  builds the input, with what arguments at **two or more sizes**; which
  call is one iteration (it has to be the entry point); how many
  iterations are warm-up and why that many; how many are counted. The
  driver is names and plain values, not code: if the hot path needs input
  that numbers and strings cannot describe, give the crate a constructor
  that builds it, in the crate's `measure_support` module: the one place
  for code written for the measurement, which the report lists by name so
  that a reviewer reads it. **One of the sizes is the size of the workload
  that was profiled in step one**; choose the other far enough from it to
  show growth. If the input has no size, give one and say why; the report
  will flag it. The tool cannot invent a workload, and a number
  "per iteration" means nothing without one. Use the input the hot path
  was profiled on in step four, or one like it. If the design note does not
  name a driver, add it to the note first, then copy it here.
- whether the port uses the **same algorithm** as the original, in a
  sentence. This is your statement; the tool prints it as declared.

A crate that has no hot path says so, with the reason, in place of the
list; the report then passes with a flag at best.

Name the **Cargo features** the port is built with, the same ones as in
the harness's suite. A structure report of the default build beside a
comparison of another build describes two different programs.

`structure/README.md` has the format. The port needs a committed
`Cargo.lock`: the measurement uses the versions in it.

### 2. Run the tool

```sh
carcinify structure structure.json --report target/carcinify-structure/report.json
```

on a committed tree. It measures allocations and bytes per iteration for
each hot path, and counts, in the functions reachable from it: copying
calls (S3), fields taken out of `self` and put back (S4), threads started
(S5). Across the crate it lists shared-ownership types (S7), every
`unsafe`, and sizes (S6).

### 3. Deal with what fails

- **Over budget (S1, S2).** The budget has to hold at every size. The
  design is not finished, or the budget was wrong. Find where the allocations come from: the S3 list for that hot
  path is the first place, then calls the report could not follow. Then
  either change the code, or change the budget **in the design note**,
  with the reason, as its own commit, and copy it here. Raising the number
  to fit the code, with no new reason, is the thing this step exists to
  stop.
- **A field taken out of `self` and put back (S4).** Usually a function
  that takes all of `self` while one field is borrowed. Passing the fields
  separately removes it. If it has to stay, write the reason as a
  justification; one line, specific to this hit.
- **A thread or pool started on the hot path (S5).** Starting threads per
  call costs more than most loops it parallelises. Move it out, or justify
  it.
- **Shared ownership with no owner stated (S7).** Say who owns the data
  and why more than one holder is needed. "To make it compile" is not an
  owner.

A justification is for a construct that is right. It is not a way to turn
a check off: each one is printed in the report beside its hit, and one that
matches nothing fails the run.

### 4. Read what does not fail

- **S3, the copying calls.** Each remaining one has a reason you can say.
  A clone of something that really needs a second life is fine. A function
  listed with "returns a new collection and is called at N place(s)" makes
  its copy at each of those calls: one construct there can be most of the
  allocations.
- **A support function was used.** Flagged, with its place. Read it: the
  numbers are for the input it builds. It should build what production
  sees, at the size asked for, and nothing else.
- **Sizes that measured the same.** Flagged when the counts are equal and
  the times within 10%. Either the hot path really costs the same at both
  sizes, or the two inputs are not different: check what builds them.
- **Counts that change with the input size.** Flagged, saying whether it
  is the number of allocations or only the bytes. A hot path whose
  allocations grow with the input has a cost the budget does not bound;
  the appeasement example goes from 102 per tick at 8 units to 708 at 24.
  Either it should not grow, or the budget should be stated per element
  and the design note should say so.
- **Calls not followed.** The count stops at a closure passed in, at a
  macro of your own, and at the edge of the crate; each is listed. If the
  measurement shows allocations and no construct is listed, the report
  says so: look there.
- **`unsafe`.** The first thing a reader of an agent-written port asks
  about. The report gives the count and the place of each. For each one in
  the port's own source, be able to say what makes it sound; aim for none,
  and `#![forbid(unsafe_code)]` at the top of the crate if there is none.
- **S6, size.** A file over 1,000 lines or a function over 150 is flagged.
  A whole port in one file, or one function, is a sign the source was
  transcribed and not designed.

### 5. Compare speed fairly (S9)

A port is usually asked for because of speed, and the number that gets
quoted should be worth quoting.

- **Time from the caller**, in the source language, not only from Rust.
  The boundary costs time (converting arguments and results), and the
  caller is where the user is. The report's nanoseconds per iteration are
  from Rust and leave that out. `sources/` says how to time from the
  caller.
- **Not through the harness with call counting on.** The harness counts
  Python calls while a function runs, which slows the original by up to
  about nine times and the compiled side not at all. Time outside the
  harness, or with `--no-call-counts`, and write down which.
- **Same algorithm.** If the port also changed the algorithm (a scan
  became a lookup), part of the gain is the algorithm's and would be had in
  the source language too. Say so. Where it is practical, make the same
  change in the original and time that as the baseline. Nothing can measure
  "same algorithm"; it is your declaration, and the report shows it as
  yours.
- Give the input, the machine, the number of runs and the spread with any
  figure. One run on one machine is an observation.

## What the report cannot show

It prints this itself, each time.

- That the structure is right. The counts find symptoms; a port can pass
  all of them and be badly built. The design note and a reader are still
  needed.
- **A stale index.** A plain index into a collection that shrinks or is
  reordered is an integer: no allocation, no counted construct. The second
  example of step four gives wrong results and passes every check here.
  What catches it is the comparison harness with cases that remove
  elements, and the three questions in step four's inventory.
- A zero budget is not a goal in itself. Code whose job is to build output
  has to allocate; the budget is what the design note argued, whatever the
  number.
- Anything in code the calls were not followed into.
