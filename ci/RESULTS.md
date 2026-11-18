# Results of the runs made so far

The runs recorded here up to commit ca706b6 were made when the tools were
four programs, `rustify-diff`, `rustify-structure`, `rustify-deps` and
`rustify-decide`. The text below names them as they are called now.

The first runs were made on 2026-11-15; later ones say when. All but
those under "On GitHub's runners" were made on one machine, an arm64 Mac
with 8 logical CPUs, with `ci/run-all.sh`. The
tables are from the runs at the commit they name; the entries under "Runs
since those tables" are newer. The tables are built from the `results.tsv` each
run wrote. Times are wall-clock seconds for the whole suite or example,
taken once, on a machine that was doing other work; they say how long a run
takes and nothing more.

## Platforms

| | macOS arm64 | Linux arm64 (container) | Linux x86-64 (container, emulated) |
|---|---|---|---|
| commit tested | 56c444b | 56c444b | fdbf53a |
| system | Darwin 25.2.0 | Debian 13.7, kernel 6.10.14-linuxkit, aarch64, in a container on the same Mac | the same image for x86-64, run through Rosetta |
| C library | the system's | glibc 2.41 | glibc 2.41 |
| filesystem built on | APFS | overlay (the container's own, not a mount) | overlay |
| Rust | 1.100.0 | 1.100.0 | 1.100.0 |
| Python | 3.12.13, with the GIL | 3.13.5, with the GIL | 3.13.5, with the GIL |
| maturin | 1.16.0 | 1.16.0 | 1.16.0 |
| NumPy, for one test | 2.5.4 | the newest on PyPI that day; version not recorded | |
| result | exit 0: as `expected.tsv`, which had 34 rows then and has 36 now | exit 0: as `expected.tsv`, 34 rows then, 36 now | **not completed**: see below |

The last commit at which a run was as expected in every row, of those
recorded below: a2ee1ff, on both, on that machine; 7675eec, on both, on
GitHub's runners. Commit ids in this file up to 97994d3
are those of the development history, which is not published; the later
ones are commits of this repository.

The image is `rust:1.100.0-trixie` at
`sha256:e19840879045c951c905d0eaa9887f3439c62000e429963371b376bac8d38c31`,
with Debian's `python3`, `python3-venv`, `python3-dev` and `git` added.
Docker 28.4.0. Both runs were made with the network tests (`RUN_LIVE=1`)
and the NumPy test, and the Linux one under the time cap.

Both runs were checked by the script itself against `ci/expected.tsv`.
Earlier runs, before that file existed, were read by eye; among them one on
Linux arm64 at commit c6ca60c with the virtual environment inside the
checkout, which passed.

## Tables

### Checks and test suites

| | macOS arm64 | Linux arm64 (container) |
|---|---|---|
| fmt | exit 0 | exit 0 |
| clippy | exit 0 | exit 0 |
| diff | 92 passed, 6 ignored; 15s | 92 passed, 6 ignored; 17s |
| diff-example-maturin | 5 passed, 0 ignored; 18s | 5 passed, 0 ignored; 30s |
| diff-mixed-maturin | 1 passed, 0 ignored; 26s | 1 passed, 0 ignored; 20s |
| structure | 45 passed, 0 ignored; 23s | 45 passed, 0 ignored; 11s |
| structure-shared-target | 45 passed, 0 ignored; 23s | 45 passed, 0 ignored; 11s |
| deps-recorded | 25 passed, 2 ignored; 4s | 25 passed, 2 ignored; 7s |
| decide | 15 passed, 1 ignored; 196s | 15 passed, 1 ignored; 167s |
| decide-numpy | 1 passed, 0 ignored; 17s | 1 passed, 0 ignored; 74s |
| deps-live | 2 passed, 0 ignored; 92s | 2 passed, 0 ignored; 94s |
| example-float | 2 passed, 0 ignored; 0s | 2 passed, 0 ignored; 5s |
| example-float-fixed | 2 passed, 0 ignored; 0s | 2 passed, 0 ignored; 2s |
| example-fleet | 3 passed, 0 ignored; 0s | 3 passed, 0 ignored; 1s |
| example-board | 3 passed, 0 ignored; 1s | 3 passed, 0 ignored; 0s |
| release-build | exit 0 | exit 0 |

### Examples

| example | verdict and counts, compared with `expected.tsv` | macOS arm64: exit status, time, and what is judged by time | Linux arm64 (container): exit status, time, and what is judged by time |
|---|---|---|---|
| diff float-buggy | fail cases 68; different 41; identical 27 | exit 1, 2s | exit 1, 1s |
| diff float-fixed | pass cases 68; different 0; identical 68 | exit 0, 2s | exit 0, 1s |
| diff float-panic | fail cases 2; different 1; identical 1 | exit 1, 2s | exit 1, 1s |
| diff fleet-designed | pass cases 35; different 0; identical 35 | exit 0, 4s | exit 0, 11s |
| diff fleet-appeasement | pass cases 35; different 0; identical 35 | exit 0, 4s | exit 0, 3s |
| diff board | pass cases 31; different 0; identical 31 | exit 0, 2s | exit 0, 8s |
| structure fleet-designed | pass 8 units: 0 allocations, 0 bytes; 24 units: 0 allocations, 0 bytes ; flags 0 ; S3 0 | exit 0, 2s; ns per iteration: 96.7, 1073.8 | exit 0, 1s; ns per iteration: 102.1, 1117.1 |
| structure fleet-appeasement | fail 8 units: 102 allocations, 4578 bytes; 24 units: 708 allocations, 39278 bytes ; flags 1 ; S3 4 | exit 1, 1s; ns per iteration: 2169.2, 14623.3 | exit 1, 0s; ns per iteration: 2157.1, 20483.8 |
| structure board-checked | pass_with_flags seed 7: 0.0005 allocations, 49.152 bytes ; flags 2 ; S3 0 | exit 0, 1s; ns per iteration: 146.7 | exit 0, 0s; ns per iteration: 58.9 |
| structure board-plain_index | pass_with_flags seed 7: 0.001 allocations, 61.44 bytes ; flags 2 ; S3 0 | exit 0, 2s; ns per iteration: 5984.5 | exit 0, 1s; ns per iteration: 5547.0 |
| structure harness | pass_with_flags  ; flags 5 ; S3 0 | exit 0, 0s; ns per iteration:  | exit 0, 0s; ns per iteration:  |
| decide waiting-decide | consistent mostly_waiting | exit 0, 8s; flags 0; fastest 0.664 s | exit 0, 7s; flags 0; fastest 0.649 s |
| decide native-decide | consistent mostly_compiled_code | exit 0, 5s; flags 1; fastest 0.341 s | exit 0, 5s; flags 2; fastest 0.339 s |
| decide own-python-decide-original | consistent mostly_own_python | exit 0, 91s; flags 2; fastest 1.019 s | exit 0, 100s; flags 2; fastest 1.144 s |
| decide own-python-decide-after-fix | consistent mostly_own_python | exit 0, 27s; flags 2; fastest 0.475 s | exit 0, 30s; flags 3; fastest 0.549 s |
| deps example-live | complete live opentelemetry official_crate; sklearn community_crates; yaml community_crates; zeep community_crates | exit 0, 93s | exit 0, 95s |
| deps example-replay | complete replay opentelemetry official_crate; sklearn community_crates; yaml community_crates; zeep community_crates | exit 0, 0s | exit 0, 0s |
| deps adk-case-live | complete live google.adk community_crates; google.genai community_crates | exit 0, 83s | exit 0, 78s |
| deps adk-case-replay | complete replay google.adk community_crates; google.genai community_crates | exit 0, 0s | exit 0, 0s |

## What is the same on both, and what is not

**The same**, and compared by the script: every suite's numbers of tests
passed, failed and ignored; every example's exit status and verdict; the
harness's counts of cases identical and different; the structure tool's
allocations and bytes per iteration, its number of flags and its count of
copying constructs; the decide tool's classification of where the time
goes. None of these examples measures code that opens files or starts
threads, where an allocation count could differ by platform.

**Not the same**, printed in the last column and not compared:

- **The number of flags `carcinify decide` raises.** Its flags are judged by
  time. `native-decide` has 1 flag on macOS and 2 on Linux, in every run
  made: library Python is 19% of the time on one and 21 to 23% on the
  other, either side of the 20% at which the flag is raised.
  `own-python-decide-after-fix` has 2 on macOS and 3 on Linux: there the
  five plain runs differ by more than a quarter of their median (51% in
  the run in the table), which raises a flag of its own.
- Nanoseconds per iteration in the structure reports, and every duration.
- Linux and Python 3.13 changed together, so none of these differences can
  be put down to the system or to the interpreter alone.

The two runs that showed the script can fail, both at 56c444b with one
change made for the purpose:

| change | where | result |
|---|---|---|
| a test added to the dependency check that always fails | Linux arm64 container, suites only, through `ci/linux/run.sh` | exit 1: "suite deps-recorded failed: 7 passed, 1 failed", and its counts differ from the expected ones |
| `expected.tsv` edited to expect 67 identical cases in the float example's fixed suite, where there are 68 | macOS, full run | exit 1: "example diff float-fixed: expected … identical 67 …, got … identical 68" |

A run with an empty `expected.tsv` also ended with exit 1 and 31 rows
reported as not expected.

## Runs since those tables

At commit 800f6c0, after the decide tool gained its checks for stand-ins,
hidden arrays and load inside a run, and a fifth example (`decide-port`):
macOS and the Linux arm64 container, both with the NumPy test and without
the network tests, both exit 0 with 32 rows as `expected.tsv` has them. The
decide suite is 21 tests and 1 ignored. The decide rows now compare the
flags that come from a count or a declaration, by kind, and print the ones
judged by time:

| example | compared, same on both | judged by time, macOS | judged by time, Linux |
|---|---|---|---|
| waiting-decide | none | none | none |
| native-decide | many_objects_crossing | none | library_python |
| own-python-decide-original | none | alternative_too_short, slow_profile | alternative_too_short, slow_profile |
| own-python-decide-after-fix | many_objects_crossing | slow_profile | runs_differ, slow_profile |
| own-python-decide-port | many_objects_crossing | slow_profile | slow_profile |

On macOS one example was run twice because the tool said the machine was
busy the first time; in the Linux run one test was repeated for the same
reason. Both are recorded in the results.

At commit 74108d8, after the four tools became the one command `carcinify`
and the decide tool gained the verdict `consistent_on_the_authors_word`:
macOS (Python 3.12.13) and the Linux arm64 container (Python 3.13.5), both
with the NumPy test and with the network tests (`RUN_LIVE=1`), both exit 0
with 36 rows as `expected.tsv` has them. New since the run above: the suite
`command` (2 tests) and the two live dependency examples with their
replays. The decide suite is 25 tests and 1 ignored; it took 640 s on macOS
and 490 s on Linux, and the NumPy test 59 s and 1,073 s. The flags of the
five decide examples, compared and judged by time, were the same as in the
table above on both platforms. No decide test was repeated for a busy
machine on macOS; seven test runs were repeated on Linux, which is where
the NumPy test's time went. Why that run drew them was not looked into.

At commit cf5cf71, in the night of 15 to 16 November, after a workload that uses half the CPUs or more stopped
being judged by the process beside it: the same two platforms with the same
options, both exit 0 with 36 rows as `expected.tsv` has them. The decide
suite is 28 tests and 1 ignored; it took 1,275 s on macOS and 689 s on
Linux, and the NumPy test 59 s and 774 s. No decide test was repeated for a
busy machine on Linux. On macOS one test was repeated three times
(`a_half_replaced_stand_in_is_refused`); other programs were using the
machine, and whether that was the whole cause was not looked into. A first
macOS run at this commit failed in the live dependency suite alone, because
the network was down; it was run again whole.

On the branch that joins the step-one and step-four work, three pairs of
runs, on 2026-11-16. At 7370974 the Linux run failed in the decide suite (24 passed, 2
failed, 34 runs repeated): other programs had the machine at a load
average of 12 to 37, and two tests gave up after six `unreliable` answers
in a row; the macOS run was stopped. At 6095106 macOS was as expected and
Linux failed in the NumPy test alone, for a reason in the tool: an honest
stand-in was refused there because idle threads of NumPy's linear algebra
library had used 0.115 s more CPU than wall time, over 1% of a 7 s
workload. That refusal became a flag. At 7096c5c both runs exit 0 with 36
rows as `expected.tsv` has them, and no decide test was repeated for a
busy machine: the decide suite took 760 s on macOS and 579 s on Linux, the
NumPy test 78 s and 598 s.

At ac2e96b, on 2026-11-16, after the flag for threads in a stand-in gained its second
wording and a unit test: neither run is as expected, and neither was run
again. Linux exits 1 with one difference: the decide suite has 29 tests
passed and 1 ignored where `expected.tsv` still says 28, because the new
unit test was not added to the file; nothing failed (579 s, the NumPy test
573 s, no repeat). macOS exits 1 in the decide suite alone: 25 passed, 1
failed, 1 ignored in 1,589 s, with 33 runs repeated for a busy machine;
`a_missing_decision_or_answer_is_incomplete` gave up after six
`unreliable` answers in a row, and the unit tests after it were not run.
Other programs were using the machine (load average up to 15). The NumPy
test passed there in 76 s and the other 34 rows are as expected. So at
this commit the tool's tests have not passed whole on macOS, and
`expected.tsv` is one test behind.

At 6513e51, on 2026-11-17 (`expected.tsv` brought to 29 decide tests), one run on each
system, one after the other: neither is as expected, and every difference
is in something judged by time while other programs had the machine.

- macOS, load average 4.9 at the start and 5.1 at the end, and between 34
  and 101 in the tool's own messages during the decide suite: exit 1. The
  decide suite: 16 passed, 10 failed, 1 ignored in 4,101 s, with 105 runs
  repeated; each of the ten gave up after six `unreliable` answers in a
  row. The reasons the tool gave: its loop of pure computation kept off
  the CPU for 42% to 96% of its time around the runs, and the process
  beside a run for 40% to 92%. Two decide examples (`waiting`, `native`)
  answered `unreliable` for the same reason. The NumPy test passed (268 s);
  all other rows are as expected.
- Linux arm64, load average on the host 5.1 at the start and 37.8 at the
  end: exit 1, two tests. `sizes_that_measure_the_same_are_flagged` in
  the structure suite with a shared target directory: the two sizes took
  428 and 231 ns per iteration, so the flag for "within 10%" was not
  raised (the same test passed in the plain structure suite of this run).
  The NumPy test: the honest stand-in was used, and the flag for idle
  threads, which the test expects on Linux, was not raised in that report.
  The decide suite passed: 29 and 1 ignored, 984 s, 2 runs repeated.

No code or limit was changed after these runs.

At 1ef13b0, on 2026-11-17 (the same code; only this file differs from 6513e51), one run
on each system, one after the other, each started only after the 1-minute
load average had read under 6 six times, a minute apart. That took 1 h 36
min of waiting. Both are as expected: exit 0, 36 rows, no decide run
repeated.

- macOS, load average 4.9 at the start and 4.7 at the end, read every
  minute: under 9 except for two minutes at 24 and 21. Decide suite 29
  passed and 1 ignored in 758 s; NumPy test passed in 77 s.
- Linux arm64, load average on the host 3.9 at the start and 7.1 at the
  end. It was not a quiet run: the load was 5 to 29 for the first quarter
  of an hour and then 48 to 94 for ten minutes. Which share of that was
  the run itself and which other programs was not measured. Decide suite
  29 passed and 1 ignored in 611 s; NumPy test passed in 692 s, with the
  flag for idle threads raised; `sizes_that_measure_the_same_are_flagged`
  passed in both structure suites.

So the two tests that failed at 6513e51 passed here, the one on Linux
under load again. Both still assert something that depends on how long
things took, so another loaded run can fail them.

At d0462fe, on 2026-11-17 (those two tests changed to assert the flag exactly when the
report's own figures call for it; the hang tests given 20 s; "1 entry"),
runs started as before, after 25 minutes of waiting. Linux is as expected.
macOS is not, in two attempts, and every failure is a test that gave up
after six `unreliable` answers in a row.

- macOS, first attempt, load average 4.9 at the start and 6.0 at the end,
  between 7 and 19 for most of the run: exit 1, 35 of 36 rows as expected.
  The NumPy test gave up (the tool's reasons: its loop of pure computation
  kept off the CPU for 11% of its time and more, the process beside a run
  for 16%). The decide suite passed, 29 and 1 ignored in 795 s, with 3 runs
  repeated; 6 more repeats are the NumPy test's.
- macOS, second attempt, started after another quiet spell, load average
  2.7 at the start and 33 at the end, above 50 for half of the hour and up
  to 154: exit 1. The decide suite: 21 passed, 5 failed, 1 ignored in
  2,720 s, 65 runs repeated in this and the NumPy test. The example
  `waiting` answered `unreliable`. The NumPy test passed (140 s); all
  other rows are as expected.
- Linux arm64, load average on the host 5.8 at the start and 6.6 at the
  end, 8 to 17 for the first quarter of an hour, then 36 to 85 for
  thirteen minutes: exit 0, 36 rows as expected, no run repeated. Decide
  suite 29 and 1 ignored in 593 s; NumPy test passed in 683 s; both
  structure suites 46.

So on macOS at this commit every row has been as expected in one attempt
or the other, and not all in one. No test or limit was changed after
these runs.

At 97994d3, on 2026-11-17 (the same code and tests as d0462fe; only this
file differs), macOS alone, up to three attempts allowed, started after 40
minutes of waiting for the load to read under 6 six times a minute apart.
The first attempt is as expected: exit 0, 36 rows. It was not a quiet run:
load average 4.3 at the start and 10.9 at the end, above 20 for a third of
its 36 minutes and up to 53. The decide suite: 29 passed and 1 ignored in
1,222 s, with 10 runs repeated in eight tests; the NumPy test passed in
196 s after 12 repeats. No test gave up. Both structure suites 46. No
second attempt was made, and Linux was not run again.

At 4d088b3, on 2026-11-17 (the structure tool sees a field taken through a
borrow or a lock, sharing written inside the shared type, and prints the
calls to a helper that returns a new collection; `ci/run-all.sh` now runs
every test binary of a suite after one fails), one attempt on each system,
macOS then Linux, each started when the load had read under 6 for five
minutes. Both are as expected: exit 0, 36 rows, no run repeated.

- macOS, 24 minutes, load average 4.7 at the start and 4.6 at the end,
  above 12 for two of those minutes and up to 20.7. The decide suite: 29
  passed and 1 ignored in 756 s; the NumPy test passed in 76 s; both
  structure suites 50.
- Linux arm64, 30 minutes, load average on the host 4.3 at the start and
  4.0 at the end, above 20 for eight minutes and up to 29.7. The decide
  suite: 29 and 1 ignored in 581 s; the NumPy test passed in 568 s; both
  structure suites 50.

Both runs included the two tests that ask crates.io (`RUN_LIVE=1`).

At cebca8f, on 2026-11-18 (the structure tool no longer takes a type the
crate declares, or one inside `PhantomData`, for a cell; `compare.py` lets
a suite off as too busy only with exit status 101; the decide tests remove
the report before each attempt), one attempt on each system as before,
with the crates.io tests. Both are as expected: exit 0, 36 rows.

- macOS, 25 minutes, load average 4.9 at the start and 6.3 at the end,
  above 12 for three of those minutes and up to 20.5. The decide suite: 29
  passed and 1 ignored in 775 s, 1 run repeated; the NumPy test passed in
  82 s; both structure suites 52.
- Linux arm64, 34 minutes, load average on the host 4.7 at the start and
  6.0 at the end, above 20 for 19 minutes and up to 86. The decide suite:
  29 and 1 ignored in 619 s; the NumPy test passed in 803 s; 2 runs
  repeated between them; both structure suites 52.

At a2ee1ff, on 2026-11-18 (`carcinify decide` says so when a machine has
fewer than four CPUs and load inside a run was not watched, and its report
counts the CPUs the process may use; one decide test is skipped on macOS
with fewer than four CPUs; the README says where the project stands), one
attempt on each system as before, with the crates.io tests, each built
from an empty target directory. Both are as expected: exit 0, 36 rows, no
run repeated. The machine has 8 CPUs, so the new message and the skip did
not come into play in these runs.

- macOS, 25 minutes, load average 1.3 at the start and 2.8 at the end,
  above 12 for one of those minutes and up to 14.2. The decide suite: 30
  passed and 1 ignored in 753 s; the NumPy test passed in 74 s; both
  structure suites 52.
- Linux arm64, 46 minutes, load average on the host 2.7 at the start and
  2.4 at the end, above 12 for six minutes, above 20 for one and up to
  23.3. The decide suite: 30 and 1 ignored in 571 s; the NumPy test passed
  in 526 s; both structure suites 52.

### On GitHub's runners

The first hosted run, on 2026-11-18 at 6e209ff (the same code and tests as
cebca8f), started by the push; run 44210618566 of the workflow. With the
NumPy test, without the tests that ask crates.io, and with
`CARCINIFY_TOO_BUSY_IS_NOT_A_FAILURE=1`. Rust 1.100.0 and maturin 1.16.0
on both.

- Linux x86-64, image `ubuntu-26.04` version 20261111.196.1, kernel
  7.0.0-1019-azure, 4 CPUs, Python 3.13.18: exit 0, 33 rows as expected
  (36 less the three that need the network). The decide suite: 29 passed
  and 1 ignored in 819 s, no run repeated, nothing listed as not judged;
  the NumPy test passed in 302 s; both structure suites 52. This is the
  first build and the first pass on x86-64.
- macOS arm64, image `macos-26-arm64` version 20261102.0473.1, Darwin
  25.6.0, 3 CPUs, Python 3.12.10: **failed**, in one test. The decide
  suite: 28 passed, 1 failed, 1 ignored in 927 s, 1 run repeated; every
  other row as expected, the NumPy test included (138 s). The test is
  `load_that_falls_inside_the_runs_is_seen`: other programs take the CPU
  only while the workload runs, and the test expects `unreliable`. The tool
  answered `consistent` (76.2% waiting, fastest of 3 runs 2.852 s).

  The cause: with fewer than four CPUs the tool does not start the process
  that computes beside a run, and on macOS nothing else sees such load.
  That is the limit the tool's README states, met by a test that did not
  allow for it. Repeated on an 8-CPU Mac by switching that process off:
  the test failed three times of three, once with the same answer. On
  Linux limited to three CPUs in the container the test passed three times
  of three. Since then the test is skipped on macOS under four CPUs and
  says so, and the report carries a flag on a machine with fewer than four
  CPUs. The next entry is the first hosted run with that change.

The second hosted run, on 2026-11-18 at 7675eec, started by the push; run
44256277301 of the workflow, 07:06 to 07:30 UTC, set up as the first. Rust
1.100.0 and maturin 1.16.0 on both. Both systems passed.

- Linux x86-64, image `ubuntu-26.04` version 20261111.196.1, kernel
  7.0.0-1019-azure, 4 CPUs, Python 3.13.18: exit 0, 33 rows as expected.
  The decide suite: 30 passed and 1 ignored in 687 s, no run repeated; the
  NumPy test passed in 235 s; both structure suites 52.
- macOS arm64, image `macos-26-arm64` version 20261112.0504.1, Darwin
  25.6.0, 5 CPUs, Python 3.12.10: exit 0, 33 rows as expected. The decide
  suite: 30 passed and 1 ignored in 813 s, no run repeated; the NumPy test
  passed in 104 s; both structure suites 52.

The macOS runner had 5 CPUs this time, not 3. So
`load_that_falls_inside_the_runs_is_seen` ran and passed, no line with
"SKIPPED," was printed, and the flag for fewer than four CPUs does not
apply on such a machine. This run shows that the suite passes on a hosted Mac with 5 CPUs.
It does not show the skip or the flag at work. The flag has run only in a
Linux container limited to three CPUs; the skip has not run anywhere. The
failure they answer was repeated on an 8-CPU Mac with the second process
switched off, before either was written. The two hosted runs are all that is known here about how
many CPUs GitHub's macOS runners have: 3 once, 5 once.

## Differences found, and what was done

The first Linux run (commit d1374ae) had 25 failing tests. All had one of
three causes, and none was a difference in what the tools report for the
examples.

1. **22 failures in the structure tests: caused by the run script.** The
   script set `CARGO_TARGET_DIR` for everything. The measuring directory was
   named after the crate alone, the test crates are all named `port`, and so
   they shared one directory and read each other's builds. The script no
   longer sets the variable, and the tool now names the directory after the
   crate's name and a digest of its path. Test:
   `one_target_directory_for_everything_does_not_mix_two_crates`. This was a
   real defect for a user who sets one target directory for several
   projects; it did not need Linux to appear.
2. **2 failures in the diff tests: Debian loads a file the tests did not
   expect.** Debian's Python imports `/etc/python3.13/sitecustomize.py` at
   start-up. The harness lists it among the files loaded from outside the
   project, which is correct. Two tests expected that list to hold only what
   they put there. The tests now set the distribution's own files under
   `/etc/` apart; the harness is unchanged. A report made on Debian will
   list this file.
3. **1 failure in the decide tests: the workload was too short on a faster
   interpreter.** Python 3.13 ran a test workload in 0.171 s, under the
   0.2 s the tool requires, and the tool refused as designed. The workload
   in the test is larger now.

The second run (commit c6ca60c) had one failure:

4. **A test that judges by time failed once.**
   `sizes_that_measure_the_same_are_flagged` gives the same input under two
   labels and expects the two times to agree within 10%. Beside the other
   tests on a loaded machine they were 220 ns and 263 ns. The threshold is
   unchanged; the test now runs with no other test of that file beside it,
   and the limit is in the structure tool's README: on a busy machine the
   same input has been seen 19% apart, so the flag can be missing there.
   Passed in the two runs since.

Also seen, not a failure: with cProfile on, a test workload of many tiny
calls ran 9.5 times slower on Python 3.13 on Linux and 2.4 times slower on
Python 3.9 on macOS. The decide tool reports this slowdown in each report.

## Linux x86-64 under emulation: attempted, not completed

One attempt, test suites only. The container started at 23:32 UTC. fmt and
clippy passed. `cargo test --locked -p carcinify-diff` then stopped making
progress about three minutes in, while compiling dependencies (the log ends
at `Compiling zmij v1.0.23`), and stayed so for 4 hours 55 minutes at no CPU
use until the container was stopped by hand.

What the container showed when stopped: `cargo` asleep with all seven of its
threads waiting on locks; one `rustc` child that had exited (3 s of CPU) and
had not been collected by cargo; no other process. So cargo itself hung
while waiting for a compiler process that had already finished. Every
process in that container runs through Rosetta. The cause was not
investigated further and the run was not repeated, so whether it happens
every time is not known. No test ran, so nothing is known about the tools
on x86-64.

**The run had no time limit, though one was intended.** The limit was meant
to come from the shell that started the run; a job started in the
background there has none, and nothing else was watching. `ci/linux/run.sh`
now stops the container from outside after `CAP_SECONDS` (default 7200),
writes a line saying so into `results.tsv`, and exits with the container's
status. Tried with a limit of 75 s on Linux arm64: the container was stopped
and removed, the line was written, exit status 137.

## Not tested

- Linux x86-64 beyond the two hosted runs above.
- macOS with fewer than four CPUs, since the test was made to skip there.
- A Python built without the GIL.
- musl (Alpine), other distributions, other glibc versions.
- Python 3.12 on Linux and Python 3.13 on macOS: each platform was run with
  one Python.
- Windows.
- A build on a bind mount or a network filesystem, where file times behave
  differently.
