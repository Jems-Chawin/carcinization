<p align="center"><img src="assets/tide-line.svg" width="560" alt="A python slithers right, grows plates and legs through a shrimp and a lobster, and ends as a crab"></p>

<h1 align="center">carcinization</h1>

<p align="center"><em>“Eventually, crab.”</em></p>

<p align="center">Move a Python codebase to Rust one piece at a time, and measure every step: <code>carcinify</code>.</p>

Tools and step texts for an agent skill that moves parts of a Python
codebase to Rust one piece at a time behind [PyO3](https://pyo3.rs). Each step comes with a
tool that measures the result, so that the numbers in a report come from a
run and are not typed by the agent. The workload, the test cases and the
driver that the tools run are still written by the agent: the tools record
what ran, with hashes and the commit, and each tool's README lists what it
cannot stop. "Do not port this" is a normal outcome of the first step.

## Status

Development stopped on 2026-11-18 and was resumed on 2026-11-19. The
project moves Python to Rust and no other pair of languages.

What exists and was tested: `carcinify decide`, `carcinify deps`,
`carcinify structure`, the comparison harness `carcinify diff` and
`carcinify speed`, with the texts of steps 1, 2, 4 and 6 and the timing
part of step 7, which are meant to be read and followed by a coding agent.
The pilot for step four stands as it was reported: the pattern was
observed in 1 of 4 runs, which is inconclusive by the rule fixed before
the runs.

They have been used on one real port, outside this repository: NumPy's
greedy path search for `einsum` (`_greedy_path`), moved to Rust through
PyO3. Its answers match the original's on 595 comparison cases. Timed with
`carcinify speed`, the search is 31 times faster on macOS and 28 times on
Linux arm64 (in a container on the same Mac) on a made-up network of 40
operands, and 22 and 8.5 times on one of 120 operands. Through
`np.einsum_path` the gain is 6.8 times on macOS and 5.9 on Linux arm64.
The earlier figures, 26 to 28 and 18, came from a script whose inputs
shared one string object per label, which made the original's set
operations faster. At 120 operands, Linux's default CPU target has LLVM
vectorize one loop of the port, and on Linux in Docker that vectorized
loop is the slow one; on macOS it is not. Step 7's text says what this
means for anyone shipping a Linux aarch64 wheel. What the port showed the
tools lack sets the next work.

What is planned and is not built, with no date for any of it:

- Step 3 as a step: the comparison tool exists, its instructions do not.
- Step 5, the port of one piece through PyO3.
- The rest of step 7, the period when the old and the new code run side
  by side. Its timing part exists.
- A top-level `SKILL.md` that ties the steps together.

Another tool for the port exists:
[rust-anything](https://github.com/erkamyaman/rust-anything). It generates
the PyO3 bridge and recorded cases, and times a finished port. It was tried
here at v1.4.0, on Python only, in a Linux arm64 container. In those runs
its profile advised a port in the four cases where `carcinify decide`
answers `contradicted`, so the two can be used together: `carcinify decide`
first.

## What exists

One command, `carcinify`, with five subcommands. Each is a library crate in
this workspace behind the one binary in [`carcinify/`](carcinify/):

| tool | what it does | step |
|---|---|---|
| [`carcinify decide`](decide/README.md) | Measures where a Python workload spends its time and checks a declared decision to port, to fix the algorithm first, or not to port, against it | [1 decide](skill/steps/01-decide/STEP.md) |
| [`carcinify deps`](deps/README.md) | Reads the imports of the code to be ported and looks up on crates.io what the proposal names for each | [2 dependency check](skill/steps/02-dependency-check/STEP.md) |
| [`carcinify diff`](harness/README.md) | Runs the Python original and the Rust port on the same cases and reports whether the outputs are identical, NumPy arrays included: by dtype, shape and elements, with floats exact unless a case names a tolerance | 3 compare (tool only; the step is not written) |
| [`carcinify structure`](structure/README.md) | Counts allocations on the port's hot path at two input sizes and lists the copies made in the code reachable from it | [6 structure report](skill/steps/06-structure-report/STEP.md) |
| [`carcinify speed`](speed/README.md) | Times the Python original and the port from the Python caller, behind a load gate, on macOS and Linux arm64, compares the two platforms, and counts what the binding allocates | [7 speed](skill/steps/07-speed/STEP.md) |

One step has no tool: [4 ownership design](skill/steps/04-ownership-design/STEP.md),
a design note written before the port. Whether ports made without it show
the pattern it is meant to prevent was tried once, in a
[pilot](skill/steps/04-ownership-design/pilot/README.md): the pattern was
observed in 1 of 4 runs on one task written to invite it, with one model.
By the rule fixed before the runs that is inconclusive.

Each step directory has an `EVIDENCE.md` saying how far each of its claims
is supported; steps 1, 2 and 4 have worked examples. [`skill/README.md`](skill/README.md)
describes the layout.

## Tested on

Results and how to repeat them are in [`ci/RESULTS.md`](ci/RESULTS.md).

- macOS arm64, Python 3.12: all test suites and worked examples pass. On
  GitHub runners: two passes, on 2026-11-18, at commit 7675eec on a runner
  with 5 CPUs and at 28955a7 on one with 3 CPUs, 33 rows each.
- Debian 13 on arm64 (glibc, Python 3.13), in a container on a Mac: all
  test suites pass, and every worked example ends with the same verdict,
  exit status, and case and allocation counts as on macOS. What is judged
  by time differs: `carcinify decide` raises a flag more or a flag fewer on
  some examples, and the times are other times. That is one distribution,
  one Python and one kernel, the container host's.
- Linux x86-64 (Ubuntu 26.04, Python 3.13, 4 CPUs): three passes, on
  GitHub runners on 2026-11-18 at commits 6e209ff, 7675eec and 28955a7:
  all test suites, and the worked examples as expected, 33 rows, without
  the three that ask crates.io. Not run anywhere else on x86-64; an earlier attempt under
  emulation hung in the build.
- Not tested: Windows, musl, a Python without the GIL.

A workflow runs `ci/run-all.sh` on GitHub's runners for every push (see
[`ci/README.md`](ci/README.md)). Its first run, at 6e209ff, failed on
macOS in one test of `carcinify decide`, for the reason given under the
open findings: the runner had 3 CPUs, and with fewer than four load inside
a run is not seen there. Since then that test is skipped on macOS with
fewer than four CPUs, and the report carries a flag. The second run, at
7675eec, passed on both systems, but its macOS runner had 5 CPUs, so the
test ran and neither the skip nor the flag came into play. The third run,
at 28955a7, passed on both systems on a macOS runner with 3 CPUs: the
test was skipped and said so, and the five example reports of `carcinify
decide` carried the flag. That is one run on one Mac with three CPUs. The
macOS runners of these three runs had 3, 5 and 3 CPUs; no more than that
was seen.

## Known open findings

- `carcinify decide`: in a project that loads a compiled package such as
  NumPy, this tool cannot show that a candidate's time is its own Python.
  A port for speed that passes there rests on stand-ins the author wrote
  or on a sentence the author typed. The verdict says which; read that
  before the number.
- `carcinify decide` on a busy machine: pure computation reads as waiting.
  The tool tests for this around and during every run and answers
  `unreliable`. With fewer than four CPUs on macOS, on Windows, or for a
  workload that itself uses half the CPUs or more, it does not see load
  that begins and ends inside one run, and the report carries a flag
  saying so. On Linux with three CPUs it was still seen for a workload on
  one thread, by the time the process spent ready and not running: that
  rests on three runs of one test in a container.
- `carcinify decide` and threads. A stand-in whose candidate still does
  threaded work, on data the tool cannot find, passes with a flag as the
  only sign, and with no sign when the library runs on one thread (worst
  measured: bounds of 50 to 52% for a candidate whose own Python was
  1.4%). The flag's wording goes by the CPU time over wall time for each
  entry of the candidate, so such work spread over many entries is worded
  as idle threads (measured: 42 to 64 s over 400 entries); read the total.
  And on
  Linux, other programs that use part of the CPUs can slow a threaded
  workload with nothing said: its time came out about 30% long and a
  Python candidate's share two to three points low, never high.
- `carcinify diff` compares the values two implementations return for the
  cases it is given. It is not a benchmark, it says nothing about inputs
  nobody wrote a case for, and it has not been run on a real project.
- `carcinify diff` compares NumPy arrays of five dtypes (float64, float32,
  int64, int32, bool) and no NumPy scalars; anything else is refused by
  name.
- `carcinify structure`: an entry point defined twice, under `cfg(feature)`
  and `cfg(not(feature))`, cannot be declared.
- `carcinify structure` now sees three things the step-four pilot ran
  into, each within limits. A field of `self` taken out and put back
  through a `RefCell` borrow or a lock is seen when the borrow is written
  in the same expression, directly on a field of `self` with only `unwrap`
  or `expect` after, not when it is held in a variable first. An `Rc`
  or `Arc` of a type whose own fields are cells is counted as shared
  ownership, by the type's name, for structs, not enums. A helper that
  returns a new collection is still one copying construct, with the number
  of calls to it written on the hot path printed beside it, which is not
  how often they run. It is recognised by the return type itself, not one
  wrapped in `Option` or `Result`, and a call on a field is counted for
  every type with a method of that name. The pilot's numbers were measured
  before this.
- The limits each tool states for itself are in its README.

## Build and test

Needs the Rust toolchain named in `rust-toolchain.toml`, git and Python
3.12 or later. Some tests and the examples also need
[maturin](https://www.maturin.rs) 1.16.

```sh
cargo build --release --locked        # target/release/carcinify
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

`cargo test` takes about a quarter of an hour, most of it the tests of
`carcinify decide` (8 to 13 minutes in most of the recorded runs). Those tests
measure time, and on a busy machine they give up with "the machine is too
busy to run this test".

A first report, from one of the worked examples of step one:

```sh
target/release/carcinify decide skill/steps/01-decide/examples/own-python/decide-original.json \
    --report target/carcinify-decide/report.json --markdown target/carcinify-decide/report.md
```

It measures the example's workload with the `python3` on the path and
writes the report to `target/carcinify-decide/report.md`; one to two minutes
(50 s and 93 s in two runs on macOS). The verdict is at the
top. The other examples are listed in
[`skill/steps/01-decide/examples/README.md`](skill/steps/01-decide/examples/README.md).

Everything, with the tests that build Python extension modules and all the
worked examples:

```sh
python3 -m venv .venv && .venv/bin/pip install maturin==1.16.0
CARCINIFY_PYTHON=$PWD/.venv/bin/python CARCINIFY_MATURIN=$PWD/.venv/bin/maturin \
    ci/run-all.sh . target/ci/local
```

[`ci/README.md`](ci/README.md) has the options and the Linux container.

## How this was written

The code and the step texts were written by AI coding agents. Every piece
was then reviewed by a separate agent acting as auditor, which reran the
work and tried to break it, and a human decided what to build and what to
accept. The audit reports and the history of findings and fixes are not
part of this repository.

This repository was published as a single commit. Commit ids in the text
refer to the development history, which is not published; that the pilot's
rule was committed before its runs is the author's statement.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work by you, as defined in the Apache-2.0
licence, shall be dual licensed as above, without any additional terms or
conditions.
