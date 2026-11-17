# Experiment results: the ownership step, with and without

**A record, kept as it was.** When this experiment was run the tools were
separate programs called `rustify-diff` and `rustify-structure`; they are
now `carcinify diff` and `carcinify structure`, and their file formats
carry the new name. The prompts, the reports and the scripts here keep the
old names and would need them changed to run again.

Reported against `PROTOCOL.md`, which was committed (`a14936b`) before any
run. Runs are in `runs/`, raw numbers in `results.json`, produced by
`measure/measure.py` on the clean tree at commit `3ff0eac`.

## Result

**The control arm did not produce the pattern, so this experiment could not
show an effect.** That is the outcome the protocol names for L = 4 in both
arms. It is a statement about this task and this model, not about the step.

Observed in 4 runs per arm, on one task, with one model (Claude Fable 5.2):

- All eight ports are identical to `relay.py` under `rustify-diff`, 31 of 31
  cases, no relaxations.
- All eight allocate 0 times and request 0 bytes per tick in steady state.
- In the functions reachable from `tick`, all eight have 0 `clone`, 0
  `collect`, 0 `to_vec`/`to_owned`/`to_string`, 0 `mem::take`/`replace`,
  0 `Rc`/`RefCell`/`Arc`/`Mutex`, and create no container.
- All eight made the same ownership decision without being told to: the
  simulation owns everything, a node is an index, peers are indices, packets
  are small `Copy` values in per-node `VecDeque`s, and the back-reference to
  the hub is gone.

Decision-rule quantities: L(control) = 4, L(step) = 4, F(control) = 0,
F(step) = 0.

## Every run

| Run | Equivalent (harness) | Alloc/tick | Bytes/tick | ns/tick | Hot path: clone | collect | to_vec etc. | take/replace | Rc/RefCell/Arc/Mutex | containers made | Lines | fmt, clippy, own tests |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| control-1 | pass, 31 of 31 | 0.0 | 0.0 | 303.3 | 0 | 0 | 0 | 0 | 0 | 0 | 260 | yes/yes/yes |
| control-2 | pass, 31 of 31 | 0.0 | 0.0 | 304.4 | 0 | 0 | 0 | 0 | 0 | 0 | 230 | yes/yes/yes |
| control-3 | pass, 31 of 31 | 0.0 | 0.0 | 562.6 | 0 | 0 | 0 | 0 | 0 | 0 | 245 | yes/yes/yes |
| control-4 | pass, 31 of 31 | 0.0 | 0.0 | 429.1 | 0 | 0 | 0 | 0 | 0 | 0 | 238 | yes/yes/yes |
| step-1 | pass, 31 of 31 | 0.0 | 0.0 | 306.8 | 0 | 0 | 0 | 0 | 0 | 0 | 224 | yes/yes/yes |
| step-2 | pass, 31 of 31 | 0.0 | 0.0 | 285.5 | 0 | 0 | 0 | 0 | 0 | 0 | 228 | yes/yes/yes |
| step-3 | pass, 31 of 31 | 0.0 | 0.0 | 262.7 | 0 | 0 | 0 | 0 | 0 | 0 | 252 | yes/yes/yes |
| step-4 | pass, 31 of 31 | 0.0 | 0.0 | 318.8 | 0 | 0 | 0 | 0 | 0 | 0 | 237 | yes/yes/yes |

32 nodes, seed 7; allocations and bytes over 2,000 ticks after 2,000 of
warm-up; time is the best of five passes of 200,000 ticks, release build,
on one machine (macOS arm64). "Lines" is non-comment, non-test source.

## What differed between the arms

None of this enters the decision rule. It is what was seen.

- **Time per tick.** Median 366.8 ns for control, 296.2 ns for the step arm.
  The gap comes from two control runs, control-3 (562.6) and control-4
  (429.1), which added a bitset of non-empty queues so that a tick skips
  idle nodes. At 32 nodes that costs more than it saves; control-1 tried the
  same thing, measured it and took it out. This is an optimisation choice,
  not an ownership one. The other six runs are within 263 to 319 ns. The
  time was measured once per run on a machine doing other work; the first
  measurement gave the same ordering (376.2 against 298.3).
- **An allocation test.** All four step runs wrote a counting-allocator test
  with a budget. No control run did. The step asks for this, so it shows the
  text was followed, and two step runs found something with it: at 17 nodes
  a queue grew late and allocated 3 times in 200,000 ticks. step-1 kept the
  budget and made its test allow exactly that; step-3 reserved queue
  capacity up front. No control run looked.
- **A design note.** All four step runs wrote `DESIGN.md` with an inventory,
  the hot path, rejected alternatives and a budget of **0 allocations per
  tick after warm-up**. The measured number is 0 for all four, so check S8
  holds for each: stated budget 0, measured 0.
- **Whether the note came first.** Not verifiable. All four notes have a
  modification time later than `src/lib.rs`, because each was updated during
  the port ("Changes during the port"). Each run's report says the note was
  written first.

## Departures from the protocol

1. **Four runs were interrupted and resumed.** A usage limit on the account
   stopped control-1, control-3, step-1 and step-3 part-way. Each was
   resumed in its own saved context with one identical message ("Your
   session was interrupted by a usage limit. Continue the task from where
   you stopped."). The protocol said no run would be rerun or replaced; none
   was, but these four did not run in one piece. Two in each arm.
2. **`measure/count.py` was fixed after the first measurement.** It cut the
   source at the first `#[cfg(test)]`, which dropped most of step-1's
   source from its "Lines" figure (117 instead of 224). The hot-path counts
   were 0 for every run before and after. The fix is commit `3ff0eac` and
   all runs were measured again there.
3. **Two reports are missing.** step-1 and step-4 could not write
   `REPORT.md` (the file write was refused for a sub-agent). Their final
   messages are saved as `FINAL-MESSAGE.md` beside the crate.
4. **Two runs wrote outside their directory.** control-3 and control-4 each
   say they created and then deleted one temporary file in `/tmp`. No run
   reports reading anything outside its directory, and I did not check that
   beyond their reports.
5. **Two path strings in one report were changed afterwards.** In
   `runs/control-1/relay/REPORT.md` the scratch directory printed by cargo
   (two lines) was replaced with `<scratch>` before publication, because it
   showed a login name and local directory names. Nothing else in the
   recorded runs was edited.

## What this does and does not say

- It does not show that the step helps, and it does not show that it does
  nothing. The task did not separate the arms.
- The task does contain the trap: a line-for-line transcription fails with
  three borrow errors (`measure/trap/`). All eight agents avoided it at the
  design stage, before meeting an error. Two step runs say so in their
  reports ("no borrow error came up during the port").
- So, for a program of about 130 lines with one obvious back-reference and
  the goal stated in the task, this model chose a sound ownership structure
  unprompted, four times out of four.
- The pattern the step is for is described for long ports under pressure:
  thousands of lines, many types, errors arriving after the structure is
  already fixed. Nothing here tests that. A harder test would be a program
  several times larger with more than one back-reference and ownership that
  is ambiguous, a session long enough for the first design to be wrong, or a
  weaker model. I have not run any of those.

## Reproducing

```sh
# the task given to every run
cat task/TASK.md
# the two prompts; {DIR} is the run's directory, holding task/relay.py,
# task/TASK.md and the contents of task/skeleton/; the step arm also gets
# step/ with ../STEP.md, ../design-note-template.md and ../sources/python.md
cat arms/control.md arms/treatment.md
# measure the runs kept in runs/ (from the repository root, clean tree)
RUSTIFY_DIFF=$PWD/target/release/rustify-diff RUSTIFY_PYTHON=$PWD/.venv/bin/python \
RUSTIFY_MATURIN=$PWD/.venv/bin/maturin \
    python3 skill/steps/04-ownership-design/experiment/measure/measure.py measure target/t002-results.json
python3 skill/steps/04-ownership-design/experiment/measure/table.py target/t002-results.json
```

A new measurement goes to the file named on the command line.
`results.json` in this directory is the record of the original measurement
and is not overwritten; times differ from run to run.

The generated ports are kept in the repository, not only their hashes: they
are small, and the finding is about their structure, which a hash does not
show. Each sub-agent was a fresh Claude Fable 5.2 agent with no shared
context. Sub-agents in this environment also receive the account's standing
instructions; those were the same for both arms.
