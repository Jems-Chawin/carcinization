# Should this piece be ported?

Verdict: **consistent**. No measurement contradicts the declared decision. That is not the same as the decision being right.

For a reader to look at:

- the instrumented run took 3.6 times as long as a plain one. The profiler's cost falls on every call, so the shares lean towards code that makes many small calls; the totals are from the plain runs and are not affected
- the alternative ran for 3.1 milliseconds. That is too short to time well, so "how many times faster" is rough; that it is much faster is not in doubt

## What was run

- One import of 900 customer records, about one in twenty a respelling of another, as the import runs today.
- `run` in `skill/steps/01-decide/examples/own-python/workload.py`, arguments `[900]`, 5 times with nothing attached
- Commit `b8f160dc9b28`, macos aarch64 with 8 logical CPUs, Python 3.9.6
- Wall time: fastest 1.993 s (median 2.001, slowest 2.007; they differ by 0.7% of the median). CPU time of the fastest run: 1.989 s. Peak memory: 23 MB

## Where the time goes: `mostly_own_python`

| Waiting | Compiled code | Library Python | The project's own Python |
|---|---|---|---|
| 0.2% | 0.0% | 0.0% | 99.8% |

Waiting is one minus CPU time over wall time in the fastest plain run. The rest is split in the proportions of one run under cProfile, with waiting first taken out of the compiled part, since a sleep or a read is a compiled call. A compiled call shorter than the limit below on average is counted with the Python code that makes it. Where the candidate uses a compiled package, the long calls into it that only `sys.monitoring` sees are moved from the project's own Python to compiled code; work reached through operators stays in the project's own Python, and the classification then says the two are not separated. The instrumented run took 3.65 times as long as a plain one.

The project's own functions, by time including what they call:

| Function | File | Calls | Callers | With what it calls | Of that, own Python | Itself |
|---|---|---|---|---|---|---|
| `run` | workload.py:21 | 1 | 0 | 100.0% | 100.0% | 0.0% |
| `find_duplicates` | dedupe.py:27 | 1 | 1 | 100.0% | 100.0% | 0.7% |
| `same_person` | dedupe.py:23 | 404550 | 1 | 99.3% | 99.3% | 2.4% |
| `fingerprint` | dedupe.py:9 | 809100 | 1 | 96.9% | 96.9% | 61.1% |
| `records` | workload.py:9 | 1 | 1 | 0.0% | 0.0% | 0.0% |

The largest outside the project:

- `<method 'isalnum' of 'str' objects>` (compiled), 15906906 calls, 0.924 s in the instrumented run
- `<method 'lower' of 'str' objects>` (compiled), 14207796 calls, 0.807 s in the instrumented run
- `<method 'append' of 'list' objects>` (compiled), 13440995 calls, 0.759 s in the instrumented run
- `<method 'join' of 'str' objects>` (compiled), 809100 calls, 0.111 s in the instrumented run
- `<built-in method builtins.len>` (compiled), 1711 calls, 0.000 s in the instrumented run
- `<method 'replace' of 'str' objects>` (compiled), 45 calls, 0.000 s in the instrumented run

## The candidate: `find_duplicates` in `dedupe.py`

- Called 1 times by 1 caller(s): ["run"]
- With everything it calls it is 100.0% of the time
- Its own Python, with the Python it calls, is **100.0%** of the time
- **Ceiling: nearly all of the time is this piece, so the formula gives no useful bound** (1 / (1 - p) with p = 0.9999). A real port gains less.
- Best case: the workload takes 1.993 s now and would take 0.000 s if the candidate's own Python cost nothing, 1.993 s saved per run
- Calls into compiled objects were not timed: that needs Python 3.12 or later. On this Python a compiled package is found by name, by the types of the objects crossing, of local variables and of named globals, and by its built-in functions only.
- Across the boundary: entered 1 times from outside; a typical call takes in 3559 Python objects (344617 bytes) and gives back 136 (5512 bytes); about 1993238.33 microseconds per call
- First call: arguments {"records":"list of dict, length 900"}, returns "list of tuple, length 45"
- Its file imports these compiled modules: []

## Language or algorithm?

Declared by the author, not measured: Mostly the algorithm: every record is compared with every other, and the fingerprint of each is worked out again for every comparison. Grouping the records by fingerprint needs one fingerprint per record. Better algorithm in Python: `tried`.

The alternative, measured by the tool in the same run: The same import with the records grouped by fingerprint first; it returns the same pairs in the same order. `run_grouped`: fastest run 0.0031 s against 1.993 s, **640.15 times faster**, 0.2% of the original's time.

## The decision

Declared by the author: `fix_algorithm_first`. Grouping by fingerprint removes nearly all of the cost in Python. Whether anything is left to port is a question for the improved code.

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
