#!/usr/bin/env bash
# Builds the image and runs everything in a Linux container.
#
#   ci/linux/run.sh [platform] [name]
#
# platform: linux/arm64 (default) or linux/amd64. name: the directory under
# target/ci/ that gets the results. Environment: RUN_LIVE=1, VENV_IN_TREE=1,
# SUITES_ONLY=1, NUMPY=1, CAP_SECONDS (default 7200), BUILD_ONLY=1 (build or
# reuse the image, and stop).
#
# Only two things of the host are mounted: the repository, read-only, and
# target/ci/ as scratch.
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
platform=${1:-linux/arm64}
name=${2:-$(echo "$platform" | tr '/' '-')}
image=carcinify-ci:$(echo "$platform" | tr '/' '-')
mkdir -p "$repo/target/ci"
# The image is built only when the tag is missing or was built from other
# inputs: the Dockerfile and the files it copies, hashed into a label. A
# rebuild fetches the system packages again, for an image of the same inputs.
# shortcut: COPY of plain files without spaces in their names; a directory
# would need hashing by its files.
sha() { if command -v sha256sum > /dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
inputs=$(cd "$repo/ci/linux" && sha Dockerfile $(awk '$1 == "COPY" { for (i = 2; i < NF; i++) if ($i !~ /^--/) print $i }' Dockerfile) | sha | cut -c1-64)
built=$(docker image inspect --format '{{ index .Config.Labels "carcinify.inputs" }}' "$image" 2> /dev/null || true)
if [ "$built" = "$inputs" ]; then
  echo "image $image: built from these inputs ($inputs), not built again"
else
  # --quiet prints nothing until the end, and a pull of the base image can
  # take ten minutes: say so, so that a slow pull is not taken for a hang.
  echo "image $image: building; it may pull the base image first, which can take about 10 minutes"
  docker build --quiet --platform "$platform" --label "carcinify.inputs=$inputs" -t "$image" "$repo/ci/linux"
fi
if [ "${BUILD_ONLY:-0}" = 1 ]; then exit 0; fi
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
