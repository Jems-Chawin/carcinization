#!/usr/bin/env bash
# Times a port on Linux arm64 in the CI's container (design §7):
#
#   LOG=<host gate log> ci/linux/speed.sh <repo> <speed.json> <out>
#
# <repo>: the repository that holds the suite, this one or a port's own; a
# full clone, not a linked worktree, whose .git names a path not mounted.
# <speed.json>: the declaration; outside <repo>, or committed in it.
# <out>: where wheels/ and report.json go; <out> and <repo> must not hold
# one another. The script exits with speed's status, or with 2, as speed
# does, when it cannot run: its own steps failed, or the cap ended the
# container under speed. Interrupted, it exits 130 or 143. Stop it by its
# process group, with Ctrl-C or kill -TERM -<pgid>: a signal to its PID
# alone waits for the current step, so a speed run goes on to its end and
# writes its report before the script exits 143. GATE_EVERY,
# GATE_READINGS and GATE_LIMIT pass to ci/gate.sh, for checking this script.
# RUSTFLAGS, when set, passes to speed build, so that the port can be built
# for another target CPU. PIP_PACKAGES, when set, is installed beside
# maturin in the container's venv, for a caller that imports them, as
# PIP_PACKAGES='numpy==2.5.4'; pin exact versions. The container ends after CAP_SECONDS (default
# 7200), as run.sh caps a run, the gate's wait included; one left by a
# killed script ends then.
#
# carcinify is built from this repository's commit, and <repo> is cloned at
# its commit, both inside the container: the harness builds in the crate's
# workspace, and a bind mount carries the host's file times (see
# in-container.sh). The clone sits at <repo>'s own path, so the paths in the
# declaration hold there. [RUST-148] speed build runs first, not gated. Then
# the host's ci/gate.sh, since /proc/loadavg in Docker on a Mac is the VM's,
# wraps a docker exec of speed run, given GATE_RUN_ID by name [RUST-149],
# which reads the host's log through a read-only mount.
set -eu
trap 'exit 2' ERR
if [ $# -ne 3 ] || [ -z "${LOG:-}" ]; then
  echo "usage: LOG=<host gate log> $0 <repo> <speed.json> <out>" >&2
  exit 2
fi
carc=$(cd "$(dirname "$0")/../.." && pwd)
repo=$(cd "$1" && pwd)
decl=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
mkdir -p "$3"
out=$(cd "$3" && pwd)
# <out> is mounted read-write: inside <repo> it would write into the suite's
# tree, and holding <repo> it would show the host's tree where the clone goes.
o=$(cd "$out" && pwd -P)/ r=$(cd "$repo" && pwd -P)/
if [[ $o == "$r"* || $r == "$o"* ]]; then
  echo "$0: <out> $out and <repo> $repo hold one another; give an <out> outside <repo>" >&2
  exit 2
fi
# Not an earlier run's report read as this one's, however this one ends.
# After the refusal above, so that it never removes a file in <repo>.
rm -f "$out/report.json"
log=$LOG
touch "$log"
logdir=$(cd "$(dirname "$log")" && pwd)
BUILD_ONLY=1 "$carc/ci/linux/run.sh" linux/arm64
c=carcinify-speed-$$
mounts=(-v "$carc:/carc:ro" -v "$repo:/src:ro" -v "$out:$out" -v "$logdir:/gate:ro")
case $decl in "$repo"/*) ;; *) mounts+=(-v "$decl:$decl:ro") ;; esac
docker run -d --rm --name "$c" --platform linux/arm64 --entrypoint sleep "${mounts[@]}" \
  carcinify-ci:linux-arm64 "${CAP_SECONDS:-7200}" > /dev/null
# Not failing here: under the ERR trap, a container already gone would turn
# speed's status into 2.
trap 'docker rm -f "$c" > /dev/null || true' EXIT
# A docker exec returns 0 when interrupted; without these the script would
# go on, and could end with 0, the pass status.
trap 'exit 130' INT
trap 'exit 143' TERM
docker exec "$c" bash -euc '
  git config --global --add safe.directory "*"
  for pair in /carc:/work/carc "/src:$1"; do
    from=${pair%%:*} to=${pair#*:}
    git clone -q --no-checkout "$from" "$to"
    git -C "$to" checkout -q "$(git -C "$from" rev-parse HEAD)"
  done
  cd /work/carc
  cargo build --locked -q -p carcinify
  python3 -m venv /work/venv
  read -ra packages <<< "$2"
  /work/venv/bin/pip install --quiet maturin==1.16.0 "${packages[@]}"' _ "$repo" "${PIP_PACKAGES:-}"
tools=(-e CARCINIFY_PYTHON=/work/venv/bin/python -e CARCINIFY_MATURIN=/work/venv/bin/maturin)
[ -z "${RUSTFLAGS:-}" ] || tools+=(-e RUSTFLAGS)
speed=/work/carc/target/debug/carcinify
# speed's own status, passed through; but 2 when the cap has ended the
# container, where docker exec's 1 or 137 would read as speed's fail or a
# crash. Docker still knows a container the cap has just ended, so running
# is asked, not known.
ran() { [ "$(docker inspect -f '{{.State.Running}}' "$c" 2> /dev/null)" = true ] || exit 2; exit "$1"; }
docker exec "${tools[@]}" "$c" "$speed" speed build "$decl" --out "$out/wheels" || ran $?
LOG=$log "$carc/ci/gate.sh" docker exec -e GATE_RUN_ID "${tools[@]}" "$c" \
  "$speed" speed run "$decl" --wheels "$out/wheels" --report "$out/report.json" \
  --load-log "/gate/$(basename "$log")" || ran $?
