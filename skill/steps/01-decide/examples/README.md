# Worked examples: three workloads, three different answers

Each directory is a small Python project with a workload, a declaration
and the report `carcinify decide` wrote for it. The committed reports were
generated from commit `b8f160d` on one machine (macOS, arm64, 8 logical
CPUs, Python 3.9.6, the interpreter these example projects are taken to
run on). The machine was in use by other programs at the time, with a load
average between 7 and 9. All five reports are the first try.
In the reports kept no run is marked as disturbed, and the process that
computes beside each run was kept off the CPU for under 1% of its time. The measurements are the tool's; the declarations and the reading
below are the author's. The projects were written to show each case and
are not real ones. None of them uses a compiled package from outside the
standard library; for that case see the tool's tests.

| | Workload | Where the time goes | Candidate's own Python, `p` | Decision declared | Verdict |
|---|---|---|---|---|---|
| `waiting/` | 400 invoices converted with rates from a service (a stand-in that sleeps 30 ms per request) | 99.9% waiting | 0.05% | do not port | consistent |
| `native/` | 24 batches of 4,000 records written as JSON, compressed and hashed | 74.5% compiled code, 25.1% library Python, 0.2% own Python | 0.2% | do not port | consistent |
| `own-python/`, original | 900 customer records, each compared with every other | 99.8% own Python | 99.99% | fix the algorithm first | consistent |
| `own-python/`, after the fix, checked in the background | 200,000 records grouped by fingerprint | 99.9% own Python | 91.1% | do not port | consistent |
| `own-python/`, after the fix, checked inside the upload request | the uploaded file of those records, 5.4 MB, read and grouped | 99.6% own Python | 99.2% | port, for speed, to finish within 0.2 s | consistent |

What each shows:

- **`waiting/`.** The candidate, with what it calls, is the whole run, and
  almost none of that is Python: it is a sleep. A profile that only ranked
  functions by time would put it first. Declaring `port` for speed here is
  refused: the workload could be at most 1.0 times faster.
- **`native/`.** `zlib.compress` is three quarters of the time. The quarter
  shown as library Python is the standard library's JSON encoder, whose
  compiled part the profiler cannot see because it is reached through an
  object, not a function; the report says so. The project's own code is
  three lines.
- **`own-python/`, original** (`decide-original.json`). All of the time is
  the project's own Python, in one function with one caller: the picture of
  a port candidate. The author's answer to "language or algorithm" names a
  better algorithm, and the tool measured it in the same run: **about 640
  times faster, in Python** (3 milliseconds against 1.99 seconds; the tool
  flags that 3 milliseconds is too short to time well). Declaring `port`
  here is refused, with the instruction to make the change and decide on
  the improved code.
- **`own-python/`, after the fix, in the background**
  (`decide-after-fix.json`). At the size of the largest file, 200,000
  records, the improved check takes 0.78 seconds, and 91% of it is the
  candidate's own Python. If that cost nothing it would take 0.07 seconds.
  **Do not port.** The check runs in the background and nobody waits for
  it: 0.71 seconds saved on such a job is not worth a boundary between two
  languages, with 200,000 records to convert on every call. With `port` and
  no need stated the tool answers `incomplete`.
- **`own-python/`, after the fix, inside the upload request**
  (`decide-port.json`, `upload.py`). The same check where the page waits
  for it and the step has 0.2 seconds. **Port.** What makes this a case
  one could defend:
  - **The workload starts where the port would.** The port is to take the
    uploaded file's bytes, so the workload hands the candidate those bytes
    and reading the file is inside what is measured: 0.74 seconds, 99% of
    it the candidate's own Python. Measuring only the grouping, as the
    example above does, would have left the port's own parsing out of the
    best case.
  - **The need is a sentence and a number**, and the report sets it beside
    the best case: 0.006 seconds if the candidate cost nothing, which
    reaches 0.2. Had the limit been 0.005 seconds the tool would have
    refused: no port of this piece can reach it.
  - **The reason deals with what crosses the boundary.** One bytes object
    in. About 10,000 pairs of ids out, which the tool flags as 30,000
    Python objects; the port can return them as one buffer.
  The verdict is `consistent`: nothing measured contradicts it. Whether the
  page really has 0.2 seconds is the author's statement. On Python 3.12 the
  same workload took 0.48 seconds in one timing, so the need holds there
  too, with less to spare; a project should measure on the interpreter it
  runs.

## How much the numbers vary

From two runs of the first four examples one after the other, at an
earlier commit of the same tool (`921c9af`, before the fifth example
existed):

| | Classification | Largest share, first and second run | Fastest plain run, s | Runs differ by (of the median) |
|---|---|---|---|---|
| `waiting/` | same | 99.92%, 99.91% | 0.673, 0.721 | 1.1%, 4.1% |
| `native/` | same | 74.5%, 74.6% | 0.371, 0.376 | 0.7%, 1.1% |
| original | same | 99.9%, 97.7% | 1.96, 2.03 | 1.2%, 5.7% |
| after the fix, background | same | 99.97%, 99.8% | 0.762, 0.775 | 2.5%, 2.9% |

`p` for the last one was between 91.1% and 91.5% in five runs, so the
ceiling was between 11.2 and 11.7: a ceiling near the top of the range
moves a lot for a small change in `p`. (In an earlier pair of runs `p` was
89.3% and 91.4%, ceilings 9.4 and 11.6.) The factor for the alternative was
between 630 and 674.

On a busy machine none of this holds. With other programs taking the CPU, a
pure computation has been reported as 31%, 40% and 70% waiting. The tool
now times a loop of pure computation around every run and keeps a process
computing beside it, leaves out runs made while the machine was busy, and
gives the verdict `unreliable` when none is left.

## Running them

```sh
cargo build --release -p carcinify
target/release/carcinify decide skill/steps/01-decide/examples/own-python/decide-original.json \
    --report target/carcinify-decide/report.json --markdown target/carcinify-decide/report.md
```
