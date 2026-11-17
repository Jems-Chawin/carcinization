# Worked example: one small job, four libraries, four different answers

`python/nightly_report.py` fetches supplier price lists over SOAP, flags
odd prices with an isolation forest, reads its settings from YAML and
traces the run. `proposal.json` is what was proposed; `report.md` (and
`report.json`) is what `carcinify deps` found on crates.io, PyPI, rustsec.org
and docs.rs on **2026-11-14**. The facts in the report are the tool's; the
decisions and the reading below are the author's.

The committed report is a replay of those answers, made when the tool was
renamed to `carcinify`: the report says "replayed from a cache", and every
fact and time in it is as fetched that day.

| Import | Why it is a fair example | What the tool reports | Decision in the proposal |
|---|---|---|---|
| `opentelemetry` | A library with a Rust project of the same name by the same organisation | `official_crate`: the crate `opentelemetry` has a crates.io owner that is a GitHub team of `open-telemetry`, where the Python project lives | use the crate |
| `yaml` | A common need with several crates and a history | `community_crates`: `serde_yaml` (latest version marked `+deprecated`, no release for 964 days, one advisory), `serde_yml` (one advisory), `serde-saphyr` (latest version 9 days old, under the 14-day minimum), `saphyr` (2 days old) | `serde-saphyr`, pinned to the version before the newest until it has aged |
| `sklearn` | The work is done in compiled code already and results depend on the exact implementation | `community_crates`: `linfa`, `smartcore`, and `extended-isolation-forest` (no release for 1,445 days) | keep it in Python |
| `zeep` | Crates with the right words in their names exist; none looks like something to build on | `community_crates`: `soap-client` (23 days old, 81 downloads in 90 days), `rsoap` (63), `wsdl` (no release for 1,204 days), `zeep-core` (one version) | keep that part in Python |

Things the example shows:

- "A crate exists" was true for all four libraries. It was a good answer
  for one.
- The most downloaded YAML crate, by a factor of seven, is the deprecated
  one. Downloads follow what older projects depend on.
- Two of the four YAML candidates were too young to take on the day of the
  check.
- The tool calls nothing "not credible". For `zeep` it shows age, releases,
  owners and use, and the conclusion is the reader's.
- `sklearn` has usable crates and the decision is still to stay in Python.
  The report lists that option for every library.
- `sklearn` carries a flag that the tool could not check that it comes
  from the PyPI project `scikit-learn`: the report was made with an
  interpreter that does not have it installed, the two names do not contain
  each other, and PyPI does not publish the modules a project provides.
  Here the proposal is right. Run with the project's own interpreter and
  the flag goes, replaced by what `importlib.metadata` says.

To run it again (answers will differ as the registry changes):

```sh
target/release/carcinify deps skill/steps/02-dependency-check/example/proposal.json \
    --report target/carcinify-deps/example.json --markdown target/carcinify-deps/example.md
```
