#!/usr/bin/env bash
# Times a port on Linux arm64 in the CI's container (design §7):
#
#   LOG=<host gate log> ci/linux/speed.sh <repo> <speed.json> <out>
#
# <repo>: the repository that holds the suite, this one or a port's own.
# <speed.json>: the declaration; outside <repo>, or committed in it.
# <out>: where wheels/ and report.json go, outside <repo>. GATE_EVERY,
# GATE_READINGS and GATE_LIMIT pass to ci/gate.sh, for checking this script.
# RUSTFLAGS, when set, passes to speed build, so that the port can be built
# for another target CPU.
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
carc=$(cd "$(dirname "$0")/../.." && pwd)
repo=$(cd "$1" && pwd)
decl=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
mkdir -p "$3" && out=$(cd "$3" && pwd)
log=${LOG:?set LOG to the host gate log}
touch "$log" && logdir=$(cd "$(dirname "$log")" && pwd)
BUILD_ONLY=1 "$carc/ci/linux/run.sh" linux/arm64
c=carcinify-speed-$$
mounts=(-v "$carc:/carc:ro" -v "$repo:/src:ro" -v "$out:$out" -v "$logdir:/gate:ro")
case $decl in "$repo"/*) ;; *) mounts+=(-v "$decl:$decl:ro") ;; esac
docker run -d --rm --name "$c" --platform linux/arm64 --entrypoint sleep "${mounts[@]}" \
  carcinify-ci:linux-arm64 infinity > /dev/null
trap 'docker rm -f "$c" > /dev/null' EXIT
# A docker exec returns 0 when interrupted; without these the script would
# go on, and could end with 0, the pass status.
trap 'exit 130' INT
trap 'exit 143' TERM
docker exec "$c" bash -euc '
  git config --global --add safe.directory "*"
  for pair in /carc:/work/carc "/src:$1"; do
    from=${pair%%:*} to=${pair#*:}
    git clone -q --no-checkout "$from" "$to" && git -C "$to" checkout -q "$(git -C "$from" rev-parse HEAD)"
  done
  cd /work/carc && cargo build --locked -q -p carcinify
  python3 -m venv /work/venv && /work/venv/bin/pip install --quiet maturin==1.16.0' _ "$repo"
tools=(-e CARCINIFY_PYTHON=/work/venv/bin/python -e CARCINIFY_MATURIN=/work/venv/bin/maturin)
[ -z "${RUSTFLAGS:-}" ] || tools+=(-e RUSTFLAGS)
speed=/work/carc/target/debug/carcinify
docker exec "${tools[@]}" "$c" "$speed" speed build "$decl" --out "$out/wheels"
# Not an earlier run's report read as this one's.
rm -f "$out/report.json"
LOG=$log "$carc/ci/gate.sh" docker exec -e GATE_RUN_ID "${tools[@]}" "$c" \
  "$speed" speed run "$decl" --wheels "$out/wheels" --report "$out/report.json" \
  --load-log "/gate/$(basename "$log")"
