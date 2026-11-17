# Should this piece be ported?

Verdict: **consistent**. No measurement contradicts the declared decision. That is not the same as the decision being right.

For a reader to look at:

- the instrumented run took 2.9 times as long as a plain one. The profiler's cost falls on every call, so the shares lean towards code that makes many small calls; the totals are from the plain runs and are not affected
- a typical call of the candidate takes in and gives back 230446 Python objects. Each has to be converted at the boundary unless the design avoids it; that cost is not in these measurements

## What was run

- The improved import at the size of the largest customer file seen so far, 200,000 records.
- `run_grouped` in `skill/steps/01-decide/examples/own-python/workload.py`, arguments `[200000]`, 5 times with nothing attached
- Commit `b8f160dc9b28`, macos aarch64 with 8 logical CPUs, Python 3.9.6
- Wall time: fastest 0.776 s (median 0.779, slowest 0.795; they differ by 2.4% of the median). CPU time of the fastest run: 0.775 s. Peak memory: 182 MB

## Where the time goes: `mostly_own_python`

| Waiting | Compiled code | Library Python | The project's own Python |
|---|---|---|---|
| 0.1% | 0.0% | 0.0% | 99.9% |

Waiting is one minus CPU time over wall time in the fastest plain run. The rest is split in the proportions of one run under cProfile, with waiting first taken out of the compiled part, since a sleep or a read is a compiled call. A compiled call shorter than the limit below on average is counted with the Python code that makes it. Where the candidate uses a compiled package, the long calls into it that only `sys.monitoring` sees are moved from the project's own Python to compiled code; work reached through operators stays in the project's own Python, and the classification then says the two are not separated. The instrumented run took 2.89 times as long as a plain one.

The project's own functions, by time including what they call:

| Function | File | Calls | Callers | With what it calls | Of that, own Python | Itself |
|---|---|---|---|---|---|---|
| `run_grouped` | workload.py:26 | 1 | 0 | 100.0% | 100.0% | 0.8% |
| `find_duplicates_grouped` | dedupe.py:37 | 1 | 1 | 91.2% | 91.1% | 5.5% |
| `fingerprint` | dedupe.py:9 | 200000 | 1 | 82.4% | 82.4% | 53.1% |
| `records` | workload.py:9 | 1 | 1 | 8.1% | 8.1% | 6.6% |
| `<listcomp>` | dedupe.py:48 | 1 | 1 | 0.1% | 0.1% | 0.1% |

The largest outside the project:

- `<method 'isalnum' of 'str' objects>` (compiled), 3934187 calls, 0.228 s in the instrumented run
- `<method 'append' of 'list' objects>` (compiled), 3736954 calls, 0.217 s in the instrumented run
- `<method 'lower' of 'str' objects>` (compiled), 3514187 calls, 0.209 s in the instrumented run
- `<method 'setdefault' of 'dict' objects>` (compiled), 200000 calls, 0.056 s in the instrumented run
- `<method 'join' of 'str' objects>` (compiled), 200000 calls, 0.029 s in the instrumented run
- `<built-in method builtins.len>` (compiled), 380001 calls, 0.020 s in the instrumented run

## The candidate: `find_duplicates_grouped` in `dedupe.py`

- Called 1 times by 1 caller(s): ["run_grouped"]
- With everything it calls it is 91.2% of the time
- Its own Python, with the Python it calls, is **91.1%** of the time
- **Ceiling: if that cost nothing, the whole workload would be at most 11.29 times faster** (1 / (1 - p) with p = 0.9114). A real port gains less.
- Best case: the workload takes 0.776 s now and would take 0.069 s if the candidate's own Python cost nothing, 0.707 s saved per run
- Calls into compiled objects were not timed: that needs Python 3.12 or later. On this Python a compiled package is found by name, by the types of the objects crossing, of local variables and of named globals, and by its built-in functions only.
- Across the boundary: entered 1 times from outside; a typical call takes in 199999 Python objects (20559130 bytes) and gives back 30447 (1222424 bytes); about 707297.82 microseconds per call
- First call: arguments {"records":"list of dict, length 200000"}, returns "list of tuple, length 10170"
- Its file imports these compiled modules: []

## Language or algorithm?

Declared by the author, not measured: What is left is the language: one pass over the records, building each fingerprint a character at a time in the interpreter. No better algorithm is known; the work is already one pass. Better algorithm in Python: `none_known`.

## The decision

Declared by the author: `do_not_port`. The largest file seen takes under a second after the fix, and an upload is answered in the background: nobody waits for it. At best a port would save most of that second per upload, before the cost of converting the 200,000 records the function takes in. That is not worth a boundary between two languages. To be looked at again if files grow tenfold.

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
