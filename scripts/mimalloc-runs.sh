#!/bin/bash
# Decision 2: does mimalloc's purging cost the off run its page zeroing?
# Off mode on Piwigo, default options against MIMALLOC_PURGE_DELAY=-1 (no
# purging): mimalloc's exit statistics for each, two hyperfine rounds in
# alternating order on the gate binary, then one perf recording of each on
# the frame-pointer binary of Step 0. Every output is checked against the
# oracle's sha256.
W=/home/torres/.cache/prs-profile/mimalloc-2026-10-17
GATE=/home/torres/.cache/prs-profile/wall-2026-10-17/target-gate/gate/phpstan-rs
CG=/home/torres/.cache/prs-profile/regress-item5/bin/head-cg
PIW=/home/torres/piwigo17-frozen
CHECK="check-project --config phpstan-rs.toml --format json"
OFF="PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1"
ORACLE=8ff144c3fee775166369b993a58814e0f247845fbf850f94d0c41b57924488b1
mkdir -p "$W"
cd "$PIW" || exit 1
log() { echo "$(date -Is) $* loadavg=$(cut -d' ' -f1-3 /proc/loadavg)" | tee -a "$W/steps.log"; }
vm() { grep -E '^(compact_stall|compact_fail|compact_success|pgmigrate_success|thp_fault_alloc|pgfault|pgmajfault) ' /proc/vmstat > "$W/vmstat-$1.txt"; }
check() {
  local sum
  sum=$(sha256sum < "$1" | cut -d' ' -f1)
  [ "$sum" = "$ORACLE" ] && echo "$2: output is the oracle's" || echo "$2: output DIFFERS ($sum)"
}
for mode in default nopurge; do
  E=""
  [ $mode = nopurge ] && E="MIMALLOC_PURGE_DELAY=-1"
  log "stats $mode"
  env $OFF $E MIMALLOC_SHOW_STATS=1 nice -n 19 "$GATE" $CHECK > "$W/stats-$mode-out.json" 2> "$W/stats-$mode.txt"
  check "$W/stats-$mode-out.json" "stats $mode"
done
vm hf-before
log "hyperfine round 1 (default, nopurge)"
nice -n 19 hyperfine -N --warmup 2 --runs 10 --export-json "$W/hf1.json" \
  "env $OFF $GATE $CHECK" "env $OFF MIMALLOC_PURGE_DELAY=-1 $GATE $CHECK" > "$W/hf1.txt" 2>&1
log "hyperfine round 2 (nopurge, default)"
nice -n 19 hyperfine -N --warmup 2 --runs 10 --export-json "$W/hf2.json" \
  "env $OFF MIMALLOC_PURGE_DELAY=-1 $GATE $CHECK" "env $OFF $GATE $CHECK" > "$W/hf2.txt" 2>&1
vm hf-after
for mode in default nopurge; do
  E=""
  [ $mode = nopurge ] && E="MIMALLOC_PURGE_DELAY=-1"
  env $OFF $E nice -n 19 "$CG" $CHECK > /dev/null 2>&1
  vm "perf-$mode-before"
  log "perf $mode"
  nice -n 19 perf record -o "$W/perf-$mode.data" -F 499 -g -- env $OFF $E "$CG" $CHECK \
    > "$W/perf-$mode-out.json" 2> "$W/perf-$mode-stderr.txt"
  vm "perf-$mode-after"
  check "$W/perf-$mode-out.json" "perf $mode"
  perf script -i "$W/perf-$mode.data" --no-inline -F tid,time,ip,sym > "$W/$mode.script" 2> "$W/$mode.script.err"
done
log "done"
