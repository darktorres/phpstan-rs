"""Runs one off-mode Piwigo check and samples the process's memory every
20 ms: the largest Rss and AnonHugePages of /proc/<pid>/smaps_rollup, and the
system's thp_fault_alloc before and after. Extra NAME=VALUE arguments go into
the environment.

usage: thp-probe.py <binary> <out.json> [NAME=VALUE]...
"""
import hashlib
import os
import subprocess
import sys
import time

ORACLE = '8ff144c3fee775166369b993a58814e0f247845fbf850f94d0c41b57924488b1'


def vmstat(name):
    with open('/proc/vmstat') as fh:
        for line in fh:
            key, value = line.split()
            if key == name:
                return int(value)
    return 0


def rollup(pid):
    out = {}
    try:
        with open(f'/proc/{pid}/smaps_rollup') as fh:
            for line in fh:
                parts = line.split()
                if len(parts) >= 3 and parts[2] == 'kB':
                    out[parts[0].rstrip(':')] = int(parts[1])
    except OSError:
        pass
    return out


binary, out_path = sys.argv[1], sys.argv[2]
env = dict(os.environ, PHPSTAN_RS_NO_FILE_CACHE='1', PHPSTAN_RS_NO_STUB_CACHE='1')
for arg in sys.argv[3:]:
    key, _, value = arg.partition('=')
    env[key] = value
args = [binary, 'check-project', '--config', 'phpstan-rs.toml', '--format', 'json']
faults_before = vmstat('thp_fault_alloc')
start = time.monotonic()
with open(out_path, 'wb') as out:
    proc = subprocess.Popen(args, cwd='/home/torres/piwigo17-frozen', env=env, stdout=out,
                            stderr=subprocess.DEVNULL)
    peak_rss = peak_huge = 0
    while proc.poll() is None:
        r = rollup(proc.pid)
        peak_rss = max(peak_rss, r.get('Rss', 0))
        peak_huge = max(peak_huge, r.get('AnonHugePages', 0))
        time.sleep(0.02)
wall = time.monotonic() - start
faults = vmstat('thp_fault_alloc') - faults_before
with open(out_path, 'rb') as fh:
    digest = hashlib.sha256(fh.read()).hexdigest()
print(f'{" ".join(sys.argv[3:]) or "default"}: wall {wall:.3f} s, peak Rss {peak_rss / 1024:.0f} MiB, '
      f'peak AnonHugePages {peak_huge / 1024:.0f} MiB, thp_fault_alloc +{faults}, '
      f'output {"is the oracle" if digest == ORACLE else "DIFFERS " + digest}')
