# Should this piece be ported?

Verdict: **consistent**. No measurement contradicts the declared decision. That is not the same as the decision being right.

For a reader to look at:

- 25% of the time is in library Python, not in the project's code. Porting the project's code does not touch it; whether a Rust crate replaces the library is the question of the dependency check. Part of it may be compiled work the profiler cannot see
- a typical call of the candidate takes in and gives back 24015 Python objects. Each has to be converted at the boundary unless the design avoids it; that cost is not in these measurements

## What was run

- One hour of events: 24 batches of 4,000 records, each written as JSON, compressed and hashed.
- `run` in `skill/steps/01-decide/examples/native/workload.py`, arguments `[24]`, 5 times with nothing attached
- Commit `b8f160dc9b28`, macos aarch64 with 8 logical CPUs, Python 3.9.6
- Wall time: fastest 0.371 s (median 0.372, slowest 0.375; they differ by 1.0% of the median). CPU time of the fastest run: 0.370 s. Peak memory: 78 MB

## Where the time goes: `mostly_compiled_code`

| Waiting | Compiled code | Library Python | The project's own Python |
|---|---|---|---|
| 0.2% | 74.5% | 25.1% | 0.2% |

Waiting is one minus CPU time over wall time in the fastest plain run. The rest is split in the proportions of one run under cProfile, with waiting first taken out of the compiled part, since a sleep or a read is a compiled call. A compiled call shorter than the limit below on average is counted with the Python code that makes it. Where the candidate uses a compiled package, the long calls into it that only `sys.monitoring` sees are moved from the project's own Python to compiled code; work reached through operators stays in the project's own Python, and the classification then says the two are not separated. The instrumented run took 1.02 times as long as a plain one.

The project's own functions, by time including what they call:

| Function | File | Calls | Callers | With what it calls | Of that, own Python | Itself |
|---|---|---|---|---|---|---|
| `run` | workload.py:19 | 1 | 0 | 100.0% | 0.2% | 0.0% |
| `pack_all` | archive.py:19 | 1 | 1 | 100.0% | 0.2% | 0.0% |
| `<listcomp>` | archive.py:20 | 1 | 1 | 100.0% | 0.2% | 0.0% |
| `pack` | archive.py:13 | 24 | 1 | 100.0% | 0.2% | 0.1% |

The largest outside the project:

- `<built-in method zlib.compress>` (compiled, long calls), 24 calls, 0.283 s in the instrumented run
- `iterencode` (library python), 24 calls, 0.095 s in the instrumented run
- `<built-in method _hashlib.openssl_sha256>` (compiled), 24 calls, 0.000 s in the instrumented run
- `<method 'encode' of 'str' objects>` (compiled), 24 calls, 0.000 s in the instrumented run
- `encode` (library python), 24 calls, 0.000 s in the instrumented run
- `dumps` (library python), 24 calls, 0.000 s in the instrumented run

## The candidate: `pack` in `archive.py`

- Called 24 times by 1 caller(s): ["<listcomp>"]
- With everything it calls it is 100.0% of the time
- Its own Python, with the Python it calls, is **0.2%** of the time
- **Ceiling: if that cost nothing, the whole workload would be at most 1 times faster** (1 / (1 - p) with p = 0.0022). A real port gains less.
- Best case: the workload takes 0.371 s now and would take 0.370 s if the candidate's own Python cost nothing, 0.001 s saved per run
- Calls into compiled objects were not timed: that needs Python 3.12 or later. On this Python a compiled package is found by name, by the types of the objects crossing, of local variables and of named globals, and by its built-in functions only.
- Across the boundary: entered 24 times from outside; a typical call takes in 24008 Python objects (1865845 bytes) and gives back 7 (565 bytes); about 15446.20 microseconds per call
- First call: arguments {"records":"list of dict, length 4000"}, returns "dict, length 3"
- Its file imports these compiled modules: []

## Language or algorithm?

Declared by the author, not measured: Neither is at fault: pack is three calls into the standard library, and the work of each is done in compiled code already. Better algorithm in Python: `none_known`.

## The decision

Declared by the author: `do_not_port`. The project's own Python is a fraction of a percent of the time. The compression level is the thing to look at, not the language.

## What this report does not measure

- Load that came and went inside a run. The machine is judged busy from a short loop of pure computation timed before and after every run, and on Linux from the time the main thread was ready and not running. On other systems a burst of load that began and ended inside one run is not seen. Slow disks and networks are not seen at all: only the CPU is tested. A workload whose own threads or child processes keep running after it returns makes the machine read as busy.
- Whether the workload is representative. The tool ran what the declaration names; a small or unusual input gives a true profile of the wrong thing. The file, function, arguments and times are above so that a reader can judge.
- Compiled work reached without a function call. Arithmetic on large integers, string building, and operators on objects of compiled packages (`a @ b`, `a + b` on arrays) run in compiled code and are counted as time of the Python function they are written in. No instrument used here sees them. Such a function looks like the best candidate and a port of it gains nothing.
- Compiled work reached by calling an object that is not a built-in function: most of NumPy's functions (`np.dot`, `np.add`), a compiled type called to make an object, the standard library's JSON encoder. The profiler counts it as time of the Python function that makes the call. On Python 3.12 and later the tool times such calls made while the candidate runs with `sys.monitoring` and takes the long ones off the candidate's share; on older Pythons it does not see them.
- Every use of a compiled package. The tool looks for one in five places: the names used by the candidate and by the project's functions it calls; the types of the objects that go in and come out of the candidate, as far as lists, dictionaries, instance attributes and slots lead; the local variables of those functions when they return and the globals they name; the built-in functions called while it runs; and on Python 3.12 and later every call made while it runs. An object of a compiled package that is never bound to a name, or sits deeper than the walk goes, and is used only through operators, is found by none of them. When such a package is loaded in the process and nothing was found, the report says so.
- What the stand-ins do. Where the candidate uses a compiled package the author names the same workload with the package's work replaced; the tool times it. It refuses one that is the workload again, that does not call the candidate, that still passes large objects of the package to it, or (Python 3.12 and later) that still makes long calls into the package. It cannot check the rest: a stand-in that still does the work on data the checks do not see leaves the candidate's share too high, and one that skips Python work as well makes it too low.
- Work in other threads, in child processes and on a GPU. The profile covers the main thread. To a comparison of wall time with this process's CPU time, a child process or a GPU at work looks the same as waiting; CPU time of child processes is given where the operating system reports it, GPU time is not seen at all.
- Whether the cost is the language's or the algorithm's. That is the author's judgment, printed as declared; where an alternative in Python was named, the tool measured it.
- What a port would cost to write, test and keep, and what the call across the boundary would cost. The number of calls and the data per call are measured; the cost of converting them is not.
- Whether static types or errors caught at compile time are worth a port here. Time measurements neither support nor contradict those goals.
