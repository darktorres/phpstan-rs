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
EXPECTED_LAYOUT_HEADER_SHA256 = (
    "601ed337205b2258ff0de217d40aaf38afd3a52f67a8edf0f91182414a6bcccd"
)
EXPECTED_PARSER_SOURCE_SHA256 = (
    "ba6d163b75905a3fc122186452a4cc08ab06b25e7fc51b22d830b38d5daca23b"
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
    layout_header = args.source.parent / "zend_types.h"
    layout_bytes = layout_header.read_bytes()
    layout_sha256 = hashlib.sha256(layout_bytes).hexdigest()
    if layout_sha256 != EXPECTED_LAYOUT_HEADER_SHA256:
        raise SystemExit(f"unexpected Zend layout header digest: {layout_sha256}")
    layout_text = layout_bytes.decode("utf-8")
    struct_match = re.search(r"struct _zend_string\s*\{([^}]+)\};", layout_text)
    if struct_match is None:
        raise SystemExit("locked Zend layout header lacks struct _zend_string")
    fields = [
        line.split("/*", 1)[0].strip().rstrip(";").split()
        for line in struct_match.group(1).splitlines()
        if line.strip() and ";" in line
    ]
    expected_fields = [
        ["zend_refcounted_h", "gc"],
        ["zend_ulong", "h"],
        ["size_t", "len"],
        ["char", "val[1]"],
    ]
    if fields != expected_fields:
        raise SystemExit(f"unexpected _zend_string fields: {fields!r}")
    parser_source = args.source.parent / "zend_API.c"
    parser_bytes = parser_source.read_bytes()
    parser_sha256 = hashlib.sha256(parser_bytes).hexdigest()
    if parser_sha256 != EXPECTED_PARSER_SOURCE_SHA256:
        raise SystemExit(f"unexpected Zend argument-parser source digest: {parser_sha256}")
    parser_text = parser_bytes.decode("utf-8")
    parser_start = parser_text.find("zend_parse_arg_str_weak(zval *arg")
    parser_end = parser_text.find("/* }}} */", parser_start)
    if parser_start < 0 or parser_end < 0:
        raise SystemExit("locked Zend source lacks zend_parse_arg_str_weak")
    parser_body = parser_text[parser_start:parser_end]
    weak_parser_contract = [
        'zend_null_arg_deprecated("string", arg_num)',
        "convert_to_string(arg);",
        "zobj->handlers->cast_object(zobj, &obj, IS_STRING)",
        "OBJ_RELEASE(zobj);",
        "*dest = Z_STR_P(arg);",
    ]
    missing_parser_facts = [fact for fact in weak_parser_contract if fact not in parser_body]
    if missing_parser_facts:
        raise SystemExit(f"Zend string parser contract changed: {missing_parser_facts}")
    strictness_by_function = {
        "zend_parse_arg_str_slow": [
            "if (UNEXPECTED(ZEND_ARG_USES_STRICT_TYPES())) {\n\t\treturn 0;\n\t}",
            "return zend_parse_arg_str_weak(arg, dest, arg_num);",
        ],
        "zend_flf_parse_arg_str_slow": [
            "if (UNEXPECTED(ZEND_FLF_ARG_USES_STRICT_TYPES())) {\n\t\treturn 0;\n\t}",
            "return zend_parse_arg_str_weak(arg, dest, arg_num);",
        ],
    }
    strictness_contract = []
    for function, facts in strictness_by_function.items():
        match = re.search(
            rf"ZEND_API bool ZEND_FASTCALL {function}\([^\n]*\)\s*(?:/\*.*?\*/\s*)?\{{.*?\n\}}",
            parser_text,
            re.DOTALL,
        )
        if match is None:
            raise SystemExit(f"locked Zend source lacks {function}")
        body = match.group(0)
        if any(fact not in body for fact in facts):
            raise SystemExit(f"Zend strict string parser contract changed: {function}")
        strictness_contract.extend(facts)
    parser_contract = weak_parser_contract + strictness_contract
    source_lines = source_bytes.decode("utf-8").splitlines()
    source_line = next(
        (index for index, line in enumerate(source_lines, 1)
         if "RETVAL_LONG(ZSTR_LEN(s));" in line),
        None,
    )
    if source_line is None:
        raise SystemExit("locked source lacks RETVAL_LONG(ZSTR_LEN(s))")
    function_start = next(
        (index for index, line in enumerate(source_lines, 1)
         if "ZEND_FUNCTION(strlen)" in line),
        None,
    )
    if function_start is None:
        raise SystemExit("locked source lacks ZEND_FUNCTION(strlen)")
    parse_markers = (
        "ZEND_PARSE_PARAMETERS_START(1, 1)",
        "Z_PARAM_STR(s)",
        "ZEND_PARSE_PARAMETERS_END();",
    )
    parse_lines = [
        {"line": index, "text": line.strip()}
        for index, line in enumerate(source_lines[function_start - 1:source_line], function_start)
        if any(marker in line for marker in parse_markers)
    ]
    if len(parse_lines) != 3:
        raise SystemExit("locked source parameter-parse macro sequence changed")
    if [next(marker for marker in parse_markers if marker in item["text"])
        for item in parse_lines] != list(parse_markers):
        raise SystemExit("locked source parameter-parse macro order changed")

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
    target_triple = re.search(r'^target triple = "([^"]+)"', ir, re.MULTILINE)
    if target_triple is None or target_triple.group(1) != "x86_64-pc-linux-gnu":
        raise SystemExit("linked module does not match the locked Linux x86-64 layout profile")
    helper_calls = {
        "parameter_count_error": "zend_wrong_parameters_count_error",
        "flf_strictness_gate": "zend_flf_parse_arg_str_slow",
        "general_strictness_gate": "zend_parse_arg_str_slow",
        "type_error": "zend_wrong_parameter_error",
    }
    missing_helpers = [
        name for name in helper_calls.values()
        if re.search(rf"\bcall\b[^\n]*@{re.escape(name)}\(", function) is None
    ]
    if missing_helpers:
        raise SystemExit(f"zif_strlen lacks configured parameter helpers: {missing_helpers}")
    helper_functions = {
        "zend_flf_parse_arg_str_slow": ["zend_parse_arg_str_weak"],
        "zend_parse_arg_str_slow": ["zend_parse_arg_str_weak"],
        "zend_parse_arg_str_weak": [
            "_convert_to_string",
            "zend_null_arg_deprecated",
            "zend_objects_store_del",
        ],
    }
    helper_witnesses = {}
    for name, required_callees in helper_functions.items():
        helper_bitcode = run([
            args.llvm_extract,
            f"--func={name}",
            str(args.linked_bitcode),
            "-o",
            "-",
        ])
        helper_ir = run([args.llvm_dis, "-o", "-"], input_bytes=helper_bitcode).decode("utf-8")
        if re.search(rf"^define .*@{re.escape(name)}\(", helper_ir, re.MULTILINE) is None:
            raise SystemExit(f"linked LLVM module lacks a definition for {name}")
        missing = [
            callee for callee in required_callees
            if re.search(rf"\bcall\b[^\n]*@{re.escape(callee)}\(", helper_ir) is None
        ]
        if missing:
            raise SystemExit(f"{name} lacks expected configured helper calls: {missing}")
        helper_witnesses[name] = {
            "extracted_bitcode_sha256": hashlib.sha256(helper_bitcode).hexdigest(),
            "extracted_ir_sha256": hashlib.sha256(helper_ir.encode()).hexdigest(),
            "calls": required_callees,
        }
    weak_ir = run([
        args.llvm_dis,
        "-o",
        "-",
    ], input_bytes=run([
        args.llvm_extract,
        "--func=zend_parse_arg_str_weak",
        str(args.linked_bitcode),
        "-o",
        "-",
    ])).decode("utf-8")
    weak_start = weak_ir.find("@zend_parse_arg_str_weak")
    weak_body_start = weak_ir.rfind("\ndefine ", 0, weak_start) + 1
    weak_body_end = weak_ir.find("\n}", weak_body_start)
    weak_function = weak_ir[weak_body_start:weak_body_end]
    object_handler_load = re.search(
        r"^\s*(%\d+) = getelementptr[^\n]*%struct\._zend_object_handlers,[^\n]*i32 0, i32 18[^\n]*$"
        r"(?:(?!^\s*%\d+ = getelementptr).*$\n)*?"
        r"^\s*(%\d+) = load ptr, ptr \1,[^\n]*$"
        r"(?:(?!^\s*%\d+ = getelementptr).*$\n)*?"
        r"^\s*%\d+ = call i32 \2\(",
        weak_function,
        re.MULTILINE,
    )
    if object_handler_load is None:
        raise SystemExit("weak string parser lost the indirect object cast handler path")
    if re.search(r"store ptr %\d+, ptr %\d+", weak_function) is None:
        raise SystemExit("weak string parser no longer writes the parsed string through dest")
    if re.search(
        r"(%\d+) = load ptr, ptr %12,[^\n]*\n\s*store ptr %\d+, ptr \1(?:, align \d+)?",
        weak_function,
    ) is None:
        raise SystemExit("weak string parser no longer writes its parsed string to the dest argument")
    field_load = re.search(
        r"getelementptr[^\n]*%struct\._zend_string,[^\n]*i32 0, i32 2"
        r"[^\n]*\n\s*%[^\n]*= load i64,[^\n]*!dbg !(\d+)",
        function,
    )
    if field_load is None:
        raise SystemExit("zif_strlen does not load _zend_string field 2 as i64")
    helper_destinations = re.finditer(
        r"call zeroext i1 @zend_(?:flf_)?parse_arg_str_slow\(ptr noundef %\d+, ptr noundef %(\d+),",
        function,
    )
    destination_chain = []
    for call in helper_destinations:
        prefix = function[:call.start()]
        destination_register = call.group(1)
        destination_loads = list(re.finditer(
            rf"^\s*%{destination_register} = load ptr, ptr %(\d+)", prefix, re.MULTILINE
        ))
        if not destination_loads:
            raise SystemExit("strlen argument parser destination is not loaded from a stack slot")
        destination_slot = destination_loads[-1].group(1)
        destination_stores = list(re.finditer(
            rf"^\s*store ptr %(\d+), ptr %{destination_slot}(?:, align \d+)?$",
            prefix,
            re.MULTILINE,
        ))
        if not destination_stores:
            raise SystemExit("strlen parser destination slot has no reaching store")
        source_register = destination_stores[-1].group(1)
        source_loads = list(re.finditer(
            rf"^\s*%{source_register} = load ptr, ptr %(\d+)", prefix, re.MULTILINE
        ))
        if not source_loads:
            raise SystemExit("strlen parser destination does not trace to its local string slot")
        source_slot = source_loads[-1].group(1)
        local_pointer_stores = list(re.finditer(
            rf"^\s*store ptr %(\d+), ptr %{source_slot}(?:, align \d+)?$",
            prefix,
            re.MULTILINE,
        ))
        if not local_pointer_stores:
            raise SystemExit("strlen parser destination is not initialized from a local string slot")
        local_slot = local_pointer_stores[-1].group(1)
        destination_chain.append((destination_slot, local_slot))
    parsed_string_slot = re.search(r"= load ptr, ptr %(\d+), align 8, !dbg !\d+\n"
                                   r"\s*%\d+ = getelementptr[^\n]*%struct\._zend_string", function)
    if len(destination_chain) != 2 or not destination_chain:
        raise SystemExit("expected both configured strictness paths to parse one string argument")
    local_slots = {local for _, local in destination_chain}
    if len(local_slots) != 1 or parsed_string_slot is None or parsed_string_slot.group(1) not in local_slots:
        raise SystemExit("strlen field load does not use the parser's output string slot")
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
            "parameter_parse_lines": parse_lines,
        },
        "parser_source": {
            "path": "Zend/zend_API.c",
            "sha256": parser_sha256,
            "functions": [
                "zend_parse_arg_str_weak",
                "zend_parse_arg_str_slow",
                "zend_flf_parse_arg_str_slow",
            ],
            "contract_facts": parser_contract,
        },
        "compiler": producer.group(1),
        "linked_bitcode": {
            "sha256": hashlib.sha256(bitcode).hexdigest(),
            "function": "zif_strlen",
            "extracted_bitcode_sha256": hashlib.sha256(extracted).hexdigest(),
            "extracted_ir_sha256": hashlib.sha256(ir.encode()).hexdigest(),
        },
        "target": {
            "triple": target_triple.group(1),
            "datalayout_has_64_bit_i64": 'i64:64' in ir.split('target datalayout = "', 1)[1].split('"', 1)[0],
        },
        "zend_string_layout": {
            "header": "Zend/zend_types.h",
            "sha256": layout_sha256,
            "fields": [
                {"index": index, "type": field[0], "name": field[1]}
                for index, field in enumerate(fields)
            ],
            "length_field_index": 2,
            "length_field_name": "len",
            "length_field_type": "size_t",
        },
        "mapping": {
            "debug_line_matches_source_return": True,
            "zend_string_length_field_index": 2,
            "loaded_as": "i64",
            "stored_as_return_zval_payload": True,
            "parameter_failure_path_present": True,
            "parsed_argument_flows_to_length_load": True,
            "parser_output_written_through_destination": True,
            "object_cast_handler_indirect_call": True,
            "object_cast_handler_field_index": 18,
        },
        "configured_implicit_operations": [
            {"operation": operation, "llvm_callee": callee, "catalog_domain": "open"}
            for operation, callee in helper_calls.items()
        ] + [
            {"operation": "weak_string_parse", "llvm_callee": "zend_parse_arg_str_weak",
             "catalog_domain": "open"},
            {"operation": "weak_string_conversion", "llvm_callee": "_convert_to_string",
             "catalog_domain": "open"},
            {"operation": "nullable_string_diagnostic", "llvm_callee": "zend_null_arg_deprecated",
             "catalog_domain": "open"},
            {"operation": "object_string_cast_callback", "llvm_callee": "indirect Zend object handler",
             "catalog_domain": "open"},
            {"operation": "object_release_during_cast", "llvm_callee": "zend_objects_store_del",
             "catalog_domain": "open"},
        ],
        "helper_witnesses": helper_witnesses,
        "argument_bridge": {
            "parser_destination_slots": [
                {"helper_argument_slot": destination, "strlen_local_slot": local}
                for destination, local in destination_chain
            ],
            "length_load_uses_local_slot": parsed_string_slot.group(1),
            "object_handler_field_index": 18,
        },
        "ir_witness": [line.strip() for line in function.splitlines()
                       if "%struct._zend_string" in line or "load i64" in line
                       or "store i64" in line
                       or any(f"@{callee}" in line for callee in helper_calls.values())],
    }
    rendered = json.dumps(witness, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
