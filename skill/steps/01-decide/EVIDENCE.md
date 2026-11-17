# Step 1: what is known, and how well

For someone deciding how far to trust `STEP.md` and the tool behind it.

- **That teams port code whose time is spent waiting, or was the
  algorithm's: two first-hand accounts and one public statement.** The two
  accounts are from other projects' private work, read by this project's
  lead, and are not reproduced here. One team that did port found that most
  of the gain came from a change of design that could have been made in the
  source language. One that did not port found nearly all of the time was
  waiting on other services. In public, a member of the TypeScript team
  wrote of a Rust port of the compiler that "a lot of the speed does not
  just come down to language choice" and that optimisations found in the
  port were going back into the original ([Hacker News item
  50165953](https://news.ycombinator.com/item?id=50165953), 2026-10-20; a
  comment, identity as stated by the commenter, read by this project's
  auditor).
- **That the boundary can cost more than the port saves: one authors' own
  benchmark.** ScyllaDB's write-up of a Rust-based Python driver
  ([2026-09-28](https://www.scylladb.com/2026/09/28/building-a-faster-rust-based-python-driver/))
  reports calls through PyO3 with small arguments at about the speed of a
  Python call, and a call passing a large list, string and bytes several
  thousand times slower than its Python equivalent. It is their benchmark,
  not repeated by anyone here. The step's advice (fewer, larger calls;
  build large data on the Rust side) is theirs.
- **That a port can lose its speed-up by holding the interpreter's lock for
  a whole call** is widely said. The one account this project has of it
  comes from a search summary of a company's blog that nobody here opened.
  It is left out of the step.
- **That the tool tells the three kinds of workload apart: measured, on
  three small examples written for the purpose.** See `examples/`. They are
  not real projects.
- **That the same workload is classified the same on a second run:
  measured on those examples**, each run twice; the figures are in
  `examples/README.md`.
- **The thresholds** (a fifth of the time to port for speed; half the time
  for "the algorithm removed most of it"; 20 microseconds between a short
  compiled call and real compiled work; half the time to name a workload)
  are first choices and have not met a real project.
- **How a function's time is split among what it calls** is the
  approximation of call-graph profilers. Not checked against a tracing
  profiler.
- **The profiler's bias** towards code that makes many small calls is real
  and its size is printed (how many times slower the instrumented run was).
  How far it moves the shares was not measured.
- **That the profiler books work inside a compiled package to the Python
  function that asked for it: measured, with NumPy 2.5.4 on Python
  3.12.** Twenty products of 1,600 by 1,600 matrices, written `a @ b` and
  written `np.dot(a, b)`: in both forms `cProfile` gave the function over
  90% of the time as its own Python. The first version of this tool
  reported that as a candidate with a ceiling above ten and passed a port
  for speed; the project's auditor found it. Calls are affected as well as
  operators, because most of NumPy's functions are objects, not built-in
  functions, and the profiler reports built-in functions only.
- **What the tool does about it now: tested with the real package** (one
  test, ignored by default, run on that NumPy and Python on macOS arm64)
  and with a package made to look compiled (three tests). With the call
  form, `sys.monitoring` times the calls and the port is contradicted.
  With the operator form nothing sees the work, the report gives no share
  and no ceiling, and the port is incomplete until a stand-in workload is
  named; with one, it is contradicted. A candidate of pure Python in the
  same file is given its share as before.
- **`sys.monitoring` was tried, not read about:** on Python 3.12 it
  reported calls of `np.dot`, `np.add`, array methods, a compiled type and
  a slot wrapper, and nothing for `a @ b` or `a + b`. Not tried on 3.13 or
  3.14 with NumPy, or on other packages. The tool takes the time of Python
  that runs beneath such a call (a callback, a generator being consumed)
  off the call's time; that path has no test.
- **Finding the use of a compiled package: tested for nine routes.** A
  name in the candidate, a helper in another function, an argument, an
  attribute of an argument, a module-level constant, an object with
  `__slots__`, a closure, a deque a global names, and the first of 30,001
  things in a list; the constant and the slots also with NumPy. Earlier
  versions missed the last six, and the auditor showed a port of array
  code passing through four of them. The tool now follows whatever an
  object refers to (`gc.get_referents`), nearest first, up to 20,000
  objects from each value, in place of a list of container types.
- **What it still misses: run with NumPy.** An array that exists only
  inside one expression (`pickle.loads(BLOB) @ pickle.loads(BLOB)`): the
  tool printed 89% for a candidate whose own Python is about a tenth of
  its time. A port for speed was stopped only by the backstop (the package
  is loaded and nothing was seen). When the author stated, falsely, that
  no compiled package is used, it passed, and the verdict then is
  `consistent_on_the_authors_word` with the condition on every figure.
  That case is a test. This is where the tool stops: a layout that escapes
  detection is a stated limit, not a defect to be closed one at a time.
- **Stand-ins** are an idea from the project's auditor. With honest ones
  the auditor found the bounds held the true share, or came within two
  points, in seven cases where the split was known. Passed by earlier
  versions and refused now, each tested with NumPy: a stand-in that is the
  workload again, one that leaves half of the arrays in place, one that
  passes the same arrays as views, and one that does so for half of them.
  What is refused is what can be seen: the same function, a candidate that
  is not called, objects of the package that address more than 64 KiB each
  or 1 MiB together, long calls that remain. More CPU time than wall time
  while the candidate runs was a refusal too, first for "any more", then
  for more than 1% of the original's time, and both refused honest
  stand-ins with NumPy's default threads on Linux, where the library's
  idle threads spin after a product (the first found by the auditor, the
  second by the Linux run of the test written for it: 0.115 s over, in
  0.152 s, against a workload of 7.1 s). Measured in the Linux container,
  8 CPUs, seven rounds each: after threaded products the process used
  0.35 s more CPU than wall time over the next 0.05 s of plain Python,
  0.53 s over the next 0.15 s, and the same 0.53 s over 0.5 s and over
  2 s (largest 0.61 s), for arrays of 400, 1,600 and 2,400 a side; with
  one thread, none. It is a fixed cost, so it is now a flag with the
  figures and refuses nothing. No test asserts that the idle threads
  spin: on a busy machine they did not, so the test with real NumPy
  checks only that the flag agrees with the report's own figures. The
  figures are for one entry of the candidate after a product; with a
  product before each of 40 entries the auditor measured 0.3 to 0.6 s in
  all, so it is not a cost for every entry. The flag's stronger wording
  goes by the surplus for each entry (over 1 s), and threaded work left in
  a candidate entered 400 times stayed under that line with 42 to 64 s in
  all, 5 times of 5. A stand-in with products of large arrays
  hidden behind a small object is still refused, by the size of the
  arrays found where the code that runs inside the candidate names them.
  A stand-in that keeps the package working on
  data the tool does not reach would pass; not run. Nobody has written
  stand-ins for a real program.
- **A candidate that runs only in another thread** was reported as 0% and
  the port as contradicted; the auditor found it. It is now incomplete,
  with no share. Tested with a thread started by the workload.
- **That a busy machine makes pure computation read as waiting: seen
  twice on the development machine**, as 40% and as 31% waiting with a load
  average of 12 to 20 on 8 CPUs, and reproduced by the auditor (70% waiting
  with three busy programs per CPU). The loop timed around each run read
  about 1% on a quiet machine and 4% to 80% under load.
- **Load only inside a run** is not seen by those loops; the auditor showed
  70% "waiting" passing as consistent on macOS. A process that computes
  beside the measured one now sees it: one test puts three busy programs
  per CPU inside the workload's function and gets `unreliable`. A first
  attempt, a thread computing in 2 ms pieces with 8 ms pauses, did not
  work: its pieces took as long with 24 busy programs as with none. The
  limits (10% and 15%) are first choices; on a machine with ambient load
  the watching process read between 1% and 27% with nothing added.
- **A workload that uses many CPUs itself** was reported as a busy
  machine in some runs and not in others (NumPy's matrix product on Linux,
  found by the auditor). Runs that use half the CPUs or more are now not
  judged by the process beside them. Measured on 8 CPUs, Linux arm64
  container and macOS, on 2026-11-16, each figure from two reports:
  - Four products of 3000 x 3000 matrices on all cores, in the container:
    5 reports of 5 `consistent`, each with the flag that the run could not
    be watched; the process used 6.9 CPUs and the one beside it lost 2% to
    13%. With `OPENBLAS_NUM_THREADS=1`: 2 of 2 `consistent`, no flag, 1.0
    CPU, nothing lost.
  - On a machine with nothing added, threads that hash (1 to 8 of them):
    the process beside them lost under 1% up to 3 threads on macOS and up
    to 7 in the container; 3% to 12% beside 4 to 7 threads on macOS, where
    other programs were using about two CPUs; 15% to 22% beside 8 threads
    on macOS and 5% to 7% in the container. Eight threads got 6.3 to 6.6
    CPUs, never 8, and once 5.9: that is why the line is not "all but two".
    Half is a choice; these figures would also allow a line at 5 of 8.
  - One plain-Python thread beside programs that never wait, started
    before the tool: with as many as CPUs, or twice as many, `unreliable`
    in 8 reports of 8, by the loops and by the process beside the run
    both. With half as many as CPUs, `consistent` in 4 of 4, and rightly:
    the workload still got 0.98 to 1.0 CPU and the process beside it lost
    at most 5%. Nothing was taken from it.
  - **Not caught:** eight threads beside four such programs got 4.3 to 4.6
    CPUs where they get 6.3 to 6.6 alone, and 3 reports of 4 said nothing
    (the fourth caught it in a run that fell under the line). The loops
    before and after are one thread and found a free CPU; the process
    beside the run lost 14% to 42% and was not used. Five threads beside
    four programs, under the line: caught 2 of 2 on macOS, 0 of 2 in the
    container, where the process beside lost 3% to 18%. So load that takes
    part of the machine from a workload that wants all of it is seen only
    when it is heavy. Beside eight outside programs every case above was
    `unreliable`.
- **The stated need** is a design choice, not a finding. No real decision
  has been written with it.
- **That operators on big integers and strings are booked to the Python
  function** follows from how the profiler works and was not demonstrated.
  That the standard library's JSON encoder is booked to library Python was
  seen in the second example.
- Only Python has been tried: Python 3.9 and 3.12 on one macOS machine,
  and 3.13 in a Linux arm64 container.
