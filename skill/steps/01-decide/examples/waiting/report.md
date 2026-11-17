# Should this piece be ported?

Verdict: **consistent**. No measurement contradicts the declared decision. That is not the same as the decision being right.

## What was run

- One day's invoices, 400 of them in twenty currencies, converted to EUR. The size of an ordinary day.
- `run` in `skill/steps/01-decide/examples/waiting/workload.py`, arguments `[400]`, 5 times with nothing attached
- Commit `b8f160dc9b28`, macos aarch64 with 8 logical CPUs, Python 3.9.6
- Wall time: fastest 0.714 s (median 0.732, slowest 0.754; they differ by 5.5% of the median). CPU time of the fastest run: 0.001 s. Peak memory: 24 MB

## Where the time goes: `mostly_waiting`

| Waiting | Compiled code | Library Python | The project's own Python |
|---|---|---|---|
| 99.9% | 0.0% | 0.0% | 0.1% |

Waiting is one minus CPU time over wall time in the fastest plain run. The rest is split in the proportions of one run under cProfile, with waiting first taken out of the compiled part, since a sleep or a read is a compiled call. A compiled call shorter than the limit below on average is counted with the Python code that makes it. Where the candidate uses a compiled package, the long calls into it that only `sys.monitoring` sees are moved from the project's own Python to compiled code; work reached through operators stays in the project's own Python, and the classification then says the two are not separated. The instrumented run took 1.05 times as long as a plain one.

The project's own functions, by time including what they call:

| Function | File | Calls | Callers | With what it calls | Of that, own Python | Itself |
|---|---|---|---|---|---|---|
| `run` | workload.py:9 | 1 | 0 | 100.0% | 0.1% | 0.0% |
| `convert` | exchange_rates.py:20 | 1 | 1 | 100.0% | 0.1% | 0.0% |
| `fetch_rate` | exchange_rates.py:13 | 20 | 1 | 100.0% | 0.0% | 0.0% |
| `<listcomp>` | workload.py:11 | 1 | 1 | 0.0% | 0.0% | 0.0% |

The largest outside the project:

- `<built-in method time.sleep>` (compiled, long calls), 20 calls, 0.748 s in the instrumented run
- `<built-in method builtins.sum>` (compiled), 20 calls, 0.000 s in the instrumented run
- `<built-in method builtins.len>` (compiled), 400 calls, 0.000 s in the instrumented run
- `<built-in method builtins.round>` (compiled), 1 calls, 0.000 s in the instrumented run

## The candidate: `convert` in `exchange_rates.py`

- Called 1 times by 1 caller(s): ["run"]
- With everything it calls it is 100.0% of the time
- Its own Python, with the Python it calls, is **0.1%** of the time
- **Ceiling: if that cost nothing, the whole workload would be at most 1 times faster** (1 / (1 - p) with p = 0.0005). A real port gains less.
- Best case: the workload takes 0.714 s now and would take 0.714 s if the candidate's own Python cost nothing, 0.000 s saved per run
- Calls into compiled objects were not timed: that needs Python 3.12 or later. On this Python a compiled package is found by name, by the types of the objects crossing, of local variables and of named globals, and by its built-in functions only.
- Across the boundary: entered 1 times from outside; a typical call takes in 822 Python objects (36420 bytes) and gives back 1 (24 bytes); about 713957.82 microseconds per call
- First call: arguments {"invoices":"list of tuple, length 400","target":"str, length 3"}, returns "float"
- Its file imports these compiled modules: []

## Language or algorithm?

Declared by the author, not measured: Neither: the code asks the rates service once per currency and waits for each answer. Asking for all twenty rates in one request, or at the same time, would shorten the wait in any language. Better algorithm in Python: `exists_untried`.

## The decision

Declared by the author: `do_not_port`. Almost all of the time is spent waiting for the rates service. A port would wait just as long.

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
