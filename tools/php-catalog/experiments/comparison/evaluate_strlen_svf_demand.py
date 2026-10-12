#!/usr/bin/env python3
"""Replay the bounded SVF query for strlen's Zend object-cast boundary.

This probe is a fallback regression, not a producer. It records an empty SVF
target result on a capsule without the input object's construction path and
requires the consumer to keep that result open.
"""

import argparse
import contextlib
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(HERE))
from evaluation import EnforcedRunner  # noqa: E402
from run import record_text, svf_command  # noqa: E402


FUNCTIONS = [
    "zif_strlen",
    "zend_flf_parse_arg_str_slow",
    "zend_parse_arg_str_slow",
    "zend_parse_arg_str_weak",
]


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_runtime_fixture(
    runtime: dict[str, object], fixture_path: Path, expected_path: str
) -> str:
    fixtures = runtime.get("fixtures")
    if not isinstance(fixtures, list):
        raise ValueError("runtime witness lacks its fixture inventory")
    if any(not isinstance(fixture, dict) for fixture in fixtures):
        raise ValueError("runtime witness fixture inventory contains a malformed record")
    matches = [fixture for fixture in fixtures if fixture.get("path") == expected_path]
    if len(matches) != 1:
        raise ValueError(f"runtime witness must record exactly one {expected_path} fixture")
    expected = matches[0].get("sha256")
    if not isinstance(expected, str) or re.fullmatch(r"[0-9a-f]{64}", expected) is None:
        raise ValueError(f"runtime witness has an invalid SHA-256 for {expected_path}")
    actual = digest(fixture_path)
    if actual != expected:
        raise ValueError(
            f"runtime fixture digest mismatch for {expected_path}: "
            f"expected {expected}, got {actual}"
        )
    return expected


def validate_recorded_outputs(runtime: dict[str, object], root: Path) -> None:
    fixtures = runtime.get("fixtures")
    if not isinstance(fixtures, list) or any(
        not isinstance(fixture, dict) for fixture in fixtures
    ):
        raise ValueError("runtime witness lacks a valid fixture inventory")
    for fixture in fixtures:
        expected = fixture.get("output_sha256")
        if not isinstance(expected, str) or re.fullmatch(r"[0-9a-f]{64}", expected) is None:
            raise ValueError("runtime witness has an invalid output SHA-256")
        for field in ("hybrid_output", "call_output"):
            relative_path = fixture.get(field)
            if not isinstance(relative_path, str):
                raise ValueError(f"runtime witness lacks {field}")
            output_path = root / relative_path
            actual = digest(output_path)
            if actual != expected:
                raise ValueError(
                    f"runtime output digest mismatch for {relative_path}: "
                    f"expected {expected}, got {actual}"
                )


def validate_linked_source_identity(linked_bitcode: Path, lock: dict[str, object],
                                    link_manifest: dict[str, object],
                                    link_manifest_path: Path) -> str:
    source_commit = lock.get("php_commit")
    if not isinstance(source_commit, str) or not re.fullmatch(r"[0-9a-f]{40}", source_commit):
        raise ValueError("source lock lacks a full PHP commit identity")
    if link_manifest.get("php_commit") != source_commit:
        raise ValueError("linked-module manifest and source lock name different PHP commits")
    expected_manifest_hash = lock.get("link_manifest_sha256")
    if not isinstance(expected_manifest_hash, str) or digest(link_manifest_path) != expected_manifest_hash:
        raise ValueError("linked-module manifest digest differs from the source lock")
    expected_hash = link_manifest.get("linked_bitcode_sha256")
    if not isinstance(expected_hash, str) or re.fullmatch(r"[0-9a-f]{64}", expected_hash) is None:
        raise ValueError("linked-module manifest lacks a valid bitcode digest")
    if not linked_bitcode.is_file():
        raise ValueError(f"linked bitcode is unavailable: {linked_bitcode}")
    actual_hash = digest(linked_bitcode)
    if actual_hash != expected_hash:
        raise ValueError(
            f"linked bitcode is not the module bound to PHP {source_commit}: "
            f"expected {expected_hash}, got {actual_hash}"
        )
    return source_commit


def validate_source_lock(source_root: Path, source_digests: dict[str, str]) -> None:
    if not isinstance(source_digests, dict) or not source_digests:
        raise ValueError("comparison source lock is empty or malformed")
    for relative, expected in source_digests.items():
        if not isinstance(relative, str) or not isinstance(expected, str):
            raise ValueError("comparison source lock contains a malformed entry")
        path = (source_root / relative).resolve()
        if source_root.resolve() not in path.parents:
            raise ValueError(f"locked source path escapes comparison directory: {relative}")
        if not path.is_file() or digest(path) != expected:
            raise ValueError(f"comparison source lock digest mismatch: {relative}")


def run_checked(command: list[str]) -> bytes:
    result = subprocess.run(command, capture_output=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {command!r}\n"
            f"{result.stderr.decode(errors='replace')}"
        )
    return result.stdout


def extract_capsule(
    linked_bitcode: Path,
    output: Path,
    llvm_extract: str,
    *,
    include_std_handlers: bool,
) -> dict[str, object]:
    command = [llvm_extract, *(f"--func={name}" for name in FUNCTIONS)]
    if include_std_handlers:
        command.extend(["--func=zend_std_cast_object_tostring", "--glob=std_object_handlers"])
    command.extend(["--keep-const-init", str(linked_bitcode), "-o", str(output)])
    run_checked(command)
    return {
        "functions": FUNCTIONS + (["zend_std_cast_object_tostring"] if include_std_handlers else []),
        "globals": ["std_object_handlers"] if include_std_handlers else [],
        "extract_command": command,
        "path": output.name,
        "bytes": output.stat().st_size,
        "sha256": digest(output),
    }


def parse_dvf_result(text: str) -> dict[str, object]:
    def stat(name: str) -> int:
        match = re.search(rf"^{name}\s+(\d+)\s*$", text, re.MULTILINE)
        if match is None:
            raise ValueError(f"SVF output lacks {name}")
        return int(match.group(1))

    query_count = stat("NumOfQuery")
    out_of_budget_query_count = stat("NumOfOOBQuery")
    queries = re.findall(
        r"##<> Source Loc: \{[^\n]*\}\s*\nPtr \d+\s+PointsTo: \{([^}]*)\}",
        text,
    )
    if not queries:
        raise ValueError("SVF output lacks a printed indirect-call query")
    return {
        "query_count": query_count,
        "out_of_budget_query_count": out_of_budget_query_count,
        "query_points_to": ["" if targets.strip() in {"", "empty"} else targets.strip() for targets in queries],
        "source_locations": re.findall(r"##<> Source Loc: (\{[^\n]*\})", text),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--linked-bitcode", type=Path, required=True)
    parser.add_argument("--svf-bin", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--llvm-extract", default="llvm-extract-21")
    parser.add_argument("--opt", default="opt-21")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    runtime_path = ROOT / "tools/php-catalog/experiments/comparison/results/runtime/hybrid-call-8.5.4.json"
    runtime = json.loads(runtime_path.read_text())
    source_lock = json.loads((ROOT / "tools/php-catalog/experiments/comparison/results/lock.json").read_text())
    link_manifest_path = ROOT / "tools/php-catalog/experiments/comparison/results/evaluation/link-manifest.json"
    link_manifest = json.loads(link_manifest_path.read_text())
    validate_source_lock(HERE, source_lock.get("source_sha256"))
    source_commit = source_lock.get("php_commit")
    if link_manifest.get("php_commit") != source_commit:
        raise ValueError("linked-module manifest and source lock name different PHP commits")
    if runtime["source_commit"] != source_commit or "string-coercion-callback" not in runtime["result"]["runtime_php_sentinels"]:
        raise ValueError("locked runtime witness does not include the Stringable callback sentinel")
    fixtures = runtime.get("fixtures")
    if not isinstance(fixtures, list) or len(fixtures) != 2 or not isinstance(fixtures[0], dict):
        raise ValueError("locked runtime witness lacks its historical and sentinel fixture records")
    fixture_relative_path = fixtures[0].get("path")
    if not isinstance(fixture_relative_path, str):
        raise ValueError("locked runtime fixture path is malformed")
    runtime_file = ROOT / fixture_relative_path
    validate_runtime_fixture(runtime, runtime_file, fixture_relative_path)
    validate_recorded_outputs(runtime, ROOT)
    if "string-coercion-callback" not in runtime_file.read_text():
        raise ValueError("current runtime callback fixture lacks the witnessed sentinel")
    validate_linked_source_identity(args.linked_bitcode, source_lock, link_manifest, link_manifest_path)

    # The capsules are evidence: they are kept under the output directory, so the records name them
    # by a stable path rather than a random temporary name.
    capsule_root = args.output / "capsules"
    capsule_root.mkdir(parents=True, exist_ok=True)
    with contextlib.nullcontext(capsule_root) as temp_dir:
        capsule_path = Path(temp_dir) / "strlen-native-demand.bc"
        expanded_path = Path(temp_dir) / "strlen-native-demand-std.bc"
        capsule = extract_capsule(args.linked_bitcode, capsule_path, args.llvm_extract,
                                  include_std_handlers=False)
        expanded_capsule = extract_capsule(args.linked_bitcode, expanded_path, args.llvm_extract,
                                           include_std_handlers=True)
        run_checked([args.opt, "-passes=verify", str(capsule_path), "-o", "/dev/null"])
        run_checked([args.opt, "-passes=verify", str(expanded_path), "-o", "/dev/null"])

        runner = EnforcedRunner(args.output)
        dvf = args.svf_bin / "dvf"
        modes = {
            "dda": [],
            "dda_blackhole": ["-blk"],
            "dda_low_budget": ["-flow-bg=1"],
        }
        summaries: dict[str, dict[str, object]] = {}
        for mode, extra in modes.items():
            name = "svf-" + mode
            if not runner.run(name, svf_command(dvf, "-dfs", *extra, "-query=funptr", "-stat",
                                                "-print-query-pts", capsule_path), timeout=180):
                raise RuntimeError(f"SVF query failed in mode {mode}")
            output = (args.output / f"{name}.out").read_text(errors="replace")
            summaries[mode] = parse_dvf_result(output)
        expanded_name = "svf-expanded-default"
        if not runner.run(expanded_name, svf_command(dvf, "-dfs", "-query=funptr", "-stat",
                                                     "-print-query-pts", expanded_path), timeout=180):
            raise RuntimeError("SVF query failed with the standard-handler global included")
        expanded_output = (args.output / f"{expanded_name}.out").read_text(errors="replace")
        summaries["expanded_with_std_handlers"] = parse_dvf_result(expanded_output)

    for mode in ["dda", "dda_blackhole", "expanded_with_std_handlers"]:
        summary = summaries[mode]
        if summary["query_count"] != 1 or summary["out_of_budget_query_count"] != 0:
            raise ValueError(f"unexpected completed query accounting in {mode}: {summary}")
        if summary["query_points_to"] != [""]:
            raise ValueError(f"expected an empty printed points-to set in {mode}: {summary}")
        if not any('"ln": 769' in location for location in summary["source_locations"]):
            raise ValueError(f"expected the zend_API.c:769 cast-object call in {mode}: {summary}")
    budget = summaries["dda_low_budget"]
    if budget["query_count"] != 1 or budget["out_of_budget_query_count"] != 1:
        raise ValueError(f"low-budget query did not report incompleteness: {budget}")

    measurements = json.loads((args.output / "measurements.json").read_text())
    result = {
        "schema": 1,
        "php_source_commit": source_commit,
        "linked_module": {
            "path": record_text(args.linked_bitcode),
            "sha256": digest(args.linked_bitcode),
        },
        "capsules": [capsule, expanded_capsule],
        "llvm_verification": "both extracted capsules passed opt -passes=verify",
        "svf": {
            "source_commit": source_lock["svf_commit"],
            "llvm_version": source_lock["compiler_versions"]["llvm-config-21"],
            "build_identity": {
                "relocatable_patch_sha256": digest(HERE / "svf-relocatable-extapi.patch"),
                "extapi_sha256": digest(dvf.parent.parent / "lib" / "extapi.bc"),
            },
            "binary": {"path": record_text(dvf), "sha256": digest(dvf)},
            "measurements": measurements,
            "queries": summaries,
        },
        "runtime_witness": {
            "path": str(runtime_path.relative_to(ROOT)),
            "sha256": digest(runtime_path),
            "witnessed_fixture_sha256": runtime["fixtures"][0]["sha256"],
            "current_fixture_sha256": digest(runtime_file),
            "positive_sentinel": "string-coercion-callback",
        },
        "finding": {
            "empty_target_is_not_closed": True,
            "blackhole_adds_no_target": True,
            "low_budget_is_explicitly_out_of_budget": True,
            "boundary_reason": "capsule omits the input object's construction/handler initialization path",
            "coverage_action": "keep invocation and affected behavior domains open; do not prune the cast callback",
        },
    }
    (args.output / "strlen-native-demand-svf.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    print("SVF capsule queries reproduced the open-object-handler boundary.")


if __name__ == "__main__":
    main()
