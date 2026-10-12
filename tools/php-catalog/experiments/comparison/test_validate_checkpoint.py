import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location("validate_checkpoint", Path(__file__).with_name("validate_checkpoint.py"))
assert SPEC is not None and SPEC.loader is not None
CHECKPOINT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKPOINT)


class CheckpointTests(unittest.TestCase):
    def test_every_recorded_claim_is_reproduced_by_the_checked_in_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "checkpoint.json"
            self.assertEqual(CHECKPOINT.main(["--output", str(output)]), 0)
            document = json.loads(output.read_text())
        self.assertEqual(document["status"], "passed")
        self.assertEqual(document["semantic_acceptance"]["status"], "OPEN")
        self.assertTrue(all(item["status"] == "passed" for item in document["checks"]))

    def test_an_r_row_is_accepted_only_when_its_status_is_author_accepted_or_complete(self):
        accepted = CHECKPOINT.r_row_accepted
        self.assertTrue(accepted("Implementation complete; author-accepted (2026-10-12)"))
        self.assertTrue(accepted("Implementation and validation complete; author-accepted (2026-10-12)"))
        self.assertTrue(accepted("Complete"))
        self.assertTrue(accepted("Complete (typed assumption records and fail-closed references)"))
        self.assertFalse(accepted("Complete (definition only; implementation conformance remains open)"))
        self.assertFalse(accepted("Complete (matrix/audit only; semantic gaps remain open)"))
        self.assertFalse(accepted("Implementation partial; acceptance open"))
        self.assertFalse(accepted("Implementation updated; identity checks validated; linked-module replay pending"))
        self.assertFalse(accepted("Open"))
        self.assertFalse(accepted("Deferred"))

    def test_the_semantic_gate_lists_the_open_r_rows_and_headings_of_the_ledger(self):
        ledger = (CHECKPOINT.HERE.parent.parent.parent.parent / "PHP_SEMANTICS_CATALOG_LEDGER.md").read_text()
        semantic = CHECKPOINT.semantic_acceptance(ledger)
        open_rows = {row["id"] for row in semantic["rows"]}
        self.assertEqual(open_rows, {
            "R00.2", "R00.3", "R02.3", "R02.5", "R02.7", "R03.1", "R03.2", "R03.3", "R03.4",
            "R22.1", "R05.2", "R05.4", "R05.7", "R06.1", "R06.2", "R06.3", "R06.4",
        })
        self.assertIn("R05.7", open_rows)
        self.assertNotIn("R01.6", open_rows)
        self.assertNotIn("R05.1", open_rows)
        self.assertIn({"id": "F00.3", "state": "Partial"}, semantic["headings"])


if __name__ == "__main__":
    unittest.main()
