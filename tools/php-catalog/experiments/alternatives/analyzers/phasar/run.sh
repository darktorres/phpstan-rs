#!/bin/bash
# run.sh OUTDIR ALIAS: one Phasar call-graph run inside the container, recording exit, time and peak memory.
out=$1; alias=$2
start=$(date +%s)
/phasar/build/tools/phasar-cli/phasar-cli --module /in/php-cli.bc --entry-points=__ALL__ --call-graph-analysis=otf \
  --alias-analysis=$alias --emit-cg-as-json --out /work/$out > /work/$out/stdout 2> /work/$out/stderr
status=$?
echo "{\"exit\": $status, \"seconds\": $(( $(date +%s) - start )), \"memory_peak\": $(cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo null), \"oom\": \"$(grep oom_kill /sys/fs/cgroup/memory.events 2>/dev/null)\"}" > /work/$out/result.json
chown -R ${HOST_UID}:${HOST_GID} /work/$out
