# Experiment: does the ownership step change what an agent writes?

Written and committed **before any run**. Results are reported against this
file, in `README.md` beside it.

## Question

Given the same porting task, does an agent that is handed the step's text
(`../STEP.md`, `../sources/python.md`, `../design-note-template.md`) produce
a port with a different ownership structure from an agent that is not?

## Task

`task/TASK.md`, with `task/relay.py` and the crate skeleton in
`task/skeleton/`. Both arms get exactly these. Both are told, in `TASK.md`,
that `tick` is the hot path, that it is called millions of times and that
the reason for the port is speed. So the comparison is not "told that
performance matters" against "not told"; it is "told the goal" against "told
the goal and given the method".

The program is not the worked example and shares no code with it. Neither
arm sees the worked example, this directory's `measure/`, or this file.

### Does the task contain the trap?

If it did not, both arms would do equally well and the experiment could show
nothing. Checked two ways before any run:

- By reading `relay.py`: `Node.hub` is a back-reference to the container,
  and `Node.pump`, which is the body of the hot loop, uses it to **mutate**:
  `self.hub.delivered[...] +=`, `self.hub.latency +=`, `self.hub.dropped +=`,
  `self.hub.traffic[...] +=`. It also mutates a sibling reached through
  `self.peers`: `peer.queue.append(packet)`, `peer.load +=`. And `Hub.tick`
  iterates `self.nodes` while each node changes other elements of it.
- By compiling a line-for-line Rust transcription of that hot path,
  `measure/trap/literal.rs`: rustc 1.100.0 rejects it with three borrow
  errors (two E0499, one E0502), recorded in `measure/trap/errors.txt`.

## Arms

- **control**: the prompt in `arms/control.md`.
- **step**: the prompt in `arms/treatment.md`, with a `step/` directory
  holding the three files named above.

`{DIR}` in a prompt is replaced by the run's working directory. Nothing else
differs.

## Runs

- Four per arm, eight in all. Reason for four: the brief's floor is three;
  one more lets a single odd run be seen as odd, and the cost stays about
  that of the rest of the task. Four per arm cannot establish a general
  effect and nothing below claims it can.
- Model: Claude Fable 5.2 for every run, one fresh sub-agent per run with no
  shared context, each in its own directory outside the repository. Runs are
  launched together; none is rerun or replaced. A run that fails or does not
  finish is reported as such and counts as stated below.
- Sub-agents are told to stay in their directory. That is an instruction,
  not an enforced sandbox; each run's report is checked for signs that it
  read anything else.
- Afterwards each run's crate is copied unchanged into `runs/<arm>-<n>/relay`
  and committed, together with its `REPORT.md` and, for the step arm, its
  `DESIGN.md`.

## Measures

For every run, by `measure/measure.py`:

1. **Equivalent?** `rustify-diff` against `relay.py` on
   `measure/cases.jsonl` (31 cases), no relaxations. A run that does not
   compile, or is not `pass`, is **failed**.
2. **Allocations per tick in steady state** (S1): 32 nodes, seed 7, counted
   over 2,000 ticks after 2,000 of warm-up, by `measure/hot_path.rs`. The
   step's text names this measure, so the step arm knows it will be looked
   at.
3. Measures the step's text does **not** name, to see whether the design
   changed or only the counted number:
   - **nanoseconds per tick**, release build, best of five passes of
     200,000 ticks, same test;
   - **bytes requested per tick** (S2), same test.
4. **Counts in the functions reachable from `tick`** (`measure/count.py`):
   `clone`/`cloned`; `collect`; `to_vec`/`to_owned`/`to_string`;
   `mem::take`/`replace`/`swap`/`Option::take`; `Rc`, `RefCell`, `Arc`,
   `Mutex`, `Cell` and their borrow calls; containers created in the body.
   The step names the first four groups and `Rc<RefCell>`/`Arc<Mutex>`.
5. For the step arm: the **allocation budget its `DESIGN.md` states**, to
   set against the measured number (check S8).
6. The run's own `cargo fmt --check`, `clippy -D warnings`, `cargo test`.

A run is **low-allocation** if it is equivalent and allocates fewer than 1
time per tick on average. (A tick injects three packets into queues that
already exist and moves packets between them; once the queues have grown to
their working size nothing needs the heap. Below 1 allows for a queue still
growing now and then.) Otherwise an equivalent run is **allocating**.

## Decision rule

Let L(arm) be the number of low-allocation runs out of 4, and F(arm) the
number of failed runs. Time comparisons use the median nanoseconds per tick
of the arm's equivalent runs.

- **"With the step, more runs were low-allocation"** if
  L(step) - L(control) >= 2 and F(step) - F(control) <= 1.
  - If, in addition, the step arm's median time is at least 10% lower than
    the control's: **"and the ports were faster on a measure the step does
    not name"**.
  - If the median times are within 10% of each other, or the step arm's is
    higher: **"the counted number moved; the unnamed measures do not show a
    different design"**.
- **"No difference observed"** if |L(step) - L(control)| <= 1 and
  |F(step) - F(control)| <= 1. If L is 4 in both arms this is reported as
  **"the control arm did not produce the pattern, so this experiment could
  not show an effect"**, which is a statement about this task and this
  model, not about the step.
- **"With the step, results were worse"** if L(control) - L(step) >= 2, or
  F(step) - F(control) >= 2, or L is equal in both arms and the step arm's
  median time is more than 1.5 times the control's.

Whatever the outcome, every run is listed with every measure, and the
wording is "observed in 4 runs per arm on one task with one model".

## What this cannot show

- Whether the effect holds for other programs, larger ports, other models
  or longer sessions.
- Whether the design note was written before the code. File times are
  recorded; they are weak evidence.
- Anything about maintainability. The measures are costs of the hot path
  and counts of constructs.
