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
import re
from pathlib import Path


IGNORED_KEYS = {
    # Raw digests of stderr and stdout include the timings and addresses named above; each
    # stdout file is compared after that normalization, and each stderr file is outcome-only.
    'max_rss_kib', 'sampled_group_peak_bytes', 'stderr_sha256', 'stdout_sha256',
    'pid', 'cgroup', 'systemd_messages', 'memory_peak_bytes', 'memory_events',
}
IGNORED_KEY_SUFFIXES = ('_seconds',)
IGNORED_FILES = {'.time.json', '.cgroup.json', 'completed.json'}
# Frontend and build-log stderr is compared by outcome, not by bytes: a frontend that crashes
# (exit 139) prints its diagnostics in an order that depends on buffering, and build logs record
# build warnings. The exit codes and classifications are compared through the JSON records.
OUTCOME_ONLY_FILES = ('.err',)
# A frontend that crashes prints its stack and its own diagnostics in an order that depends on
# buffering; such a file is compared by outcome only. Other stderr is compared as text.
CRASH_MARKERS = ('Stack dump:', 'PLEASE submit a bug report')
# Build trees are inputs to a run, not evidence; their binaries' digests are recorded in
# compact-lock.json and the gcc manifest, and those files are compared.
IGNORED_PREFIXES = ('compact-svf/', 'compact-svf-source/', 'php-gcc/')
# Text that varies between identical runs: timings printed by the analysis, and addresses
# in the stack traces that LLVM prints when a frontend rejects a unit (ASLR).
IGNORED_PATTERNS = [
    (re.compile(r'^(\S*Time)\s+[0-9.eE+-]+$', re.M), r'\1 <time>'),
    (re.compile(r'0x[0-9a-f]{6,}'), '0x<address>'),
    (re.compile(r'\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d+'), '<timestamp>'),
    (re.compile(r'^(AvgTime\S*|MemoryUsage\S*)\s+[0-9.eE+-]+$', re.M), r'\1 <measured>'),
]


def normalize(value):
    if isinstance(value, dict):
        return {key: normalize(item) for key, item in value.items()
                if key not in IGNORED_KEYS and not key.endswith(IGNORED_KEY_SUFFIXES)}
    if isinstance(value, list):
        return [normalize(item) for item in value]
    return value


def replace_runs(text, run_dirs):
    for directory in run_dirs:
        text = text.replace(str(directory), '<run>')
        # Records name a run's directory by the <cache> placeholder as well.
        text = text.replace('<cache>/' + directory.name, '<run>')
    for pattern, replacement in IGNORED_PATTERNS:
        text = pattern.sub(replacement, text)
    return text


def text_for(path, run_dirs):
    return replace_runs(path.read_bytes().decode('utf-8', errors='replace'), run_dirs)


def json_for(path, run_dirs):
    return replace_runs(json.dumps(normalize(json.loads(path.read_text())), sort_keys=True, indent=1), run_dirs)


def compare(run_a, run_b, also_runs=()):
    a, b = Path(run_a).resolve(), Path(run_b).resolve()
    runs = [a, b, *(Path(x).resolve() for x in also_runs)]
    names_a = {p.relative_to(a) for p in a.rglob('*') if p.is_file()}
    names_b = {p.relative_to(b) for p in b.rglob('*') if p.is_file()}
    differences = []
    for name in sorted(names_a ^ names_b, key=str):
        differences.append(f'{name}: present in only one run')
    for name in sorted(names_a & names_b, key=str):
        if any(str(name).endswith(suffix) for suffix in IGNORED_FILES):
            continue
        if str(name).startswith(IGNORED_PREFIXES):
            continue
        if str(name).endswith(OUTCOME_ONLY_FILES) and any(
                marker in (a / name).read_text(errors='replace') or marker in (b / name).read_text(errors='replace')
                for marker in CRASH_MARKERS):
            continue
        path_a, path_b = a / name, b / name
        if path_a.suffix == '.json':
            try:
                same = json_for(path_a, runs) == json_for(path_b, runs)
            except ValueError:
                same = text_for(path_a, runs) == text_for(path_b, runs)
        else:
            same = text_for(path_a, runs) == text_for(path_b, runs)
        if not same:
            differences.append(f'{name}: differs')
    return differences


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run_a', type=Path)
    parser.add_argument('run_b', type=Path)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--also-run', type=Path, action='append', default=[],
                        help='another run directory the records refer to; replaced like the two compared runs')
    args = parser.parse_args(argv)
    differences = compare(args.run_a, args.run_b, args.also_run)
    text = '\n'.join(differences) + ('\n' if differences else '')
    if args.report and differences:
        args.report.write_text(text)
    print(text or 'no unexplained difference')
    return 1 if differences else 0


if __name__ == '__main__':
    raise SystemExit(main())
