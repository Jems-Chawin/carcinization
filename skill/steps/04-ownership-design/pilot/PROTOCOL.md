# Pilot: does an agent produce the pattern at all?

Written and committed **before any run**. The result will be reported
against this file, in a `README.md` beside it.

## Question

The first experiment (`../experiment/`) could not separate its two arms:
the task handed both of them the design. And the project has never seen an
agent write the pattern step four is about: a port that keeps the object
graph of the original and gets past the borrow checker with copies, with
fields taken out and put back, or with shared ownership around every part.
Every instance so far was written by hand to show it.

So one question, before any comparison: **on a task where the pattern is
possible, does an agent that is given no step produce it?** One arm, four
runs. This does not test the step.

## The task

`task/`, given to every run exactly as it is: `TASK.md`, `depot.py` (387
lines; the first experiment's program had 130), `callers.py`, and a crate
with an empty `src/lib.rs`, `tests/callers.rs` and `tests/golden/`.

How it differs from the first task, point by point:

1. **No design is handed over.** `src/lib.rs` is one comment line. The
   interface is fixed by `tests/callers.rs`, which is the Python callers
   written in Rust: they keep a `Courier`, a `Zone` and `Parcel`s, read
   them after later steps (also after the courier has retired and the
   parcel is gone from the depot), change a courier from outside, and
   register a listener that is called in the middle of a step, receives
   the parcel and the courier, changes the courier and keeps the parcel.
   So the interface keeps an object per part because callers hold them.
   What those objects are inside is left to the port.
2. **Neither the hot path nor the goal is named.** `TASK.md` says "we are
   moving it to Rust. Port it." No profile is given: I judged that a
   request of this kind usually comes without one.
3. **Size and shape.** Four classes that refer to each other (`Depot`,
   `Zone`, `Courier`, `Parcel`; each of the last three holds the depot,
   couriers and parcels hold zones, zones hold couriers and parcels,
   parcels hold their carrier). Parcels are removed during a run
   (delivered, expired), couriers too (retired, while the depot goes
   through its couriers). One callback.
4. **The pressure.** "Keep the structure of the original. The port will be
   reviewed side by side with `depot.py` ... A reviewer should find each
   class and each method of the original in the Rust."

The agents are not told that allocations or structure will be looked at.

**The task was written to invite the pattern.** `tests/callers.rs` needs
objects that a caller keeps while the depot goes on changing them, and
that a listener changes in the middle of a step: handles that share state
and can change it through a shared reference. No port can avoid that; the
reference has it too, once. Together with "keep the structure" this pushes
towards one shared, changeable object per part. So a high count here shows
that an agent takes the invitation, not that it would write the pattern
without one. The result will be worded with that in it: **"observed in n
runs on one task written to invite it, with one model"**.

### Is the task fair in both directions?

- **The trap is there.** `measure/trap/literal.rs` is the busiest part of
  `depot.py` written line for line, each class a struct that owns what the
  Python object holds. rustc 1.100.0 rejects it with five borrow errors
  (two E0499, three E0502), in `measure/trap/errors.txt`.
- **A designed port exists and passes.** `measure/reference/depot`: one
  owner, parts as records referred to by number, handles that are a number
  and a share of the owner, the borrow released around each listener call.
  It passes `tests/callers.rs` and is identical to the Python on all 32
  cases of `measure/cases.jsonl` under `carcinify diff`. No agent sees it.

## The prompt

`prompt.md`, with `{DIR}` replaced by the run's directory. Nothing else is
said to a run.

## Runs

- **Four**, fixed. Claude Fable 5.2 for each, one fresh sub-agent per run
  with no shared context, each in its own copy of `task/` outside the
  repository, in `/private/tmp/w7/depot-1` to `depot-4`. That path does not
  contain this project's name, and no relative path from it leads to the
  repository, the reference or the calibration ports. They are on the same
  machine and readable by an absolute path, which nothing prevents. A
  sub-agent starts in the directory of the session that launches it, which
  is the repository's root; the prompt names its working directory. The
  result will state the starting directory as the transcripts show it.
- Two runs at a time. If a run looks like passing twice the upper estimate
  given before the runs (120,000 output tokens or 80 minutes), no further
  run is launched and that is reported.
- Isolation is by instruction ("work only inside that directory"), not
  enforced. Sub-agents also receive the account's standing
  instructions. Afterwards each run's transcript is searched for every file
  it read and every command it ran, and what lay outside its directory is
  reported.
- No run is discarded, rerun or replaced. A run stopped by a usage limit is
  reported as interrupted and resumed once in its own context with the one
  message "Your session was interrupted by a usage limit. Continue the task
  from where you stopped."; it is counted, with the interruption noted. A
  run interrupted a second time is measured as it stands.
- Each run's directory is copied unchanged (without `target/`) to
  `runs/depot-<n>/depot` and committed.

## Measures

`measure/measure.py`, for each run, the reference and the two calibration
ports:

1. `carcinify diff` against `depot.py` and `callers.py` on
   `measure/cases.jsonl`: 22 plain runs and 10 runs of the callers'
   scenario, no relaxations, through the wrapper `measure/py`, which is
   written against the interface of `tests/callers.rs` and is the same for
   every port.
2. `carcinify structure` on `Depot::step`, driver as data: state built by
   `depot::Depot::new`, at two sizes (8 zones and 16 couriers; 16 zones and
   64 couriers; seed 7), 300 steps of warm-up, 300 counted, nobody
   listening. From its report: allocations, bytes and nanoseconds per step
   at each size; **S3**, the copying constructs in the functions reachable
   from `Depot::step`; **S4**, fields taken out of `self` and put back;
   **S7**, shared-ownership constructs in the crate.
   Both tools are the `carcinify` binary built from this repository at the
   last commit before the runs. No file of the tools has changed since
   commit `74108d8`, and the result will show that with
   `git diff --stat 74108d8 <measuring commit> -- carcinify harness
   structure Cargo.toml Cargo.lock`.
3. The run's own `cargo fmt --check`, `clippy -D warnings`, `cargo test`,
   and whether `tests/callers.rs` is still the task's file.

## Calibration: five ports written by hand

The rule below was first written from the reference alone, and its cost
part fired on a clean port. So before any run it was tried on ports
written by hand, none by a sub-agent, none shown to any agent:

- **reference** (`measure/reference`): the designed port described above.
- **POS** (`measure/calibration/pos`): a port that has the pattern. Every
  depot, zone, courier and parcel is its own `Rc<RefCell<..>>` holding the
  others as the Python objects do, and every list that is walked while
  something in it may change is cloned first: the couriers and the zones
  once per step; in a courier's turn its bag (when dropping off, for each
  colleague it may hand over to, and when moving), the zone's queue and
  the zone's couriers.
- **POS behind an alias** (`measure/calibration/pos-alias`): POS with
  `type Shared<T> = Rc<RefCell<T>>` and `type Back<T> = Weak<RefCell<T>>`
  used in every struct. The code is otherwise the same.
- **NEG** (`measure/calibration/neg`): the reference with one change:
  every parcel is its own heap object (`Box`). Nothing is copied on the
  step path.
- **KEPT** (`measure/calibration/kept`): the reference with the five list
  copies the Python itself makes put back as `.clone()` or a new vector:
  `list(self.couriers)` in `Depot.step`, `list(self.bag)` in `drop_off`
  and in `hand_over`, `list(self.waiting)` in `settle`, and the list
  `Zone.offer` returns. One owner, parts as records, as in the reference.

All five pass their own checks and are identical to the Python on 32 of 32
cases under `carcinify diff`. Measured at commit `f3d4c53` on this machine:

| | reference | NEG | KEPT | POS | POS behind an alias |
|---|---|---|---|---|---|
| allocations per step, small | 0.21 | 6.21 | 23.88 | 68.20 | 68.20 |
| allocations per step, large | 0.82 | 22.82 | 174.86 | 351.92 | 351.92 |
| the same beyond one per arriving parcel (22), large | below 0 | 0.82 | 152.86 | 329.92 | 329.92 |
| bytes per step, small | 806 | 487 | 1,325 | 2,204 | 2,204 |
| bytes per step, large | 3,226 | 1,837 | 6,662 | 11,138 | 11,138 |
| S3 on the hot path | 2 | 2 | 6 | 34 | 34 |
| S4 on the hot path | 0 | 0 | 0 | 0 | 0 |
| S7 in the crate | 5 | 5 | 5 | 11 | 6 |

What the five ports showed about the measures:

- **Allocations alone do not tell a clean port from the pattern.** The
  first version of part A was "10 or more allocations per step at the
  larger size". NEG allocates 22.82, one for each of the 22 parcels that
  arrive per step. A now counts allocations beyond one per arriving
  parcel.
- **Allocations beyond arrivals do not tell copies kept from the original
  from the pattern.** KEPT has one owner and no shared part, and allocates
  152.86 beyond arrivals, nearly half of what POS does, from five copies
  the Python also makes.
- **S7 can be hidden by a name.** Behind an alias POS reads 6, one more
  than the reference: the tool counts the construct where it is written,
  which is then the alias and the four places a part is built. A wrapper
  struct would hide it the same way.
- **S3 is not hidden by the alias** (34 either way) and separates KEPT (6)
  from POS (34).
- **Bytes do not order the ports by what they copy:** NEG requests fewer
  bytes than the reference, whose figure is vectors doubling.
- **S4 is 0 for POS** although it takes the depot's listeners out and puts
  them back around each call. It does so through a `RefCell` borrow, and
  S4 reads only a field of `self` taken and assigned again in one
  function (finding RUST-092). Part B cannot see that form.

## The rule

**FIXED** on the five ports above, decided by the lead before any run,
and applied by `measure/table.py`, which holds the same numbers.

A run **shows the pattern** if the structure tool could measure it and at
least one of these holds:

- **A with copies.** At the larger size, allocations per step minus 22
  (one for each parcel that arrives in a step) are **16 or more**, **and**
  S3 lists **14 or more** copying constructs on the hot path. POS, with
  and without the alias: 329.92 and 34. KEPT: 152.86 and 6. NEG: 0.82 and
  2. 16 is the geometric midpoint of NEG and POS in allocations; 14 is the
  geometric midpoint of KEPT and POS in S3.
- **B, check-out.** S4 lists **1 or more** fields taken out of `self` and
  put back on the hot path. No calibration port has one.
- **C, shared ownership with copies.** S3 lists **14 or more** copying
  constructs on the hot path **and** S7 lists **8 or more** constructs.
  This is for a port that shares every part and copies handles without
  allocating. POS: 34 and 11.

**A alone** (16 or more beyond arrivals, S3 under 14) does not count
towards the two. It is reported as "allocates on the step path with few
copying constructs", with the constructs named from the S3 list. KEPT is
this case: copies kept from the original, one owner.

By this rule POS and POS behind an alias show the pattern; the reference,
NEG and KEPT do not.

Why S3 and not S7 beside A: had the condition been S7 at 8 or more, POS
behind an alias (S7 6) would not count. What this rule still misses, known
in advance: a port that makes its copies in one helper called from many
places has few S3 constructs and many allocations, and reads as "A alone";
a port that shares every part behind an alias or a wrapper and copies
without allocating has a low S7 and reaches neither A nor C. For every run
the report therefore also says, from reading the source and marked as a
reading, whether an alias or wrapper over `Rc`, `RefCell`, `Weak`, `Arc`,
`Mutex` or `Cell` is defined, and whether copies sit in a shared helper.
A reading does not change a run's count.

**A contaminated run.** After the runs every transcript is searched for a
read of anything under this directory's `measure/` (the reference, the
calibration ports, `table.py`, the cases) or of another run's directory.
A run with such a read is marked contaminated and counted as not
measurable. `table.py` takes their names as its second argument.

The tool's own verdict is not used. The declaration gives a budget of zero
and no owner for any S7 construct (a justification that matches nothing
stops the tool, so none can be written in advance), which makes the verdict
`fail` for every port, the reference included. Only its numbers are used.

Bytes per step and time per step are reported and are not in the rule.

**A run that does not pass the harness** but can be measured is measured
and judged by the same rule; the table says both things. The pattern is
about structure, and a port can have it and be wrong.

**A run that cannot be measured** (it does not compile, or does not offer
the interface, so the driver cannot be built) is reported as "not
measurable", with what the tools said, and does not count as showing the
pattern.

## Stopping rule

Four runs, then stop, whatever they show.

- **No run shows the pattern:** the result is worded "Not observed in 4
  runs; a rate up to about one in two is not excluded. On this evidence,
  and at this cost, step four is kept for its artefacts." (If the true
  rate were one in two, four runs would all miss it one time in sixteen.)
- **Two or more show it:** reported as such. A comparison of arms is a
  separate decision, with at least eight runs per arm and an exact test
  fixed in advance.
- **Exactly one shows it:** inconclusive; reported as that, and no run is
  added. This is a guard against one misjudged run: the rule was fixed on
  a handful of ports, and one run on the wrong side of a threshold should
  not decide the outcome.
- **Two or more runs not measurable:** inconclusive, whatever the others
  show.

## What the result must contain

- The table of `measure/table.py` for the runs, with the rows of the
  reference and the calibration ports printed beside them from the same
  measurement. Those rows must equal the table above in S3, S4, S7 and
  allocations per step; if they do not, the measurement is not the one the
  rule was fixed on and the result says so before anything else.
- The output of `git diff --stat <last commit before the runs> --
  PROTOCOL.md prompt.md task measure` from this directory, which must be
  empty.
- For each run: the starting directory, every path outside its directory
  that its transcript shows it read or wrote, tokens and time, and whether
  it was interrupted.
- The wording "observed in n runs on one task written to invite it, with
  one model".

## What this cannot show

- Anything about the step: there is one arm.
- Anything about other programs, larger ports, other models, or sessions
  long enough for a first design to prove wrong late.
- Whether the rule's numbers are the right ones for ports unlike the five
  it was fixed on. A port between them may be judged either way.
- Whether a run read outside its directory, beyond what its transcript
  records.
