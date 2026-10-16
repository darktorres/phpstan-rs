#!/bin/bash
start=$(date +%s)
/home/usea/seahorn/bin/seadsa --sea-dsa-callgraph-dot --sea-dsa-callgraph-stats --sea-dsa-dot-outdir=/work/seadsa-out /work/php-cli-14.bc > /work/seadsa-out/stdout 2> /work/seadsa-out/stderr
status=$?
end=$(date +%s)
echo "{\"exit\": $status, \"seconds\": $((end - start)), \"memory_peak\": $(cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo null), \"oom_events\": \"$(grep oom_kill /sys/fs/cgroup/memory.events 2>/dev/null)\"}" > /work/seadsa-out/result.json
chown -R ${HOST_UID}:${HOST_GID} /work/seadsa-out
