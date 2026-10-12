import importlib.util
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location("campaign", Path(__file__).with_name("campaign.py"))
assert SPEC is not None and SPEC.loader is not None
CAMPAIGN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAMPAIGN)

GIB = 1024 ** 3


def gate_record(exit_code, oom_kill=1, kernel_oom=True, peak=10 * GIB, enforced=True):
    proof = {
        "aggregate_hard_limit_enforced": enforced,
        "memory_max_bytes": 10 * GIB,
        "memory_peak_bytes": peak,
        "memory_events": f"low 0\nhigh 0\nmax 9\noom 1\noom_kill {oom_kill}\n",
    }
    if kernel_oom is not None:
        proof["kernel_oom_kill"] = kernel_oom
    return {"name": "whole-compact-dda", "exit_code": exit_code, "resource_proof": proof}


class ResourceGateTests(unittest.TestCase):
    def test_an_oom_exit_with_kernel_proof_is_the_recorded_finding(self):
        self.assertTrue(CAMPAIGN.step_passed(gate_record(143)))
        self.assertTrue(CAMPAIGN.step_passed(gate_record(137)))

    def test_the_observed_wrapper_exit_247_passes_with_kernel_proof(self):
        self.assertTrue(CAMPAIGN.step_passed(gate_record(247)))

    def test_an_oom_exit_without_kernel_proof_fails(self):
        self.assertFalse(CAMPAIGN.step_passed(gate_record(143, kernel_oom=False)))
        self.assertFalse(CAMPAIGN.step_passed(gate_record(143, kernel_oom=None)))

    def test_an_oom_exit_without_a_counted_kill_fails(self):
        self.assertFalse(CAMPAIGN.step_passed(gate_record(143, oom_kill=0)))

    def test_an_oom_exit_below_the_cap_fails(self):
        self.assertFalse(CAMPAIGN.step_passed(gate_record(143, peak=1 * GIB)))

    def test_an_unenforced_worker_cannot_prove_the_gate(self):
        self.assertFalse(CAMPAIGN.step_passed(gate_record(143, enforced=False)))

    def test_a_clean_exit_is_not_a_gate_pass_by_itself(self):
        self.assertFalse(CAMPAIGN.step_passed({"name": "whole-compact-dda", "exit_code": 0}))


class ExpectedOutcomeTests(unittest.TestCase):
    def test_an_unlisted_step_must_exit_zero(self):
        self.assertTrue(CAMPAIGN.step_passed({"name": "smt", "exit_code": 0}))
        self.assertFalse(CAMPAIGN.step_passed({"name": "smt", "exit_code": 1}))

    def test_an_unlisted_step_exiting_with_an_oom_code_fails(self):
        self.assertFalse(CAMPAIGN.step_passed({"name": "unlisted-step", "exit_code": 143,
                                               "resource_proof": {"kernel_oom_kill": True}}))

    def test_a_reviewed_cbmc_exit_code_passes_and_another_fails(self):
        self.assertTrue(CAMPAIGN.step_passed({"name": "cbmc-invalid-composition", "exit_code": 10}))
        self.assertFalse(CAMPAIGN.step_passed({"name": "cbmc-invalid-composition", "exit_code": 0}))

    def test_the_table_names_the_resource_gate_with_its_reviewed_codes(self):
        outcome = CAMPAIGN.expected_outcome("whole-compact-dda")
        self.assertEqual(outcome["kind"], "resource_gate_oom")
        self.assertEqual(outcome["exit_codes"], [137, 143, 247])


if __name__ == "__main__":
    unittest.main()
