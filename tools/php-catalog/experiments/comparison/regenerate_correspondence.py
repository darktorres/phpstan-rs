#!/usr/bin/env python3
"""Regenerate the checked-in strlen representation correspondence.

    regenerate_correspondence.py --source PHP_SRC/Zend/zend_builtin_functions.c --output OUT.json

Every input other than `--source` is the checked-in evidence named below. The
lock pins each input's digest, and the builder refuses an input that differs
from its pin before parsing anything. `--source` is the locked PHP 8.5.4
`zend_builtin_functions.c`; the builder checks its digest, not its location.
"""

import argparse
from pathlib import Path

import build_strlen_representation_correspondence as builder


HERE = Path(__file__).resolve().parent
RESULTS = HERE / "results"
EVIDENCE = {
    "campaign_inputs": RESULTS / "main" / "inputs.json",
    "cir_log": RESULTS / "evaluation" / "cir-campaign.json",
    "clang_ast": RESULTS / "main" / "php-type-ast-0.out",
    "clang_llvm": RESULTS / "main" / "php-type-llvm-read-0.out",
    "clang_manifest": RESULTS / "main" / "representations.json",
    "clang_measurements": RESULTS / "main" / "measurements.json",
    "cleanup_ast": RESULTS / "main" / "cleanup-ast-0.out",
    "cleanup_gimple": RESULTS / "main" / "cleanup.cpp.007t.gimple",
    "cleanup_llvm": RESULTS / "main" / "cleanup-llvm-read-0.out",
    "cleanup_measurements": RESULTS / "main" / "measurements.json",
    "cleanup_source": HERE / "cleanup.cpp",
    "gcc_cfg": RESULTS / "main" / "php-type.c.016t.cfg",
    "gcc_gimple": RESULTS / "main" / "php-type.c.007t.gimple",
    "gcc_measurements": RESULTS / "main" / "measurements.json",
}


def main(argv=None) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--recorded-root")
    args = parser.parse_args(argv)

    command = ["--source", str(args.source), "--output", str(args.output)]
    if args.recorded_root:
        command += ["--recorded-root", args.recorded_root]
    for name, path in EVIDENCE.items():
        command += [f"--{name.replace('_', '-')}", str(path)]
    builder.main(command)


if __name__ == "__main__":
    main()
