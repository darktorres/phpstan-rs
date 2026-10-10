import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("evaluate_strlen_svf_demand.py")
SPEC = importlib.util.spec_from_file_location("evaluate_strlen_svf_demand", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
EVALUATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVALUATOR)


class DdaOutputTests(unittest.TestCase):
    def test_empty_completed_query_is_distinguished_from_cutoff(self):
        output = '''
NumOfOOBQuery       0
NumOfQuery          1
##<> Source Loc: { "ln": 769, "cl": 7, "fl": "Zend/zend_API.c" }
Ptr 66572 \tPointsTo: {empty}
'''
        parsed = EVALUATOR.parse_dvf_result(output)
        self.assertEqual(parsed["query_count"], 1)
        self.assertEqual(parsed["out_of_budget_query_count"], 0)
        self.assertEqual(parsed["query_points_to"], [""])

        cut_off = output.replace("NumOfOOBQuery       0", "NumOfOOBQuery       1")
        parsed_cut_off = EVALUATOR.parse_dvf_result(cut_off)
        self.assertEqual(parsed_cut_off["out_of_budget_query_count"], 1)
        self.assertEqual(parsed_cut_off["query_points_to"], [""])

    def test_missing_query_or_stats_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "lacks NumOfQuery"):
            EVALUATOR.parse_dvf_result("NumOfOOBQuery 0\n")
        with self.assertRaisesRegex(ValueError, "lacks a printed indirect-call query"):
            EVALUATOR.parse_dvf_result("NumOfOOBQuery 0\nNumOfQuery 0\n")

    def test_runtime_fixture_digest_rejects_one_byte_change(self):
        expected_path = "tools/php-catalog/experiments/comparison/runtime.php"
        original = b"<?php echo 'pinned';\n"
        expected_digest = hashlib.sha256(original).hexdigest()
        runtime = {"fixtures": [{"path": expected_path, "sha256": expected_digest}]}
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Path(temporary) / "runtime.php"
            fixture.write_bytes(original)
            self.assertEqual(
                EVALUATOR.validate_runtime_fixture(runtime, fixture, expected_path),
                expected_digest,
            )
            fixture.write_bytes(original + b" ")
            with self.assertRaisesRegex(ValueError, "runtime fixture digest mismatch"):
                EVALUATOR.validate_runtime_fixture(runtime, fixture, expected_path)

    def test_hybrid_and_call_output_artifacts_still_match_the_recorded_hashes(self):
        witness_path = (
            EVALUATOR.ROOT
            / "tools/php-catalog/experiments/comparison/results/runtime/hybrid-call-8.5.4.json"
        )
        runtime = json.loads(witness_path.read_text())
        EVALUATOR.validate_recorded_outputs(runtime, EVALUATOR.ROOT)

    def test_linked_bitcode_must_match_the_php_source_manifest(self):
        payload = b"module bytes"
        expected = hashlib.sha256(payload).hexdigest()
        lock = {"php_commit": "e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03"}
        manifest = {"php_commit": lock["php_commit"], "linked_bitcode_sha256": expected}
        with tempfile.TemporaryDirectory() as temporary:
            bitcode = Path(temporary) / "php-cli.bc"
            manifest_path = Path(temporary) / "link-manifest.json"
            bitcode.write_bytes(payload)
            manifest_path.write_text(json.dumps(manifest))
            lock["link_manifest_sha256"] = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
            self.assertEqual(EVALUATOR.validate_linked_source_identity(bitcode, lock, manifest, manifest_path), lock["php_commit"])
            bitcode.write_bytes(payload + b" altered")
            with self.assertRaisesRegex(ValueError, "not the module bound"):
                EVALUATOR.validate_linked_source_identity(bitcode, lock, manifest, manifest_path)
            bitcode.write_bytes(payload)
            bad_manifest = {**manifest, "php_commit": "0" * 40}
            manifest_path.write_text(json.dumps(bad_manifest))
            lock["link_manifest_sha256"] = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
            with self.assertRaisesRegex(ValueError, "different PHP commits"):
                EVALUATOR.validate_linked_source_identity(bitcode, lock, bad_manifest, manifest_path)

    def test_source_lock_rejects_changed_runner_or_fixture(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "run.py"
            source.write_bytes(b"pinned runner")
            lock = {"run.py": hashlib.sha256(source.read_bytes()).hexdigest()}
            EVALUATOR.validate_source_lock(root, lock)
            source.write_bytes(b"changed runner")
            with self.assertRaisesRegex(ValueError, "digest mismatch: run.py"):
                EVALUATOR.validate_source_lock(root, lock)

    def test_main_stops_before_tool_execution_when_linked_module_is_unpinned(self):
        with tempfile.TemporaryDirectory() as temporary:
            linked_bitcode = Path(temporary) / "php-cli.bc"
            linked_bitcode.write_bytes(b"not the pinned module")
            with patch.object(
                sys,
                "argv",
                [
                    str(SCRIPT),
                    "--linked-bitcode",
                    str(linked_bitcode),
                    "--svf-bin",
                    "/missing/svf",
                    "--output",
                    temporary,
                ],
            ):
                with self.assertRaisesRegex(ValueError, "not the module bound"):
                    EVALUATOR.main()


if __name__ == "__main__":
    unittest.main()
