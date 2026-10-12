import hashlib
import importlib.util
import json
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
LOCK = json.loads((HERE / "results" / "lock.json").read_text())
EVAL_LOCK = json.loads((HERE / "results" / "evaluation" / "lock.json").read_text())


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class LockStalenessTests(unittest.TestCase):
    def test_every_source_digest_in_the_lock_matches_the_file(self):
        stale = [name for name, digest in LOCK["source_sha256"].items() if sha(HERE / name) != digest]
        self.assertEqual(stale, [])

    def test_the_correspondence_inputs_match_their_files(self):
        spec = importlib.util.spec_from_file_location("regenerate", HERE / "regenerate_correspondence.py")
        regenerate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(regenerate)
        pins = LOCK["representation_correspondence_inputs"]
        stale = [name for name, path in regenerate.EVIDENCE.items() if sha(path) != pins[name]]
        self.assertEqual(stale, [])

    def test_the_checkpoint_records_match_their_files(self):
        pins = LOCK["campaign_checkpoint_sha256"]
        stale = [name for name, digest in pins.items() if sha(HERE / "results" / "evaluation" / name) != digest]
        self.assertEqual(stale, [])

    def test_the_checked_in_link_manifest_and_checkpoint_are_pinned(self):
        self.assertEqual(sha(HERE / "results" / "evaluation" / "link-manifest.json"), LOCK["link_manifest_sha256"])
        self.assertEqual(sha(HERE / "results" / "evaluation" / "checkpoint-validation.json"),
                         LOCK["checkpoint_validation_sha256"])


if __name__ == "__main__":
    unittest.main()
