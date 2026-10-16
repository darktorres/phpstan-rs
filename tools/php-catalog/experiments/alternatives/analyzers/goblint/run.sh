#!/bin/bash
start=$(date +%s)
cd /work/pre
ulimit -s unlimited
export OCAMLRUNPARAM=l=4000M
/opt/goblint/analyzer/bin/goblint --set dbg.timing.enabled true --set warn.info false --set warn.success false \
  --set mainfun '["main"]' *.i > /work/out/stdout 2> /work/out/stderr
status=$?
echo "{\"exit\": $status, \"seconds\": $(( $(date +%s) - start )), \"memory_peak\": $(cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo null), \"oom\": \"$(grep oom_kill /sys/fs/cgroup/memory.events 2>/dev/null)\"}" > /work/out/result.json
chown -R ${HOST_UID}:${HOST_GID} /work/out
