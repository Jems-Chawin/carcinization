# Step 6: what is known, and how well

For someone deciding how far to trust `STEP.md` and the tool behind it.

- **That lints miss the pattern: measured, on small samples.** In the first
  example of step four, `clippy -D warnings` is clean on both ports; the
  tool measures 0 and 708 allocations per tick. The auditor's separate
  sample gave the same (default lints silent, one `collect` flagged by
  pedantic and nursery).
- **That the tool tells the two example ports apart: measured.** The
  designed port passes. The counter-example fails on S1/S2 (708 allocations
  and 39,278 bytes per tick against a budget of 0) and S4 (`self.heat`),
  with four S3 hits listed in two functions. Both examples were written by
  hand to show the pattern.
- **That it agrees with an independent count: measured, on eight ports.**
  On the eight agent-written ports of the step-four experiment it gives 0
  allocations, 0 bytes and 0 hot-path constructs for every one, as the
  experiment's own script and test did, and the same set of reachable
  functions. That was at 32 nodes, the one size the experiment measured.
  Since the driver takes two sizes they are also run at 8 nodes, and there
  five of the eight (the four control runs and one step run) allocate
  twice in 4,000 ticks after 2,000 of warm-up, against their budget of 0,
  and fail. The auditor traced it: one queue growing once from four slots
  to eight, in the ports that do not pre-size their queues; with a longer
  warm-up all read zero. It was found after the fact at a size chosen
  later, and is an observation of low weight, not a result. Those ports contain none of the
  pattern, so the agreement is on clean code, not detection in
  agent-written code.
- **That a result cannot be typed into the declaration: tested for the
  ways found.** The auditor's reproduction and seven variants are tests
  through the real binary. `structure/README.md` lists the ways to a pass
  that remain; a port written to behave differently under measurement is
  one, and is not tested.
- **The support module** (`measure_support`) has been used in tests
  written for it and once on the first example's `ledger-core`, in a
  scratch copy with a support function added (the example itself is
  unchanged): `load` on files of 10 and 1,000 records. Whether one module
  of plain-argument functions is enough for a real port is not known.
- **Features: one comparison, no difference found.** `ledger-core`'s
  `load`, built with and without `float-roundtrip`, one run each on one
  machine: 24 and 2,016 allocations, 9,894 and 154,098 bytes per call at
  the two sizes in both builds; 183,000 and 185,167 ns per call at 1,000
  records, a difference of about 1% in a single run, which shows nothing.
  The amounts in the file were chosen to need all their digits. That the
  feature costs about twice the parsing time is `serde_json`'s description
  and was not seen here; most of this call's time is not float parsing.
- **Public paths** are worked out from `pub mod` and `pub use` by name,
  without the compiler. Tested on a re-export, a glob, a trait method, a
  generic type and a standard-library trait; not on a real crate's layout.
- **The same-measurement flag** uses a 10% band on a best-of-five time. It
  was raised on a support function that ignores its size, with 20,000
  iterations; with few iterations timing noise can hide it.
- **The implicit calls now followed** (operators, `for`, conversions,
  formatting, `Drop`) are followed by name, so they add functions that may
  not run. Tried on one crate written for the purpose and the examples; the
  examples' counts did not change.
- **That it is blind to a stale index: measured.** The second example's
  plain-index port, which gives wrong results, passes every check.
- **On a larger crate: one run.** The harness (about 6,000 lines without its tests) is read
  in full and gets one S6 flag, for cli::main at 216 lines (its one
  file over 1,000 lines is 640 without its tests); it declares that it has no hot path, with the
  reason, so nothing was measured there and the verdict is
  `pass_with_flags`.
- **That agents produce the pattern this looks for: not observed here.**
  See `../04-ownership-design/EVIDENCE.md`.
- **Sharing written inside the shared type: measured on nine ports of
  one task, after the fact.** The step-four pilot's ports (five written by
  hand, four by agents) were measured with the tool as it was for the
  pilot and again after three changes: a field taken out through a borrow
  (S4), `Rc<T>` of a type with cell fields (S7), and calls to helpers that
  return a new collection (beside S3). S3 and the allocations did not
  move.

  | port | S3 | calls to such helpers | S4 before, after | S7 before, after |
  |---|---|---|---|---|
  | reference | 2 | 0 | 0, 0 | 5, 5 |
  | calibration, pattern | 34 | 1 | 0, 1 | 11, 11 |
  | calibration, pattern with an alias | 34 | 1 | 0, 1 | 6, 6 |
  | calibration, none | 2 | 0 | 0, 0 | 5, 5 |
  | calibration, the Python's own copies kept | 6 | 0 | 0, 0 | 5, 5 |
  | depot-1 | 9 | 12 | 0, 0 | 1, 11 |
  | depot-2 | 7 | 12 | 0, 0 | 0, 16 |
  | depot-3 | 24 | 8 | 0, 0 | 8, 15 |
  | depot-4 | 6 | 12 | 0, 0 | 5, 15 |

  The pilot's own tables give the numbers before, and its result (1 of 4)
  was judged by them and stands. The changes were written after those
  ports had been read, so that they fit these nine shows nothing about
  other code. The alias port still reads 6: a type alias for the handle is
  not followed.
- **The thresholds** (1,000 lines, 150 lines, which constructs need a
  justification) are the lead's first choice and have not met a real port.
- **The S9 guidance** is reasoned, plus one measurement by the auditor of
  what call counting costs (0.8 s against 7.5 s for two million small
  calls). No speed comparison has been made with it yet.
- **Reachability by name** is tested on closures, trait objects, generics,
  macros and two types of one name. It has not been run on a crate with
  heavy generic or macro use.
- Only Python sources and ports reached through PyO3 have been tried.
