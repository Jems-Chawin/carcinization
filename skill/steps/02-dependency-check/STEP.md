# Step 2: Check the dependencies, by lookup

Do this once you know which piece is to be ported, before designing or
writing anything. The piece uses libraries. For each one there are four
answers: an official Rust crate, community crates, nothing usable, or leave
it in Python. This step finds out which, from the registry and not from
memory.

Two mistakes are being guarded against, and they pull in opposite
directions.

- **"That does not exist in Rust."** Said from memory, it is often wrong.
  New crates appear every week and a model's knowledge has a date.
- **"A crate with that name exists, so use it."** A name is not a
  maintainer, and a registry anyone can publish to holds abandoned crates,
  machine-generated ones, and now and then a malicious one.

How far each claim in this step is supported is in `EVIDENCE.md`.

## What you hand over

- `proposal.json` next to the piece.
- The report written by `carcinify deps`, JSON and Markdown, committed with
  its fetch date.
- For each dependency, a decision with its reason, taken after reading the
  report.

## What to do

### 1. List what the piece really uses

Not the project's requirements file: the imports of the files being ported,
and what they use from each. The tool reads these itself; your part is to
name the files. An import whose name is computed when the program runs
(`import_module(name)`) cannot be read; the tool lists each one and the
report stays incomplete. Find out by hand what it loads. Then either
change the code or the scope, or acknowledge it in the proposal with what
it loads; your account is printed as yours and flagged.

Run the tool with the project's own interpreter (`--python
.venv/bin/python`). Where the library is installed, the interpreter says
which project provides each import, and the tool no longer has to take the
project's name on your word. A library the piece does not import is not this step's
business, however much of the project depends on it.

### 2. Propose, for each library

- its name on the package index;
- **search terms for what it does**, in the words a Rust author would use,
  not only its name. A crate is rarely named after the library it
  resembles: the counterpart of a "requests" is not called requests. Give
  at least two.
- **candidates**: crate names you know or suspect. Guesses are fine; the
  tool will say which exist.

Do not write what you believe about any of them. That is the tool's part.

### 3. Run the tool and read the search results

```sh
carcinify deps proposal.json --report report.json --markdown report.md
```

Then look at the searches. Add as candidates the results that could be
what you need, and run again. Results that are plainly something else go
under `dismissed`, each with a reason a reader can check ("an SDK for a
speaker system, not a general SOAP client").

The tool does not judge a reason, beyond refusing one shorter than three
words. What it does: it looks up every dismissed crate, prints its release
dates, downloads and advisories beside your reason, and marks the ones
that are plainly alive (a release in the last two years and 1,000
downloads in the last 90 days; or 100,000 downloads in the last 90 days
whatever its last release, which is a deprecated crate still in wide use).
So a reader sees at once when something
that looks usable was set aside, and will ask why. If a crate could do the
job, it is a candidate, not a dismissal: decide against it in the
decision, with the facts in the table.

**"None" has to be earned.** The tool reports that nothing was found only
when at least two descriptive searches were run besides the names, and
every crate in the top five of each search was either looked up or
dismissed with a reason, and none of the dismissed ones is plainly alive.
Until then the dependency is unresolved and the run fails. If a dismissed
crate is alive the outcome reads "candidates dismissed by the proposal",
never "none". Even a real "none" is worded "none found by these queries":
it is a statement about searches, and a reader can run more.

### 4. Read each candidate against what the piece uses

The report gives, for each crate, facts with their dates. What they can
and cannot tell you:

- **Official or community.** `official` is given only on evidence the
  crate's publisher could not have typed: a crates.io owner that is the
  organisation's team, or a release through trusted publishing from its
  repository. A link to the crate in the Python project's description gets
  a class of its own, `named_by_upstream`: read the sentence, it may be a
  list of alternatives.
  A repository link into the right organisation proves nothing; anyone
  can type it. Many genuine official crates are owned by one person's
  account and will read `community`; go to the organisation's own
  documentation, and if it names the crate, record where.
- **Maintained?** Date of the latest release, releases in the last year,
  number of owners, yanked versions. A crate whose last version is marked
  deprecated, or two years old, is telling you something.
- **Used?** Downloads are a **weak** signal. They are dominated by being a
  dependency of something popular, and a crate with hundreds of millions of
  downloads has been taken over. Low downloads on a young crate with one
  owner is still worth noticing.
- **Safe to take today?** A latest version younger than the minimum
  publish age is flagged: wait, or pin the version before it. So is a name
  one character from a far more downloaded crate, and a crate days old. A
  candidate an agent proposed is exactly where a typosquat gets in.
- **What runs at build time.** A build script or a proc-macro runs code on
  your machine when you compile. That is normal for many good crates. Name
  each one in the review. "Not known" is flagged: the tool could not read
  the crate's files, so look yourself.
- **Advisories.** Read them; many concern old versions. "Not known" means
  the lookup gave no usable answer, not that there are none.

What no lookup tells you, and you must find out: **does the crate cover
what the piece uses?** The report lists the names the piece takes from the
library. Read the crate's documentation against that list. A crate for
"machine learning" may not have the one model the piece calls.

### 5. Decide, and keeping it in Python is a full answer

For each library choose one, with a reason:

- **Use a crate.**
- **Keep it in Python** and call it across the boundary, or keep the part
  of the piece that uses it in Python. For numerical, scientific and ML
  libraries this is usually right: their work is already done in compiled
  code, a port buys little, and matching their results exactly (random
  number use, numerical details) is often impossible with a different
  implementation. A port that keeps three libraries in Python and moves
  the loop around them is a good port.
- **Write the needed part.** When the piece uses one small function of a
  large library.

A report to be relied on is one made with `--refresh`: its first line then
says every answer was fetched on that run.

A piece whose time goes into a library that should stay in Python may not
be worth porting. That belongs to step 1; say so if this step shows it.

### 6. Set the supply-chain defaults once

In the port's repository, before the first dependency goes in:

- **A minimum publish age.** In `.cargo/config.toml`:

  ```toml
  [registry]
  global-min-publish-age = "14 days"
  ```

  Cargo 1.100 and later then will not pick a version younger than that
  unless it is already in the lock file (`resolver.incompatible-publish-age`
  is `deny` by default). The default for the setting itself is `"0"`, that
  is, off. Do not count on it for `cargo install`: the project's auditor
  reads the Rust 1.100 announcement as saying `cargo install` ignores the
  setting, and nothing in the Cargo reference or release notes installed
  with 1.100 says that it applies there. Install tools with `--locked`
  and a version you chose. A registry can be given its own value with
  `[registries.<name>] min-publish-age`. The malicious versions in the
  2026-08-20 attack were online for under two hours.
- **`Cargo.lock` committed**, and every build and test run with
  `--locked`, so that what was reviewed is what is built.
- **`cargo deny check`** in the checks that run on every change, for
  advisories, licences and sources.
- **Few dependencies.** Each new one is a line in a review, with whether it
  has a build script or is a proc-macro.

## What this step does not do

- It does not judge code. A crate can pass every signal here and be wrong.
- It does not look beyond crates.io: a crate that lives only in a git
  repository is not found.
- It searches by words. A crate described in words nobody tried is missed.
