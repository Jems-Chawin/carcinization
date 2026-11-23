#!/bin/bash
# The load rule for timing: the 1-minute load average under 6 at 30
# readings in a row, 10 s apart (five minutes), then the command, with a
# reading every 10 s while it runs. Every reading goes to the log, from the
# start of the wait.
#
#   LOG=<file> ci/gate.sh <command> [args...]
#
# The log is appended to, so it can hold several runs; each starts with a
# "wait:" line that states the rule and a random run id. Every line starts
# with the clock time and the epoch seconds; the reader judges gaps by the
# epoch seconds, so a wait across midnight is judged like any other. The command gets
# that id as GATE_RUN_ID, and a timed run checks that the log's last block
# carries it (carcinify speed, speed/src/gate.rs).
#
# Run it on the machine itself. Inside Docker on a Mac, /proc/loadavg is
# the load of Docker's VM, not of the Mac, and a gate there would open
# while the Mac is busy. Linux timing in a container is gated on the host:
# run this script on the Mac with `docker exec -e GATE_RUN_ID ...` as the
# command. Pass the id by name, as here: `-e GATE_RUN_ID=$GATE_RUN_ID`
# would be expanded by the shell before this script has made the id.
#
# GATE_EVERY, GATE_READINGS and GATE_LIMIT change the rule; they exist to
# check this script quickly, and a timed run refuses a log written with them.
set -u
log=${LOG:?set LOG to the log file}
every=${GATE_EVERY:-10}
need=${GATE_READINGS:-30}
limit=${GATE_LIMIT:-6}
id=$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')
stamp() { date '+%H:%M:%S %s'; }
load() {
  if [ -r /proc/loadavg ]; then cut -d' ' -f1-3 /proc/loadavg; else sysctl -n vm.loadavg | tr -d '{}' | xargs; fi
}
echo "$(date '+%Y-%m-%d %H:%M:%S %s') wait: $need readings under $limit, $every s apart, run $id, then: $*" >> "$log"
under=0
while [ "$under" -lt "$need" ]; do
  r=$(load)
  if awk -v x="${r%% *}" -v l="$limit" 'BEGIN { exit !(x < l) }'; then under=$((under + 1)); else under=0; fi
  echo "$(stamp) load $r ($under under $limit in a row)" >> "$log"
  [ "$under" -lt "$need" ] && sleep "$every"
done
echo "$(stamp) gate open; start: $*" >> "$log"
( while sleep "$every"; do echo "$(stamp) load $(load)" >> "$log"; done ) &
sampler=$!
GATE_RUN_ID=$id "$@"
status=$?
kill "$sampler" 2>/dev/null
wait "$sampler" 2>/dev/null
echo "$(stamp) end, exit $status; load $(load)" >> "$log"
exit "$status"
