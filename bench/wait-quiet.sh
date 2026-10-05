#!/usr/bin/env bash
# Block until the machine is quiet enough to benchmark: 1-min load below QUIET_LOAD
# (default 3) for 3 consecutive checks 20 s apart. Gives up after QUIET_TIMEOUT s.
set -euo pipefail
limit="${QUIET_LOAD:-3}"; deadline=$(( $(date +%s) + ${QUIET_TIMEOUT:-3600} )); ok=0
while [ "$ok" -lt 3 ]; do
  load=$(cut -d' ' -f1 /proc/loadavg)
  if awk -v l="$load" -v m="$limit" 'BEGIN{exit !(l < m)}'; then ok=$((ok+1)); else ok=0; fi
  [ "$(date +%s)" -lt "$deadline" ] || { echo "machine never went quiet (load $load)" >&2; exit 1; }
  [ "$ok" -lt 3 ] && sleep 20
done
echo "quiet: load $(cut -d' ' -f1 /proc/loadavg)"
