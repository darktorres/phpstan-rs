#!/usr/bin/env python3
"""Bind raw compiler-summary artifacts to successful campaign measurements."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def record_outputs(measurements_path, capture_root, bindings):
    measurements = json.loads(measurements_path.read_text())
    by_name = {}
    for record in measurements:
        by_name.setdefault(record.get("name"), []).append(record)
    for run_name, artifact_path, archive_path in bindings:
        runs = by_name.get(run_name, [])
        if len(runs) != 1 or runs[0].get("exit_code") != 0:
            raise ValueError(f"{run_name}: expected exactly one successful measurement")
        path = artifact_path.resolve()
        archive = archive_path.resolve()
        if not path.is_file():
            raise ValueError(f"{run_name}: output artifact does not exist: {path}")
        archive.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, archive)
        record = runs[0]
        captured_stdout = (capture_root / (run_name + ".out")).resolve()
        if not captured_stdout.is_file():
            raise ValueError(f"{run_name}: captured stdout is missing: {captured_stdout}")
        record["stdout_capture_path"] = str(captured_stdout)
        record["stdout_capture_sha256"] = sha(captured_stdout)
        stdout_archive = archive.parent / (run_name + ".out")
        shutil.copyfile(captured_stdout, stdout_archive)
        record["stdout_archive_path"] = str(stdout_archive.resolve())
        record["stdout_sha256"] = sha(stdout_archive)
        outputs = record.setdefault("output_artifacts", [])
        if any(output.get("path") == str(path) for output in outputs):
            raise ValueError(f"{run_name}: duplicate output artifact binding: {path}")
        outputs.append({"path": str(path), "archive_path": str(archive), "sha256": sha(archive)})
    measurements_path.write_text(json.dumps(measurements, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--measurements", type=Path, required=True)
    parser.add_argument("--capture-root", type=Path, required=True,
                        help="Runner output directory containing RUN.out captures")
    parser.add_argument(
        "--artifact", action="append", default=[], metavar="RUN=PATH",
        help="bind RUN=GENERATED_PATH=ARCHIVE_PATH to a successful campaign run; repeatable",
    )
    args = parser.parse_args()
    bindings = []
    for item in args.artifact:
        parts = item.split("=", 2)
        if len(parts) != 3 or any(not part for part in parts):
            parser.error(f"invalid --artifact {item!r}; expected RUN=GENERATED_PATH=ARCHIVE_PATH")
        bindings.append((parts[0], Path(parts[1]), Path(parts[2])))
    if not bindings:
        parser.error("at least one --artifact binding is required")
    record_outputs(args.measurements, args.capture_root.resolve(), bindings)


if __name__ == "__main__":
    main()
