#!/bin/bash
start=$(date +%s)
/home/usea/seahorn/bin/seadsa --sea-dsa=ci --sea-dsa-callgraph-dot --sea-dsa-callgraph-stats --sea-dsa-dot-outdir=/work/seadsa-ci /work/php-cli-14-m2r.bc > /work/seadsa-ci/stdout 2> /work/seadsa-ci/stderr
status=$?
end=$(date +%s)
echo "{\"exit\": $status, \"seconds\": $((end - start)), \"memory_peak\": $(cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo null), \"oom_events\": \"$(grep oom_kill /sys/fs/cgroup/memory.events 2>/dev/null)\"}" > /work/seadsa-ci/result.json
chown -R ${HOST_UID}:${HOST_GID} /work/seadsa-ci
