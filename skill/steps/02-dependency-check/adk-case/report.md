# Dependency check

Generated 2026-11-14T21:11:47Z by carcinify deps, **replayed from a cache: no request was made.** All 82 answers were read from files, the oldest fetched 2026-11-14T21:10:24Z. A cache is a directory of files that can be edited; this is not a live report. Every fact below comes from those answers, each with the time it was fetched; the proposal supplied only names to look up, search terms, dismissals and decisions.

Verdict: **complete**

Minimum publish age applied: 14 days.

## `google.adk`

- The piece imports `google.adk.agents` (python/assistant.py, line 8) and uses: Agent
- The piece imports `google.adk.runners` (python/assistant.py, line 9) and uses: Runner
- PyPI project, as the proposal names it: `google-adk`, version 2.14.1, hosted by ["github.com/google"]

**Outcome: `community_crates`.** candidates exist; none could be tied to the organisation behind the Python library by evidence the tool can check

| Crate | Class | Latest | Released | First release | Releases, last year | Downloads, 90 days (weak signal) | Licence | Owners | Build script | Proc-macro | Advisories |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `adk` | community | 0.2.1 | 2025-04-12 | 2025-04-12 | 0 | 20 | MIT | Yougigun | false | false | 0 |
| `adk-rust` | community | 2.3.0 | 2026-10-09 | 2025-11-30 | 31 | 17458 | Apache-2.0 | jkmaina | false | false | 0 |
| `adk-core` | community | 2.3.0 | 2026-10-09 | 2025-11-30 | 31 | 34773 | Apache-2.0 | jkmaina | false | false | 0 |
| `adk-agent` | community | 2.3.0 | 2026-10-09 | 2025-11-30 | 31 | 19717 | Apache-2.0 | jkmaina | false | false | 0 |
| `adk-model` | community | 2.3.0 | 2026-10-09 | 2025-11-30 | 30 | 26697 | Apache-2.0 | jkmaina | false | false | 0 |
| `adk-runner` | community | 2.3.0 | 2026-10-09 | 2025-11-30 | 31 | 19757 | Apache-2.0 | jkmaina | false | false | 0 |
| `google-adk` | **does not exist** | | | | | | | | | | |

- `adk`: no crates.io owner is a team of ["google"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `adk`, flag: 1 version(s) were yanked: 0.1.0
- `adk-rust`: no crates.io owner is a team of ["google"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `adk-core`: no crates.io owner is a team of ["google"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `adk-agent`: no crates.io owner is a team of ["google"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `adk-model`: no crates.io owner is a team of ["google"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `adk-runner`: no crates.io owner is a team of ["google"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `google-adk`: crates.io answered 404 for https://crates.io/api/v1/crates/google-adk at 2026-11-14T21:11:06Z
- `google-adk`, flag: there is no crate of this name on crates.io

Searches run:

- `google-adk` (tool; 7 results; fetched 2026-11-14T21:10:25Z): `keel-core-api`, `keel-journal`, `keelrun-core`, `keel-macros`, `keelrun-cli`
- `google adk` (tool; 85 results; fetched 2026-11-14T21:10:25Z): `mcp-a2a`, `keel-core-api`, `keel-journal`, `keelrun-core`, `keel-macros`
- `adk` (proposal; 243 results; fetched 2026-11-14T21:10:26Z): `adk`, `adk-model`, `agentx-adk-model`, `adk-cli`, `adk-rag`
- `agent development kit` (proposal; 369 results; fetched 2026-11-14T21:10:28Z): `adk-artifact`, `agentx-adk-artifact`, `adk-server`, `adk-telemetry`, `agentx-adk-telemetry`
- `google adk agent` (proposal; 76 results; fetched 2026-11-14T21:10:30Z): `mcp-a2a`, `bitpolar`, `keel-core-api`, `keel-journal`, `keelrun-core`
- `adk-rust` (proposal; 169 results; fetched 2026-11-14T21:10:32Z): `adk-rust`, `adk-cli`, `adk-acp`, `agentx-adk-acp`, `agentx-adk-tool`

In the top 5 of a search and not examined: `keel-core-api`, `keel-journal`, `keelrun-core`, `keel-macros`, `keelrun-cli`, `mcp-a2a`, `agentx-adk-model`, `adk-cli`, `adk-rag`, `adk-artifact`, `agentx-adk-artifact`, `adk-server`, `adk-telemetry`, `agentx-adk-telemetry`, `bitpolar`, `adk-acp`, `agentx-adk-acp`, `agentx-adk-tool`

Options:

- Use the crate `adk` (community).
- Use the crate `adk-rust` (community).
- Use the crate `adk-core` (community).
- Use the crate `adk-agent` (community).
- Use the crate `adk-model` (community).
- Use the crate `adk-runner` (community).
- Keep it in Python and call across the boundary, or keep the part of the piece that uses it in Python. For numerical, scientific and ML libraries, whose work is already done in compiled code, this is usually the right choice.
- Write the part the piece needs in Rust, if what it uses is small. The list of names used, above, is the size of that job.

Flag: the proposed candidate `google-adk` does not exist on crates.io

## `google.genai`

- The piece imports `google` (python/assistant.py, line 7) and uses: genai.Client
- PyPI project, as the proposal names it: `google-genai`, version 3.3.0, hosted by ["github.com/googleapis"]

**Outcome: `community_crates`.** candidates exist; none could be tied to the organisation behind the Python library by evidence the tool can check

| Crate | Class | Latest | Released | First release | Releases, last year | Downloads, 90 days (weak signal) | Licence | Owners | Build script | Proc-macro | Advisories |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `google-genai` | community | 0.0.2 | 2024-12-24 | 2024-12-21 | 0 | 257 | MIT | cortesi | false | false | 0 |
| `gemini-genai` | community | 0.4.0 | 2026-10-03 | 2026-08-22 | 5 | 245 | Apache-2.0 | siosig | false | false | 0 |
| `gemini-rust` | community | 2.1.0 | 2026-09-05 | 2025-03-25 | 6 | 16021 | MIT | npatsakula, flachesis | false | false | 0 |
| `adk-gemini` | community | 2.3.0 | 2026-10-09 | 2025-12-29 | 23 | 10577 | Apache-2.0 | jkmaina | false | false | 0 |
| `gemini` | community | 0.0.5 | 2022-07-15 | 2020-06-07 | 0 | 271 | GPL-3.0-only | sloane-shark | false | false | 0 |
| `genai` | community | 0.7.4 | 2026-11-11 | 2024-06-01 | 81 | 303011 | MIT OR Apache-2.0 | jeremychone | false | false | 0 |

- `google-genai`: no crates.io owner is a team of ["googleapis"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `google-genai`, flag: names no repository
- `gemini-genai`: no crates.io owner is a team of ["googleapis"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `gemini-genai`, flag: the crate itself is 83 day(s) old
- `gemini-rust`: no crates.io owner is a team of ["googleapis"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `adk-gemini`: no crates.io owner is a team of ["googleapis"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `gemini`: no crates.io owner is a team of ["googleapis"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `gemini`, flag: no release for 1583 days
- `gemini`, flag: 1 version(s) were yanked: 0.0.1
- `genai`: no crates.io owner is a team of ["googleapis"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `genai`, flag: the latest version is 2 day(s) old, younger than the minimum publish age of 14: do not take it yet
- `genai`, flag: 6 version(s) were yanked: 0.5.0-alpha.8, 0.5.0-alpha.7, 0.4.0-alpha.13-WIP, 0.4.0-alpha.2, 0.4.0-alpha.1, 0.2.0-rc.4

Searches run:

- `google-genai` (tool; 10 results; fetched 2026-11-14T21:11:09Z): `google-genai`, `gemini-genai`, `keel-core-api`, `keel-journal`, `keelrun-core`
- `google genai` (tool; 101 results; fetched 2026-11-14T21:11:10Z): `elizaos-plugin-google-genai`, `google-genai`, `gemini-genai`, `keel-core-api`, `keel-journal`
- `gemini` (proposal; 3208 results; fetched 2026-11-14T21:11:12Z): `gemini`, `adk-gemini`, `gemini-genai`, `gemini-rust`, `gemini-tokenizer`
- `gemini api client` (proposal; 1451 results; fetched 2026-11-14T21:11:14Z): `gemini-client-api`, `adk-gemini`, `agentx-adk-gemini`, `api_gemini`, `miyabi-llm-google`
- `genai` (proposal; 426 results; fetched 2026-11-14T21:11:16Z): `genai`, `gemini-genai`, `genai-rs-macros`, `genai-rs`, `genai-agentprism`

In the top 5 of a search and not examined: `keel-core-api`, `keel-journal`, `keelrun-core`, `elizaos-plugin-google-genai`, `gemini-tokenizer`, `gemini-client-api`, `agentx-adk-gemini`, `api_gemini`, `miyabi-llm-google`, `genai-rs-macros`, `genai-rs`, `genai-agentprism`

Options:

- Use the crate `google-genai` (community).
- Use the crate `gemini-genai` (community).
- Use the crate `gemini-rust` (community).
- Use the crate `adk-gemini` (community).
- Use the crate `gemini` (community).
- Use the crate `genai` (community).
- Keep it in Python and call across the boundary, or keep the part of the piece that uses it in Python. For numerical, scientific and ML libraries, whose work is already done in compiled code, this is usually the right choice.
- Write the part the piece needs in Rust, if what it uses is small. The list of names used, above, is the size of that job.

## What this report cannot judge

- Whether a crate covers the part of the library the piece uses. The report lists what the piece uses; someone has to read the crate's documentation against that list.
- Whether a crate is correct, sound, or well written. Downloads, age and release dates say that people use it and that it is maintained, not that it is right.
- Whether a crate is what it claims to be. Its repository link, description and name are typed by whoever published it, and are not evidence of anything. Download counts are a weak signal: they are dominated by being a dependency of something else, and a crate with 245 million downloads has been taken over. Only an owner that is a GitHub team of the organisation, or a release through trusted publishing from its repository, is checked by crates.io.
- Whether "none found by these queries" means none exists. A crate's name often has nothing in common with the Python library's. It means these searches of crates.io, on this day, returned nothing that was kept; the queries are in the report so that they can be run again and added to.
- Whether porting the dependency is worth it at all. Keeping it in Python is listed for every dependency because it is often the right choice, not as a fallback.

## Requests recorded

82 recorded requests, read from the cache. None was made on this run. The URLs are in the JSON report; run them to check.
