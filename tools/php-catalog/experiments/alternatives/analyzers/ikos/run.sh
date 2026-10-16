#!/bin/bash
mkdir -p /root/ikospkg && cp -r /ikos/analyzer/python/ikos /root/ikospkg/ && cp -r /ikos/build/analyzer/python/ikos/settings /root/ikospkg/ikos/
export PYTHONPATH=/root/ikospkg
start=$(date +%s)
cd /work/out
ikos -d interval -a dbz --opt=none --entry-points=main /in/php-cli-14b.bc > /work/out/stdout 2> /work/out/stderr
status=$?
echo "{\"exit\": $status, \"seconds\": $(( $(date +%s) - start )), \"memory_peak\": $(cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo null), \"oom\": \"$(grep oom_kill /sys/fs/cgroup/memory.events 2>/dev/null)\"}" > /work/out/result.json
chown -R ${HOST_UID}:${HOST_GID} /work/out
