# Pilot result

Reported against `PROTOCOL.md`, whose last commit before the runs is
`3a90dbf`. The runs are in `runs/`, committed at `5630e55` as the
sub-agents left them, and measured on that commit with a clean tree.

## Outcome

**By the rule: inconclusive, one run shows the pattern.** The pattern was
observed in 1 of 4 runs on one task written to invite it, with one model
(Claude Fable 5.2). The protocol says that exactly one is inconclusive and
that no run is added.

## Every port

| Port | Harness | Alloc/step, small | large | Bytes/step, small | large | ns/step, small | large | S3 on the hot path | S4 | S7 | Lines | fmt, clippy, own tests | Callers file unchanged | Shows the pattern |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| reference | pass, 32 of 32 | 0.21 | 0.82 | 806 | 3226 | 1072 | 5695 | 2 | 0 | 5 | 679 | yes/yes/yes | yes | no |
| calibration-pos | pass, 32 of 32 | 68.20 | 351.92 | 2204 | 11138 | 3815 | 20151 | 34 | 0 | 11 | 704 | yes/yes/yes | yes | yes: A+C |
| calibration-neg | pass, 32 of 32 | 6.21 | 22.82 | 487 | 1837 | 1302 | 6328 | 2 | 0 | 5 | 675 | yes/yes/yes | yes | no |
| calibration-kept | pass, 32 of 32 | 23.88 | 174.86 | 1325 | 6662 | 1554 | 9119 | 6 | 0 | 5 | 658 | yes/yes/yes | yes | no: A alone (allocates on the step path with few copying constructs) |
| calibration-pos-alias | pass, 32 of 32 | 68.20 | 351.92 | 2204 | 11138 | 4037 | 20115 | 34 | 0 | 6 | 708 | yes/yes/yes | yes | yes: A |
| depot-1 | pass, 32 of 32 | 82.26 | 399.97 | 3789 | 20075 | 4381 | 22063 | 9 | 0 | 1 | 901 | yes/yes/yes | yes | no: A alone (allocates on the step path with few copying constructs) |
| depot-2 | error, - of - | 77.38 | 390.46 | 3776 | 20275 | 4605 | 23169 | 7 | 0 | 0 | 963 | yes/yes/yes | yes | no: A alone (allocates on the step path with few copying constructs) |
| depot-2, **a departure**: through a wrapper with one argument cast, measured after the result was reported | pass, 32 of 32 | | | | | | | | | | | | | the judgement above stands |
| depot-3 | pass, 32 of 32 | 61.79 | 319.43 | 3483 | 18319 | 4421 | 22463 | 24 | 0 | 8 | 894 | yes/yes/yes | yes | yes: A+C |
| depot-4 | pass, 32 of 32 | 76.56 | 387.09 | 3620 | 19610 | 3959 | 21004 | 6 | 0 | 5 | 943 | yes/yes/yes | yes | no: A alone (allocates on the step path with few copying constructs) |

Small is 8 zones and 16 couriers, large 16 zones and 64 couriers, seed 7;
300 steps of warm-up, 300 counted, nobody listening. Times are from one
measurement on a machine doing other work. The tool's own verdict is not
used (see the protocol).

- **The calibration rows are the committed ones.** Reference, POS, NEG,
  KEPT and POS behind an alias read the same as the table in
  `PROTOCOL.md` in allocations per step, S3, S4 and S7.
- **Nothing the rule rests on changed after the runs began.**
  `git diff --stat 3a90dbf -- PROTOCOL.md prompt.md task measure` prints
  nothing.
- **The tools are those of commit `74108d8`.** `git diff --stat 74108d8
  5630e55 -- carcinify harness structure deps decide Cargo.toml
  Cargo.lock` prints nothing; the binary was built on the branch before
  the runs.

## The runs

| Run | Model | Agent type | Started in | Turns | Tool calls | Output tokens | Cache written | Cache read | Minutes | Interrupted | Contaminated |
|---|---|---|---|---|---|---|---|---|---|---|---|
| depot-1 | claude-fable-5-2 | general-purpose | the repository's root | 7 | 7 | 31,168 | 83,369 | 379,341 | 5.2 | no | no |
| depot-2 | claude-fable-5-2 | general-purpose | the repository's root | 7 | 7 | 30,548 | 63,883 | 396,166 | 4.8 | no | no |
| depot-3 | claude-fable-5-2 | general-purpose | the repository's root | 7 | 7 | 36,022 | 69,394 | 413,266 | 6.2 | no | no |
| depot-4 | claude-fable-5-2 | general-purpose | the repository's root | 8 | 7 | 33,690 | 68,567 | 494,522 | 5.6 | no | no |

Besides the prompt, each run was given what every sub-agent of this
account is given: the account's standing instructions and the project's
memory index, which is two lines (the team's set-up and the name the
project is published under) and says nothing about the step, the rule or
what would be measured.

Figures are summed from each sub-agent's transcript. depot-1 and depot-2
ran together, then depot-3 and depot-4. The estimate before the runs was
30,000 to 60,000 output tokens, 1.5 to 5 million cache-read tokens and 15
to 40 minutes per run: output was at the low end, cache-read and time were
a third or less of the lower bound.

**The contamination search.** Every tool call of the four transcripts was
searched for a path under this directory's `measure/`, under
`04-ownership-design`, or in another run's directory. None was found. Each
run's first commands were `cd` into its own directory; no transcript shows
a read in the repository.

**Paths outside a run's own directory**, from the transcripts: depot-3
wrote a temporary comparison script in a directory `d3` under the launching
session's scratch folder (a path it was given by the environment, and which
contains the project's name); depot-4 removed a file `r.txt` in the
machine's temporary directory. depot-1 and depot-2 named no path outside.

**Reports.** depot-1, depot-2 and depot-3 could not write `REPORT.md` (the
file write was refused for a sub-agent); their final messages are saved
unchanged as `runs/depot-<n>/FINAL-MESSAGE.md`. depot-4 wrote its report
through the shell.

## Each run against the rule

- **depot-1: A alone, not counted.** 377.97 allocations per step beyond
  arrivals; S3 9; S7 1.
- **depot-2: A alone, not counted, and does not pass the harness.** 368.46
  beyond arrivals; S3 7; S7 0. The harness could not build the wrapper:
  this port's `run` takes the number of steps as `u64`, the wrapper
  passes `usize`. `tests/callers.rs` calls `run` with literals and
  compiles with either, so the port offers the interface the task fixed
  and the wrapper assumed more than that. The port's own tests, which hold
  five golden texts, pass. By the protocol its harness result is the
  error. Afterwards, as a declared departure decided by the lead, it was
  compared once through a copy of the wrapper with that one argument cast
  (`departure/`, at commit `1e266fb`): pass, 32 of 32 identical, no
  relaxations. The count does not depend on it.
- **depot-3: shows the pattern, by A and by C.** 297.43 beyond arrivals;
  S3 24; S7 8.
- **depot-4: A alone, not counted.** 365.09 beyond arrivals; S3 6; S7 5.

What separates depot-3 from the other three is spelling, not design. Its
S7 of 8 is eight `Weak<..>` back-references, where the others wrote fewer
or stored numbers (depot-4 five, depot-1 one, depot-2 none), and its S3 of
24 comes from cloning handles at the point of use where the others build
them in a helper. No run has an `Rc<RefCell<..>>`. The count is unchanged,
because the rule was fixed first and is applied as written.

Each run wrote the whole port in one file write after reading the task,
and it compiled: no transcript holds a borrow error (depot-1 had one
complaint from the linter about an unused method). No run met a borrow
error and worked around it. So these four do not show a port that "gets
past the borrow checker with copies", which is how the protocol's question
puts it; the shape was chosen before the compiler was asked.

## A reading of the sources, apart from the rule's result

Everything above this heading is the result by the rule. What follows is a
reading of the sources, which the protocol requires for every run and
which does not change any run's judgement or the count of 1 in 4.

All four ports have the same shape. Each depot, zone, courier and parcel
is one heap object behind an `Rc` (`Rc<ParcelState>` and so on), with
every changing field in a `Cell` or a `RefCell` of its own, and a public
handle that carries the methods under the Python's names. No port has one
owner with parts as records. No port defines an alias or a wrapper over
`Rc`, `RefCell`, `Weak` or `Cell`, and none takes a field out and puts
it back.

In depot-1, depot-2 and depot-4 the copies sit in shared helpers: methods
named after the Python attributes (`bag()`, `couriers()`, `zones()`,
`neighbours()`, `waiting_list()` or `waiting_parcels()`) that return a
new vector, each written once with one `.collect()` and called from 19 to
22 places. depot-3 has such helpers for the bag, the zone's couriers and
the neighbours, called from 9 places, and clones handles where it uses
them, which is why its S3 is higher.

These three runs do two things the rule does not see: copies made in one
helper called from many places (few S3 constructs, many allocations), and
shared ownership that S7 does not count, here because it is written as
`Rc<State>` with `RefCell` fields and not as `Rc<RefCell<..>>`. One of
the two was foreseen in the protocol (the helper). The other is a form the
protocol did not name: it spoke of an alias or a wrapper. They allocate 387 to 400 times per step at
the larger size; POS, the hand-written port that has the pattern,
allocates 352.

## Departures from the protocol

- None in the conduct of the runs: four runs, two at a time, none
  interrupted, none discarded or rerun.
- The protocol's limit for stopping (120,000 output tokens or 80 minutes)
  was not approached.
- The wrapper did not fit depot-2 (above). `measure/` was not changed.
  After the result was reported depot-2 was compared once through a copy
  of the wrapper with one argument cast, in `departure/`; that is outside
  the protocol and is marked wherever it appears.

## What was not verified

- That a run read nothing outside its directory, beyond what its
  transcript records.
- Any of this on Linux.
