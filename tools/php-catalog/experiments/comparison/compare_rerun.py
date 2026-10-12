#!/usr/bin/env python3
"""Compare two evidence directories from independent runs, failing on any unexplained difference.

    compare_rerun.py RUN_A RUN_B [--report FILE]

Only the fields in IGNORED_KEYS may differ: timings, memory samples, process and cgroup
identifiers, and the systemd journal. A run's own directory is replaced by a placeholder
in text files before comparison, so a path inside one run's tree is not a difference.
Every other difference is listed. Exit status 0 means no unexplained difference.
"""

import argparse
import json
from pathlib import Path


IGNORED_KEYS = {
    'elapsed_seconds', 'user_seconds', 'system_seconds', 'max_rss_kib', 'sampled_group_peak_bytes',
    'pid', 'cgroup', 'systemd_messages', 'memory_peak_bytes', 'memory_events',
}
IGNORED_FILES = {'.time.json', '.cgroup.json'}


def normalize(value):
    if isinstance(value, dict):
        return {key: normalize(item) for key, item in value.items() if key not in IGNORED_KEYS}
    if isinstance(value, list):
        return [normalize(item) for item in value]
    return value


def text_for(path, run_dirs):
    text = path.read_bytes().decode('utf-8', errors='replace')
    for directory in run_dirs:
        text = text.replace(str(directory), '<run>')
    return text


def compare(run_a, run_b):
    a, b = Path(run_a).resolve(), Path(run_b).resolve()
    names_a = {p.relative_to(a) for p in a.rglob('*') if p.is_file()}
    names_b = {p.relative_to(b) for p in b.rglob('*') if p.is_file()}
    differences = []
    for name in sorted(names_a ^ names_b, key=str):
        differences.append(f'{name}: present in only one run')
    for name in sorted(names_a & names_b, key=str):
        if any(str(name).endswith(suffix) for suffix in IGNORED_FILES):
            continue
        path_a, path_b = a / name, b / name
        if path_a.suffix == '.json':
            try:
                same = normalize(json.loads(path_a.read_text())) == normalize(json.loads(path_b.read_text()))
            except ValueError:
                same = text_for(path_a, [a, b]) == text_for(path_b, [a, b])
        else:
            same = text_for(path_a, [a, b]) == text_for(path_b, [a, b])
        if not same:
            differences.append(f'{name}: differs')
    return differences


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run_a', type=Path)
    parser.add_argument('run_b', type=Path)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args(argv)
    differences = compare(args.run_a, args.run_b)
    text = '\n'.join(differences) + ('\n' if differences else '')
    if args.report and differences:
        args.report.write_text(text)
    print(text or 'no unexplained difference')
    return 1 if differences else 0


if __name__ == '__main__':
    raise SystemExit(main())
