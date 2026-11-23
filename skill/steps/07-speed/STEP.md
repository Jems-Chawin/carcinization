# Step 7: Time the port against the original, from the caller

Do this when the port passes the comparison harness on the commit you
will time, and step six's report is written. Step six says how the port is
built. This step says how fast it is where the user calls it, and what a
call costs in allocations. It is the speed part of step seven, the period
when the old and the new code run side by side: here both are imported
into one Python process and timed in turn.

A port's speed figure is the one people quote. This step takes it from
the caller, with the two sides timed in alternation on a quiet machine,
and gives it with its spread, on each platform the users run.

How far each claim here is supported is in `EVIDENCE.md`. In short: the
tool's checks are tested on its own fixture, and it has timed one real
port, the einsum port, in a scratch copy during one investigation; nothing
else yet.

## What you hand over

- `speed.json`, the declaration, committed with the suite.
- For each platform: `build.json` from `speed build`, and `report.json`
  from `speed run` under the gate, with the commit they describe.
- With two platforms, the output of `carcinify speed --compare`.
- A sentence on each flag and each `unreliable` reason the reports give.
- If you ship a Linux aarch64 wheel, the lines of section 6, with your
  own figures where you have them.

## What to do

### 1. Write the declaration

`speed.json` names the harness's suite, the number of `runs`, and the
targets. `speed/README.md` has the format.

- **Direct targets** time the compared function on one case of the suite.
  Take the workload step one profiled, at the size step six measures. The
  case file has to be one of
  the suite's own, so that `carcinify diff` compares what is timed; a case
  that sets `expect_raise` is refused.
- **Caller targets** time the function the user actually calls, which
  reaches the compared function a few calls down. Write a small Python
  function that calls it, in a file in the suite's directory, and name
  the compared function it reaches (`fn`) and the attribute where each
  side's function is put (`replace`). The gain the user sees is this one:
  on the einsum port, the search alone at 40 operands ran 26 to 28 times
  faster on macOS, and `np.einsum_path` 6 to 7 times.
  - Every input the caller gives the compared function has to be a case
    of the suite. `speed build` calls the caller twice with the original
    in place and records each call; a call that is not a case refuses the
    target (`timed_input_not_compared`), and so do two recordings that
    differ (`timed_input_unstable`). Add the missing case to the suite,
    run diff again, then build again.
- **`runs`** is the number of pairs per target: each side runs that many
  times. The investigation in `EVIDENCE.md` used 15.
- **`calls_per_run`** is how many calls one run times; the run's figure is
  their mean. A run shorter than about 1 ms is flagged `short_run`, since
  loop and timer overhead start to count: raise it until each run is
  longer.

### 2. Add the counting allocator

Copy [`counting-allocator-template.md`](counting-allocator-template.md)
into the port's crate: a feature `count_allocations`, a module of about 35
lines that counts every allocation, and one exported function,
`_speed_allocations()`. `speed build` makes a second wheel with the
feature, counts with it, and never times it. Without the feature the port
is still timed, and each target is flagged `counts_not_measured`. A Python
port has no binding to count.

### 3. Run diff, then build, then time

In this order, on a committed tree:

1. **`carcinify diff`** on the commit, passing. speed compares one timed
   call's output with the original's, and no more: it keeps both sides'
   directories on `sys.path`, so a port that imports a module only the
   original provides would run. diff stops that.
2. **`speed build`**, not under the gate:

   ```sh
   carcinify speed build speed.json --out target/carcinify-speed
   ```

   It builds the timed wheel as diff builds it, and the counted wheel;
   makes each target's untimed calls; takes the counts; and writes the
   wheels, their SHA-256 and `build.json` to `--out`. Keep `--out` under
   `target/` or outside the repository. It refuses to run under the gate.
3. **`speed run`**, under the gate:

   ```sh
   LOG=target/gate.log ci/gate.sh carcinify speed run speed.json \
     --wheels target/carcinify-speed --report target/carcinify-speed/report.json \
     --load-log target/gate.log
   ```

   `ci/gate.sh` waits for 30 readings of the load average under 6, 10 s
   apart, then starts the command. `speed run` checks the wheels against
   the hashes `speed build` wrote, and builds nothing.

**Linux arm64.** `LOG=<host gate log> ci/linux/speed.sh <repo> <speed.json>
<out>` does all of this in the CI's container, gated by the host. Gating
inside the container would read the load of Docker's VM, not of the Mac.
`ci/README.md` and `speed/README.md` give its rules.

### 4. Read the report

**The figures.** For each target:

- each side's best, median and worst time per call over the runs kept;
- the ratio original ÷ port within each pair of runs, with its median and
  its range over the pairs. The sides alternate original, port, then
  port, original, so a drift favours each side in turn.

**The range is not an interval.** It is the lowest and the highest pair
ratio, and shows how noisy the pairs were; a confidence interval for the
median would be narrower. Quote the median with its range, the machine,
the input and the number of runs.

**A case's arguments share no objects.** The case encoding cannot say
that two equal values are one object, so a direct target's case is decoded
with no object shared but those CPython itself shares, such as small ints
and one-character Latin-1 strings. A function whose speed depends on its
arguments' identity is timed in that setting; a caller target's inputs are
the caller's own.

**Pairs dropped.** A loop of pure computation is timed before and after
every run. A pair with a run whose loop was kept off the CPU for more than
10% of its time is dropped, and the report counts the pairs dropped.

**The verdict**, with the exit status:

- `pass` (0): every target timed, nothing flagged.
- `pass_with_flags` (0): read each flag below.
- `unreliable` (1): the figures are in the report, and cannot pass.
  - The log does not show the gate's rule for this run: `no_gate`,
    `gate_rule`, `not_this_run`, `gate_not_open`, `load_not_watched`.
  - `busy_beside`: a process computing beside the timing was kept off the
    CPU for more than 15% of its time.
  - `every_pair_dropped`.

  Time again on a quiet machine.
- `fail` (1): a target failed, for a reason to fix before timing again.
  A side raised on its case (`raised`); its output differed from the
  original's (`output_differs`) or could not be compared with it
  (`output_not_compared`); diff would refuse the function
  (`function_refused`); a caller's inputs changed between build and run
  (`timed_input_unstable`); the port raised or answered otherwise in a new
  thread (`raised_in_new_thread`, `output_differs_in_new_thread`); one
  side changed its arguments and the other left them as they were,
  whichever side it was (`arguments_changed_differently`); or it ran past
  the time limit (`timeout`) or crashed (`crashed`).

`speed run` exits 2 when it was refused or could not be carried out;
`speed build` exits 1 when it refuses targets and 2 when it cannot run.

**The flags:**

- `load_at_or_over_6`: a reading of 6 or more while timing. The figures
  stand; set them beside a clean run before quoting them.
- `short_run`: see `calls_per_run`.
- `load_beside_not_watched`: no process computed beside the timing (fewer
  than four CPUs, or a timing under 0.05 s), so only the loops around each
  run watched for load.
- `cold_thread_not_measured`, `counts_not_measured`: each says why.
- `workspace_grows`: see the counts.

**Cold minus warm.** Warm, which the ratios use, is a call after the same
call has run in that thread. The cold thread is measured apart: in each of
`runs` new threads, a cheap call into the port's module, then the port's
first call there and its next, each timed alone. The report gives cold
minus warm per thread, with its median and range: what the first call in a
new thread costs, a per-thread workspace being made, for instance. The
cheap call is the compared function with no arguments, which PyO3 refuses
before the body runs. A function with no required parameter needs an
exported `_speed_noop` instead.

**The counts**, for a compiled port built with the allocator. `speed build`
takes them; they are exact and need no quiet machine.

- `warm`: the port's Rust allocations, frees, bytes allocated and live
  bytes over one warm call. The first call is given apart (`first`): it
  grows workspaces and caches, and counted alone it would inflate both.
- `result_bytes`: Rust memory the result still holds when the caller gets
  it, which is the result's, not a workspace's.
- `workspace_kept_bytes`: the live bytes the first call left behind, the
  workspace the port keeps.
- `growth_over_16_warm_calls_bytes`: how far the live bytes moved over 16
  more warm calls, each result dropped. Anything but zero is flagged
  `workspace_grows`: a workspace that keeps growing, or a leak.
- `python`: by `tracemalloc`, the objects a warm call leaves to its
  caller (the result), in blocks and bytes, and the call's peak. Both are
  read from emptied free lists, so the peak counts every block the call
  touches, with no help from free lists. It is not the number of
  allocations the call made. Before Python 3.11 it is not counted
  (`not_counted`, naming the version): CPython then keeps a frame object
  with each function after its first call, and the tool's own frames
  would be most of it.

These are the full call's counts, as a Python caller pays them, with
nothing subtracted. Step six's structure report has the core's own counts
per hot path. For a caller target the Python figure includes the caller's
own objects; for the binding's share alone, add a direct target on a case
the caller reaches.

### 5. Both platforms

Run steps 3 and 4 on each platform you ship for, from the same commit,
then:

```sh
carcinify speed --compare macos/report.json linux/report.json
```

It prints each target's figures, every one beside its platform, and
flags:

- `platform_dependent`: the two median ratios are more than 1.25 apart.
  1.25 is a first value, and arbitrary. Both per-pair ranges, and each
  build's target CPU, feature count, RUSTFLAGS and rustc options, are
  printed beside it.
- `builds_differ`: the two builds' rustc flags differ. These are the
  flags in the build's environment (`RUSTFLAGS`, and Cargo's
  `CARGO_ENCODED_RUSTFLAGS` and `CARGO_BUILD_RUSTFLAGS`), and these options
  of the command rustc ran with:
  - each `-C`, however it was set, by a Cargo configuration file or a
    `CARGO_PROFILE_*` variable as well; but not those that name a hash or
    a path, only link, or say where the debug info goes, since they differ
    by platform and change no code;
  - each feature (`--cfg feature=…`).

  No other `--cfg` is compared, from a configuration file either: a
  crate's build script gives cfgs that differ by interpreter, such as
  pyo3's `Py_3_12`, and they would flag every pair. An option Cargo quoted
  because it holds a space, as `-C 'llvm-args=-a -b'`, is compared only up
  to its first space. Each target apart is then
  `builds_and_platforms_differ`: two builds on two platforms, and
  `--compare` cannot say which one made the difference.
- `interpreters_differ`: the two Pythons differ in version,
  implementation or free-threading, or their NumPy versions differ.
- `verdict`: a report that did not pass.

It refuses two reports of different code, with exit 2: another commit,
other uncommitted changes, another declaration or suite, other measuring
code, or a carcinify built from another commit or with other changes.
Compare two runs of one commit.

### 6. If you ship a Linux aarch64 wheel

Say these three things with the figures, in the port's notes.

1. A speed figure belongs to a build, not only to a platform. On the
   einsum port's 120-operand search, changing `-C target-cpu` without
   adding any instruction-set feature changed the time 2.5 times: the CPU
   tuning decided whether LLVM vectorized one 4-word loop, and on Linux in
   Docker on an Apple-silicon Mac the vectorized loop was the slow one.
   With 8-word bitsets the apple-m1 tuning vectorizes it too, and was slow
   too: the generic build with `-C no-vectorize-loops` was as fast as
   apple-m1 at 4 words and 2.7 times as fast at 8. On macOS the vectorized
   loop was not slower.
2. A Linux aarch64 figure taken on an Apple-silicon Mac, in Docker or on
   the metal, says little about other aarch64 machines. Measure on the
   hardware the users run.
3. The report records each build's target CPU, its target-feature count,
   its RUSTFLAGS and the options rustc ran with. A target flagged `platform_dependent` is worth
   rebuilding with the other target CPU before the platform is blamed:
   `ci/linux/speed.sh` passes `RUSTFLAGS` into the container. It is a
   diagnostic: on the einsum port it helped at 4 words and not from 8.

Which tuning suits the users' CPUs is unmeasured: no run was made on
Neoverse, Graviton or Cortex-A, and this project advises only from
measurements.

## What the report cannot show

- That the port is right. diff says that; speed compares one timed call's
  output.
- Speed on a machine it did not run on (section 6).
- Calls from several threads at once, free-threaded Python, or a caller
  that runs its own threads.
- The binding's time apart from the core's; the caller's own Python
  outside the call, which is timed but not compared; memory the allocator
  keeps after a free; the Rust side's peak memory (RSS).
- Contention inside a virtual machine that the host's load average does
  not see, beyond what the loops around each run catch.
