#!/usr/bin/env bash
# Runs inside the container. The repository is mounted read-only at /repo
# and a scratch directory at /out. Nothing is built on the mount: the
# repository is cloned to the container's own filesystem first, because a
# bind mount from another operating system carries that system's file
# times, and the stale-build and stale-bytecode checks depend on them.
#
#   BRANCH        the branch to test (default: what /repo has checked out)
#   VENV_IN_TREE  1 to put the virtual environment inside the checkout
#   RUN_LIVE      1 to run the tests that use the network
#   NUMPY         1 to install NumPy in a second environment and run the
#                 decide tool's test that needs it
#   PYTHON        the interpreter to build the virtual environment from
set -eu
git config --global --add safe.directory /repo/.git
git config --global --add safe.directory /repo
branch=${BRANCH:-$(git -C /repo rev-parse --abbrev-ref HEAD)}
rm -rf /work/checkout
git clone --quiet --branch "$branch" /repo /work/checkout
venv=/work/venv
if [ "${VENV_IN_TREE:-0}" = 1 ]; then
  venv=/work/checkout/.venv
fi
# Debian's own Python refuses `pip install`; a virtual environment does not.
"${PYTHON:-python3}" -m venv "$venv"
"$venv/bin/pip" install --quiet maturin==1.16.0
export CARCINIFY_PYTHON=$venv/bin/python CARCINIFY_MATURIN=$venv/bin/maturin
if [ "${NUMPY:-0}" = 1 ]; then
  # A second environment, so that the first stays as the tests expect it.
  "${PYTHON:-python3}" -m venv /work/numpy-venv
  /work/numpy-venv/bin/pip install --quiet numpy==2.5.4
  export CARCINIFY_NUMPY_PYTHON=/work/numpy-venv/bin/python
fi
exec /work/checkout/ci/run-all.sh /work/checkout "/out/${OUT_NAME:-run}"
