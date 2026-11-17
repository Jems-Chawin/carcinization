# Step 2: what is known, and how well

For someone deciding how far to trust `STEP.md` and the tool behind it.

- **That an agent's claim of "no Rust crate" can be wrong: observed once
  here, reported by the project owner more often.** One of this project's
  own agents said there was no Rust crate for Google's Agent Development
  Kit or Gemini SDK. On 2026-11-14 the tool's searches of crates.io found
  a family of crates for the first (`adk-rust`, `adk-core`, `adk-agent`,
  `adk-model`, `adk-runner`, all at 2.3.0, one owner) and several clients
  for the second; none is tied to Google by evidence the tool can check.
  The report is in `adk-case/`. That is one case.
- **That crates.io holds crates not worth building on: observed in the
  worked example, not measured.** For a SOAP client the searches return
  four crates with 81, 63, 10 and 73 downloads in ninety days, one of them
  23 days old. The tool reports the figures; "not credible" is a reader's
  conclusion.
- **The 2026-08-20 attack** (malicious versions of `arrayref` and two other
  crates, online 86 to 107 minutes) is as the project's auditor read it
  from the Rust blog. Not re-read for this step.
- **The Cargo settings** (`registry.global-min-publish-age`,
  `registries.<name>.min-publish-age`, `resolver.incompatible-publish-age`
  with default `deny`, respected from 1.100) were read in the Cargo
  reference installed with toolchain 1.100.0 on this machine. Not tested by
  resolving against a young version. Whether `cargo install` honours it:
  the release notes and Cargo reference installed with 1.100.0 do not say.
  The announcement on the Rust blog is outside the hosts this work may
  fetch from, so the step repeats the auditor's reading of it and marks it
  as such.
- **The trusted-publishing record's shape** was read from six real crates
  on 2026-11-14 (UTC). One of them, `maturin`, is also a Python project hosted by
  the same organisation, and is the recorded test of the rule.
- **"Plainly alive"** (a release within 730 days and 1,000 downloads in 90
  days, or 100,000 downloads in 90 days alone) is a first choice; the
  second arm has been exercised only on an edited recording. On the PyYAML searches it marks 12 of the 19
  top results, the crate the worked example chooses among them, and leaves
  out the deprecated `serde_yaml`, which has the downloads and no release
  for 964 days. For the SOAP searches it marks none of 14.
- **The interpreter lookup** (`importlib.metadata`) is tested on a virtual
  environment made for the test, under Python 3.9, where the tool reads
  each project's file list because `packages_distributions` arrived in
  3.10. Not run on 3.10 or later, nor on a real environment with the four
  mappings the audit names.
- **That PyPI does not say which modules a project provides** was read
  from the JSON of two projects on 2026-11-14 (UTC): no such field.
- **What the crates.io API exposes** was read from live answers on
  2026-11-14: no field says whether a crate has a build script or is a
  proc-macro. The tool reads the docs.rs source view for those; checked by
  hand on five crates, right on five.
- **The classification rule** has been run on about forty crates. It gave
  `official` to `opentelemetry` (team owner). `polars`, whose crate is
  linked from the Python project's description, was `official` under the
  first rule and would now be `named_by_upstream`; not re-run. It gave `community` to `sentry` (owned by
  an account named `getsentry-bot`, repository link in `getsentry`), to
  `protobuf` (an owner is a team of `protocolbuffers`, but the Python
  project names no organisation on PyPI to compare with) and to `arrow`
  (owned by eight personal accounts). A reader may well take those three
  for the organisations' own crates; the tool could not show it. So
  `community` does not mean unofficial. This is the rule's known weakness.
- **The look-alike flag** is tested only on answers written by hand for a
  crate that does not exist. It has not met a real typosquat.
- **The minimum search for "none"** (two descriptive terms, top five of
  each search accounted for) is a first choice, not derived from data.
- **That keeping numerical and ML libraries in Python is usually right** is
  reasoning plus common advice, not a measurement made here.
- Only Python sources, PyPI and crates.io have been tried.
