#!/usr/bin/env python3
"""Extract a source-linked LLVM witness for PHP 8.5.4's strlen return."""

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

EXPECTED_SOURCE_SHA256 = (
    "0d4477170a78540131893a8cfca2b9d1033c79eab754f9ca3d85e05ae6919505"
)


def run(command: list[str], *, input_bytes: bytes | None = None) -> bytes:
    result = subprocess.run(command, input=input_bytes, capture_output=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {command!r}\n"
            f"{result.stderr.decode(errors='replace')}"
        )
    return result.stdout


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("linked_bitcode", type=Path)
    parser.add_argument("--llvm-extract", default="llvm-extract-21")
    parser.add_argument("--llvm-dis", default="llvm-dis-21")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    source_bytes = args.source.read_bytes()
    source_sha256 = hashlib.sha256(source_bytes).hexdigest()
    if source_sha256 != EXPECTED_SOURCE_SHA256:
        raise SystemExit(f"unexpected PHP source digest: {source_sha256}")
    source_lines = source_bytes.decode("utf-8").splitlines()
    source_line = next(
        (index for index, line in enumerate(source_lines, 1)
         if "RETVAL_LONG(ZSTR_LEN(s));" in line),
        None,
    )
    if source_line is None:
        raise SystemExit("locked source lacks RETVAL_LONG(ZSTR_LEN(s))")

    bitcode = args.linked_bitcode.read_bytes()
    extracted = run([
        args.llvm_extract,
        "--func=zif_strlen",
        str(args.linked_bitcode),
        "-o",
        "-",
    ])
    ir = run([args.llvm_dis, "-o", "-"], input_bytes=extracted).decode("utf-8")
    function_name = ir.find("@zif_strlen")
    start = ir.rfind("\ndefine ", 0, function_name) + 1
    end = ir.find("\n}", start)
    if start < 0 or end < 0:
        raise SystemExit("linked LLVM module lacks a zif_strlen definition")
    function = ir[start:end]
    field_load = re.search(
        r"getelementptr[^\n]*%struct\._zend_string,[^\n]*i32 0, i32 2"
        r"[^\n]*\n\s*%[^\n]*= load i64,[^\n]*!dbg !(\d+)",
        function,
    )
    if field_load is None:
        raise SystemExit("zif_strlen does not load _zend_string field 2 as i64")
    debug_id = field_load.group(1)
    metadata_pattern = rf"^{re.escape('!' + debug_id)} = !DILocation\(line: {source_line},"
    if re.search(metadata_pattern, ir, re.MULTILINE) is None:
        raise SystemExit("LLVM debug information does not map the load to the source return line")
    if "@zend_wrong_parameter_error" not in function:
        raise SystemExit("zif_strlen lost the parameter-parse failure path")
    if not re.search(r"store i64 %\d+, ptr %\d+", function[field_load.end():]):
        raise SystemExit("loaded string length is not stored as the return zval payload")
    producer = re.search(r'producer: "([^"]+)"', ir)
    if producer is None or "clang version 21.1.8" not in producer.group(1):
        raise SystemExit("LLVM debug metadata does not identify the locked Clang 21.1.8 producer")

    witness = {
        "schema": 1,
        "source": {
            "path": "Zend/zend_builtin_functions.c",
            "sha256": source_sha256,
            "function": "zif_strlen",
            "return_line": source_line,
            "expression": "RETVAL_LONG(ZSTR_LEN(s))",
        },
        "compiler": producer.group(1),
        "linked_bitcode": {
            "sha256": hashlib.sha256(bitcode).hexdigest(),
            "function": "zif_strlen",
            "extracted_bitcode_sha256": hashlib.sha256(extracted).hexdigest(),
            "extracted_ir_sha256": hashlib.sha256(ir.encode()).hexdigest(),
        },
        "mapping": {
            "debug_line_matches_source_return": True,
            "zend_string_length_field_index": 2,
            "loaded_as": "i64",
            "stored_as_return_zval_payload": True,
            "parameter_failure_path_present": True,
        },
        "ir_witness": [line.strip() for line in function.splitlines()
                       if "%struct._zend_string" in line or "load i64" in line
                       or "store i64" in line or "@zend_wrong_parameter_error" in line],
    }
    rendered = json.dumps(witness, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
