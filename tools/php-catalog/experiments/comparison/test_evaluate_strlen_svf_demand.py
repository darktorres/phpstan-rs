import importlib.util
import unittest
from pathlib import Path


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


if __name__ == "__main__":
    unittest.main()
