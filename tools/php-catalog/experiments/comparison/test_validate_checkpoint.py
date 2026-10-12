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
        self.assertEqual(document["semantic_acceptance"], "OPEN")
        self.assertTrue(all(item["status"] == "passed" for item in document["checks"]))


if __name__ == "__main__":
    unittest.main()
