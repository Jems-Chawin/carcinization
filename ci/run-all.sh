#!/usr/bin/env bash
# Runs every test suite and every worked example of the four tools in a
# checkout, and writes what happened to a directory.
#
#   ci/run-all.sh <checkout> <out-dir>
#
# Needs: cargo (the toolchain in rust-toolchain.toml), git, and
# CARCINIFY_PYTHON and CARCINIFY_MATURIN pointing at a Python 3.12 or later and
# maturin 1.16. Set RUN_LIVE=1 to run the dependency check's tests against
# the live services as well (crates.io, docs.rs, rustsec.org, pypi.org).
# Set CARCINIFY_NUMPY_PYTHON to an interpreter that has NumPy to run the
# decide tool's test with the real package.
# Set SUITES_ONLY=1 to stop after the test suites.
#
# It does not stop at the first failure: a failure is a line in the result,
# not a reason to hide the rest. At the end ci/compare.py sets the results
# beside ci/expected.tsv, and the exit status is 0 only if every check and
# suite passed with the expected number of tests, every example ended with
# the expected exit status, verdict and counts, the release build worked
# and the tree was clean before the run, unchanged after it, and at the same
# commit. 1 otherwise.
set -u
checkout=$(cd "$1" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
cd "$checkout" || exit 2
: "${CARCINIFY_PYTHON:?set CARCINIFY_PYTHON}" "${CARCINIFY_MATURIN:?set CARCINIFY_MATURIN}"
export CARCINIFY_PYTHON CARCINIFY_MATURIN
py=$CARCINIFY_PYTHON
results=$out/results.tsv
: > "$results"
now() { date +%s; }
# What git sees in the checkout, by line, and its commit, at the start and
# at the end.
record_tree() {
  git status --porcelain > "$out/tree-$1.txt"
  git rev-parse HEAD > "$out/head-$1.txt"
  echo "tree $1 the run: $(wc -l < "$out/tree-$1.txt" | tr -d ' ') change(s), HEAD $(cat "$out/head-$1.txt")"
  sed 's/^/  /' "$out/tree-$1.txt"
}
record_tree before

{
  echo "date_utc	$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "commit	$(git rev-parse HEAD)"
  echo "uname	$(uname -srm)"
  echo "libc	$( (command -v ldd > /dev/null && ldd --version 2>&1 | head -1) || echo "not glibc or musl: $(uname -s)")"
  echo "filesystem	$( (df -T . 2>/dev/null | tail -1 | awk '{print $2}') || true) $( (mount 2>/dev/null | grep " on / " | head -1) || true)"
  echo "rustc	$(rustc --version)"
  echo "cargo	$(cargo --version)"
  echo "python	$($py --version 2>&1) at $py"
  echo "python_gil	$($py -c 'import sys; print(getattr(sys, "_is_gil_enabled", lambda: True)())')"
  echo "maturin	$($CARCINIFY_MATURIN --version 2>&1)"
  echo "cpus	$($py -c 'import os; print(os.cpu_count())')"
} > "$out/versions.tsv"
cat "$out/versions.tsv"

# The tests that run the binary are test targets of the `carcinify` crate,
# each kept in the directory of the tool it tests; a tool's unit tests are in
# its own library crate. A suite takes both.
# suite <name> <cargo test arguments...>
# SUITE_DIR: the workspace to run it in, when not the checkout's own.
suite() {
  local name=$1; shift
  local start; start=$(now)
  (cd "${SUITE_DIR:-$checkout}" && cargo test --locked --no-fail-fast "$@") > "$out/suite-$name.log" 2>&1
  local status=$?
  local counts
  counts=$(grep '^test result:' "$out/suite-$name.log" | awk '{p+=$4; f+=$6; i+=$8} END {printf "%d passed, %d failed, %d ignored", p, f, i}')
  printf 'suite\t%s\t%s\texit %s\t%ss\n' "$name" "${counts:-no result}" "$status" "$(( $(now) - start ))" | tee -a "$results"
  # A test that could not run on this machine says so in a line with "SKIPPED,".
  grep 'SKIPPED,' "$out/suite-$name.log" | sed 's/^/note\t/' | tee -a "$results"
  if [ "$status" != 0 ]; then
    grep -E '^test .*FAILED|panicked|^error' -A8 "$out/suite-$name.log" | head -60
  fi
}

check() {
  local name=$1; shift
  "$@" > "$out/check-$name.log" 2>&1
  printf 'check\t%s\t\texit %s\t\n' "$name" "$?" | tee -a "$results"
}

check fmt cargo fmt --all --check
check clippy cargo clippy --all-targets --locked -- -D warnings

# The command itself, and that every test file of a tool is built.
suite command -p carcinify --test command
suite diff -p carcinify-diff -p carcinify --lib --test diff --test diff-example --test diff-mixed
suite diff-example-maturin -p carcinify --test diff-example -- --ignored
suite diff-mixed-maturin -p carcinify --test diff-mixed -- --ignored
suite structure -p carcinify-structure -p carcinify --lib --test structure
# Once more with one target directory for everything, as CI often has it:
# every test crate is called `port`, and they once shared a build that way.
CARGO_TARGET_DIR=$checkout/target/shared suite structure-shared-target -p carcinify-structure -p carcinify --lib --test structure
suite deps-recorded -p carcinify-deps -p carcinify --lib --test deps
# A decide test that is told the machine was busy runs the tool again. The
# repeats are counted here, so that a tool which began to say so on a quiet
# machine would show.
export CARCINIFY_DECIDE_ATTEMPTS=$out/decide-attempts.log
: > "$CARCINIFY_DECIDE_ATTEMPTS"
suite decide -p carcinify-decide -p carcinify --lib --test decide
if [ -n "${CARCINIFY_NUMPY_PYTHON:-}" ]; then
  suite decide-numpy -p carcinify --test decide -- --ignored
fi
printf 'note\tdecide tests: %s run(s) repeated because the tool said the machine was busy\n' "$(wc -l < "$CARCINIFY_DECIDE_ATTEMPTS" | tr -d ' ')" | tee -a "$results"
if [ "${RUN_LIVE:-0}" = 1 ]; then
  suite deps-live -p carcinify --test deps -- --ignored
fi
# speed: so far the reader of the host gate's log.
suite speed -p carcinify-speed --lib
# The worked examples' own tests. Each is a workspace of its own.
export PYO3_PYTHON=$py
SUITE_DIR=examples/float_roundtrip/rust suite example-float
SUITE_DIR=examples/float_roundtrip/rust suite example-float-fixed -p ledger-core --features float-roundtrip
SUITE_DIR=skill/steps/04-ownership-design/example/rust suite example-fleet
SUITE_DIR=skill/steps/04-ownership-design/example-removal/rust suite example-board

# Sets the results beside ci/expected.tsv and ends the run with its status.
finish() {
  "$py" "$checkout/ci/compare.py" "$checkout/ci/expected.tsv" "$results" | tee "$out/comparison.txt"
  local status=${PIPESTATUS[0]}
  echo "results in $results"
  exit "$status"
}

if [ "${SUITES_ONLY:-0}" = 1 ]; then
  printf 'note\tsuites only: the examples were not run\n' | tee -a "$results"
  finish
fi

cargo build --release --locked > "$out/build-release.log" 2>&1
printf 'check\trelease-build\t\texit %s\t\n' "$?" | tee -a "$results"
bin=$checkout/target/release

# example <tool> <name> <what to print from the report> <command...>
#
# The printing script gives two lines: what must be the same on every
# machine (verdict, counts), and what is judged by time and may differ
# (how many flags a time-based tool raised, how long a run took). Both go
# into the results; only the first is compared with ci/expected.tsv.
#
# carcinify decide answers `unreliable` when the machine was busy. That is
# the right answer and not the example's result, so the example is run
# again, up to three times, and the line says how many runs it took.
example() {
  local tool=$1 name=$2 show=$3; shift 3
  local start; start=$(now)
  local status facts varies tries=0
  while :; do
    tries=$((tries + 1))
    "$@" --report "$out/$tool-$name.json" > "$out/$tool-$name.log" 2>&1
    status=$?
    facts=$($py -c "$show" "$out/$tool-$name.json" 2>/dev/null || printf 'no report\n\n')
    case "$facts" in unreliable*) [ "$tries" -lt 3 ] && sleep 5 && continue ;; esac
    break
  done
  varies=$(printf '%s\n' "$facts" | sed -n 2p)
  facts=$(printf '%s\n' "$facts" | sed -n 1p)
  [ "$tries" -gt 1 ] && varies="$varies; runs made $tries"
  printf 'example\t%s %s\t%s\texit %s\t%ss\t%s\n' "$tool" "$name" "$facts" "$status" "$(( $(now) - start ))" "$varies" | tee -a "$results"
}

DIFF='import json,sys; r=json.load(open(sys.argv[1])); s=r.get("summary",{}); print(r["verdict"], "; ".join("%s %s" % (k, v) for k, v in sorted(s.items()) if isinstance(v, int))); print()'
STRUCTURE='import json,sys; r=json.load(open(sys.argv[1])); print(r["verdict"], "; ".join("%s: %s allocations, %s bytes" % (m["size"], m["allocations_per_iteration"], m["bytes_per_iteration"]) for h in r["hot_paths"] for m in h["measured"]), "; flags", len(r["flags"]), "; S3", sum(h["copying_constructs"] for h in r["hot_paths"])); print("ns per iteration:", ", ".join("%.1f" % m["nanoseconds_per_iteration"] for h in r["hot_paths"] for m in h["measured"]))'
DEPS='import json,sys; r=json.load(open(sys.argv[1])); print(r["verdict"], r["source"]["mode"], "; ".join("%s %s" % (d["import"], d["outcome"]) for d in r["dependencies"])); print()'
DECIDE='import json,sys; r=json.load(open(sys.argv[1])); k=r["flag_kinds"]; print(r["verdict"], r["where_the_time_goes"]["classification"], "; flags not judged by time:", ", ".join(sorted(f["kind"] for f in k if not f["judged_by_time"])) or "none"); print("flags judged by time: %s; fastest %.3f s" % (", ".join(sorted(f["kind"] for f in k if f["judged_by_time"])) or "none", r["workload"]["measured_with_nothing_attached"]["wall_seconds"]["fastest"]))'

for s in buggy fixed panic; do
  example diff "float-$s" "$DIFF" "$bin/carcinify" diff "examples/float_roundtrip/suite-$s.json"
done
for s in designed appeasement; do
  example diff "fleet-$s" "$DIFF" "$bin/carcinify" diff "skill/steps/04-ownership-design/example/suite-$s.json"
done
example diff board "$DIFF" "$bin/carcinify" diff skill/steps/04-ownership-design/example-removal/suite.json
for d in fleet-designed fleet-appeasement board-checked board-plain_index harness; do
  example structure "$d" "$STRUCTURE" "$bin/carcinify" structure "skill/steps/06-structure-report/declarations/$d.json"
done
for d in waiting/decide native/decide own-python/decide-original own-python/decide-after-fix own-python/decide-port; do
  example decide "$(echo "$d" | tr '/' '-')" "$DECIDE" "$bin/carcinify" decide "skill/steps/01-decide/examples/$d.json" --python "$py"
done
if [ "${RUN_LIVE:-0}" = 1 ]; then
  for c in example adk-case; do
    example deps "$c-live" "$DEPS" "$bin/carcinify" deps "skill/steps/02-dependency-check/$c/proposal.json" --cache "$out/deps-cache" --refresh --python "$py"
    example deps "$c-replay" "$DEPS" "$bin/carcinify" deps "skill/steps/02-dependency-check/$c/proposal.json" --cache "$out/deps-cache" --replay --python "$py"
  done
fi

record_tree after
head_before=$(cat "$out/head-before.txt") head_after=$(cat "$out/head-after.txt")
if [ "$head_before" = "$head_after" ]; then heads="HEAD unchanged"; else heads="HEAD $head_before at the start, $head_after at the end"; fi
printf 'tree\t%s change(s) before the run, %s after; %s\n' \
  "$(wc -l < "$out/tree-before.txt" | tr -d ' ')" "$(wc -l < "$out/tree-after.txt" | tr -d ' ')" "$heads" | tee -a "$results"
finish
