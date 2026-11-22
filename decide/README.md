# carcinify decide

Runs a workload of a Python project and writes down where its time goes,
what a candidate piece holds of it, and whether the measurements contradict
the decision its author declares: port this piece, fix the algorithm in
Python first, or do not port.

It exists because the first question of a port is rarely asked. Code that
waits for a network gains nothing from another language. Code whose work is
already done by compiled libraries gains little. And a port that is fast
because the algorithm was fixed on the way would have been fast in Python.

## Use

```sh
cargo build --release -p carcinify
target/release/carcinify decide path/to/decide.json --report target/carcinify-decide/report.json \
    --markdown target/carcinify-decide/report.md --python .venv/bin/python
```

Exit status: `0` no measurement contradicts the declared decision; `1` one
does, something needed is missing, or the machine was too busy to measure
on; `2` the run could not be carried out
(bad declaration, uncommitted changes, a workload that fails).
`--allow-dirty` runs on a tree with uncommitted changes and the report says
so. Needs `git` and a Python 3.9 or later; the project's own interpreter if
it has dependencies. On Python 3.12 and later it sees more (see "Compiled
packages"). Nothing is installed.

## The declaration

```json
{
  "schema": "carcinify/decide/declaration/2",
  "project_root": "src",
  "workload": {
    "description": "One import of 900 customer records, as it runs today.",
    "file": "bench/workload.py", "function": "run", "args": [900], "repeat": 5
  },
  "candidate": {"file": "dedupe.py", "function": "find_duplicates"},
  "language_or_algorithm": {
    "statement": "Mostly the algorithm: every record is compared with every other.",
    "better_algorithm_in_python": "tried",
    "tried": {"description": "grouped by fingerprint first", "function": "run_grouped", "args": [900]}
  },
  "decision": {"choice": "fix_algorithm_first", "reason": "Grouping removes nearly all of the cost in Python."}
}
```

- `project_root`: the directory of the project's own Python. Code under it
  is "the project's own"; everything else is a library. Virtual
  environments and `site-packages` under it are left out.
- `workload`: a Python file and a function in it, called with the given
  arguments, which are data. `repeat` (at least 3) is how often it is run
  with nothing attached. `description` says what it is and why it is
  representative.
- `candidate`: the piece that might be ported, a function in a file under
  the root (`line` when the file has two of the name). `with_stand_ins`,
  for a candidate that uses a compiled package: the same workload with the
  package's work replaced, as a function the tool times.
  `uses_no_compiled_package`, a sentence, for a candidate of a port for
  speed in a process where such a package is loaded and the tool saw the
  candidate use none (see "Compiled packages" for both).
- `language_or_algorithm`: the author's answer in words, and whether a
  better algorithm in Python is `none_known`, `exists_untried` or `tried`.
  For `tried`, the alternative is a function the tool measures in the same
  run.
- `decision`: `port` (with `goals` from `speed`, `static_types`,
  `errors_at_compile_time`, `memory_safety`), `fix_algorithm_first` or
  `do_not_port`, with a reason. A port for speed also declares a `need`:

  ```json
  "need": {"statement": "An upload is answered while the user waits; the largest file takes four seconds.",
           "workload_must_take_at_most_seconds": 1.0}
  ```

  a sentence saying who needs the speed and why, and exactly one number:
  the time the workload must finish in, or `runs_per_day`.

Unknown fields are rejected. There is no field for a measured time, a share
or a classification. The need is a requirement, printed as declared.

## What the tool does

Each step is a fresh Python process, started with `-I`, running a script
that is part of the binary. The workload file is loaded and its function
called.

1. **Plain runs.** The workload `repeat` times with nothing attached: wall
   time, CPU time of the process, of the main thread and of child
   processes, and peak memory. **Every total in the report is from these.**
2. **One run under `cProfile`**, timed by the wall clock. **Every share is
   from this one.** It is slower than a plain run, and the report says how
   many times.
3. **One run with a hook on the candidate alone**: how often it is entered
   from outside, for a sample of calls how many Python objects go in and
   come out, with their size and the packages their types come from, which
   packages' built-in functions are called while it runs, and the types of
   what it and the project's functions it calls hold in local variables
   and named globals.
4. **On Python 3.12 and later, one run under `sys.monitoring`**, which
   times the calls the project's code makes into compiled code while the
   candidate runs.
5. **The alternative**, if one is named: plain runs, like step 1. **The
   stand-in workload**, if one is named: plain runs, and steps 3 and 4.
6. **With `--record-calls <file>`, one more run with a hook on the
   candidate**, untimed, which writes each distinct call of the candidate
   from outside it as a case line of `carcinify diff`, in its typed value
   format, so that the input measured here can be checked to be among the
   comparison's cases (see the harness's "Measured inputs").

   - The hook is `sys.setprofile`, matched on the candidate's code, as in
     step 3. So it sees every call however the candidate was reached:
     through its module, through a name taken with `from m import f`
     before the run, or through a reference kept anywhere else. It works
     on every Python the tool supports.
   - The arguments are encoded when the call is entered, before the body
     runs, so an argument the candidate changes in place is recorded as it
     came in.
   - Named parameters are written by position, keyword-only ones by
     keyword, and `*args` and `**kwargs` as they were passed; defaults are
     filled in. A call the candidate makes of itself is not recorded: it is
     not an input of the workload.
   - A call that holds a value the format does not cover (an array, an
     instance of a class) is written as
     `{"id": "call-4", "fn": "f", "unencodable": "numpy.ndarray"}`, named
     on the screen and in the report under `recorded_calls`, with the
     number of calls and of distinct lines.
   - `fn` is the candidate's name in the original, which may be private;
     the harness's measured entry can rename it. Name the file
     `*.calls.jsonl`.

Before and after every run the script times a tenth of a second of pure
computation, and for as long as each of these processes runs a second
process does nothing but compute, to learn whether other programs were
taking the CPU (see "A busy machine").

### Where the time goes

Four shares:

- **Waiting**: one minus CPU time over wall time, in the fastest plain
  run that was not made on a busy machine. The fastest is used because a
  run that lost the CPU to another program has wall time that is neither
  its work nor its waiting. Runs that differ by more than a quarter of
  their median are flagged.
- **Compiled code**: time in calls into code with no Python source (the
  interpreter's built-ins, C extensions) that last 20 microseconds or more
  on average, less the waiting share, since a sleep or a read from a socket
  is such a call.
- **Library Python**: time in Python files outside the project root.
- **The project's own Python**: time in files under it.

The time that is not waiting is split among the last three in the
profile's proportions, so the four add up to one.

A compiled call shorter than 20 microseconds on average (`str.lower`,
`list.append`, `len`) is counted with the Python code that makes it. Its
cost is mostly the call, and it goes away when the caller is ported.

A share of at least half names the workload: `mostly_waiting`,
`mostly_compiled_code`, `mostly_library_python`, `mostly_own_python`;
otherwise `mixed`. When the largest share is the project's own Python and
the candidate uses a compiled package, the name is
`own_python_and_compiled_packages_not_separated`.

### The candidate

For the declared function the report gives: calls and callers; its share of
the time with everything it calls; and **`p`, the share that is the
project's own Python inside it**, with the ceiling `1 / (1 - p)`: how many
times faster the whole workload would be if that cost nothing. A piece
holding 30% gives at most about 1.4 times. Above 95% the figure is not
printed.

What a function "is made of" is worked out from the profiler's record of
who called whom and for how long. A callee's time is shared among its
callers in proportion to the time of each call edge. That is the usual
approximation of call-graph profilers, and it is wrong when a function
costs much more from one caller than from another.

Beside the ceiling the report gives the best case in seconds: what the
workload takes now, what it would take if the candidate's own Python cost
nothing, and the difference per run.

The report also lists the project's own functions by time, each with its
callers, so that a smaller piece or a better one can be seen. **The tool
does not choose the candidate.**

### Compiled packages

**The profiler books work done inside a compiled package to the Python
function that asked for it**, in two cases:

- operators: `a @ b`, `a + b` on arrays. An operator is not a call;
- calls of objects that are not built-in functions. Most of NumPy's
  functions are such objects (`np.dot`, `np.add`), and so is a compiled
  type called to make an object.

With NumPy 2.5 and 20 products of 1,600 by 1,600 matrices, `cProfile` gives
the function that writes `np.dot(a, b)` over 90% of the time as its own.
A port of that function would gain nothing.

So when the candidate, or a function of the project that it calls, uses a
compiled package from outside the standard library, **the report gives no
share and no ceiling as findings.** It gives bounds:

- **at most**: the profiler's figure, less the long calls into compiled
  objects that `sys.monitoring` timed (Python 3.12 and later; it reports
  calls of every callable, where the profiler reports built-in functions
  only). On an older Python nothing is taken off. Operator work is still
  inside this bound: nothing sees it.
- **at least**: not known, unless a stand-in workload is named.

**A stand-in workload** is the author's other measurement: the same
workload with the package's work replaced by stand-ins of the same shape
(an object whose `@` returns a small array of the right kind without
computing it). The tool times it. All of the candidate's own Python still
runs in it, so the candidate's share is at most about the stand-in
workload's share of the original time; and what the stand-ins removed is
the most the profiler can have counted wrongly, so the share is at least
about the profiler's figure less that. The parts come from runs under
different instruments, so the bounds are good to a point or two, not more.

**The lower bound holds only if the stand-ins replace all of the package's
work and nothing else**, and every place that prints it says so. The tool
checks what it can and refuses a stand-in workload (the report is then
`incomplete`) when

- it names the same function and arguments as the workload;
- the candidate is not called in it;
- an object of a compiled package that addresses more than 64 KiB still
  reaches the candidate in it, as an argument, a result, a local variable
  or a named global, or such objects add up to more than 1 MiB: the data is
  still there for an operator to work on. What counts is the data an object
  shows (`nbytes`, or its buffer), not what it owns, so a view of an array
  counts as the array. A stand-in may return a small array. Both limits
  are chosen, not derived: 64 KiB is a 90 x 90 array of floats, 1 MiB about a
  360 x 360 one. The total counts every object seen, so it grows with the
  number of calls looked at: a stand-in that makes a new small array for
  each of many calls is refused, and the message says to use one small
  object for every call. The total is not set against what the original
  run addressed: "much less than before" would pass megabytes of arrays,
  still worth an operator's work, whenever the original had gigabytes;
- on Python 3.12 and later, the candidate's code still spends more than 1%
  of the original time in long calls into compiled packages.

It flags stand-ins that remove under 5% of the time: either the package
does almost no work, or they replace little.

It flags, and does not refuse, a stand-in workload in which the process
uses more CPU time than wall time while the candidate runs (counted
between the candidate's entry and its return, so threads at work in the
rest of the workload do not count). The flag gives the figures. This was
a refusal, and it refused honest stand-ins: the idle threads of a linear
algebra library use CPU for a moment after a threaded product, wherever
the product was. Measured with NumPy's default threads on Linux, 8 CPUs:
0.35 to 0.6 s of CPU over about 0.1 s of wall time when the candidate is
entered once after a product, whether it then runs for 0.15 s or 2 s, and
none with the library set to one thread. That is not a cost for every
entry: the auditor measured a threaded product before each of 40 entries,
and it cost 0.3 to 0.6 s in all. Neither figure is general; they are two
cases on one machine. A cost that does not grow with the work cannot be told from work by any share of the workload's time, so no
limit refuses. The flag gives the surplus for each entry of the candidate,
and over 1 s per entry (a chosen line, above the 0.6 s measured) its text
says that this is more than idle threads were measured to cost and that
the candidate is probably still doing threaded work in the stand-in. The
verdict is the same either way. To tell the two apart, run again with
`OPENBLAS_NUM_THREADS=1` or `OMP_NUM_THREADS=1` and compare
`candidate.with_stand_ins.inside_the_candidate.wall_seconds`: about the
same, and the threads were idle; longer, and they were working. So a
stand-in whose candidate still does threaded work on data the tool does
not find, through calls that are not long calls from the candidate's own
code, is used, with this flag as the only sign. The worst case measured
(by the auditor, Linux, 8 CPUs): a candidate that multiplies arrays held
as class-level constants, which the stand-in left in place. Its own Python
was 1.4% of the time and the tool gave bounds of 50 to 52%, with the
verdict `consistent_if_the_stand_ins_are_faithful`, the flag (24.9 s of
CPU in 3.6 s) and nothing else; with the library set to one thread the
bounds were the same and there was no flag at all.

The line is per entry. Threaded work left in a candidate that is entered
many times stays under it: measured by the auditor, 42 to 64 s of such
work over 400 entries was worded as idle threads, 5 times of 5, with
bounds of 37 to 54% for a candidate whose own Python was about 0.3%; at 40
entries (0.86 to 1.14 s for each) the wording changed between runs of the
same case. Read the total.

What it cannot check: that the stand-in workload does the rest of the work.
One that leaves operators in on data held where the tool does not look
passes these checks.

A port for speed of such a candidate is `contradicted` when the upper bound
is under 20%, and `incomplete` when no lower bound is known or the lower
bound is under 20%. When it is not contradicted, the verdict is
**`consistent_if_the_stand_ins_are_faithful`**, exit 0, and not
`consistent`: nothing but the author's stand-ins shows that the time is the
candidate's own.

The tool looks for the use of a compiled package in five places:

1. the names used in the candidate's function and in the project's
   functions it calls, when they were imported from such a package (a
   helper in another file counts; an import the function does not use does
   not);
2. the types of the objects that go in and come out of the candidate, and
   of everything they refer to, whatever holds it (a list, a dictionary,
   an instance, a slot, a deque, a closure, a bound method), nearest first
   and objects of packages before the interpreter's own, up to 20,000
   objects from each value, not counting plain numbers and strings. The walk stops at modules, classes
   and code, and does not go from a function into its globals;
3. the local variables of the candidate, and of the project's functions it
   calls, when they return (the first twenty returns of each), which
   includes what a closure holds; and the globals those functions name,
   followed the same way, with `module.NAME` for a module of the project;
4. the built-in functions and methods called while it runs (`a.sum()`);
5. on Python 3.12 and later, every call made by the project's code while
   it runs.

**And a backstop, because these do not find everything.** When a compiled
package is loaded in the process and none of the five places showed the
candidate using one, the report says so, and a port for speed is
`incomplete` until the author either names a stand-in workload or states in
`candidate.uses_no_compiled_package` what the candidate works on instead.
The statement is printed as the author's and is not checked. A port that
passes on it gets the verdict **`consistent_on_the_authors_word`**, exit 0,
and the share, the ceiling and the best case are each printed with the
condition in the same sentence: "if the candidate uses no compiled package,
which the author states and the tool has not checked". A project that
imports NumPy somewhere unrelated pays one sentence for this, and reads
that verdict.

With no candidate named, which is the first report made, to choose one, the
line that gives the split says which loaded packages the "own Python" share
may include work of.

What the five places still miss. Run with NumPy, a candidate whose own
Python is about a tenth of its time: an array that exists only inside one
expression and is never bound to a name, here
`pickle.loads(BLOB) @ pickle.loads(BLOB)`. The tool printed a share of 89%.
The backstop made the port for speed `incomplete`, and with a statement
that no compiled package is used, which was false, it passed, as
`consistent_on_the_authors_word`. An array further than 20,000 objects from
anything the tool starts at is missed the same way. For these the backstop
and the author's word are all there is. "Compiled" means: outside the
standard library, with a module loaded that has no Python source.

The walk costs time in the run with the hook only, which is not used for
any timing: at most 20,000 objects for each value looked at, for the first
fifty calls of the candidate and fewer after, and the first twenty returns
of each function.

### The need

A port for speed declares what speed is needed, and the report prints it as
declared, beside what was measured: the time now, the time if the
candidate's own Python cost nothing, the saving per run and, with
`runs_per_day`, per day. For a candidate with bounds the upper bound is
used, and the report says so.

### A busy machine

On a machine where other programs take the CPU, a workload of pure
computation is kept waiting for it, and that time reads as "waiting": on
one such machine a loop that never waits was reported as 31% waiting, and
an alternative as 1.2 times faster where it was hundreds.

Two things are done about it.

**Around every run** the measuring script runs a loop of pure computation,
a tenth of a second of CPU time, and reports what fraction of the loop's
time the process was not on the CPU. On a quiet machine that is about 1%.
Over 10% the run counts as disturbed. On Linux the time the main thread was
ready and not running is read as well (`/proc/self/schedstat`), and a run
that lost more than 10% of its time that way is disturbed too.

**During every run** a second Python process, started by the tool, does
nothing but compute for as long as the measured process runs, and at the
end reports its CPU time against its wall time. It never waits, so what it
did not get, other programs took, and the workload beside it fared the
same. Over 15% the run counts as disturbed. This sees load that begins and
ends inside a run, which the loops around it miss. It costs one CPU for the
length of the measurement, and with fewer than four CPUs it is not started;
the report then carries a flag: "this machine has N CPUs, and with fewer
than 4 no process is run beside the workload, so load inside a run was not
watched".

- Plain runs that were disturbed are left out, and a flag says how many.
- **If every plain run was disturbed, or an instrumented run was, or the
  process beside a run was kept off the CPU, the verdict is `unreliable`**,
  exit 1, whatever the decision. The figures are still in the report, and
  none should be used.

**A workload that itself uses half the CPUs or more** (CPU time of all
its threads over wall time, per run) cannot be watched this way: what the
process beside it loses, it may lose to the workload, and the workload's
threads wait for each other. NumPy on Linux uses every CPU by default, and
before this rule an idle machine was reported as busy for it in some runs
and not in others. For such a run the process beside it and the time spent ready and
not running are not used, the loops before and after still are, and the
report carries a flag: "the workload itself used N CPUs of M, so load
inside the run could not be watched". The line is at half because the
figure is the CPU time the process got, not what it asked for: eight
threads on eight CPUs were measured at 5.9 CPUs while other programs took
the rest, which a line at "all but two" would have missed by a hair. The
CPUs counted are those the process may use, which in a container with a
CPU limit is fewer than the machine has.

What this does not do: only the CPU is tested, so a slow disk or network is
not noticed. A workload that leaves threads or child processes running
after it returns makes the machine read as busy. With fewer than
four CPUs on macOS, and on Windows, load inside a run is not seen. On
Linux the time the process spent ready and not running does not need the
second process, and with three CPUs such load was still seen for a
workload on one thread; one that uses half the CPUs or more is not watched
there at all. That rests on three runs of one test in a container limited
to three CPUs. A first version
of the second check, a thread that computed in short pieces with pauses,
read the same under heavy load as under none, because a thread that mostly
sleeps is served first; that is why the process computes without pause.
And the limits are choices: a machine at 9% passes.

### What contradicts a decision

The verdict is `contradicted`, exit 1, when:

- the decision is `port` for speed and `p`, or its upper bound, is under
  20% (the workload could gain at most a quarter);
- the decision is `port` for speed and the need cannot be reached: the
  workload must take at most some time, and it would take longer than that
  even if the candidate's own Python cost nothing. Or the need is already
  met;
- the decision is `port` for speed and the author says a better algorithm
  exists in Python and has not been tried;
- the decision is `port` for speed and the alternative in Python takes half
  the original's time or less: make the change, then decide on the improved
  code;
- the decision is `fix_algorithm_first` and the alternative measured is not
  faster (over 90% of the original's time);
- the only goal of a port is memory safety. Python has it.

It is `incomplete`, exit 1, when the candidate never ran on the main
thread, when the decision, the answer to "language or
algorithm" or a reason of three words is missing, a port names no
candidate or goal, a port for speed declares no need, the candidate of a
port for speed uses a compiled package and its share is not bounded from
below (see above), the candidate was never called, the workload takes under
0.2 seconds, or the profile saw none of the project's code.

It is `unreliable`, exit 1, when the machine was busy (see above). That
comes before everything else.

Otherwise it is `consistent`: no measurement contradicts the decision.
**That is not "supported", and not "right".** `do_not_port` is never
contradicted: there are good reasons not to port the hottest function.

**Why refuse and not only flag.** The decision is what this step hands to
the next ones, and a flag travels with it unread. The cases refused are
those where arithmetic on the tool's own measurements says the stated goal
cannot be reached; everything that needs judgment is a flag.

### Flags

A candidate entered a thousand times or more for under a microsecond each
(port the loop that calls it); ten thousand or more Python objects crossing
in a typical call; a candidate that uses a compiled package; a stand-in
workload that is slower than the original or removes under 5% of the time;
more CPU time than wall time while the candidate runs in a stand-in
workload;
a statement that the candidate uses no compiled package; a port
that by the declared runs would save under a minute a day; plain runs left
out because the machine was busy; a machine with fewer than four CPUs, or
a workload that used half of them or more, so that load inside a run was
not watched; a fifth of the time or more in library
Python; an instrumented run at least
twice as slow as a plain one; more CPU time than wall time; child processes
at work; a port whose goals do not include speed.

Every flag has a kind and says whether it was judged by a time measured in
this run (`flag_kinds` in the report). Those may be present on one machine
and absent on another, and the flag for a machine with too few CPUs is
booked with them for that reason; the others come from counts and
declarations.

## What the report does not measure

Printed in every report:

- load that came and went inside one run, and slow disks and networks;
- whether the workload is representative;
- **compiled work reached without a function call.** Arithmetic on large
  integers, string building, and operators on objects of compiled packages
  are counted as time of the Python function they are written in, and no
  instrument used here sees them. Such a function looks like the best
  candidate, and a port of it gains nothing;
- compiled work reached by calling an object that is not a built-in
  function, on a Python before 3.12;
- every use of a compiled package: see the five places above and what they
  miss;
- whether the stand-ins are faithful;
- work in other threads, in child processes and on a GPU. To a comparison
  of wall time with CPU time, a child or a GPU at work looks like waiting;
- whether the cost is the language's or the algorithm's;
- what a port would cost, and what the boundary would cost;
- whether static types are worth a port.

## Limits

What this tool cannot do, for someone deciding how far to trust a report.

> **In a project that loads a compiled package such as NumPy, this tool
> cannot show that a candidate's time is its own Python.** A port for speed
> that passes there rests on stand-ins the author wrote or on a sentence
> the author typed. The verdict says which; read that before the number.

**The verdict tells you what a "pass" rests on.**

| verdict | a port for speed passed because |
|---|---|
| `consistent` | no compiled package from outside the standard library is loaded, and nothing measured contradicts the port |
| `consistent_if_the_stand_ins_are_faithful` | the candidate uses such a package, and a stand-in workload written by the author shows the time is the candidate's own. The tool checked the stand-ins in part |
| `consistent_on_the_authors_word` | such a package is loaded, the tool saw the candidate use none, and the author states that it uses none. Nothing checked that |

In none of the three does it mean the decision is right.

**What the author supplies and nothing checks.**

- **The workload.** A small input, an unusual one, or one chosen so that
  the candidate dominates. The report prints the file, its hash, the
  function, the arguments, every run's time and the commit; nothing checks
  that production looks like it. The same goes for the alternative: an
  "improved" function that does less work is faster.
- **`project_root`.** Set narrowly, the rest of the project becomes
  "library" and the candidate's share of own Python rises; set widely, a
  vendored library becomes the project's own.
- **The need.** "It runs 100,000 times a day" by an author who did not
  count passes. A need in `runs_per_day` is never refused, only set beside
  the saving.
- **The language-or-algorithm answer.** "None known" by an author who did
  not look passes.
- **The sentence that no compiled package is used.** With it, a candidate
  whose array work the tool does not find gets a share and a ceiling as if
  it were pure Python. Each of those figures is printed with the condition,
  and the verdict is `consistent_on_the_authors_word`.
- **What the stand-ins replace.** The tool refuses the ones it can see
  through (see "Compiled packages"). One that keeps the package's work
  going by operators, on data kept where the tool does not look, passes
  with `consistent_if_the_stand_ins_are_faithful`.
- **Goals.** Leaving `speed` out of a port's goals avoids every time
  check; the report then flags that time says nothing about the goals.

**What the instruments do not see.**

- **Compiled work reached by an operator**: on arrays, on big integers, on
  strings. For a compiled package the tool falls back on bounds, stand-ins
  and the backstop. For big integers and strings there is no package to
  notice, and nothing is raised: read the candidate.
- **Every use of a compiled package.** The tool looks in five places and
  follows what objects refer to, up to 20,000 objects from each value. The
  walk stops at classes, so arrays kept as class-level constants and used
  through the class (`Cfg.A @ Cfg.B`) are not seen; in a run with NumPy
  that candidate got no finding of its own, and the backstop made the port
  `incomplete` until the author answered for it. An array that exists only
  inside one expression was missed the same way; so is anything further
  away than the limit.
- **Other threads, child processes, a GPU.** A candidate that runs only in
  another thread gets no share and the report is `incomplete`.
- **Load inside a run**, with fewer than four CPUs on macOS, on Windows, or
  when the workload itself uses half the CPUs or more: other programs that
  take the CPU only while such a workload runs are not noticed. With fewer
  than four CPUs, and for such a workload, the report says that it could
  not watch; on Windows it says nothing. Nor are other programs that leave one CPU
  free the whole time: the loops are one thread, so a workload that wants
  every CPU can lose a third of them unnoticed (measured: eight threads
  beside four busy programs on eight CPUs). With such programs on part of
  the CPUs a threaded workload's time came out about 30% long, and the
  share of a Python candidate in it two to three points low, in reports
  with no flag (Linux; on macOS the same load was caught). The share is
  understated by this, never overstated. Slow disks and networks,
  anywhere.
- **The runs that count what crosses the boundary, and the stand-in's run
  that times calls into compiled code, are not judged for load.** They
  give counts, and one ratio that load can only lower.
- **A workload that behaves differently when measured**, or that forges a
  result: it runs in the measuring process and is trusted.
- **A workload that cannot be loaded twice.** The tool loads the workload
  file under a name of its own, so a function defined in that file cannot
  be sent to a process pool.

**Choices that could have been made otherwise.**

- **Thresholds.** A piece at 21% passes the check that one at 19% fails.
  The limits are listed in every report under `limits_used`. All of them
  were chosen by the tool's author; none is derived from a measurement of
  what a faithful stand-in or a quiet machine looks like in general.
- **`--allow-dirty`.** The report says the tree was not clean.
- The bounds for a candidate that uses a compiled package come from runs
  under different instruments and are good to a point or two.

## Design choices

**A fourth program.** It measures Python by running Python, and shares only
the commit, the dirty check and hashes with the others, which it takes from
the harness's library.

**`cProfile`, from the standard library.** It tells Python functions from
compiled ones and records callers. A sampling profiler would disturb less,
but the standard library has none, and installing one into the project
under measurement is not this tool's place.

**Totals from plain runs, shares from the profile.** A deterministic
profiler slows code that makes many small calls far more than code that
makes few. One profile timed by the wall clock is used; a second, timed by
the CPU clock, was tried to attribute waiting to functions and dropped: it
made one example fifteen times slower.

**A workload file, not a command.** The file is committed and hashed, its
function is called in the tool's own process arrangement, and the arguments
are data.

**Dependencies:** `serde`, `serde_json`, the harness library.

## Tests

```sh
cargo test --locked -p carcinify-decide -p carcinify --lib --test decide
```

The files in `tests/` run the `carcinify` binary, so they are built as test
targets of the `carcinify` crate (see `carcinify/Cargo.toml`); the unit
tests are in this crate.

`tests/cli.rs` runs the real binary on a small Python project in a
temporary git repository: a contradicted decision, missing parts, a
workload that fails, one that shows nothing, a dirty tree, the algorithm
question, the need, the stand-ins, the token, and the flags. Several tests
use a package made to look compiled to the interpreter and say so; one of
its types is a number whose arithmetic really is compiled work reached by
an operator. Two tests start three busy programs for every CPU and expect
`unreliable`: one for the whole measurement, one only while the workload's
function runs.

A test that gets `unreliable` where it is testing something else runs the
tool again, up to six times, and then fails saying the machine is too busy.
Each repeat is printed, and written to the file named in
`CARCINIFY_DECIDE_ATTEMPTS` when that is set; `ci/run-all.sh` records the
total. A test whose workload runs for less than the tool's minimum fails
saying so: a faster interpreter has twice made a test workload too short.

One test uses the real NumPy and is ignored by default:

```sh
python3 -m venv target/numpy-venv && target/numpy-venv/bin/pip install numpy
CARCINIFY_NUMPY_PYTHON=$PWD/target/numpy-venv/bin/python \
    cargo test --locked -p carcinify --test decide -- --ignored
```

It was run with NumPy 2.5.4 on Python 3.12 (macOS arm64).
