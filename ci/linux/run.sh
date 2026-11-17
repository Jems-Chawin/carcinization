#!/usr/bin/env bash
# Builds the image and runs everything in a Linux container.
#
#   ci/linux/run.sh [platform] [name]
#
# platform: linux/arm64 (default) or linux/amd64. name: the directory under
# target/ci/ that gets the results. Environment: RUN_LIVE=1, VENV_IN_TREE=1,
# SUITES_ONLY=1, NUMPY=1, CAP_SECONDS (default 7200).
#
# Only two things of the host are mounted: the repository, read-only, and
# target/ci/ as scratch.
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
platform=${1:-linux/arm64}
name=${2:-$(echo "$platform" | tr '/' '-')}
image=carcinify-ci:$(echo "$platform" | tr '/' '-')
mkdir -p "$repo/target/ci"
docker build --quiet --platform "$platform" -t "$image" "$repo/ci/linux"
# A run that has not finished after CAP_SECONDS is stopped from outside the
# container, because a process hung inside it cannot be relied on to stop
# itself. What had finished by then is in results.tsv, with a line saying so.
cap=${CAP_SECONDS:-7200}
container=carcinify-ci-$$
(
  sleep "$cap"
  printf 'stopped\tnot finished after %s s; the container was stopped\n' "$cap" \
    >> "$repo/target/ci/$name/results.tsv"
  docker stop -t 10 "$container" > /dev/null
) &
watchdog=$!
status=0
docker run --rm --name "$container" --platform "$platform" \
  -v "$repo:/repo:ro" -v "$repo/target/ci:/out" \
  -e RUN_LIVE="${RUN_LIVE:-0}" -e VENV_IN_TREE="${VENV_IN_TREE:-0}" -e OUT_NAME="$name" \
  -e SUITES_ONLY="${SUITES_ONLY:-0}" \
  -e NUMPY="${NUMPY:-0}" \
  -e BRANCH="$(git -C "$repo" rev-parse --abbrev-ref HEAD)" \
  "$image" || status=$?
{
  pkill -P "$watchdog" || true
  kill "$watchdog" || true
  wait "$watchdog" || true
} 2> /dev/null
exit "$status"
