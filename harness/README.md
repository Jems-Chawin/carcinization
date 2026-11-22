# carcinify diff

Runs a Python original and its Rust port on the same input cases and reports
whether the outputs are identical, bit for bit.

It exists because of one bug. A project ported a store to Rust and
`serde_json` read `396.03999999999996` back as `396.04`. A comparison "within
an epsilon" passes that at 1e-12. Only an exact comparison on real data
catches it. The worked example in `examples/float_roundtrip` reproduces it.

## Use

```sh
cargo build --release
target/release/carcinify diff path/to/suite.json --report target/carcinify-diff/report.json \
    --python .venv/bin/python --maturin .venv/bin/maturin
```

Exit status: `0` every case identical; `1` any difference, any case that
failed to run, a function never compared on a returned value, or no cases at
all; `2` the run could not be carried out (bad suite, a tree that does not
match the commit, the same code on both sides, failed build). A report is
written in all three.

`--strict` also exits `1` when any case was identical only under a
relaxation. Use it wherever the exit status is all that gets looked at. The
report's `outcome` (`pass` or `fail`) and `exit_status` say how the run
ended with `--strict` taken into account; `verdict` is the finding before
it.

Each case on each side has a time limit, `--timeout-secs`, 120 by default.
While a function under test runs, the harness counts the Python calls it
makes (see "Where the work was done"), and that slows code that is mostly
Python by up to about nine times: one measurement was 0.8 s without counting
and 7.5 s with it for two million small calls. Compiled code is not slowed.
If an original runs out of time only because it is being counted, pass
`--no-call-counts`; the report then says the calls were not counted.

Python orders a set of strings by a hash that changes with every process
unless `PYTHONHASHSEED` fixes it. Both sides run under one seed: the one
given with `--hash-seed` (0 to 4294967295), or one the harness draws. The
report records it as `hash_seed`, with `chosen` set to `given` or `drawn`,
and the screen prints it, so `--hash-seed` with that number repeats the run.
A `PYTHONHASHSEED` in the harness's own environment is not used. To see
whether an original's answers depend on set order, run the suite under
several seeds.

Needs: `git`, a Python 3.9+ interpreter, and for a PyO3 side `cargo` and
maturin 1.16. Nothing is installed by the harness. To get maturin without
touching the system: `uv venv && uv pip install maturin==1.16.0`.

### Suite

```json
{
  "schema": "carcinify/diff/suite/1",
  "original":  {"kind": "python", "path": ["python"], "module": "ledger"},
  "candidate": {"kind": "pyo3", "crate": "rust/py", "module": "ledger_rs",
                "features": ["float-roundtrip"]},
  "cases": ["cases/real-data.jsonl", "cases/roundtrip.jsonl"]
}
```

Paths are relative to the suite file. A `python` side is a module imported
from the listed directories. A `pyo3` side is a crate directory holding
`Cargo.toml` and `pyproject.toml`; `features` and `no_default_features` are
passed to the build. Either side may be either kind.

Two optional fields are for partial ports, where the candidate is not all
Rust and not all new:

```json
{
  "shared": ["common"],
  "candidate": {"kind": "pyo3", "crate": "rust/py", "module": "ledger_rs",
                "python_functions": ["summarise"]}
}
```

- `shared`: what both sides are meant to use, such as helpers the port has
  not touched. An entry is a directory of its own, or a file or
  sub-directory inside one side's directory.
- `python_functions`, on a `pyo3` side: functions of the wheel that are
  allowed to be Python rather than compiled, for a wheel with a Python layer
  over its compiled core. The list must be exact: a name that does not
  exist, or that is compiled, stops the run.

Both are explained under "What a pass requires".

### Measured inputs

A speed figure is for the inputs it was measured on. If those inputs are not
among the cases, the comparison says nothing about them. The suite lists the
files of calls that a measurement recorded, and every recorded call must
also be a case:

```json
{
  "measured": [
    {"file": "decide/workload.calls.jsonl", "fn": "greedy_path"},
    {"file": "speed/timing.calls.jsonl",
     "not_comparable": "the arrays are compared by the port's own test"}
  ]
}
```

- A recorded file is a case file: each line is a case line, written in the
  typed value format, so the fix for a missing input is to copy its line
  into the cases. `carcinify decide --record-calls` and `carcinify
  structure --record-calls` write such files. Name them `*.calls.jsonl`.
- A line may instead say that the call held a value the format does not
  cover: `{"id": "call-3", "fn": "f", "unencodable": "numpy.ndarray"}`.
  Such a line fails the run unless the entry gives `not_comparable`, the
  author's reason, which the report and the screen print as the author's
  word.
- `fn` names the function every line of the file calls, in place of the
  name each line gives. A measurement of the original may call a private
  function that the cases reach under a public name. A parameter the
  recording leaves out (a sentinel default) is read as the default of the
  function `fn` names; decide writes every other default as the call
  received it. What remains: a sentinel default that differs between the
  two functions is not seen.
- A recorded call matches a case when the original's signature binds both
  to the same arguments, compared as outputs are: sets in no order, ints by
  every digit, a list never equal to a tuple, and dict key order counted.
  A parameter whose value is its default, written or not, is left out, so
  a default the format cannot hold (a sentinel `object()`) does not stop
  the match. So a call by keyword matches a case by position, `f(1)`
  matches `f(1, None)` when `None` is the default, and a typed line
  matches a plain-JSON case that holds the same Python value. A dict
  argument with its keys in another order is another input, and is
  reported missing.
- The check runs on the original alone, before any case. A call missing
  from the cases is listed by `file:line` and function; the verdict is
  `fail`, with the reason `measured_input_not_compared`. A listed file that
  is not there stops the run.
- Any `*.calls.jsonl` file in the repository that the suite does not list
  gets a warning on the screen and in the report: a recording that may
  have been made and then forgotten. It does not fail the run.

### How to lay a port out

```
repo/
  original/     the Python being ported          "path": ["original"]
  candidate/    the port (a crate, or Python)    "crate": "candidate"
  common/       code both use, if any            "shared": ["common"]
  suite.json
```

The original and the candidate each get a directory of their own, side by
side. Neither may be inside the other, and neither may list the other's
directory in its `path`: either would let one side import the other's code,
and the run stops. Code both sides need is declared as `shared`, not put in
either side's `path`.

#### Porting one module of a package

The usual partial port leaves the original where it is and reuses parts of
it. Name those parts, and only those, as shared; nothing in the original
has to move:

```
original/pkg/__init__.py
original/pkg/store.py      being ported
original/pkg/util.py       helper the port keeps using
original/pkg/table.json    data both read
candidate/pkg_new/store.py the port: `from pkg.util import clean`
```

```json
{
  "original":  {"kind": "python", "path": ["original"],  "module": "pkg.store"},
  "candidate": {"kind": "python", "path": ["candidate"], "module": "pkg_new.store"},
  "shared": ["original/pkg/__init__.py", "original/pkg/util.py", "original/pkg/table.json"]
}
```

- When a shared entry is inside one side's directory, that directory is put
  last on the other side's import path, so `import pkg.util` works there.
  Everything in it that is not shared stays off limits: the candidate
  importing, opening or copying `pkg/store.py` stops the run as before.
- Importing `pkg.util` runs `pkg/__init__.py`, so that file has to be shared
  too. If `__init__.py` itself imports the module being ported
  (`from .store import load`), the port cannot import the package without
  loading the original implementation, and the run stops. The port then has
  to load the helper by file path
  (`importlib.util.spec_from_file_location`), which touches only the shared
  file.
- A file that is kept identical on both sides (a constants module each side
  has its own copy of) is declared once, on either side:
  `"shared": ["original/consts.py"]`. Shared is by content as well as by
  path, so the identical copy on the other side, or the copy maturin puts
  in a wheel, is covered. A copy is shared for every purpose: the report
  lists it under the entry as loaded by both sides, calls into it are
  counted as `shared`, and no function under test may be defined in it.
- No function under test may be defined in a shared file. Declaring the
  module being ported as shared therefore stops the run.
- The output lists each shared entry with the files both sides loaded, and
  separately those only one side loaded. An entry nobody but its owner
  loads is an exemption that is not being used; consider removing it.

### Cases

One JSON object per line. A case names a function, present under the same
name in both modules, and its inputs. Only a module's own public functions
can be named: the name must be in the module's namespace, must not start
with an underscore, and must be a function (or, for an extension module, a
built-in function) defined by that side's own files. Classes, modules and
`functools.lru_cache` objects are refused; a function behind a
`functools.wraps` decorator is accepted.

```json
{"id": "small", "fn": "total", "args": [[1.5, 2.25]], "kwargs": {"round": false}}
```

`args` and `kwargs` here are ordinary JSON, and the harness forwards them to
both sides as the exact text in the file. It does not read the numbers.

With `"encoding": "typed"` the inputs are written in the value format below,
which reaches what JSON cannot (bytes, NaN, a float by its bits, tuples) and
adds three placeholders the harness fills in:

| Placeholder | The function receives |
|---|---|
| `{"t": "file_path", "path": "../data/ledger.jsonl"}` | the path of a private copy of that file |
| `{"t": "file_bytes", "path": "../data/ledger.jsonl"}` | the file's contents as bytes |
| `{"t": "scratch_dir"}` | the path of a new empty directory |

File paths are relative to the case file. This is how real data gets in: the
case points at the file, the harness hashes it into the report, and each side
gets its own copy at the same path, so one side cannot change what the other
reads and paths in outputs still match.

Generated cases are a case file written by a script. The harness does not run
generators; commit the script and its output, so that what was reviewed is
what gets hashed. `examples/float_roundtrip/gen_cases.py` is one.

Two optional fields:

- `"expect_raise": true`. Both sides must raise, with the same exception type
  and message. Without it, a case where both sides raise counts as failed to
  run, since otherwise a broken case (a typo in an argument) would pass.
  Arguments that do not fit the function's signature are a `bad_call` error
  either way: both sides would raise the same `TypeError` before running a
  line of their own.
- `"relax"`. See below.

A case has no field for an expected output, a count or a verdict, and unknown
fields are rejected, so a misspelt option stops the run instead of being
ignored.

### Value format

Used for typed inputs and for every output:

| Value | Written as |
|---|---|
| None | `{"t": "none"}` |
| bool | `{"t": "bool", "v": true}` |
| int | `{"t": "int", "v": "18446744073709551617"}` (decimal string) |
| float | `{"t": "float", "bits": "4078c0a3d70a3d70"}` (binary64, big-endian hex) |
| str | `{"t": "str", "v": "text"}`, or `{"t": "str", "cp": [55296]}` when it holds a lone surrogate |
| bytes | `{"t": "bytes", "hex": "00ff"}` |
| list, tuple, set | `{"t": "list", "v": [...]}`, likewise `tuple`, `set` |
| dict | `{"t": "map", "v": [[key, value], ...]}` in iteration order |

### What identical means

| | Compared |
|---|---|
| float | the 64 bits. `-0.0` is not `0.0`; a NaN equals only a NaN with the same sign and payload |
| int | every digit, at any size; never equal to a bool or a float |
| str | code points. No Unicode normalisation: NFC `é` is not NFD `e` + U+0301 |
| bytes | every byte |
| list, tuple | element by element; a list is not a tuple |
| dict | keys, values and key order |
| set | elements, in no order |
| exception | type, `str(e)` and `e.args` |

Exception types are compared by full name, `module.ClassName`, with one
exception: a class each side defines in its own code is compared by class
name alone, since `orig.StoreError` and `port.StoreError` can never have the
same module. The base classes are then compared too: the whole method
resolution order, own classes by name and all others by full name, since
that is what decides which `except` clauses catch the error.
`class StoreError(ValueError)` against `class StoreError(Exception)` is a
difference (`exception_bases`). When they agree the case carries a note
saying the module part was ignored and giving the line of classes compared,
for example `StoreError < builtins.ValueError < builtins.Exception <
builtins.BaseException`. Built-in and third-party exceptions always keep the
full name. For a PyO3 exception, `create_exception!(module, Name, Base)`
must name the module the extension is imported as, or the class is not
recognised as the port's own and the full names are compared; `Base` is
compared as above. Attributes other than `args` are not compared.

So for an error case to match, the port has to reproduce the original's
chain of own exception classes by name, down to the first built-in base:
`class Corrupt(StoreError)` over `class StoreError(ValueError)` needs a
`Corrupt` whose base is a `StoreError` whose base is `ValueError`.

When the types differ, the difference gives each side's line of base
classes. A Rust panic reaches Python as `pyo3_runtime.PanicException <
builtins.BaseException`, which `except Exception` does not catch;
`examples/float_roundtrip/suite-panic.json` shows one.

Anything else (a `Decimal`, a NumPy scalar, an object) fails the case with
the type named. Subclasses are not narrowed to their base type.

### Relaxations

The only way to loosen a comparison is for a case to say so, by name:

```json
{"id": "c", "fn": "f", "relax": {
  "floats": [{"path": "$[*][\"amount\"]", "ulps": 1, "reason": "x87 rounding in the original"}],
  "key_order": [{"path": "$[*]", "reason": "the original iterates a set"}],
  "exception_message": true}}
```

- `floats`: a tolerance for the floats at one path. `$` is the returned
  value, then `[3]`, `["key"]` or `[*]`. Either `ulps` (a count of
  representable floats) or `abs` (a decimal string). A `reason` is required.
  A tolerance never covers NaN or infinity. Floats elsewhere stay exact.
- `key_order`: dicts whose key order is not compared, each named by its
  path in the same notation, with a `reason`. `$` is the returned value
  itself if that is a dict; `$[*]` is every dict directly inside it. A dict
  the list does not name is still compared in order, so the relaxation does
  not spread to everything nested in the output.
- `exception_message`: only the exception type is compared, not the message
  and not `e.args`. If an exception's `args` hold something the value format
  does not cover, this is the only way that case can match.

The report lists what each case declared and every place a relaxation was
actually needed, with both values. Such a case is counted as
`identical_with_relaxations`, not `identical`, and one such case is enough
to change the verdict from `pass` to `pass_with_relaxations` and the last
line of output from `PASS` to `PASS WITH RELAXATIONS: n of m case(s) ...`.
The exit status stays `0` unless `--strict` is given.

### What a pass requires

Matching outputs are not enough. The run passes only if all of these hold:

1. There is at least one case, every case ran on both sides, and every
   comparison came out identical (or identical under the relaxations the case
   declared).
2. Every function the cases name was compared at least once on a value both
   sides returned. A function seen only raising fails the run: agreeing on
   what to reject does not show that the port does the work.
3. The two sides are different code. The run stops with exit 2 when:
   - both sides name the same directory, or one side's directory is inside
     the other's;
   - a file loaded by one side lies in the other side's directories;
   - a file loaded by one side has the same content as any file in the
     other side's directories (a bundled copy of the original, whatever it
     is called and whether a Python or a compiled function calls it);
   - a function under test is defined on both sides by the same file or the
     same content;
   - one side's Python code opens a file in the other side's directories.
     The driver installs a Python audit hook, which sees an import, an
     `exec` of the source, `runpy` and a plain `open` or `os.open` alike,
     including an import that is afterwards removed from `sys.modules`.

   The directory and content rules are checked before the first case and
   again after the last; the audit hook is checked after every case. A suite
   naming one module twice, a candidate that does `from original import *`,
   a port that ships the original and calls it, and a half-finished port
   that falls back to the original all stop here.
4. Shared code is declared. Shared files may be loaded and opened by both
   sides and are exempt from the rules in 3, by path and by content. In
   return: a shared entry may not be a side's whole directory or contain
   one; a shared directory of its own may not hold a file with the same
   content as a side's own file; and no function under test may be defined
   in shared code (exit 2). The report lists every shared file each side
   loaded, and the output names the ones both ran. That code was run by both
   sides and compared by nobody.
5. On a `pyo3` side, every function called is a compiled function from the
   extension file in the wheel just built, unless the suite lists it under
   `python_functions`. An undeclared Python function stops the run with
   exit 2. A listed name that does not exist or is compiled also stops the
   run, so the list cannot go stale.

#### Where the work was done

`extension` and `python` say what the function under test is, not who did
the work. A compiled function can hand everything to Python it ships
alongside, and both sides can forward to the same shared helper. So while
each function under test runs, the driver counts the Python function calls
made, by where the called code lives, and the output shows the totals over
all its cases under each row:

```
function                     cases identical relaxed different failed  on a returned value
scale                            3         3       0         0      0  pass (3 returned, 0 raised)
    original:  python, 3 Python calls (3 own)
    candidate: extension, 0 Python calls
label                            2         2       0         0      0  pass (2 returned, 0 raised)
    original:  python, 2 Python calls (2 own)
    candidate: python, 2 Python calls (2 in wheel)
tidy                             1         1       0         0      0  pass (1 returned, 0 raised)
    original:  python, 2 Python calls (1 own, 1 shared)
    candidate: python, 2 Python calls (1 own, 1 shared)
```

- `extension, 0 Python calls`: compiled, and it called no Python.
- `extension, 4 Python calls (4 in wheel)`: compiled, and it called Python
  code shipped in the wheel. A half-finished port looks like this.
- `1 own, 1 shared` on both sides, as for `tidy`: each side is one function
  of its own that calls shared code. If the shared count is where the
  numbers are, the port was not what got compared.
- `in other packages` is the standard library and installed packages.

A Python function under test counts as one call itself. An import that
happens during a call counts too, module bodies and import machinery
included, so a first call can show a hundred calls "in other packages"
where later ones show none. The counts are a number to read, not a rule:
they do not change the verdict. `--no-call-counts` switches them off, and
the rows then say `Python calls not counted`.

Processes that a side's Python code starts are recorded in the report
(`checks.processes_started`) and mentioned in the output. They do not fail
the run: a port may have good reason to start one, and the harness does not
know what it ran.

#### Why some bad names end the run and others only the case

A case that names something the harness will not call is a case error,
`refused_function`, exit 1, and the other cases still run: a name starting
with an underscore, a class, a module, or a function the module imported
from the standard library or a third-party package. The suite is fine and
one case is wrong.

Exit 2 is for a suite that is set up to compare the wrong thing, where no
case result would mean anything: the same code on both sides, a function
under test that lives in a shared directory, or an undeclared Python
function on a `pyo3` side.

### Report

Written by the harness and by nothing else; the previous report at the same
path is deleted before the run starts. Top level:

| Field | Content |
|---|---|
| `verdict`, `reasons`, `strict` | `pass`, `pass_with_relaxations`, `fail` or `error`, and why; whether `--strict` was given |
| `hash_seed` | `value`, the `PYTHONHASHSEED` both sides ran under, and `chosen`: `given` with `--hash-seed` or `drawn` by the harness |
| `outcome`, `exit_status` | how the run ended once `--strict` is applied: `pass` and 0, `fail` and 1, or `error` and 2 |
| `summary` | counts: `cases`, `identical`, `identical_with_relaxations`, `different`, `errors` |
| `checks` | `sides_are_different_code` with the rules applied, and `functions`: for each function called, how many cases returned on both sides, raised on both, differed or failed, whether it was `compared_on_a_returned_value`, whether it is `python` or `extension` on each side, the Python calls made while it ran on each side (`own`, `shared`, `other`), and the file (with SHA-256) that defines it on each side; `public_functions_not_called`: each module's own public functions that no case named; `shared`: the declared entries, whether each is inside a side's directory, and the files each side loaded from them; `processes_started`: per side, a count and the first twenty |
| `git` | commit, `dirty`, the `git status` lines, and `used_but_not_tracked`: files the run used that git does not track |
| `suite`, `inputs` | SHA-256 of the suite, every case file, every data file and every measured file |
| `measured` | per listed file: its SHA-256, `fn`, how many calls it holds, how many are among the cases, the `missing` ones by `file:line` and function (with `why` when the original could not bind it), the `unencodable` ones and the type each holds, and `not_comparable` as the suite gives it |
| `calls_files_not_measured` | `*.calls.jsonl` files in the repository that the suite does not list |
| `sides` | for each side: interpreter, the project files of the code that ran with their SHA-256, the installed-package files it loaded (`third_party_files`) with theirs, one digest over all of them; for a PyO3 side also the maturin command, the wheel's SHA-256 and a digest of the Cargo workspace's tracked files |
| `tools` | rustc, cargo, maturin, OS and architecture, SHA-256 of the harness binary and of the Python driver; for a PyO3 build also Cargo and rustup configuration found outside the repository, and the environment variables that change what Cargo builds |
| `cases` | every case: `identical`, `identical_with_relaxations`, `different` or `error` |

A `different` case has `first_difference`: the path, both values (floats with
their bits), and `record`, the simple fields of the dict around the
difference. An `error` case says which side failed and how: `crashed`,
`timeout`, `raised`, `did_not_raise`, `unencodable`, `no_such_function`,
`refused_function`, `bad_call`, `input`, `protocol`. Each case also records
whether each side `returned` or `raised`, and `notes` for anything the
comparison did by rule (an exception compared by class name).

The same per-function table is printed before the verdict, with the
functions no case called:

```
function                     cases identical relaxed different failed  on a returned value
load                             1         1       0         0      0  pass (1 returned, 0 raised)
    original:  python, 23 Python calls (1 own, 22 in other packages)
    candidate: extension, 0 Python calls
roundtrip                       67        67       0         0      0  pass (56 returned, 11 raised)
    original:  python, 7776 Python calls (1225 own, 6551 in other packages)
    candidate: extension, 0 Python calls
not called by any case (original): first_amount, save
not called by any case (candidate): first_amount, save
```

The two lines under each row say what the function is on each side and how
many Python calls were made while it ran (see "Where the work was done").
A line for each shared entry and for any processes started follows when
there are any.

### Kept mutants

How strong are the cases? A mutant is a deliberate bug in the port, kept
as a patch in the repository. `--mutants` runs the cases against each one:

```sh
carcinify diff suite.json --mutants mutants --report out/mutants.json
```

- Each `*.patch` in the directory, in name order, is applied on its own to a
  scratch clone of the commit (`git clone --shared`), never to the working
  tree. The tree must be clean, and `--allow-dirty` is refused: the patches
  are against the commit.
- Before anything runs, every patch is applied in the clone and checked.
  It must apply, and every path that then differs from the commit (`git
  status` with renames as a deletion and an addition, untracked and
  ignored files included) must pass the rule: inside the candidate's code,
  which is a PyO3 side's Cargo workspace (the crate and the crates beside
  it) or a Python side's `path` directories, and even there nothing the
  comparison reads: the suite, the case, data and measured files, the
  original's code and the shared directories. Any failure stops the run,
  as a bad case file does. The patch directory is read from where it is
  given, relative to the current directory.
- The unmutated suite runs first in the clone and must pass.
- Then, for each patch, the candidate is built and every case is run, all
  under the same hash seed. A patch is **killed** when any case is not
  identical. The report says how: `different`, `panicked` (a Rust panic,
  which reaches Python as `pyo3_runtime.PanicException`), `crashed`,
  `timed out` or `failed`. The first case that times out ends that patch's
  run: the hang has caught it, and the cases after it would each wait out
  `--timeout-secs`. A patch whose candidate does not build or import is
  `did_not_run`. A patch no case catches **survived**.
- After each patch's run the clone is checked again by the same rule. A
  build script, a procedural macro or the candidate at run time can write
  where the patch itself may not: into the original, or a compiled module
  beside it, which Python imports before the `.py`. Then the comparison
  did not read what the commit holds, the patch is **invalid**, and the
  run fails. What it wrote is removed before the next patch; the
  candidate's ignored target directory passes the rule and is kept.
- Only the candidate is mutated. A case that fails on the original's side
  (a time-out on a busy machine, say) makes the patch **invalid**, never
  killed, and the run fails. For the same reason the original must give, on
  every case, the output it gave in the unmutated run: by SHA-256, or,
  where the case declares relaxations, as the two sides are compared
  under them. If it does not, something changed what it reads (a file
  outside the repository that the candidate wrote, say), or the original
  does not give the same output twice, and the patch is **invalid**.
- Text before a patch's first `diff --git` line is its description, which
  `git apply` ignores. A line there reading `Expected to survive: <reason>`
  marks a mutant that cannot change an answer, such as one that only adds
  work. Its survival is expected; if it is killed, the description is
  wrong, and the run says so.
- One target directory, inside the clone, serves every patch, so only the
  port's own crates are rebuilt between patches. The clone and its target
  are removed when the run ends; with `--keep-work` they are kept and the
  report names the clone (`scratch`). A process that is killed cannot
  clean up after itself; the next run removes clones left by processes
  that no longer exist.

Exit status: `0` when every patch was killed, except those expected to
survive, which survived; `1` for a survivor, an expected survivor that was
killed, a patch that did not run, or an invalid one; `2` when the run could
not be carried out. The report (`carcinify/diff/mutants/1`) gives the hash seed, the
commit, the unmutated counts, and per patch its file and SHA-256, the
result, how it was killed, the cases run, the cases not identical in all
and per case file, the first few of them, and whether the run stopped at a
time-out. `--hash-seed` with the recorded seed repeats the run.

#### Writing mutants for arithmetic on Python's `int`

Python's `int` has no bound; a port stands for it with a fixed-width type,
an arbitrary-precision one, or both. Cases can hold large numbers and still
decide nothing on exact integers. For each type that stands for `int`,
keep these patches; each must be killed:

1. **53 significant bits.** After each multiplication, keep the top 53
   bits and drop the rest, which is what a `float64` keeps. If it survives,
   no case's answer depends on two numbers above 2^53 that differ by a few
   units. Add such cases, captured from the real caller where it can pass
   them.
2. **Off by one in addition.** Add 1 at each addition. A sum then grows by
   the number of additions in it, so two sums with the same number of
   terms keep their order. The mutant survives unless some comparison
   turns on that: two sums with different numbers of terms that tie or are
   a unit apart, or a sum exactly on a bound the code compares it with.
   Add such a case.
3. **Comparison flips.** Turn `>` into `>=` at each bound (and so on). A
   survivor means no case lands exactly on that bound.
4. **Swapped signs.** Wherever a value can be negative, swap its sign.
   Where the port does not hold a value but marks it, for instance "larger
   than any limit", swap the mark.
5. **Overflow.** Make the fixed-width arithmetic wrap instead of moving to
   the arbitrary-precision type. If it survives, no case leaves the fixed
   width.
6. **Zero.** Remove a check for a zero size or product.

Rounding depends on the order of multiplication. If the port's order
follows something the hash seed changes, such as labels numbered in set
order, a rounding mutant may be killed under one seed and survive under
another, and a count that moves between runs is no check. Choose values so
that the order cannot matter: multiplying by a power of two commutes with
the rounding, so scale by powers of two. Or check every order when the
cases are generated.

## Design choices

**Rust for the harness.** The comparison must not depend on the interpreter
under test, and a port project already has the toolchain. Three
dependencies: `serde`, `serde_json`, `sha2`; reasons are in `Cargo.toml`.

**Each reply is tied to its request.** Every request carries an id made up
for it, and only a line that returns that id is its answer. A line without
it, or one that arrives when no request is open, is a `protocol` error for
the case and the side is restarted. So code under test that finds the
protocol pipe and writes to it can make a case fail, but cannot supply an
answer or shift later answers by one. (It runs in the same process as the
driver, so this is a guard against accident and casual forgery, not a
security boundary.)

**Each side is a child process speaking JSON lines.** The harness starts
both, sends each case, reads each outcome. If a side dies (a Rust
`panic = "abort"`, a segfault, `os._exit`) or exceeds the time limit, that
case is an error and a new process serves the next case. A panic that PyO3
turns into `PanicException` is caught and reported as a raise.

**No number crosses as a JSON number.** Outputs come back as tagged values:
floats as sixteen hex digits, integers as decimal strings. The harness
therefore cannot commit the bug it is looking for, whatever its own JSON
parser does.

**The module that runs is the one just built.** A PyO3 side is built with
`maturin build --release --locked` into a temporary directory, unpacked, and
put first on `sys.path`. The interpreter runs isolated (next paragraph), so
`PYTHONPATH`, the user site, the script's directory and the current
directory are out. After import the driver checks
where the module came from and refuses anything outside the unpacked wheel
(for a Python side, outside the suite's directories). A copy left in a venv
by `maturin develop` cannot be picked up. Each feature set builds in its own
target directory, `target/carcinify-diff/<features>` in the crate's workspace:
Cargo writes the finished library to one path per target directory, so two
runs with different features would otherwise swap libraries.

**The interpreter's environment is the harness's.** Each side runs as
`python -s -X pycache_prefix=... driver.py`, not with `-I`: `-I` includes
`-E`, which ignores every `PYTHON*` variable, `PYTHONHASHSEED` among them,
so under `-I` no seed could take effect. Instead the harness removes every
`PYTHON*` variable from the side's environment, which is what `-E` ignored,
then sets `PYTHONHASHSEED` and `PYTHONSAFEPATH=1`. `-s` keeps the user site
out. `PYTHONSAFEPATH` keeps the script's directory off `sys.path` on Python
3.11 and later, where it is `-P`; earlier versions ignore it, so the driver
sits alone in a directory of its own and takes that directory off
`sys.path` itself. Before anything else the driver refuses to run if the
interpreter ignores the environment (one wrapped to add `-E`), if the seed
is not the one the harness set, if any other `PYTHON*` variable is set, if
the user site is on, or, on 3.11 and later, if safe-path is off. It then
removes `PYTHONSAFEPATH` from its own environment.

One behaviour changed with this. A process that a side starts inherits the
side's environment, which now holds `PYTHONHASHSEED` and no other `PYTHON*`
variable. Under `-I` it inherited the user's `PYTHON*` variables,
`PYTHONPATH` among them: `-I` made only the side's own interpreter ignore
them. A child started by `multiprocessing` gets the side's flags, `-s`, and
on 3.11 and later `-P`, and runs under the seed; under `-I` it got `-I` and
ignored the seed. On 3.9 and 3.10 it has no `-P`, so the current directory
is on its `sys.path` while it starts.

Cargo decides whether to recompile by comparing file times, so a source
file put in place with a time older than the last build (`cp -p`,
`rsync -t`, an archive unpacked over the tree, clocks that disagree on a
shared directory) would not be recompiled, and the previous library would
run under the new commit's name. The harness therefore keeps its own record
by content. After each successful build it writes a digest of the sources
into the target directory: the crate's Cargo workspace and every path
dependency outside it. Before the next build it compares, and if the digest
differs or there is no record it runs `cargo clean -p` for every package
built from local sources, path dependencies included. Registry dependencies
stay built, so a forced rebuild costs seconds, and an unchanged tree is not
rebuilt (the check adds about 0.2 s). The report says which happened, as
`rebuilt_from_clean` on the side.

A file compiled in from outside those directories is added to the digest by
content, taken from the last build's dep-info. Only one run at a time
builds in a target directory: a lock file beside it is held from the check
of the record until the wheel is unpacked.

For a directory with nothing uncommitted the digest is its git tree id,
which names the content exactly. With uncommitted changes (`--allow-dirty`)
the files are hashed instead: tracked files as they are on disk, and
untracked files git does not ignore. Toolchain, features and `RUSTFLAGS`
are left to Cargo, which notices those itself.

The same care for the Python sides: each interpreter is started with
`-X pycache_prefix` pointing into the run's temporary directory. Python
trusts a cached bytecode file when the source's recorded time and size
still match, so a `__pycache__` left by an earlier run could be executed in
place of a source file changed without either changing. With the prefix, no
cache from an earlier run is read, and none is written into the repository.

**The commit means something.** The run stops (exit 2) when what ran is not
what the commit holds. Two checks, because `git status` is silent about
ignored files and an ignored file runs like any other:

1. `git status` must be empty: no modified, staged or untracked files. A
   tracked file marked `assume-unchanged` or `skip-worktree` is invisible
   to `git status`, so each such file is compared with its content at
   `HEAD`, and one that differs counts as a change. Files inside a
   submodule are tracked files; the report gives each submodule's commit.
2. Every file the run is known to have used must be tracked by git:
   - every file either side imported from inside the repository (an ignored
     `local_patch.py`, or an extension file left beside a `.py` of the same
     name by `maturin develop`, which Python loads first). Installed
     packages are the exception: see below;
   - for a PyO3 side, `Cargo.toml`, `Cargo.lock`, `pyproject.toml` and
     `build.rs` of the crate and its workspace, and any `.cargo/config.toml`
     or `rust-toolchain.toml` from the crate directory up to the repository
     root;
   - every source file that went into the build (this covers `mod`,
     `include!` and `include_str!`). The list comes from the dep-info file
     Cargo writes beside the final library, `target/.../release/lib<name>.d`,
     which it rewrites on every build. Paths in it are resolved first, so a
     tracked file reached with `..` (a schema at the repository root) is
     found, and the same list covers inputs a build script declares. The
     per-compilation records under
     `deps/` are not read, because those of earlier builds stay there and
     name files the crate no longer uses. One false stop is left: a library
     that was renamed leaves its old `.d` file behind, and the message then
     gives the directory to delete;
   - every file in the built wheel whose content equals an untracked or
     ignored file of the crate's workspace, which is how a stale file in a
     mixed Python/Rust layout gets packaged.

`--allow-dirty` overrides both, and the report then has `"dirty": true` with
the `git status` lines and the list of files used but not tracked. Both
sides must be inside the repository.

Installed packages are third-party code, not project code. A file under the
interpreter's package directories (`sysconfig`'s `purelib` and `platlib`,
that is `site-packages`) is not required to be tracked, even when the
virtualenv is inside the repository, which is where most projects keep it.
Each side's report lists these files with their SHA-256 under
`third_party_files`, and they are part of `implementation_sha256`.

A report made with Debian's or Ubuntu's own Python carries one more line
there: `/etc/python3.x/sitecustomize.py`, with `"kind":
"interpreter_start_up"`. The interpreter imports `sitecustomize` and
`usercustomize` by itself before any code of the project, so the file runs
in the measured process and is listed and hashed like the others. The kind
sets it apart from packages the project chose. Only
where the file is decides this:

- A project package installed in editable mode stays in the source tree
  (site-packages holds a pointer to it), so its files are project files and
  must be tracked.
- A project package installed as a copy (`pip install .`) is in
  site-packages and is treated as third-party. A stale copy would be hashed
  and listed, not refused. Install project code in editable mode, or put it
  on the side's `path`.
- A side's own directory, or a shared directory, may not be inside
  site-packages: the run stops.

What no commit pins is recorded instead of refused: Cargo and rustup
configuration above the repository or in Cargo's home directory (path and
SHA-256), and environment variables such as `RUSTFLAGS`, `RUSTC_WRAPPER`,
`PYO3_*`, `CARGO_PROFILE_*` and `MATURIN_*`.

**Key order is compared unless a case opts out.** The task brief asked for
order to be compared "when the case says order matters". This is the stricter
reading: order matters by default, a case can say it does not, and that
shows in the report. Python dicts are ordered and a reordered dict changes
any text or hash made from it.

**One function name on both sides.** If the port names it differently, add a
one-line wrapper to the port. There is no rename table.

## Layout

| File | |
|---|---|
| `src/value.rs`, `compare.rs` | value format and exact comparison |
| `src/suite.rs` | suite and case files |
| `src/side.rs` | child processes: protocol, timeout, restart |
| `src/report.rs`, `provenance.rs` | report, hashes, git, tool versions |
| `src/python/` | everything that knows about Python: `driver.py` (runs inside the interpreter) and the maturin build |

Only `src/python/` is specific to Python. Another source language would be a
sibling directory providing its own driver; nothing has been built for one.

## Limits

- The report can be reproduced, not authenticated. Anyone can write a JSON
  file by hand. What the harness gives is a result that a second person gets
  again by running the same command at the same commit, with hashes to
  confirm they ran the same thing.
- The same-code rules compare files and watch what Python opens. They do
  not catch:
  - native code that reads the other side's files itself (Rust `std::fs`
    raises no Python audit event);
  - an edited copy of the original: one added comment line makes the
    content different, and no file comparison can say whose code it is;
  - a process a side starts that runs the original. The start is recorded;
    what it ran is not known.
- Code in a `shared` directory is not compared. If the logic under test
  lives there and each side's function only forwards to it, the run passes
  and the port was not tested. The harness lists the shared files both sides
  ran so that a reader can see when "helpers" is most of the program.
- The Python-call counts have limits of their own:
  - they count calls of Python functions, not time or lines; one call can
    be all the work or none of it;
  - native code calling native code is not seen, so `0 Python calls` says
    the function called no Python, not that the Rust in it is the port's;
  - the profile hook covers the calling thread and threads started during
    the call. A thread started earlier and still running is not counted,
    and one that outlives its call is counted into a later one;
  - code that sets its own profile function (a profiler, a coverage tool)
    or calls `sys.setprofile(None)` takes the counter away for the rest of
    that call. The harness checks for this when the call returns. A case
    where it happened is left out of the totals, and the function's row says
    how many cases were not counted and why;
  - the hook slows sides that are mostly Python, by up to about nine times
    (see `--timeout-secs` under "Use"). The per-case time limit is enforced
    from outside the process and still applies;
  - the hook is a Python function called on each event; it never raises and
    returns nothing, so it does not change a result. It does use one frame
    of the recursion limit while it runs, so code that recurses to within
    one frame of the limit can hit it one level earlier.
- The record that guards against a stale build covers the crate's
  workspace and its path dependencies. Under `--allow-dirty` it does not
  cover a file that git ignores, and it never covers a file outside those
  directories that a `build.rs` reads without telling Cargo.

## Tests

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

The files in `tests/` run the `carcinify` binary, so they are built as test
targets of the `carcinify` crate (see `carcinify/Cargo.toml`); the unit
tests are in this crate.

`cargo test` needs `git` and `python3` (3.9+; set `CARCINIFY_PYTHON` to choose
another). `tests/cli.rs` runs the real binary against real Python processes
in a temporary git repository per test. Two test files need maturin and are
ignored by default: `tests/example.rs` runs the worked example, and
`tests/mixed.rs` builds the fixture in `tests/mixed/` (a wheel with a Python
layer over a compiled core) in a temporary repository, step by step.

The isolation test plants a `PYTHONPATH` directory, a user site (through
`HOME` and through `PYTHONUSERBASE`), a current directory and a
`PYTHONSTARTUP` file, each of which leaves a mark if Python runs it. A
virtual environment has no user site, so the test runs the harness with the
interpreter the environment was made from (`sys._base_executable`). If that
one has no user site either, that part shows nothing, and the test says so
on stderr.

```sh
CARCINIFY_PYTHON=$PWD/.venv/bin/python CARCINIFY_MATURIN=$PWD/.venv/bin/maturin \
    cargo test -p carcinify --test diff-example --test diff-mixed -- --ignored
```
