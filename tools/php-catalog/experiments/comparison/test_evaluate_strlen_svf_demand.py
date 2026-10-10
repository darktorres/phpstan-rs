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

    def test_main_stops_before_tool_execution_when_checked_in_fixture_is_stale(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(
                sys,
                "argv",
                [
                    str(SCRIPT),
                    "--linked-bitcode",
                    "/missing/php-cli.bc",
                    "--svf-bin",
                    "/missing/svf",
                    "--output",
                    temporary,
                ],
            ):
                with self.assertRaisesRegex(ValueError, "runtime fixture digest mismatch"):
                    EVALUATOR.main()


if __name__ == "__main__":
    unittest.main()
