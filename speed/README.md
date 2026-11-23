# carcinify speed

Times a Rust port against its Python original from the Python caller, on
cases the comparison harness compares, and counts what each call costs in
allocations. The two sides are imported into one process and timed in
alternation, on a machine the host gate has found quiet; two reports, one
per platform, can then be set side by side. Step seven
(`skill/steps/07-speed/STEP.md`) says when to use it and how to read what
it reports.

## Use

```sh
cargo build --release -p carcinify
target/release/carcinify speed build speed.json --out target/carcinify-speed
LOG=target/gate.log ci/gate.sh target/release/carcinify speed run speed.json \
  --wheels target/carcinify-speed --report target/carcinify-speed/report.json \
  --load-log target/gate.log
target/release/carcinify speed --compare macos/report.json linux/report.json
```

- **`speed build`**, not under the gate, builds the timed wheel as
  `carcinify diff` builds it, and a counted wheel; makes each target's
  untimed calls; takes the counts; and writes the wheels, their SHA-256 and
  `build.json` to `--out`. It refuses to run while `GATE_RUN_ID` is set,
  so that no build happens inside a gated window.
- **`speed run`**, under `ci/gate.sh`, checks the wheels and the record
  against what it is about to time, then only times. It reads the gate's
  log (`--load-log`) to judge the run's load.
- **`--compare`** prints two reports of one commit side by side and flags
  the differences.

Exit status:
- `speed build`: `0` built; `1` targets refused; `2` it could not run.
- `speed run`: `0` pass, with or without flags; `1` a target refused or
  failed, or the run unreliable; `2` refused, or it could not run.
- `--compare`: `0` once printed, flags or not; `2` refused.

`--allow-dirty` builds or times a tree with uncommitted changes, and the
record and the report say so. `carcinify speed` with no step prints every
option. Needs `git`, `cargo` and a Python interpreter (`--python`), and
maturin for a PyO3 port; nothing is installed.

## The declaration

```json
{
  "schema": "carcinify/speed/declaration/1",
  "suite": "../suite.json",
  "runs": 15,
  "targets": [
    {"name": "search-40", "case": {"file": "cases/workload.jsonl", "id": "workload-40ops"}, "calls_per_run": 50},
    {"name": "einsum-path-40", "calls_per_run": 20,
     "caller": {"file": "speed/callers.py", "function": "einsum_path_40",
                "fn": "greedy_path",
                "replace": "numpy._core.einsumfunc._greedy_path"}}
  ]
}
```

- `suite`: the harness's suite, relative to this file. It names both sides,
  how the PyO3 side is built, and the cases; `harness/README.md` has its
  format. speed asks the harness for the same build, so there is one
  definition of the port.
- `runs`: pairs of runs per target, at least 1.
- Each target has a `name` used once, a `calls_per_run` of at least 1, and
  one of:
  - `case`: a file the suite lists among its `cases`, and a case id in it.
    A case that sets `expect_raise` is refused.
  - `caller`: a Python `file` (relative to the suite's directory) and the
    `function` in it that calls the real caller; `fn`, the compared
    function the caller reaches, named as the suite's cases name it; and
    `replace`, the `module.attribute` where each side's function is put.
    Each input the caller gives `fn` has to be a case of the suite.

## What is refused, and what to do

- **`--out` inside the repository**, where git does not ignore it, is
  refused before anything is built: the wheels would leave the tree unlike
  its commit. Write them in a directory git ignores, such as `target/`, or
  outside the repository.
- **Run diff on the commit first.** speed keeps both sides' directories on
  `sys.path` and compares one timed call's output with the original's;
  it does not stop a port importing a module only the original provides.
  diff does.
- **Two modules of one name.** Both sides are imported into one process,
  so a top-level module name that each side would find in its own file
  gives both sides one file: a side's own `colorsys.py` beside the
  standard library's, for instance, or two `helper` modules each side
  reaches through a path entry it adds itself. speed build refuses such a
  name, naming both files: "the two sides would import a top-level module
  of one name from different files: `name` (`file A` and `file B`)". It
  runs each side alone, in a process of its own on its own path, making
  the same untimed calls, and refuses a name when:
  - both sides hold it alone, from different files;
  - one side holds it alone from one file, and the process with both
    sides holds it, or would find it on its path, from another;
  - the process with both sides holds it from a file neither side's path
    gives.

  Rename one module, or make each side a package of its own. A side's own
  process that hangs or dies stops speed build (exit 2), naming the side
  and the target it was calling.
- **An installed copy of a side's module**, as `maturin develop` leaves in
  the interpreter's `site-packages`, is refused only when a side really
  imports that name and would get another file than the one it gets
  alone. The message names both files; the installed one is the one to
  remove from that interpreter, or run speed with an interpreter that has
  none.
- **A failed build leaves the previous build's files in `--out`.**
  `speed run` times only what `build.json` records, and refuses a record
  that no longer fits: wheels whose SHA-256 differ, another `carcinify`
  binary, another declaration, suite, commit or interpreter, or other
  uncommitted changes. Run `speed build` again.

## What the figures include

- **A caller target's Python figure includes the caller's own objects.**
  The Rust counts are the extension's alone, over the caller's whole call.
  For the binding's share alone, add a direct target on a case the caller
  reaches.
- **Keyword names that cannot be written as names.** Counts call each case
  as written-out source, so that the call costs the interpreter nothing of
  its own. A case whose keyword names cannot be written that way (`a-b`, a
  name Python would read as another, such as the ligature `ﬁ` read as
  `fi`, or `__debug__`) is called through `functools.partial` instead, and
  the counts carry a note: the figures may include blocks `partial` takes
  for the call.
- **The core's own counts.** The report gives the full call's counts, as a
  Python caller pays them, with nothing subtracted. The core's counts per
  hot path are in `carcinify structure`'s report (step six).
- **`tools.carcinify_dirty`** is git's hash of `git diff HEAD` in the tree
  carcinify was built from, or `clean`. Untracked files are not in it: a
  new source file counts only through the tracked change that brings it
  in, such as a `mod` line, so two builds that differ only in an untracked
  file read the same.

## On Linux arm64: `ci/linux/speed.sh`

```sh
LOG=<host gate log> ci/linux/speed.sh <repo> <speed.json> <out>
```

It builds carcinify from this repository's commit and clones `<repo>` at
its commit, both inside the CI's container, runs `speed build` there, and
then `speed run` under the host's `ci/gate.sh`: in Docker on a Mac, the
container's load average is the VM's, not the Mac's. `ci/README.md` has the
rest.

- `<repo>` must be a full clone. A linked worktree's `.git` names a path
  that is not mounted in the container.
- `<out>` and `<repo>` must not hold one another; the script refuses them
  before a container starts.
- The container ends after `CAP_SECONDS` (default 7200). The gate's wait
  counts, so give a longer cap for a long wait.
- `RUSTFLAGS`, when set, passes to `speed build` in the container, so that
  the port can be built for another target CPU.
- `PIP_PACKAGES`, when set, is installed beside maturin in the container's
  venv, for a caller that imports them, as `PIP_PACKAGES='numpy==2.5.4'`.
  Pin exact versions: the report records NumPy's version, and no other
  package's.
- Exit status: speed's own; `2` also when one of the script's own steps
  failed, or the cap ended the container under speed; `130` or `143` when
  interrupted (SIGINT, SIGTERM), and if the gate had opened, its log ends
  with a line saying so.
- Stop it by its process group: Ctrl-C, or `kill -TERM -<pgid>`. A signal
  to its PID alone waits for the current step to end: a `speed run` goes
  on to its end and writes its report before the script exits 143.

## Limits

- Python before 3.12 is not tested. The timing driver is written for 3.9
  and later.
- Before Python 3.11, the Python figure of the counts is not counted
  (`not_counted`, naming the version): CPython then keeps a frame object
  with each function after its first call, and the tool's own frames would
  be most of the figure. The Rust counts stand.
- Only Python originals, and ports that are Python or reached through
  PyO3, have been tried.
- What a report cannot show is listed at the end of step seven.

## Tests

```sh
CARCINIFY_PYTHON=$PWD/.venv/bin/python \
  cargo test --locked -p carcinify-speed -p carcinify --lib --test speed
CARCINIFY_PYTHON=$PWD/.venv/bin/python CARCINIFY_MATURIN=$PWD/.venv/bin/maturin \
  cargo test --locked -p carcinify --test speed -- --ignored
```

The files in `tests/` run the `carcinify` binary, so they are built as test
targets of the `carcinify` crate (see `carcinify/Cargo.toml`); the unit
tests are in this crate. The second command runs the tests that build the
PyO3 port, which need maturin 1.16.

`tests/cli.rs` runs the real binary on the fixture in `tests/fixture`, a
Python original and a port ten times slower on purpose, in Python and in
Rust; `tests/fixture/README.md` lists what each function is for. Unit tests
in `src/gate.rs` read gate logs, among them a copy of a real one; those in
`src/figures.rs` and `src/run.rs` cover the pairing, the dropped pairs
and the verdict, and those in `src/platform.rs` the build's target CPU and
the other platform facts.
