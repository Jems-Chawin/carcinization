# Step 4: what is known, and how well

For someone deciding how far to trust `STEP.md`. The agent following the step
does not need this file.

- **That the pattern can be built, and what it costs: measured, on two
  small hand-written samples.** In this step's first example the
  counter-example allocates 708 times per tick against 0 for the designed
  port, with identical results. Clippy's default lints report nothing on it.
  Both samples were written by a person to show the pattern.
- **That agents produce it when porting: reported by others, and not
  shown here.** Several practitioners describe it in public discussion, and
  one disagrees that it is a problem. Those are accounts, not measurements.
  This project's own two attempts to see it are the next two points: none
  in eight ports, and one in four by a rule that calls one inconclusive.
- **This project's own experiment did not see it.** Eight ports of a
  130-line program by one model, four with this step and four without. All
  eight chose a single owner with indices and allocated nothing on the hot
  path. The task named the hot path and its skeleton already gave the single
  owner, so it could not have told the arms apart. See `experiment/`.
- **A pilot on a task written to invite it: inconclusive.** The pattern
  was observed in 1 of 4 runs on one task written to invite it, with one
  model; by the rule fixed in advance, that is inconclusive. Two things
  the pilot's account adds. What separates that one run from the other
  three is spelling, not design: more back-references and handles cloned
  where they are used, and no run has an `Rc<RefCell<..>>`. And no run met
  a borrow error and worked around it: each wrote the port in one go and
  it compiled, so none shows a port that gets past the borrow checker with
  copies. The runs were made without this step. See
  [`pilot/README.md`](pilot/README.md). Its
  numbers are those of the structure tool at the time; the tool has since
  been changed to count three things those ports showed, and gives higher
  S7 counts for the four runs now (see step six's `EVIDENCE.md`). The
  result was judged by the rule and the tool fixed before the runs.
- **What the step was observed to change:** all four runs given it wrote an
  allocation test with a budget stated beforehand, and no run without it
  did. In two of those four the test found a queue that still allocated
  late in a run, which nobody else looked for.

So the step does not claim to prevent the pattern. What it has been seen to
do is leave a decision and a measurement that a reviewer can check. Whether
it changes the design of a larger port is not known.

The description above fits sources in garbage-collected languages, and only
Python has been tried. C and C++ sources fail differently (the ownership is
there but unwritten, and the difficulty is finding it), and this step has
not been used on them.
