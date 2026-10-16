#!/bin/bash
# run_limited.sh NAME SECONDS -- CMD...: runs CMD in a transient systemd scope with a 10 GiB memory
# cap (no swap) and a runtime cap, and records exit status, elapsed time and peak memory.
set -u
name=$1; secs=$2; shift 3
out=$HOME/.cache/php-alt-exp/exp1/runs/$name
mkdir -p "$out"
cat /proc/loadavg > "$out/loadavg-start"
start=$(date +%s)
systemd-run --user --scope --quiet --unit="altexp-$name-$start" -p MemoryMax=10G -p MemorySwapMax=0 -p OOMPolicy=continue -p RuntimeMaxSec="$secs" \
  nice -n 19 /usr/bin/time -v "$@" > "$out/stdout" 2> "$out/stderr"
status=$?
end=$(date +%s)
cat /proc/loadavg > "$out/loadavg-end"
peak=$(grep 'Maximum resident set size' "$out/stderr" | awk '{print $NF}')
echo "{\"name\": \"$name\", \"exit\": $status, \"seconds\": $((end - start)), \"peak_rss_kb\": ${peak:-null}}" > "$out/result.json"
cat "$out/result.json"
