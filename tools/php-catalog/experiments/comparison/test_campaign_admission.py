import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


SCRIPT = Path(__file__).with_name("campaign.py")
sys.path.insert(0, str(SCRIPT.parent))
SPEC = importlib.util.spec_from_file_location("php_catalog_campaign", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
CAMPAIGN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAMPAIGN)
FINISH_SCRIPT = Path(__file__).with_name("finish_campaign.py")
FINISH_SPEC = importlib.util.spec_from_file_location("php_catalog_finish_campaign", FINISH_SCRIPT)
assert FINISH_SPEC is not None and FINISH_SPEC.loader is not None
FINISH = importlib.util.module_from_spec(FINISH_SPEC)
FINISH_SPEC.loader.exec_module(FINISH)


class CampaignAdmissionTests(unittest.TestCase):
    def test_failed_required_campaign_step_is_never_admitted(self):
        with self.assertRaisesRegex(RuntimeError, "campaign step failed: pointer-query"):
            CAMPAIGN.require_success(False, "pointer-query")

    def test_waits_for_a_late_enforcing_result(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            report = root / "proof.json"
            now = [0.0]

            def sleep(_seconds):
                now[0] += 1.0
                if now[0] == 2:
                    report.write_text(json.dumps({
                        "cgroup": str(root), "exit_code": 0,
                        "aggregate_hard_limit_enforced": True,
                        "memory_peak_bytes": 1024, "memory_events": "oom 0\n",
                    }))

            result = CAMPAIGN.wait_for_enforcing_worker(
                report, timeout_seconds=5, sleep=sleep,
                monotonic=lambda: now[0],
            )
            self.assertEqual(result["exit_code"], 0)
            self.assertEqual(now[0], 2.0)

    def test_completed_resource_limited_run_is_admitted_even_if_job_failed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            report = root / "proof.json"
            report.write_text(json.dumps({
                "cgroup": str(root), "exit_code": 137,
                "aggregate_hard_limit_enforced": True,
                "memory_peak_bytes": 10 * 1024**3,
                "memory_events": "oom_kill 1\n",
            }))
            result = CAMPAIGN.wait_for_enforcing_worker(report)
            self.assertEqual(result["exit_code"], 137)

    def test_missing_exit_record_after_cgroup_disappears_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            report = Path(temporary) / "proof.json"
            report.write_text(json.dumps({"cgroup": str(report.parent / "gone")}))
            with self.assertRaisesRegex(RuntimeError, "disappeared before publishing exit_code"):
                CAMPAIGN.wait_for_enforcing_worker(report, timeout_seconds=1)

    def test_completion_marker_is_published_only_for_successful_steps(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            campaign_dir = output / "campaign"
            campaign_dir.mkdir()
            measurements = campaign_dir / "measurements.json"
            measurements.write_text(json.dumps([{"name": "step", "exit_code": 0}]))
            runner = SimpleNamespace(output=campaign_dir)
            CAMPAIGN.publish_completion(runner)
            marker = json.loads((campaign_dir / "completed.json").read_text())
            self.assertEqual(marker["status"], "complete")
            self.assertEqual(marker["steps"], ["step"])
            FINISH.validate_campaign_completion(output)
            measurements.write_text(json.dumps([{"name": "failed", "exit_code": 1}]))
            with self.assertRaisesRegex(RuntimeError, "cannot publish completion"):
                CAMPAIGN.publish_completion(runner)

    def test_completion_marker_classifies_only_locked_negative_controls(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "campaign"
            output.mkdir()
            measurements = output / "measurements.json"
            measurements.write_text(json.dumps([
                {"name": "cbmc-contracts", "exit_code": 0},
                {"name": "cbmc-invalid-composition", "exit_code": 10},
                {"name": "cbmc-low-budget", "exit_code": 10},
            ]))
            CAMPAIGN.publish_completion(SimpleNamespace(output=output))
            marker = json.loads((output / "completed.json").read_text())
            self.assertEqual(marker["expected_exit_codes"], {
                "cbmc-contracts": 0,
                "cbmc-invalid-composition": 10,
                "cbmc-low-budget": 10,
            })
            FINISH.validate_campaign_completion(root)

    def test_completion_marker_rejects_measurement_mutation_and_unexpected_exit(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "campaign"
            output.mkdir()
            measurements = output / "measurements.json"
            measurements.write_text(json.dumps([{"name": "step", "exit_code": 0}]))
            CAMPAIGN.publish_completion(SimpleNamespace(output=output))
            measurements.write_text(json.dumps([{"name": "step", "exit_code": 1}]))
            with self.assertRaisesRegex(ValueError, "measurement digest"):
                FINISH.validate_campaign_completion(root)

            measurements.write_text(json.dumps([{"name": "step", "exit_code": 1}]))
            marker = json.loads((output / "completed.json").read_text())
            marker["measurements_sha256"] = FINISH.digest(measurements)
            (output / "completed.json").write_text(json.dumps(marker))
            with self.assertRaisesRegex(ValueError, "contains failed steps"):
                FINISH.validate_campaign_completion(root)

    def test_finish_campaign_classifies_only_explicit_cir_rejections(self):
        measurements = [
            {"name": "instrument-link", "exit_code": 0},
            {"name": "zend_builtin_functions-cir", "exit_code": 1, "stderr_sha256": "a" * 64},
        ]
        result = {
            "name": "zend_builtin_functions",
            "imported": False,
            "exit_code": 1,
            "classification": "frontend rejected this translation unit; retained as unsupported evidence",
            "stderr_sha256": "a" * 64,
        }
        expected = FINISH.finish_expected_exit_codes(measurements, [result])
        self.assertEqual(expected, {"instrument-link": 0, "zend_builtin_functions-cir": 1})
        with self.assertRaisesRegex(ValueError, "not an explicitly classified"):
            FINISH.finish_expected_exit_codes(measurements, [{**result, "exit_code": 139}])

    def test_finish_completion_binds_measurements_and_cir_outcomes(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            measurements = [
                {"name": "instrument-link", "exit_code": 0},
                {"name": "zend_builtin_functions-cir", "exit_code": 1, "stderr_sha256": "a" * 64},
            ]
            measurements_path = output / "measurements.json"
            measurements_path.write_text(json.dumps(measurements))
            cir_results = [{
                "name": "zend_builtin_functions", "imported": False, "exit_code": 1,
                "classification": "frontend rejected this translation unit; retained as unsupported evidence",
                "stderr_sha256": "a" * 64,
            }]
            marker = {
                "schema": 1,
                "status": "complete",
                "step_count": len(measurements),
                "steps": [record["name"] for record in measurements],
                "expected_exit_codes": FINISH.finish_expected_exit_codes(measurements, cir_results),
                "measurements_sha256": FINISH.digest(measurements_path),
            }
            marker_path = output / "completed.json"
            marker_path.write_text(json.dumps(marker))
            FINISH.validate_finish_completion(output, cir_results)

            marker["measurements_sha256"] = "0" * 64
            marker_path.write_text(json.dumps(marker))
            with self.assertRaisesRegex(ValueError, "measurement digest"):
                FINISH.validate_finish_completion(output, cir_results)

    def test_successful_cir_outcome_requires_the_emitted_artifact_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            emitted = root / "unit.cir"
            emitted.write_text("cir output\n")
            stderr_hash = "b" * 64
            measurements = [{"name": "unit-cir", "exit_code": 0, "stderr_sha256": stderr_hash}]
            result = {
                "name": "unit", "imported": True, "exit_code": 0,
                "classification": "emitted", "stderr_sha256": stderr_hash,
                "output_path": str(emitted), "output_sha256": FINISH.digest(emitted),
            }
            self.assertEqual(FINISH.finish_expected_exit_codes(measurements, [result]), {"unit-cir": 0})
            emitted.write_text("mutated output\n")
            with self.assertRaisesRegex(ValueError, "successful CIR outcome is malformed"):
                FINISH.finish_expected_exit_codes(measurements, [result])


if __name__ == "__main__":
    unittest.main()
