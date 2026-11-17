# The case that started this: Google's ADK and Gemini SDK

An agent on this project said there was no Rust crate for Google's Agent
Development Kit or the Gemini SDK. `python/assistant.py` uses both;
`proposal.json` gives search terms and guessed names; `report.md` is what
`carcinify deps` found on **2026-11-14**. No crate name from earlier notes
was assumed: the candidates were taken from the tool's own search results,
plus one deliberate guess, `google-adk`.

The committed report is a replay of those answers, made when the tool was
renamed to `carcinify`: the report says "replayed from a cache", and every
fact and time in it is as fetched that day.

What the tool fetched:

- `google.adk` (PyPI `google-adk`, organisation `google`): outcome
  `community_crates`. A family of crates from one publisher, `adk-rust`,
  `adk-core`, `adk-agent`, `adk-model`, `adk-runner`, all at 2.3.0 released
  2026-10-09, first published 2025-11-30, about thirty releases in the last
  year, one owner. A separate crate `adk` with three versions and none in
  the last year. No crates.io owner is a team of `google`.
- `google-adk`, the name one would guess from the Python package: does not
  exist on crates.io.
- `google.genai` (PyPI `google-genai`, organisation `googleapis`): outcome
  `community_crates`. `gemini-rust`, `adk-gemini`, `gemini-genai`,
  `genai`, `google-genai` (two versions, none since 2024, no repository
  named) and `gemini`. None is tied to `googleapis`.

What the author concludes from that, which the tool does not say:

- The right statement is "no official Google crate found; several
  community crates, of which one family is actively released". "None" was
  wrong.
- A crate called `google-genai` exists and is not Google's. The name
  proves nothing.
- `gemini` is a crate for the Gemini network protocol, an unrelated thing
  with the same name, and `genai` is a client for many providers. The tool
  cannot tell; reading the descriptions does. This is what "whether a
  crate covers what the piece uses" means in practice.
