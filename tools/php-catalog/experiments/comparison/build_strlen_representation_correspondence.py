#!/usr/bin/env python3
"""Validate the preserved, bounded PHP strlen compiler correspondence."""

import argparse
import hashlib
import json
from pathlib import Path
import re


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path):
    return json.loads(path.read_text())


def reachable(blocks, start, target) -> bool:
    by_id = {block["id"]: block for block in blocks}
    pending, seen = [start], set()
    while pending:
        current = pending.pop()
        if current == target:
            return True
        if current in seen or current not in by_id:
            continue
        seen.add(current)
        pending.extend(by_id[current].get("successors", []))
    return False


def gcc_cfg_blocks(function_text):
    matches = list(re.finditer(r"(?m)^  <bb (\d+)> :$", function_text))
    blocks = []
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(function_text)
        blocks.append({"id": int(match.group(1)), "text": function_text[match.end():end]})
    for index, block in enumerate(blocks):
        text = block["text"]
        successors = [int(value) for value in re.findall(r"goto <bb (\d+)>", text)]
        if index + 1 < len(blocks) and "if (" in text:
            successors.append(blocks[index + 1]["id"])
        elif (index + 1 < len(blocks) and not successors
              and "__builtin_unreachable" not in text and "return;" not in text):
            successors.append(blocks[index + 1]["id"])
        block["successors"] = sorted(set(successors))
    return blocks


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--clang-ast", type=Path, required=True)
    parser.add_argument("--clang-llvm", type=Path, required=True)
    parser.add_argument("--gcc-gimple", type=Path, required=True)
    parser.add_argument("--gcc-cfg", type=Path, required=True)
    parser.add_argument("--cir-log", type=Path, required=True)
    parser.add_argument("--cleanup-ast", type=Path, required=True)
    parser.add_argument("--cleanup-gimple", type=Path, required=True)
    parser.add_argument("--cleanup-llvm", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    source = args.source.read_text()
    source_hash = sha(args.source)
    expected_source_hash = "0d4477170a78540131893a8cfca2b9d1033c79eab754f9ca3d85e05ae6919505"
    if source_hash != expected_source_hash:
        raise SystemExit(f"unexpected locked PHP source digest: {source_hash}")
    if "ZEND_FUNCTION(strlen)" not in source or "RETVAL_LONG(ZSTR_LEN(s));" not in source:
        raise SystemExit("source lacks the locked strlen registration/return expression")

    ast_doc, llvm_doc = load(args.clang_ast), load(args.clang_llvm)
    ast_fn = next((f for f in ast_doc["functions"] if f.get("name") == "zif_strlen"), None)
    llvm_fn = next((f for f in llvm_doc["functions"] if f.get("name") == "zif_strlen"), None)
    if ast_fn is None or llvm_fn is None:
        raise SystemExit("Clang AST/CFG or LLVM summary lacks zif_strlen")
    if ast_doc.get("representation") != "clang-ast-cfg" or llvm_doc.get("representation") != "llvm":
        raise SystemExit("unexpected Clang summary representation kinds")
    llvm_blocks = llvm_fn.get("blocks", [])
    llvm_text = "\n".join(s.get("text", "") for b in llvm_blocks
                            for s in b.get("statements", []))
    llvm_length_load = bool(re.search(
        r"getelementptr[^\n]*%struct\._zend_string[^\n]*i32 0, i32 2", llvm_text))
    length_value = re.search(r"(%[-a-zA-Z$._0-9]+) = load i64, ptr %len", llvm_text)
    llvm_length_to_result = bool(length_value and re.search(
        rf"store i64 {re.escape(length_value.group(1))}, ptr %value", llvm_text))
    if not llvm_length_load or not llvm_length_to_result:
        raise SystemExit("Clang LLVM lacks the string-length field load to zval result store")
    llvm_result_blocks = [b["id"] for b in llvm_blocks if any(
        "%len = getelementptr" in s.get("text", "") for s in b.get("statements", [])) and any(
        length_value and f"store i64 {length_value.group(1)}, ptr %value" in s.get("text", "")
        for s in b.get("statements", []))]
    llvm_return_blocks = [b["id"] for b in llvm_blocks if any(
        s.get("kind") == "ret" for s in b.get("statements", []))]
    llvm_path = bool(llvm_result_blocks and llvm_return_blocks and any(
        reachable(llvm_blocks, result_id, ret_id)
        for result_id in llvm_result_blocks for ret_id in llvm_return_blocks))
    if not llvm_path:
        raise SystemExit("Clang LLVM result store does not reach a normal return")
    ast_blocks = ast_fn.get("blocks", [])
    ast_return = [s for b in ast_blocks for s in b.get("statements", [])
                  if s.get("kind") == "MemberExpr" and s.get("field") == "len"]
    ast_assign = any(".value.lval = (s)->len" in s.get("text", "")
                     for b in ast_blocks for s in b.get("statements", []))
    ast_parser = any("zend_parse_arg_str" in s.get("text", "")
                     for b in ast_blocks for s in b.get("statements", []))
    ast_result_blocks = [b["id"] for b in ast_blocks if any(
        s.get("kind") == "MemberExpr" and s.get("field") == "len"
        for s in b.get("statements", [])) and any(
        ".value.lval = (s)->len" in s.get("text", "")
        for s in b.get("statements", []))]
    ast_parser_blocks = [b["id"] for b in ast_blocks if any(
        "zend_parse_arg_str" in s.get("text", "") for s in b.get("statements", []))]
    ast_path = bool(ast_result_blocks and ast_parser_blocks and any(
        reachable(ast_blocks, parse_id, result_id)
        and reachable(ast_blocks, result_id, ast_fn["exit"])
        for parse_id in ast_parser_blocks for result_id in ast_result_blocks))
    if not ast_return or not ast_assign or not ast_parser or not ast_path:
        raise SystemExit("Clang CFG does not expose the parse/length/result bridge")

    gimple, cfg = args.gcc_gimple.read_text(), args.gcc_cfg.read_text()
    gimple_fn_match = re.search(r"(?ms)^void zif_strlen \(.*?^}\s*$", gimple)
    cfg_fn_match = re.search(r"(?ms)^void zif_strlen \(.*?^}\s*$", cfg)
    if not gimple_fn_match or not cfg_fn_match:
        raise SystemExit("GCC dump lacks zif_strlen function body")
    gimple_fn, cfg_fn = gimple_fn_match.group(), cfg_fn_match.group()
    gcc_parse = "zend_parse_arg_str (_arg, &s" in gimple_fn and "zend_parse_arg_str (_arg, &s" in cfg_fn
    gcc_len_return = "s.28_52->len" in gimple_fn and "__z->value.lval = _54" in gimple_fn
    gcc_blocks = gcc_cfg_blocks(cfg_fn)
    gcc_parse_blocks = [b["id"] for b in gcc_blocks if "zend_parse_arg_str (_arg, &s" in b["text"]]
    gcc_result_blocks = [b["id"] for b in gcc_blocks
                         if "s.28_52->len" in b["text"] and "__z->value.lval = _54" in b["text"]]
    gcc_exit_blocks = [b["id"] for b in gcc_blocks if "return;" in b["text"]]
    gcc_path = bool(gcc_parse_blocks and gcc_result_blocks and gcc_exit_blocks and any(
        reachable(gcc_blocks, parse_id, result_id)
        and reachable(gcc_blocks, result_id, exit_id)
        for parse_id in gcc_parse_blocks for result_id in gcc_result_blocks
        for exit_id in gcc_exit_blocks))
    if not gcc_parse or not gcc_len_return or "s.28_52->len" not in cfg_fn or not gcc_path:
        raise SystemExit("GCC GIMPLE/CFG does not expose the parse/length/result bridge")

    cir = load(args.cir_log)
    cir_units = [u for u in cir.get("units", [])
                 if u.get("source", "").endswith("/Zend/zend_builtin_functions.c")]
    cir_status = "unsupported_or_failed"
    if cir_units:
        emitted = [u for u in cir_units if u.get("emitted")]
        cir_status = "emitted" if emitted else "failed"

    cleanup_ast, cleanup_llvm = load(args.cleanup_ast), load(args.cleanup_llvm)
    ast_cleanup = next((f for f in cleanup_ast.get("functions", [])
                        if f.get("name") == "cleanup"), None)
    llvm_cleanup = next((f for f in cleanup_llvm.get("functions", [])
                         if f.get("name") == "cleanup"), None)
    cleanup_gimple = args.cleanup_gimple.read_text()
    if ast_cleanup is None or llvm_cleanup is None:
        raise SystemExit("cleanup counterexample lacks AST or LLVM representation")
    ast_has_destructor_edge = any(
        "Guard::~Guard" in s.get("text", "") for b in ast_cleanup.get("blocks", [])
        for s in b.get("statements", []))
    llvm_has_unwind_cleanup = any(
        s.get("kind") in {"invoke", "landingpad", "cleanuppad", "cleanupret"}
        for b in llvm_cleanup.get("blocks", []) for s in b.get("statements", []))
    gcc_has_eh_cleanup = "finally" in cleanup_gimple and "Guard::~Guard (&guard)" in cleanup_gimple
    if not llvm_has_unwind_cleanup or not gcc_has_eh_cleanup:
        raise SystemExit("cleanup LLVM/GCC artifact lacks the expected exceptional cleanup")

    artifacts = {}
    for label, path in vars(args).items():
        if isinstance(path, Path) and label != "output":
            artifacts[label] = {"sha256": sha(path), "bytes": path.stat().st_size}
    report = {
        "schema": 1,
        "source_sha256": source_hash,
        "provenance_note": "The source hash is checked against the PHP source digest pinned in the LLVM witness. The preserved AST/LLVM summary outputs do not embed source hashes, so their per-file digests are recorded here and their campaign commands remain the external provenance record.",
        "scope": "PHP 8.5.4 zif_strlen parse-to-byte-length-return slice plus a separate C++ cleanup counterexample",
        "strlen_correspondence": {
            "source": {"function": "zif_strlen", "parser": "Z_PARAM_STR(s)",
                       "return": "RETVAL_LONG(ZSTR_LEN(s))"},
            "clang_ast_cfg": {"status": "matched", "blocks": ast_fn["block_count"],
                              "edges": ast_fn["edges"], "length_field_reads": len(ast_return),
                              "parser_call_present": ast_parser, "length_to_zval_lval": ast_assign,
                              "parser_to_result_to_exit_path": ast_path},
            "clang_llvm": {"status": "matched in Clang translation-unit LLVM",
                           "blocks": llvm_fn["block_count"], "instructions": llvm_fn["elements"],
                           "length_field_load": llvm_length_load,
                           "length_to_result_store": llvm_length_to_result,
                           "result_store_reaches_return": llvm_path},
            "gcc_gimple_cfg": {"status": "matched", "gimple_parser_call": gcc_parse,
                               "gimple_length_to_result": gcc_len_return,
                               "cfg_parser_to_result_to_exit_path": gcc_path},
            "cir": {"status": cir_status,
                    "interpretation": "No CIR semantic correspondence can be claimed when this translation unit does not emit CIR."},
            "agreement": "Clang AST/CFG, Clang LLVM summary, and GCC GIMPLE/CFG retain the selected parse-to-length result path. This is a structural correspondence for the selected demand, not a general semantic equivalence proof.",
        },
        "cleanup_counterexample": {
            "clang_ast_cfg_blocks": ast_cleanup["block_count"],
            "ast_has_explicit_destructor_edge": ast_has_destructor_edge,
            "gcc_gimple_has_exceptional_finally_destructor": gcc_has_eh_cleanup,
            "llvm_has_unwind_cleanup_blocks": llvm_has_unwind_cleanup,
            "finding": "GCC GIMPLE and Clang LLVM retain exceptional cleanup; the Clang AST CFG artifact has no explicit Guard destructor edge. Do not use that CFG alone to claim cleanup preservation.",
        },
        "unsupported_coverage": [
            "CIR could not emit the selected PHP translation unit; no CIR-to-LLVM or CIR semantic comparison is supported.",
            "Clang analyzer results are fixture-level probes, not a PHP zif_strlen analysis.",
            "The structural crosswalk does not compare all transfers, path conditions, ABI effects, or exceptional semantics.",
            "This does not close ZEND_STRLEN VM-opcode correspondence or any open PHP effect/exception domain.",
        ],
        "artifacts": artifacts,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
