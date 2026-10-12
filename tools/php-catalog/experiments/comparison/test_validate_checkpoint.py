import importlib.util
import json
import re
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

    def ledger(self):
        return (CHECKPOINT.HERE.parent.parent.parent.parent / "PHP_SEMANTICS_CATALOG_LEDGER.md").read_text()

    def test_every_r_row_states_its_acceptance(self):
        rows = CHECKPOINT.r_rows(self.ledger())
        self.assertEqual(len(rows), 38)
        self.assertEqual([row["id"] for row in rows if row["acceptance"] is None], [])

    def test_the_open_rows_are_the_rows_the_ledger_marks_not_accepted(self):
        semantic = CHECKPOINT.semantic_acceptance(self.ledger())
        self.assertEqual({row["id"] for row in semantic["rows"]}, {
            "R02.3", "R02.5", "R02.7", "R03.1", "R03.2", "R03.3", "R03.4", "R05.2", "R05.4",
            "R05.6", "R05.7", "R06.1", "R06.2", "R06.3", "R06.4", "R22.1",
        })
        self.assertIn({"id": "F00.3", "state": "Partial"}, semantic["headings"])

    def test_every_scoped_acceptance_names_a_remainder_that_exists(self):
        ledger = self.ledger()
        names = {row["id"] for row in CHECKPOINT.r_rows(ledger)}
        names |= set(re.findall(r"^### (F\d+\.\d+) — ", ledger, re.M))
        scoped = CHECKPOINT.semantic_acceptance(ledger)["scoped"]
        self.assertEqual({row["id"] for row in scoped}, {"R00.2", "R00.3", "R04.1", "R05.1", "R05.3", "R05.5"})
        for row in scoped:
            remainder = [name.strip() for name in row["remainder"].split(",")]
            self.assertTrue(remainder)
            self.assertLessEqual(set(remainder), names, row["id"])

    def test_a_row_without_an_acceptance_line_is_not_accepted(self):
        ledger = (
            "- **R09.1 — Complete** (depends on: none)\n"
            "  Description without a line that states acceptance.\n"
            "- **R09.2 — Complete**\n"
            "  Acceptance: accepted\n"
            "- **R09.3 — Complete**\n"
            "  Acceptance: maybe\n"
            "  Description after a title whose acceptance is not one of the two values.\n"
        )
        rows = {row["id"]: row["acceptance"] for row in CHECKPOINT.r_rows(ledger)}
        self.assertEqual(rows, {"R09.1": None, "R09.2": "accepted", "R09.3": None})
        semantic = CHECKPOINT.semantic_acceptance(ledger)
        self.assertEqual(semantic["status"], "OPEN")
        self.assertEqual({row["id"]: row["acceptance"] for row in semantic["rows"]},
                         {"R09.1": "unclassified", "R09.3": "unclassified"})

    def test_an_acceptance_line_that_does_not_follow_the_title_is_not_read(self):
        ledger = (
            "- **R09.4 — Complete** (depends on: none)\n"
            "  A paragraph first.\n"
            "  Acceptance: accepted\n"
        )
        self.assertIsNone(CHECKPOINT.r_rows(ledger)[0]["acceptance"])

if __name__ == "__main__":
    unittest.main()
