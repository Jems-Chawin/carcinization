# Running everything

`run-all.sh` runs every test suite and every worked example of the four
tools in a checkout, writes one line per result, compares the results with
`expected.tsv` and exits 1 on any difference. `linux/` runs it in a
Linux container, and `.github/workflows/ci.yml` runs it on GitHub's runners.

## On this machine

```sh
CARCINIFY_PYTHON=$PWD/.venv/bin/python CARCINIFY_MATURIN=$PWD/.venv/bin/maturin \
    ci/run-all.sh . target/ci/local
```

Needs the toolchain in `rust-toolchain.toml`, git, a Python 3.12 or later
and maturin 1.16. `RUN_LIVE=1` adds the dependency check's tests against
crates.io, docs.rs, rustsec.org and pypi.org, and its two examples live and
then replayed from the answers just fetched.

The script does not stop at the first failure. Results are in
`<out>/results.tsv`, versions in `<out>/versions.tsv`, the comparison in
`<out>/comparison.txt`, and each suite's and example's output beside them.

## What makes a run fail

`compare.py` sets `results.tsv` beside `expected.tsv`. The run exits 1 when

- fmt, clippy or the release build fails;
- a test suite fails, or runs a different number of tests than expected;
- an example ends with another exit status, verdict or count than expected;
- a row appears that the file does not have, or one is missing;
- a live dependency example and its replay give different outcomes;
- the run leaves a change in the checkout, or was stopped by the time cap.

Some values are judged by time and differ between machines: the flags
`carcinify decide` raises from a time measured in the run (its flags that
come from a count or a declaration are compared, by kind), nanoseconds per
iteration, durations. Comparing
them would make the run fail at random; leaving them out silently would
hide them. They are printed in the last column of every example row and are
not compared; `expected.tsv` says so, and `RESULTS.md` records the
differences seen. `carcinify decide` answers `unreliable` on a busy machine;
an example that gets that answer is run again, up to three times, and the
row says how many runs were made.

`CARCINIFY_NUMPY_PYTHON` (an interpreter with NumPy) adds the decide tool's
test with the real package. `SUITES_ONLY=1` stops after the suites.

## Tests a loaded machine can fail, and what it looks like

All are in `decide/tests/cli.rs`. Each measures a workload and asserts
something that follows from the times. `carcinify decide` answers
`unreliable` when it sees that the machine was busy, and the tests then run
it again; what is listed here is what that does not cover. None of it has
been made to fail on purpose; the first is the only one seen.

- **Any test that measures**: six `unreliable` answers in a row fail it
  with "the machine is too busy to run this test". Seen on macOS with a
  load average above 30, in up to ten tests of one run
  (`a_missing_decision_or_answer_is_incomplete` among them). Each repeat is
  counted in the `note` row of `results.tsv`.
- `a_port_for_speed_states_what_speed_is_needed`: wants the candidate's
  share between 35% and 65%. Load too light for the tool to call the run
  unreliable can still move a share; the failure prints the report.
- `a_workload_that_uses_every_cpu_is_not_taken_for_a_busy_machine`: the
  flag "the workload itself used ..." needs the workload to get half the
  CPUs, and the test wants no reason for a busy machine. Other programs
  can take the CPUs from it: the flag is missing, or a reason is listed.
- `threads_at_work_outside_the_candidate_do_not_refuse_a_stand_in`: wants
  more than 1.05 times as much CPU time as wall time for threads that
  really work inside a stand-in. Threads kept off the CPU lower that.
- One-sided limits with wide margins, least likely to move: a share under
  5% in `a_port_for_speed_of_code_that_waits_is_contradicted`; over 100
  times faster in `the_algorithm_question_is_set_against_the_decision`; a
  share or bound over 50% in
  `a_candidate_that_uses_a_compiled_package_gets_no_share_and_no_ceiling`
  and `a_stand_in_workload_gives_bounds_and_the_verdict_says_it_rests_on_them`;
  over 90% in `a_compiled_package_is_found_in_a_helper_and_in_an_attribute`
  and `a_package_that_is_loaded_and_not_seen_in_the_candidate_has_to_be_answered_for`;
  under 10% and over 90% in
  `real_numpy_is_not_reported_as_the_projects_own_python`.
- `load_that_falls_inside_the_runs_is_seen` and
  `a_report_made_on_a_busy_machine_says_so_in_its_verdict` make the load
  themselves and expect `unreliable`, without a repeat. More load makes
  that answer surer. Each also expects one reason by name (on macOS only,
  in the first); other load is believed to add reasons and not to remove
  that one. The second waits one second for its own load to start.

Two tests were on this list and no longer are: the idle-threads flag in
the NumPy test and `sizes_that_measure_the_same_are_flagged` in
`structure/tests/cli.rs` now assert the flag exactly when the report's own
figures call for it. The harness tests of a hang give the case after the
hang 20 s to answer where they gave it 2 s.

## On GitHub's runners

`.github/workflows/ci.yml` runs `run-all.sh` on `ubuntu-latest` (x86-64)
and `macos-latest` (arm64) for every push to `main` and every pull request,
with the NumPy test and without the network tests. It has read access to
the repository and nothing else, uses no secrets, and names the two actions
it uses by commit.

Those runners are shared machines, and the list above says what that does
to the tests of `carcinify decide`. So the workflow sets
`CARCINIFY_TOO_BUSY_IS_NOT_A_FAILURE=1`, and `compare.py` then lists two
things as "not judged, the machine was too busy", with a warning on the
run, where it would otherwise fail:

- a decide suite in which every failed test gave up after six `unreliable`
  answers in a row, the number of tests that ran is the expected one, at
  least one test passed (the NumPy suite, one test, is let off that), and
  cargo's exit status is 101, the one a failed test gives;
- a decide example that answered `unreliable` three times.

No assertion is changed by it: a test that fails with any other message,
a missing test, another verdict, all fail the run as before, and the
variable does nothing to the other tools. What it gives up: a run with
warnings has not tested what the warnings name, and a change that made the
tool call every machine busy would show as warnings (and as the count of
repeats in the `note` row), not as a failure, unless no decide test passes
at all. The tests that check the tool does not call a quiet machine busy
are among those let off, so a hosted run cannot catch that kind of
regression; a local run does. `ci/test_compare.py` tests the rule.

A local run does not set the variable.

One test does not run on the macOS runner, which has three CPUs:
`load_that_falls_inside_the_runs_is_seen` needs the process the tool runs
beside a workload, and under four CPUs there is none. On macOS with fewer
than four CPUs the test prints a line with "SKIPPED," and counts as
passed; `run-all.sh` copies such lines into `results.tsv` as `note` rows,
so they are in the run's summary. That detector is tested by local runs
and on the Linux runner only.

## In a Linux container

```sh
ci/linux/run.sh                      # linux/arm64
ci/linux/run.sh linux/amd64          # under emulation on an arm64 host: slow
RUN_LIVE=1 ci/linux/run.sh
VENV_IN_TREE=1 ci/linux/run.sh linux/arm64 venv-in-tree
```

- The image is `rust:1.100.0-trixie`, pinned by digest in the Dockerfile:
  Debian 13, glibc. Alpine (musl) is another target for wheels and is not
  covered.
- Only two things of the host are mounted: the repository, read-only, and
  `target/ci/` for the results.
- **Nothing is built on the mount.** The repository is cloned inside the
  container and built on the container's own filesystem. A bind mount from
  macOS carries macOS file times, and the stale-build and stale-bytecode
  checks depend on file times.
- Debian's Python refuses `pip install`, so maturin goes into a virtual
  environment. `VENV_IN_TREE=1` puts that environment inside the checkout,
  which is where many projects keep it, to exercise the rule about
  third-party files under the repository.

- A run that has not finished after `CAP_SECONDS` (default 7200) is stopped
  from outside the container. A run under emulation has hung without this.

Results from the runs made so far are in `RESULTS.md`.
