# The skill, as it is being built

How the skill's files are laid out, for the people and agents working on
it. The overview is the README at the top of the repository; there is no
`SKILL.md` yet.

```
skill/
  steps/
    NN-name/              one directory per step, numbered in the order an agent meets them
      STEP.md             the instructions, true of any source language
      sources/<lang>.md   what is specific to one source language (python.md for now)
      *-template.md       anything the step asks the agent to fill in
      example/            a worked example small enough to read in one sitting
      example-*/          a further example for one point the first does not show
      experiment/         evidence that the step changes what an agent does
      EVIDENCE.md         how far each claim in STEP.md is supported, for a reviewer
      declarations/       (step 6) declaration files for the examples the tool is run on
harness/                  carcinify diff, the program step 3 uses (built first, lives at the top level)
structure/                carcinify structure, the program step 6 uses
deps/                     carcinify deps, the program step 2 uses
decide/                   carcinify decide, the program step 1 uses
examples/                 the harness's own worked example
```

The seven steps: 1 decide whether to port, 2 check dependencies on
crates.io, 3 compare against the original (`harness/`), 4 design ownership,
5 port a small piece through PyO3, 6 report on structure, 7 manage the
period when old and new run side by side. Steps 1, 2, 4 and 6 have directories
so far; the harness predates this layout and stays where it is.

Why this shape:

- **One directory per step** so a step can be read, tested and audited on
  its own, and the others can land beside it without touching it.
- **`STEP.md` apart from `sources/`** keeps what is true of any source
  language apart from what is specific to Python, so another language would
  add a file, not a rewrite.
- **The example is separate from the experiment** on purpose. The example is
  shown to agents using the step. The experiment's program must never be
  shown to the agents it tests.
- **Programs stay out of `skill/`** except as examples. A step that needs a
  tool (as step 3 needs the harness) points to it.
