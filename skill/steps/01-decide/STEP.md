# Step 1: Decide whether to port, and what

Do this before anything else. A port is expensive, it adds a boundary
between two languages that has to be kept for good, and a large share of
the ports that disappoint were never going to help. "Do not port this" is
an ordinary, respectable result of this step.

How far each claim here is supported is in `EVIDENCE.md`.

## What you hand over

- `decide.json` and the report written by `carcinify decide`, committed.
- The decision, with its reason: port this piece; fix the algorithm in the
  source language first; do not port.
- If the decision is to port, for the later steps:
  - **the hot path and how it was found**: the candidate function, the
    workload, the share it holds (step four's design note starts here);
  - **the size of the profiled workload** (step six measures at that size);
  - **the candidate's files** (step two looks up what they import).

## What to do

### 1. Choose a workload that is the real work

The whole step rests on this and nothing can check it for you. Take a job
the program really does, at a size it really sees: a day's file, a typical
request repeated, the slow case someone complained about. Put it in a file
as a function that takes plain arguments. Say in the declaration what it is
and why it is representative.

A workload chosen because it makes one function look hot gives a true
profile of the wrong thing.

### 2. Run the tool and read where the time goes

```sh
carcinify decide decide.json --report report.json --markdown report.md
```

The first table splits the time four ways. If the project loads a compiled
package, the line above it says that the "own Python" share may include
that package's work: with no candidate named yet, nothing has looked.

- **Mostly waiting** (a network, a database, a model, a disk). Stop.
  Another language waits just as long. What helps is asking less often, in
  larger batches, or at the same time, and all of that can be done where
  the code is.
- **Mostly compiled code.** The work is already done by compiled
  libraries. A port replaces the thin Python around them and gains that
  thin part. Look at how the library is used (the options, the sizes, how
  often) before anything else.
- **Mostly library Python.** The time is in someone else's Python. Whether
  a Rust crate does that job is step two's question, and it is a different
  port: the library, not your code.
- **Mostly the project's own Python.** Now there may be something to port.
  Go on.

**If the verdict is `unreliable`, stop and measure again.** The machine
was busy, and on a busy machine pure computation reads as waiting. Do not
quote a figure from such a report.

Read the limits printed with the report. Work done inside a compiled
package (a numerical library, an image library) is counted as the Python
function that asked for it, when it is reached by an operator (`a @ b`) or
by calling one of the package's objects (most of a numerical package's
functions are such objects, not plain functions). A function of array
arithmetic looks like the hottest code in the project and a port of it
gains nothing.

### 3. Pick the candidate: the smallest piece that holds the time

From the table of the project's own functions, look for one that

- holds a real share of the time together with what it calls,
- has few callers, so the boundary is in one place,
- is called few times with much work per call, and
- takes in and gives back data that is cheap to hand over.

Name it in the declaration and run again. The report gives its share `p`
and **the ceiling `1 / (1 - p)`**: how much faster the whole workload could
be if the piece cost nothing. A piece that is 30% of the time gives at most
about 1.4 times; one that is 80% gives at most 5. A real port gains less
than the ceiling. If the ceiling is not worth the work, the decision is
made.

**If the candidate uses a compiled package**, the report gives no share and
no ceiling, only an upper bound, and a port for speed stays incomplete
until you show where the time is by another measurement. Two ways:

- **Stand-ins.** Write a second workload that does the same with the
  package's work replaced by stand-ins of the same shape: an object whose
  `@` returns a small array of the right kind without computing it, a
  function that returns a ready-made result. **Replace the objects
  themselves, every one**: an array left in place still does the package's
  work whenever an operator touches it, and nothing sees that. Keep
  everything else, and above all keep all of the candidate's own Python.
  Name it in the declaration (`candidate.with_stand_ins`); the tool times
  it and gives bounds. If the time goes away with the stand-ins, it was the
  package's, and the answer is "do not port". The tool refuses a stand-in
  workload that is the workload again, that still hands the candidate
  large objects of the package, or that does not call the candidate. It
  cannot check the rest, so a port that passes this way gets the verdict
  `consistent_if_the_stand_ins_are_faithful`: whoever reads the report has
  to read the stand-in workload too.
- **A line profiler**, in a scratch environment, not in the project's own:
  it shows the time line by line, and a line that is one array operation is
  the package's time. The tool takes no figure from it. Use it to decide,
  and write in the decision's reason what you ran and what it showed.

Record which of the two you did.

**If the project loads a compiled package and the tool saw the candidate
use none**, a port for speed still stays incomplete until you answer for
it, because the tool does not find every use. Read the candidate and what
it calls. If any array, frame, tensor or image reaches it by any route,
write stand-ins. If none does, say in `candidate.uses_no_compiled_package`
what it works on instead. That sentence is printed as yours, and it is the
only thing between a wrong number and the reader. A port that passes on it
gets the verdict `consistent_on_the_authors_word`, and the share is printed
with "if the candidate uses no compiled package, which the author states
and the tool has not checked".

**Hand over the verdict word for word.** `consistent`,
`consistent_if_the_stand_ins_are_faithful` and
`consistent_on_the_authors_word` are three different things, and whoever
reads your decision needs to know which one they are looking at before
they look at any number.

Then read what would cross the boundary. Every call from the source
language into Rust has a fixed cost, and every object passed has to be
converted unless the design avoids it. **Many small calls** cost more at
the boundary than they save: port the loop that makes them, not the
function it calls. **Large structures of small objects** (a list of a
hundred thousand records) can cost more to convert than the port saves:
the design has to pass them in bulk, or keep them on one side.

### 4. Answer in writing: is it the language or the algorithm?

No tool can measure this. Look at what the candidate does and write the
answer down in the declaration.

- Is it doing something more often than it needs to: comparing everything
  with everything, re-reading, re-computing the same value, rewriting a
  whole file to add a line?
- Would the same code be slow in any language?

If a better algorithm exists, **make the change in the source language
first** and name it in the declaration; the tool measures it in the same
run and prints both. It is cheaper than a port, it can be reviewed by the
people who know the code, and it tells you how much of the hoped-for gain
was never the language's. Then run this step again on the improved code:
what is left is what a port can win.

A port that also changes the algorithm will be faster, and nobody will know
which change did it.

### 5. Decide, and write the reason

- **Port this piece.** For speed, when the time is the project's own
  Python, concentrated in a piece with a cheap boundary, no better
  algorithm is left, **and somebody needs the speed**. Write the need in
  the declaration: who waits for this or pays for it, in a sentence, and
  one number, either the time the workload has to finish in or how often
  it runs. The report sets it beside the best case: the time if the
  candidate cost nothing, and the saving per run. A high share is not a
  need. Half a second saved on something nobody waits for is not worth a
  boundary between two languages. Or for static types and errors caught at compile time,
  where the piece is central enough for that to matter; say so, and know
  that the time measurements say nothing for or against it.
- **Fix the algorithm first**, and come back.
- **Do not port.** With the reason. Most code should get this answer.

Memory safety is not a reason on its own: the source language already has
it.

The tool sets the decision beside the measurements. It refuses a few that
its own numbers rule out (a port for speed of a piece under a fifth of the
time; one whose stated need cannot be reached even if the piece cost
nothing, or is already met; a port for speed while a better algorithm is
untried, or after one has been shown to remove most of the cost). It leaves
a port for speed incomplete when no need is stated, when the piece uses a
compiled package and nothing shows how much of its time is its own, or when
such a package is loaded and nobody has answered for the piece. It passes everything else as
"consistent", which means only that nothing measured contradicts it.

## What this step does not do

- It does not check that the workload is the real work.
- It does not see threads other than the main one, child processes, or a
  GPU.
- It does not see compiled work reached by operators, and it does not find
  every use of a compiled package. Read the candidate.
- It does not check that stand-ins are faithful or that a stated need is
  true.
- It does not estimate what the port will cost to write and to keep.
- It measures one machine. Give the machine with any figure you quote.
