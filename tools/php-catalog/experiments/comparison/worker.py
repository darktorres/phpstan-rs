#!/usr/bin/env python3
"""Run one evaluation job only inside an enforcing, fresh systemd cgroup."""
import json
import subprocess
import sys
from pathlib import Path

LIMIT = 10 * 1024**3


def main():
    report = Path(sys.argv[1])
    group = Path('/sys/fs/cgroup') / Path('/proc/self/cgroup').read_text().strip().split('::')[1].lstrip('/')
    memory_max = (group / 'memory.max').read_text().strip()
    swap_max = (group / 'memory.swap.max').read_text().strip()
    if memory_max != str(LIMIT) or swap_max != '0':
        raise RuntimeError(f'Unenforced acceptance worker: {memory_max=}, {swap_max=}')
    record = dict(cgroup=str(group), memory_max_bytes=int(memory_max), swap_max_bytes=int(swap_max),
                  aggregate_hard_limit_enforced=True, command=sys.argv[2:])
    report.write_text(json.dumps(record, indent=2) + '\n')
    process = subprocess.run(sys.argv[2:])
    record.update(exit_code=process.returncode,
                  memory_peak_bytes=int((group / 'memory.peak').read_text()),
                  memory_events=(group / 'memory.events').read_text())
    report.write_text(json.dumps(record, indent=2) + '\n')
    return process.returncode


if __name__ == '__main__':
    sys.exit(main())
