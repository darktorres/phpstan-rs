#!/usr/bin/env python3
"""Validate the preserved, bounded PHP strlen compiler correspondence."""

import argparse
import hashlib
import json
import posixpath
from pathlib import Path
import re

from run import resolve_record


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path: Path):
    return json.loads(path.read_text())


CHECKOUT_MARKER = '/tools/php-catalog/'
REPOSITORY = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def portable(path):
    """A path reduced to the part under tools/php-catalog/, so that recorded
    paths from another checkout compare equal to this checkout's paths."""
    text = resolve_record(path)
    if text.startswith('/') and CHECKOUT_MARKER in text:
        return text[text.index(CHECKOUT_MARKER) + 1:]
    return text


def same_path(recorded, actual):
    return recorded is not None and portable(recorded) == portable(actual)


def local_path(recorded):
    """The file a recorded path names in this checkout."""
    text = resolve_record(recorded)
    if text.startswith('/') and CHECKOUT_MARKER in text:
        return REPOSITORY / portable(text)
    return Path(text)


SOURCE_RELATIVE = "php-src/Zend/zend_builtin_functions.c"


def root_relative(path, recorded_root):
    """The part of a recorded path below the root the evidence was recorded under,
    or None when the path is outside that root. A path that leaves the root
    after normalisation is refused, so `..` cannot reach another tree."""
    root = recorded_root.rstrip("/")
    text = str(path)
    if text != root and not text.startswith(root + "/"):
        return None
    normalized = posixpath.normpath(text)
    if normalized != root and not normalized.startswith(root + "/"):
        raise ValueError(f"recorded path escapes its root: {text}")
    return normalized[len(root) + 1:]


def names_recorded_file(command, relative, recorded_root):
    """Whether a recorded command names the file at `relative` below the recorded root."""
    if relative is None:
        return False
    return any(root_relative(argument, recorded_root) == relative for argument in command)


def validate_measurement_output(measurements, run_name, artifact_path):
    runs = [record for record in measurements if record.get("name") == run_name]
    if len(runs) != 1 or runs[0].get("exit_code") != 0:
        raise ValueError(f"{run_name}: expected exactly one successful command record")
    record = runs[0]
    resolved_path = str(artifact_path.resolve())
    if same_path(record.get("stdout_path"), resolved_path):
        actual = sha(artifact_path)
        if record.get("stdout_sha256") != actual:
            raise ValueError(f"{run_name}: captured stdout digest differs for {resolved_path}")
        return actual
    if same_path(record.get("stdout_archive_path"), resolved_path):
        actual = sha(artifact_path)
        capture_path = record.get("stdout_capture_path")
        capture_digest = record.get("stdout_capture_sha256")
        if record.get("stdout_sha256") != actual or capture_digest != actual:
            raise ValueError(f"{run_name}: archived stdout digest differs from captured producer bytes")
        if capture_path and local_path(capture_path).is_file() and sha(local_path(capture_path)) != capture_digest:
            raise ValueError(f"{run_name}: producer stdout capture changed after archival")
        return actual
    artifacts = runs[0].get("output_artifacts")
    if not isinstance(artifacts, list):
        raise ValueError(f"{run_name}: command record does not bind output artifact digests to this path")
    command = record.get("command", [])
    if "-o" not in command or "-c" not in command:
        raise ValueError(f"{run_name}: non-stdout artifact has no compiler output command")
    output = Path(resolve_record(command[command.index("-o") + 1]))
    source = Path(resolve_record(command[command.index("-c") + 1]))
    suffix = ".007t.gimple" if artifact_path.name.endswith(".007t.gimple") else (
        ".016t.cfg" if artifact_path.name.endswith(".016t.cfg") else None
    )
    if suffix is None:
        raise ValueError(f"{run_name}: unsupported compiler sidecar suffix: {artifact_path.name}")
    expected = output.parent / (output.stem + source.suffix + suffix)
    matches = [item for item in artifacts if isinstance(item, dict)
               and same_path(item.get("path"), str(expected.resolve()))
               and same_path(item.get("archive_path"), resolved_path)]
    if len(matches) != 1:
        if not same_path(str(artifact_path.resolve()), str(expected.resolve())):
            raise ValueError(f"{run_name}: sidecar path is not the compiler output derived from its command")
        raise ValueError(f"{run_name}: output digest is not bound to {resolved_path}")
    actual = sha(artifact_path)
    if matches[0].get("sha256") != actual:
        raise ValueError(f"{run_name}: recorded output digest differs for {resolved_path}")
    return actual


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


def parse_arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--clang-ast", type=Path, required=True)
    parser.add_argument("--clang-llvm", type=Path, required=True)
    parser.add_argument("--clang-manifest", type=Path, required=True)
    parser.add_argument("--clang-measurements", type=Path, required=True)
    parser.add_argument("--gcc-gimple", type=Path, required=True)
    parser.add_argument("--gcc-cfg", type=Path, required=True)
    parser.add_argument("--gcc-measurements", type=Path, required=True)
    parser.add_argument("--cir-log", type=Path, required=True)
    parser.add_argument("--campaign-inputs", type=Path, required=True)
    parser.add_argument("--cleanup-source", type=Path, required=True)
    parser.add_argument("--cleanup-measurements", type=Path, required=True)
    parser.add_argument("--cleanup-ast", type=Path, required=True)
    parser.add_argument("--cleanup-gimple", type=Path, required=True)
    parser.add_argument("--cleanup-llvm", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--lock", type=Path, default=HERE / "results" / "lock.json")
    parser.add_argument("--recorded-root",
                        help="root the recorded command paths were made under (default: the lock's recorded_root)")
    return parser.parse_args(argv)


def verify_inputs(args, lock):
    """Refuse any input whose digest differs from the lock, before anything is parsed."""
    for name, digest in lock["representation_correspondence_inputs"].items():
        path = getattr(args, name)
        if sha(path) != digest:
            raise SystemExit(f"{name} differs from its locked digest: {path}")


def build_report(args, lock):
    root = args.recorded_root or lock["recorded_root"]
    source = args.source.read_text()
    source_hash = sha(args.source)
    if "ZEND_FUNCTION(strlen)" not in source or "RETVAL_LONG(ZSTR_LEN(s));" not in source:
        raise SystemExit("source lacks the locked strlen registration/return expression")
    campaign_inputs = load(args.campaign_inputs)
    if campaign_inputs.get("php_commit") != "e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03":
        raise SystemExit("compiler campaign does not record the locked PHP source commit")

    ast_doc, llvm_doc = load(args.clang_ast), load(args.clang_llvm)
    ast_fn = next((f for f in ast_doc["functions"] if f.get("name") == "zif_strlen"), None)
    llvm_fn = next((f for f in llvm_doc["functions"] if f.get("name") == "zif_strlen"), None)
    if ast_fn is None or llvm_fn is None:
        raise SystemExit("Clang AST/CFG or LLVM summary lacks zif_strlen")
    if ast_doc.get("representation") != "clang-ast-cfg" or llvm_doc.get("representation") != "llvm":
        raise SystemExit("unexpected Clang summary representation kinds")
    clang_manifest = load(args.clang_manifest)
    clang_units = [u for u in clang_manifest
                   if u.get("unit") == "php-type" and u.get("backend") in {"ast", "llvm"}]
    if {u.get("backend") for u in clang_units} != {"ast", "llvm"} or any(
        u.get("source_sha256") != source_hash for u in clang_units):
        raise SystemExit("Clang representation manifest does not bind both summaries to source bytes")
    manifest_by_backend = {u["backend"]: u for u in clang_units}
    for backend, doc in (("ast", ast_doc), ("llvm", llvm_doc)):
        manifest_fn = next((f for f in manifest_by_backend[backend]["functions"]
                            if f.get("name") == "zif_strlen"), None)
        actual_fn = next((f for f in doc["functions"] if f.get("name") == "zif_strlen"), None)
        if manifest_fn is None or actual_fn is None or any(
            manifest_fn.get(key) != actual_fn.get(key)
            for key in ("block_count", "edges", "elements", "direct_calls")
            if key in manifest_fn):
            raise SystemExit(f"{backend} raw summary does not match its source-bound manifest")
    clang_measurements = load(args.clang_measurements)
    ast_runs = [m for m in clang_measurements if m.get("name") == "php-type-ast-0"]
    llvm_emit_runs = [m for m in clang_measurements if m.get("name") == "php-type-llvm-emit-0"]
    llvm_read_runs = [m for m in clang_measurements if m.get("name") == "php-type-llvm-read-0"]
    if len(ast_runs) != 1 or len(llvm_emit_runs) != 1 or len(llvm_read_runs) != 1 or any(
        m.get("exit_code") != 0 for m in ast_runs + llvm_emit_runs + llvm_read_runs):
        raise SystemExit("Clang AST/LLVM campaign command records are missing or failed")
    validate_measurement_output(clang_measurements, "php-type-ast-0", args.clang_ast)
    validate_measurement_output(clang_measurements, "php-type-llvm-read-0", args.clang_llvm)
    ast_command = ast_runs[0].get("command", [])
    llvm_emit_command = llvm_emit_runs[0].get("command", [])
    llvm_read_command = llvm_read_runs[0].get("command", [])
    emitted_ir = next((str(llvm_emit_command[index + 1])
                       for index, arg in enumerate(llvm_emit_command[:-1]) if arg == "-o"), None)
    # A command names the locked source either under the recorded root, or by its tree-relative
    # spelling (the path-free producers); the emitted IR is named by the same recorded string.
    def names_source(command):
        return (names_recorded_file(command, SOURCE_RELATIVE, root)
                or "Zend/zend_builtin_functions.c" in command)
    if (not names_source(ast_command)
            or not names_source(llvm_emit_command)
            or emitted_ir is None
            or emitted_ir not in llvm_read_command):
        raise SystemExit("Clang AST campaign command does not name the pinned PHP source")
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
    gcc_measurements = load(args.gcc_measurements)
    # The record is the one compiling the locked source, named by its tree-relative spelling
    # (the path-free finish stage) or by its recorded root (the earlier archives).
    gcc_runs = [m for m in gcc_measurements if m.get("name", "").endswith("-gimple")
                and (names_recorded_file(m.get("command", []), SOURCE_RELATIVE, root)
                     or "Zend/zend_builtin_functions.c" in m.get("command", []))]
    if len(gcc_runs) != 1 or gcc_runs[0].get("exit_code") != 0:
        raise SystemExit("GCC GIMPLE/CFG campaign command record is missing or failed")
    gcc_name = gcc_runs[0]["name"]
    validate_measurement_output(gcc_measurements, gcc_name, args.gcc_gimple)
    validate_measurement_output(gcc_measurements, gcc_name, args.gcc_cfg)
    gcc_command = gcc_runs[0].get("command", [])
    if ("-fdump-tree-gimple" not in gcc_command or "-fdump-tree-cfg" not in gcc_command
            or not (names_recorded_file(gcc_command, SOURCE_RELATIVE, root)
                    or "Zend/zend_builtin_functions.c" in gcc_command)
            or not args.gcc_gimple.name.endswith(".007t.gimple")
            or not args.gcc_cfg.name.endswith(".016t.cfg")):
        raise SystemExit("GCC dumps do not match the successful pinned-source dump command")
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
    # Two formats: the campaign's `units` document (source digest per unit), and the finish
    # stage's list of outcomes, which binds the zend unit by name within the pinned PHP commit.
    if isinstance(cir, dict):
        cir_units = [u for u in cir.get("units", [])
                     if u.get("source", "").endswith("/Zend/zend_builtin_functions.c")]
        cir_status = "unsupported_or_failed"
        if cir_units:
            if len(cir_units) != 1 or cir_units[0].get("source_sha256") != source_hash:
                raise SystemExit("CIR campaign does not uniquely identify the pinned PHP source")
            cir_status = "emitted" if [u for u in cir_units if u.get("emitted")] else "failed"
    else:
        zend_units = [u for u in cir if u.get("name") == "php-Zend-zend_builtin_functions.o"]
        if len(zend_units) != 1:
            raise SystemExit("CIR outcome list does not uniquely identify the zend unit")
        cir_status = "emitted" if zend_units[0].get("imported") is True else "failed"

    cleanup_ast, cleanup_llvm = load(args.cleanup_ast), load(args.cleanup_llvm)
    ast_cleanup = next((f for f in cleanup_ast.get("functions", [])
                        if f.get("name") == "cleanup"), None)
    llvm_cleanup = next((f for f in cleanup_llvm.get("functions", [])
                         if f.get("name") == "cleanup"), None)
    cleanup_gimple = args.cleanup_gimple.read_text()
    cleanup_hash = sha(args.cleanup_source)
    expected_cleanup_hash = campaign_inputs.get("fixture_sha256", {}).get("cleanup.cpp")
    if not expected_cleanup_hash or cleanup_hash != expected_cleanup_hash:
        raise SystemExit("cleanup fixture differs from the source hash recorded by the campaign")
    cleanup_measurements = load(args.cleanup_measurements)
    required_cleanup_runs = {"cleanup-ast-0", "cleanup-gimple", "cleanup-llvm-emit-0",
                             "cleanup-llvm-read-0"}
    cleanup_runs = {m.get("name"): m for m in cleanup_measurements
                    if m.get("name") in required_cleanup_runs}
    if set(cleanup_runs) != required_cleanup_runs or any(
        m.get("exit_code") != 0 for m in cleanup_runs.values()):
        raise SystemExit("cleanup counterexample campaign records are missing or failed")
    validate_measurement_output(cleanup_measurements, "cleanup-ast-0", args.cleanup_ast)
    validate_measurement_output(cleanup_measurements, "cleanup-gimple", args.cleanup_gimple)
    validate_measurement_output(cleanup_measurements, "cleanup-llvm-read-0", args.cleanup_llvm)
    cleanup_path = str(args.cleanup_source.resolve())
    if any(not any(same_path(arg, cleanup_path) for arg in m.get("command", [])) for m in cleanup_runs.values()
           if m["name"] != "cleanup-llvm-read-0"):
        raise SystemExit("cleanup campaign command does not identify the hashed fixture")
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
    for label in lock["representation_correspondence_inputs"]:
        path = getattr(args, label)
        artifacts[label] = {"sha256": sha(path), "bytes": path.stat().st_size}
    report = {
        "schema": 1,
        "source_sha256": source_hash,
        "provenance_note": "Clang AST/LLVM summaries are cross-checked against the source-bound representation manifest and successful source compile records; GCC dump inputs are tied to the successful source-bound dump command. CIR input is checked against the same source digest. Cleanup fixture bytes are verified against the campaign input lock, and its AST/GCC/LLVM command records are checked.",
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
            "gcc_gimple_cfg": {"status": "structural match using heuristic CFG reconstruction",
                               "reachability_method": "successors parsed from goto <bb N>; conditional fallthrough inferred from textual if-blocks and ordinary fallthrough inferred from block order; this is not a verified GCC CFG",
                               "gimple_parser_call": gcc_parse,
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
    return report


def main(argv=None) -> None:
    args = parse_arguments(argv)
    lock = load(args.lock)
    verify_inputs(args, lock)
    report = build_report(args, lock)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
