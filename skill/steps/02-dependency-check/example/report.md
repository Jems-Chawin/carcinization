# Dependency check

Generated 2026-11-14T21:10:24Z by carcinify deps, **replayed from a cache: no request was made.** All 90 answers were read from files, the oldest fetched 2026-11-14T21:08:56Z. A cache is a directory of files that can be edited; this is not a live report. Every fact below comes from those answers, each with the time it was fetched; the proposal supplied only names to look up, search terms, dismissals and decisions.

Verdict: **complete**

Minimum publish age applied: 14 days.

## `opentelemetry`

- The piece imports `opentelemetry` (python/nightly_report.py, line 13) and uses: trace.get_tracer
- PyPI project, as the proposal names it: `opentelemetry-api`, version 1.45.2, hosted by ["github.com/open-telemetry"]

**Outcome: `official_crate`.** a candidate is published by the organisation behind the PyPI project "opentelemetry-api", on evidence its publisher could not have typed.

| Crate | Class | Latest | Released | First release | Releases, last year | Downloads, 90 days (weak signal) | Licence | Owners | Build script | Proc-macro | Advisories |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `opentelemetry` | official | 0.33.1 | 2026-10-09 | 2019-11-08 | 3 | 90289171 | Apache-2.0 | TommyCpp, cijothomas, github:open-telemetry:rust-publishers | false | false | 0 |

- `opentelemetry`: crates.io owner `github:open-telemetry:rust-publishers` is a GitHub team of `open-telemetry`, the organisation that hosts the Python project (checked by crates.io)

Searches run:

- `opentelemetry-api` (tool; 6 results; fetched 2026-11-14T21:08:56Z): `opentelemetry_api`, `ts_opentelemetry_api`, `pyo3-opentelemetry-macros`, `pyo3-opentelemetry`, `pyproject-udeps`
- `opentelemetry` (tool; 2330 results; fetched 2026-11-14T21:08:57Z): `opentelemetry`, `pact-broker-cli`, `init-tracing-opentelemetry`, `datadog-opentelemetry`, `opentelemetry-spanprocessor-any`
- `tracing spans` (proposal; 3008 results; fetched 2026-11-14T21:08:59Z): `rootcause-tracing`, `eros`, `tracing-record-hierarchical`, `blockscout-tracing-actix-web`, `tracing-actix-web`
- `distributed tracing` (proposal; 3144 results; fetched 2026-11-14T21:09:01Z): `tracing-distributed`, `aprender-profile`, `hammerwork`, `remoc`, `tracing-honeycomb`

In the top 5 of a search and not examined: `opentelemetry_api`, `ts_opentelemetry_api`, `pyo3-opentelemetry-macros`, `pyo3-opentelemetry`, `pyproject-udeps`, `pact-broker-cli`, `init-tracing-opentelemetry`, `datadog-opentelemetry`, `opentelemetry-spanprocessor-any`, `rootcause-tracing`, `eros`, `tracing-record-hierarchical`, `blockscout-tracing-actix-web`, `tracing-actix-web`, `tracing-distributed`, `aprender-profile`, `hammerwork`, `remoc`, `tracing-honeycomb`

Options:

- Use the crate `opentelemetry` (official).
- Keep it in Python and call across the boundary, or keep the part of the piece that uses it in Python. For numerical, scientific and ML libraries, whose work is already done in compiled code, this is usually the right choice.
- Write the part the piece needs in Rust, if what it uses is small. The list of names used, above, is the size of that job.

Decision declared in the proposal: `crate:opentelemetry`. The piece uses get_tracer, start_as_current_span, set_attribute and record_exception; the crate is the same project's Rust API. Coverage of those four still has to be read against its documentation.

## `sklearn`

- The piece imports `sklearn.ensemble` (python/nightly_report.py, line 14) and uses: IsolationForest
- PyPI project, as the proposal names it: `scikit-learn`, version 1.9.1, hosted by ["github.com/scikit-learn"]

**Outcome: `community_crates`.** candidates exist; none could be tied to the organisation behind the Python library by evidence the tool can check

| Crate | Class | Latest | Released | First release | Releases, last year | Downloads, 90 days (weak signal) | Licence | Owners | Build script | Proc-macro | Advisories |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `linfa` | community | 0.8.1 | 2025-12-23 | 2018-04-05 | 1 | 577131 | MIT OR Apache-2.0 | LukeMathWalker, github:rust-ml:wg | true | false | 0 |
| `smartcore` | community | 0.7.0 | 2026-10-26 | 2020-09-26 | 26 | 132735 | Apache-2.0 | Mec-iS | false | false | 0 |
| `extended-isolation-forest` | community | 0.2.3 | 2022-11-30 | 2021-05-12 | 0 | 25985 | MIT | nmandery | false | false | 0 |

- `linfa`: no crates.io owner is a team of ["scikit-learn"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `linfa`, flag: has a build script, which runs on your machine at build time: name it in the review
- `smartcore`: no crates.io owner is a team of ["scikit-learn"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `smartcore`, flag: 9 version(s) were yanked: 0.6.9, 0.6.7, 0.6.5, 0.6.4, 0.6.3, 0.6.2
- `extended-isolation-forest`: no crates.io owner is a team of ["scikit-learn"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `extended-isolation-forest`, flag: no release for 1445 days

Searches run:

- `scikit-learn` (tool; 229 results; fetched 2026-11-14T21:09:09Z): `datarust`, `simian`, `sklears-python`, `betula-cluster`, `anofox-ml-core`
- `sklearn` (tool; 103 results; fetched 2026-11-14T21:09:09Z): `datarust`, `sklearn-rs`, `anofox-ml-core`, `anofox-ml-svm`, `anofox-ml-metrics`
- `isolation forest` (proposal; 85 results; fetched 2026-11-14T21:09:10Z): `isolation_forest`, `u-insight`, `extended-isolation-forest`, `sklears-tree`, `miniml`
- `anomaly detection` (proposal; 688 results; fetched 2026-11-14T21:09:13Z): `ruvllm-esp32`, `aprender-profile`, `anomaly_detection`, `llm-sentinel-detection`, `turbolog`
- `machine learning` (proposal; 5522 results; fetched 2026-11-14T21:09:15Z): `rusty-machine`, `mlx-rs`, `rustlearn`, `mlx-rs-burn`, `smartcore`

In the top 5 of a search and not examined: `datarust`, `simian`, `sklears-python`, `betula-cluster`, `anofox-ml-core`, `sklearn-rs`, `anofox-ml-svm`, `anofox-ml-metrics`, `isolation_forest`, `u-insight`, `sklears-tree`, `miniml`, `ruvllm-esp32`, `aprender-profile`, `anomaly_detection`, `llm-sentinel-detection`, `turbolog`, `rusty-machine`, `mlx-rs`, `rustlearn`, `mlx-rs-burn`

Options:

- Use the crate `linfa` (community).
- Use the crate `smartcore` (community).
- Use the crate `extended-isolation-forest` (community).
- Keep it in Python and call across the boundary, or keep the part of the piece that uses it in Python. For numerical, scientific and ML libraries, whose work is already done in compiled code, this is usually the right choice.
- Write the part the piece needs in Rust, if what it uses is small. The list of names used, above, is the size of that job.

Decision declared in the proposal: `keep_in_python`. The work is already done in compiled code inside scikit-learn, and the results of an isolation forest depend on its exact random-number use, which no other implementation reproduces. Port the code around it and call fit_predict across the boundary.

Flag: the proposal says `sklearn` comes from the PyPI project "scikit-learn". The tool could not check that: `sklearn` is not installed in the interpreter it ran, PyPI does not publish which modules a project provides, and neither name contains the other. Everything said about an "official" crate here is about that project

## `yaml`

- The piece imports `yaml` (python/nightly_report.py, line 11) and uses: safe_load
- PyPI project, as the proposal names it: `PyYAML`, version 6.0.3, hosted by ["github.com/yaml"]

**Outcome: `community_crates`.** candidates exist; none could be tied to the organisation behind the Python library by evidence the tool can check

| Crate | Class | Latest | Released | First release | Releases, last year | Downloads, 90 days (weak signal) | Licence | Owners | Build script | Proc-macro | Advisories |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `serde_yaml` | community | 0.9.34+deprecated | 2024-03-25 | 2016-02-27 | 0 | 101356317 | MIT OR Apache-2.0 | dtolnay | false | false | 1 |
| `serde_yml` | community | 0.0.13 | 2026-05-27 | 2024-03-29 | 1 | 4380410 | MIT OR Apache-2.0 | sebastienrousseau | false | false | 1 |
| `serde-saphyr` | community | 2.1.1 | 2026-11-05 | 2025-09-27 | 32 | 14912086 | MIT OR Apache-2.0 | bourumir-wyngs | false | false | 0 |
| `saphyr` | community | 0.3.4 | 2026-11-11 | 2024-04-02 | 17 | 1323551 | MIT OR Apache-2.0 | davvid, Ethiraric | false | false | 0 |

- `serde_yaml`: no crates.io owner is a team of ["yaml"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `serde_yaml`, flag: no release for 964 days
- `serde_yaml`, flag: RustSec has advisories for this crate: RUSTSEC-2018-0005. Read them; they may concern old versions only
- `serde_yml`: no crates.io owner is a team of ["yaml"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `serde_yml`, flag: RustSec has advisories for this crate: RUSTSEC-2025-0068. Read them; they may concern old versions only
- `serde-saphyr`: no crates.io owner is a team of ["yaml"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `serde-saphyr`, flag: the latest version is 9 day(s) old, younger than the minimum publish age of 14: do not take it yet
- `serde-saphyr`, flag: 3 version(s) were yanked: 2.1.0, 0.0.9, 0.0.8-alpha-pre
- `saphyr`: no crates.io owner is a team of ["yaml"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `saphyr`, flag: the latest version is 2 day(s) old, younger than the minimum publish age of 14: do not take it yet

Searches run:

- `PyYAML` (tool; 62 results; fetched 2026-11-14T21:09:32Z): `cfgv`, `openapi-spec-validator-rs`, `openapi-spec-validator-rs-cli`, `oyl-sys`, `oyl`
- `yaml` (tool; 11107 results; fetched 2026-11-14T21:09:34Z): `yaml`, `serde-saphyr`, `vsh-serde-saphyr`, `rust-yaml`, `serde_yaml_gtc`
- `yaml parser` (proposal; 2271 results; fetched 2026-11-14T21:09:35Z): `rlsp-yaml-parser`, `libyml`, `yaml_parser`, `oxc-yaml-parser`, `rattler_build_yaml_parser`
- `serde yaml` (proposal; 2243 results; fetched 2026-11-14T21:09:37Z): `noyalib-serde-yaml`, `yaml_serde`, `serde_yaml_neo`, `serde_yaml_gtc`, `mk_ext_serde_yaml`

In the top 5 of a search and not examined: `cfgv`, `openapi-spec-validator-rs`, `openapi-spec-validator-rs-cli`, `oyl-sys`, `oyl`, `yaml`, `vsh-serde-saphyr`, `rust-yaml`, `serde_yaml_gtc`, `rlsp-yaml-parser`, `libyml`, `yaml_parser`, `oxc-yaml-parser`, `rattler_build_yaml_parser`, `noyalib-serde-yaml`, `yaml_serde`, `serde_yaml_neo`, `mk_ext_serde_yaml`

Options:

- Use the crate `serde_yaml` (community).
- Use the crate `serde_yml` (community).
- Use the crate `serde-saphyr` (community).
- Use the crate `saphyr` (community).
- Keep it in Python and call across the boundary, or keep the part of the piece that uses it in Python. For numerical, scientific and ML libraries, whose work is already done in compiled code, this is usually the right choice.
- Write the part the piece needs in Rust, if what it uses is small. The list of names used, above, is the size of that job.

Decision declared in the proposal: `crate:serde-saphyr`. The piece only calls safe_load on a small config file. serde_yaml is marked deprecated by its author and serde_yml has an advisory; serde-saphyr is the maintained one. Its latest version is younger than the minimum publish age, so pin the one before it until that has passed.

Flag: the chosen crate `serde-saphyr`: the latest version is 9 day(s) old, younger than the minimum publish age of 14: do not take it yet

Flag: the chosen crate `serde-saphyr`: 3 version(s) were yanked: 2.1.0, 0.0.9, 0.0.8-alpha-pre

## `zeep`

- The piece imports `zeep` (python/nightly_report.py, line 12) and uses: Client, exceptions.Error
- PyPI project, as the proposal names it: `zeep`, version 4.3.3, hosted by ["github.com/mvantellingen"]

**Outcome: `community_crates`.** candidates exist; none could be tied to the organisation behind the Python library by evidence the tool can check

| Crate | Class | Latest | Released | First release | Releases, last year | Downloads, 90 days (weak signal) | Licence | Owners | Build script | Proc-macro | Advisories |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `soap-client` | community | 0.1.9 | 2026-10-28 | 2026-10-22 | 8 | 81 | Apache-2.0 | adminypc | false | false | 0 |
| `rsoap` | community | 0.4.1 | 2026-10-15 | 2026-06-05 | 5 | 63 | MIT | ouertani | false | false | 0 |
| `wsdl` | community | 0.1.3 | 2023-07-29 | 2021-09-26 | 0 | 10 | Apache-2.0/MIT | DrChat | false | false | 0 |
| `zeep-core` | community | 1.0.0 | 2026-08-08 | 2026-08-08 | 1 | 73 | MIT OR Apache-2.0 | JulioAugustoS | false | false | 0 |

- `soap-client`: no crates.io owner is a team of ["mvantellingen"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `soap-client`, flag: the crate itself is 23 day(s) old
- `rsoap`: no crates.io owner is a team of ["mvantellingen"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `wsdl`: no crates.io owner is a team of ["mvantellingen"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it
- `wsdl`, flag: no release for 1204 days
- `zeep-core`: no crates.io owner is a team of ["mvantellingen"] (where the Python project lives), the crate was not released by trusted publishing from there, and the project's PyPI description does not name it

Searches run:

- `zeep` (tool; 10 results; fetched 2026-11-14T21:10:02Z): `zeep-orbit-client`, `zeep-core`, `zeep-ai`, `zeep-logger`, `zeep-updater`
- `soap client` (proposal; 126 results; fetched 2026-11-14T21:10:03Z): `sonos-sdk-soap-client`, `soap-client`, `yuki-client`, `polarion-soap`, `polarion`
- `wsdl` (proposal; 35 results; fetched 2026-11-14T21:10:04Z): `wsdl`, `soap-server`, `rsoap-macros`, `rsoap`, `soap-client`

In the top 5 of a search and not examined: `zeep-orbit-client`, `zeep-ai`, `zeep-logger`, `zeep-updater`, `sonos-sdk-soap-client`, `yuki-client`, `polarion-soap`, `polarion`, `soap-server`, `rsoap-macros`

Options:

- Use the crate `soap-client` (community).
- Use the crate `rsoap` (community).
- Use the crate `wsdl` (community).
- Use the crate `zeep-core` (community).
- Keep it in Python and call across the boundary, or keep the part of the piece that uses it in Python. For numerical, scientific and ML libraries, whose work is already done in compiled code, this is usually the right choice.
- Write the part the piece needs in Rust, if what it uses is small. The list of names used, above, is the size of that job.

Decision declared in the proposal: `keep_part_in_python`. Crates with SOAP in the name exist; the figures in the report (age, releases, owners, use) do not describe anything to build a nightly job on. fetch_prices stays in Python and hands plain rows to the ported part.

## What this report cannot judge

- Whether a crate covers the part of the library the piece uses. The report lists what the piece uses; someone has to read the crate's documentation against that list.
- Whether a crate is correct, sound, or well written. Downloads, age and release dates say that people use it and that it is maintained, not that it is right.
- Whether a crate is what it claims to be. Its repository link, description and name are typed by whoever published it, and are not evidence of anything. Download counts are a weak signal: they are dominated by being a dependency of something else, and a crate with 245 million downloads has been taken over. Only an owner that is a GitHub team of the organisation, or a release through trusted publishing from its repository, is checked by crates.io.
- Whether "none found by these queries" means none exists. A crate's name often has nothing in common with the Python library's. It means these searches of crates.io, on this day, returned nothing that was kept; the queries are in the report so that they can be run again and added to.
- Whether porting the dependency is worth it at all. Keeping it in Python is listed for every dependency because it is often the right choice, not as a fallback.

## Requests recorded

90 recorded requests, read from the cache. None was made on this run. The URLs are in the JSON report; run them to check.
