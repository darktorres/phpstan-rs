#!/bin/bash
# Scaling curve of the gate binary on Piwigo: hyperfine -N over the worker
# counts, off mode and warm mode. hyperfine discards stdout, so one more run
# per mode writes it, and its sha256 must be the oracle's.
W=/home/torres/.cache/prs-profile/wall-2026-10-17
BIN=$W/target-gate/gate/phpstan-rs
PIW=/home/torres/piwigo17-frozen
CHECK="check-project --config phpstan-rs.toml --format json"
ORACLE=8ff144c3fee775166369b993a58814e0f247845fbf850f94d0c41b57924488b1
cd "$PIW" || exit 1
for mode in off warm; do
  if [ $mode = off ]; then
    ENV="PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1"
  else
    ENV="PHPSTAN_RS_CACHE_DIR=$W/cache-warm"
  fi
  CMDS=()
  for t in 1 2 4 8; do
    CMDS+=("env PHPSTAN_RS_THREADS=$t $ENV $BIN $CHECK")
  done
  [ $mode = warm ] && touch "$W/warm-marker"
  echo "$(date -Is) hyperfine $mode loadavg=$(cut -d' ' -f1-3 /proc/loadavg)"
  nice -n 19 hyperfine -N --warmup 1 --runs 5 --export-json "$W/scaling-$mode.json" \
    "${CMDS[0]}" "${CMDS[1]}" "${CMDS[2]}" "${CMDS[3]}"
  echo "$(date -Is) hyperfine $mode exit $?"
  env $ENV $BIN $CHECK > "$W/scaling-$mode-out.json"
  sum=$(sha256sum < "$W/scaling-$mode-out.json" | cut -d' ' -f1)
  [ "$sum" = "$ORACLE" ] && echo "$mode output is the oracle's" || echo "$mode output DIFFERS: $sum"
done
echo "cache files written during the warm runs: $(find "$W/cache-warm" -type f -newer "$W/warm-marker" | wc -l)"
