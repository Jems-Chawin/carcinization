# carcinify deps

For each third-party library a piece of Python uses, looks up what Rust
crates there are and what is known about each, and writes it down with the
time it was fetched.

It exists because of two opposite mistakes. One is the claim, made from
memory, that a library "does not exist in Rust"; a search usually finds
something. The other is taking a crate because it exists. So an agent may
propose names and search terms, and every fact in the report comes from a
lookup this tool made.

## Use

```sh
cargo build --release -p carcinify
target/release/carcinify deps path/to/proposal.json --report target/carcinify-deps/report.json \
    --markdown target/carcinify-deps/report.md
```

Exit status: `0` the report is complete; `1` something is missing from it
(an import with no entry, an import the tool could not read, a "none" that
has not been earned, a dismissal without a reason, a decision for a crate
that does not exist, an advisory lookup that does not work); `2` the run
could not be carried out.

Options: `--cache <dir>` (default `target/carcinify-deps/cache`), `--replay`
(use only the cache, make no request), `--refresh` (ask again),
`--python <path>`, `--min-age-days <n>` (default 14).

Needs `python3` to read the imports; the libraries need not be installed.

## The proposal

Written by the agent or person doing the port.

```json
{
  "schema": "carcinify/deps/proposal/1",
  "python_files": ["python/nightly_report.py"],
  "dependencies": {
    "yaml": {
      "pypi": "PyYAML",
      "search_terms": ["yaml parser", "serde yaml"],
      "candidates": ["serde_yaml", "serde-saphyr"],
      "dismissed": {"yaml-rust": "unmaintained since 2020, superseded by saphyr"},
      "declared_official_evidence": {},
      "decision": {"choice": "crate:serde-saphyr", "reason": "..."}
    }
  }
}
```

- `python_files`: the piece being ported. The check is of these files, not
  of the project's requirements.
- One entry per third-party import the files use, keyed by the import
  (`yaml`; `google.adk` for a namespace package). An import with no entry
  fails the run, and so does an entry for something the files do not
  import.
- `pypi`: the project's name on PyPI. This is the proposal's word, and the
  tool checks it where it can. If the library is installed in the
  interpreter the tool runs (`--python`, so point it at the project's
  virtual environment), `importlib.metadata` says which project provides
  the import: agreement is recorded with that source, disagreement is
  flagged. If it is not installed, PyPI cannot help (its JSON does not say
  which modules a project provides), and the pair is flagged when neither
  name contains the other, which is true of correct pairs such as `sklearn`
  and `scikit-learn`. The report says which of the three it was, under
  `pypi.provides_the_import`.
- `search_terms`: words for what the library does. The tool always searches
  the PyPI name and the import name as well; those cannot be left out.
- `candidates`: crates to look up.
- `dismissed`: search results looked at and set aside, each with a reason
  of at least three words. The tool looks each one up and prints its facts
  beside the reason.
- `declared_official_evidence`: for a candidate, where the organisation
  behind the Python library itself names the crate. Shown as the proposal's
  statement; the tool does not fetch it.
- `decision`: `crate:<name>`, `keep_in_python`, `keep_part_in_python` or
  `write_it`, with a reason. Shown as declared.
- `acknowledged_unread_imports`, beside `dependencies`: for an import made
  at run time under a name the tool cannot read, `{"file", "line",
  "reason"}`, the reason saying what it loads in at least three words. The
  account is printed, marked as not checked, and flagged; the report can
  then complete. One that matches no such call fails the run.

## What the tool does

1. **Reads the imports** of the Python files with `ast` (nothing is
   imported or run): each module, the lines, and the names used from it
   (`safe_load`, `trace.get_tracer`). Standard-library and project-local
   modules are listed as not looked up. `importlib.import_module("x")` and
   `__import__("x")` with a literal name count as imports. With any other
   argument the name is not in the source: the call is listed under
   `unread_imports` and the report is incomplete until the proposal
   acknowledges it with a reason.
2. **Fetches the Python project** from PyPI: version, summary, and the
   GitHub or GitLab organisation its links name.
3. **Runs every search** on crates.io and records the query, the URL, the
   time, the total and the top ten results.
4. **Looks up every candidate**, under the name crates.io has for it
   (`serde-yaml` is `serde_yaml`; the report shows both): whether it exists; latest version and its
   date; first release; number of releases, and in the last year; yanked
   versions; licence; repository link; owners; downloads; RustSec
   advisories; whether it has a build script or is a proc-macro. A
   dismissed crate gets the same lookups except the build facts and the
   look-alike search.
5. **Classifies and flags**, as below, and writes the report as JSON and,
   if asked, Markdown.

### Where each fact comes from

| Fact | Source |
|---|---|
| existence, versions, dates, yanks, licence, repository, downloads, native library linked, trusted-publishing record | `crates.io/api/v1/crates/<name>` |
| owners | `crates.io/api/v1/crates/<name>/owners` |
| search results | `crates.io/api/v1/crates?q=<query>&per_page=10` |
| advisories | `rustsec.org/packages/<name>.html`, by the crate's own name: the page exists only for crates that have advisories. 200 is read, 404 is "none listed", anything else is "not known". On every run the page of `serde_yaml`, which has an advisory, is fetched first; if that is not found, advisories are "not known" for every crate and the report is incomplete |
| build script, proc-macro | docs.rs source view of the latest version: a `build.rs` in the file list, `proc-macro = true` in `Cargo.toml`. The crates.io API does not say. Best effort: `null` when the page could not be read, and a build script under another name is caught only if `Cargo.toml` names it |
| the Python project | `pypi.org/pypi/<name>/json` |

The detection of build scripts and proc-macros was checked by hand on five
crates where the answer is known (`serde_derive`, `thiserror-impl`, `ring`,
`serde_json`, `libz-sys`) and was right on all five.

### Classification

- **`official`**: only on evidence the crate's publisher could not have
  typed, and that shows who publishes it. One of: a crates.io owner that is
  a GitHub team of the organisation hosting the Python project (crates.io
  checks team membership); the latest version released by trusted
  publishing from a repository whose owner **is** that organisation.
- **`named_by_upstream`**: the Python project's description on PyPI
  contains a link to the crate, and neither of the above holds. The tool
  found a link. It did not read the sentence, which may present the crate
  as the project's own, as someone's port, or as an alternative.

  The trusted-publishing record is
  `{"provider": "github", "repository": "<owner>/<name>", "run_id", "sha"}`,
  read on 2026-11-14 (UTC) from the real records of `jiff`, `bon`, `pyo3`,
  `maturin`, `rand` and `uv-version`. The owner is compared whole and only
  with GitHub organisations; a record in another shape is not used, and the
  report says so. The description rule matches the crate's name up to its
  end.

  "The organisation hosting the Python project" is the GitHub or GitLab
  account in the links the project gives for its own source and home on
  PyPI (labels containing source, repository, code, home), and its home
  page. Other links (funding, changelog, tracker, anything else) are not
  used. Those links are typed by whoever publishes the Python project.
- **`community`**: everything else that exists. A repository link pointing
  into the upstream organisation is noted and is **not** evidence: the
  publisher types it.
- **`not_found`**: crates.io answered 404.

Many official crates are owned by a personal or bot account and will show
as `community` with that note. The proposal can then say where upstream
names the crate; that is printed as a declaration.

### Outcome per dependency

- `official_crate`, `crate_named_by_upstream`, `community_crates`: by the
  strongest class among the candidates that exist.
- `none_found_by_these_queries`: no candidate exists **and** the proposal
  gave at least two search terms beyond the two names **and** every crate
  in the top five of every search is a candidate or dismissed with a
  reason **and none of the dismissed crates is plainly alive**. The wording
  is deliberate: a crate's name often has nothing in common with the Python
  library's.
- `candidates_dismissed_by_the_proposal`: as above, but a dismissed crate
  is plainly alive. That is either of: a release at most 730 days old with
  at least 1,000 downloads in the last 90 days; or at least 100,000
  downloads in the last 90 days whatever the date of the last release (a
  crate like `serde_yaml`, deprecated and in wide use, exists; it is not
  grounds for "none"). The report says which of the two it was for each
  crate. The run completes; the
  outcome is not "none", and whether the dismissals were right is left
  visible for a reader.

Whenever anything was dismissed, the outcome line gives how many, and how
many of those are plainly alive, by name.
- `unresolved`: no candidate exists and the above is not met. The run
  fails.

Every dependency also lists keeping it in Python, and writing the needed
part by hand.

### Flags

On a candidate: latest version younger than the minimum publish age; the
crate itself under 90 days old; a name one character away (with `-` and `_`
taken as the same) from a crate with at least fifty times its downloads; no
release for two years; yanked versions; RustSec advisories; a build script;
a proc-macro; build script or proc-macro **not known** because docs.rs
could not be read; no repository named. Download counts are shown labelled as a
weak signal: they are dominated by being a dependency of something else,
and `arrayref` had 245 million when it was taken over.

## What the tool cannot judge

Printed in every report:

- whether a crate covers the part of the library the piece uses;
- whether it is correct, sound or well written;
- whether it is what it claims to be;
- whether "none found by these queries" means none exists;
- whether porting the dependency is worth it at all.

## Requests

- Only to crates.io, docs.rs, rustsec.org and pypi.org; any other host is
  refused in code.
- `User-Agent: carcinify/<version> (dependency check tool)`. No name,
  address or project in any request.
  A report made with `--replay` names no User-Agent: no request was made on
  that run, and what the recorded requests carried is not stored with them.
  It says "recorded requests".
- At most one request a second to each host.
- Answers are cached with the time they were fetched; one younger than a
  day is used again. The cache belongs in a work directory such as
  `target/`, not in the repository. Every answer is listed in the report
  once, with whether it came from the cache.
- **The report says where its answers came from**, in the JSON (`source`)
  and in its first line: all fetched on this run; so many from the cache
  with the oldest date; or replayed, with no request made. A replayed
  report is never called live. The cache is a directory of plain files, and
  the tool cannot tell whether one was changed after it was fetched; for a
  report to rely on, run with `--refresh`.

## Ways to a verdict the facts do not support

Known, and not closed. Each leaves something a reader can see.

A false "none":

- Search terms chosen to miss. Two terms nobody would use for the library
  return unrelated crates, which are rightly dismissed. The queries are
  printed.
- Only the top five of each search have to be accounted for.
- "Plainly alive" is two thresholds. A young, real crate under 1,000
  downloads can be dismissed and the outcome is "none"; its facts are in
  the table of dismissed crates.
- An acknowledgement of a run-time import is the proposal's word. It is
  printed and flagged, not checked.
- A reason of three words that says nothing.
- A file left out of `python_files`, or an import made in a way the tool
  does not read (`exec`, entry points, a plugin registry).

A false "official":

- A PyPI project that is not the library. The names-resemble check stops
  `yaml` being judged as `opentelemetry-api`; it does not stop a look-alike
  project whose name contains the import. The report prints the project.
- A project whose own source link points at another organisation's
  repository (a fork published under a new name) makes that organisation's
  crates "official" for it.
- The interpreter's word on which project provides an import is as good as
  the environment: a look-alike project installed there is believed.
- An edited cache, outside `--replay` too: an entry less than a day old is
  used. The first line gives the number taken from the cache.

A credible-looking row for a bad crate:

- A crate older than 90 days whose newest version is older than the
  minimum publish age, with no advisory yet and no name one character from
  a popular one, raises no flag. That is most compromised crates on the day
  after the minimum age.
- The look-alike check sees names within one character among the top ten
  search results for the name. A prefix or suffix (`rust-serde`,
  `tokio-utils2`) is not caught.
- Downloads can be inflated.

## Design choices

**A proposal file, not free search.** The agent knows what the library is
for and can guess names; the tool cannot. The tool can fetch and cannot be
talked out of what it fetched. So each does its part, and the report says
which words came from whom.

**`ureq` as the HTTP client.** Blocking, no async runtime, TLS through
rustls with nothing to install on the system. The tool makes a few dozen
GET requests one after another.

**`ast` in Python to read the imports.** It is the language's own parser,
so it reads exactly what Python would. The script is embedded in the binary
and runs with `-I`.

**No score.** Age, releases, owners, downloads and flags are shown side by
side. A single number would hide which of them is weak.

## Tests

```sh
cargo test --locked -p carcinify-deps -p carcinify --lib --test deps   # no network
cargo test --locked -p carcinify --test deps -- --ignored             # live
```

The files in `tests/` run the `carcinify` binary, so they are built as test
targets of the `carcinify` crate (see `carcinify/Cargo.toml`); the unit
tests are in this crate.


`tests/fixtures/cache` holds answers recorded on 2026-11-14 and trimmed to
the fields the tool reads; with `--replay` the tests make no request. Each
audit finding has a test that repeats the auditor's case. Tests that need
an answer nobody has given (a trusted-publishing record on a crate that
has none, a changed status) edit the copy of the recording and say
SYNTHETIC or say what was edited; the real trusted-publishing record
tested is `maturin`'s. One test uses answers written by hand for a crate
that does not exist (`opentelemetri`). The two ignored tests run the
worked example and the ADK case against the live services, always with
`--refresh`.
