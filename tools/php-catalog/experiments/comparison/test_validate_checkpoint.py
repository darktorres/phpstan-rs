import importlib.util
import json
import re
import subprocess
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

    def test_every_heading_and_row_states_its_acceptance(self):
        entries = CHECKPOINT.ledger_entries(self.ledger())
        self.assertEqual(len(entries), 71)
        self.assertEqual([e["id"] for e in entries if e["acceptance"] is None], [])

    def test_a_heading_is_accepted_exactly_when_its_title_says_complete(self):
        headings = [e for e in CHECKPOINT.ledger_entries(self.ledger()) if e["id"].startswith("F")]
        self.assertEqual(len(headings), 33)
        self.assertEqual([e["id"] for e in headings
                          if (e["acceptance"] == "accepted") != e["status"].startswith("Complete")], [])

    def test_the_open_entries_are_the_ones_the_ledger_marks_not_accepted(self):
        semantic = CHECKPOINT.semantic_acceptance(self.ledger())
        self.assertEqual(semantic["status"], "OPEN")
        self.assertEqual({e["id"] for e in semantic["open"]}, {
            "F00.1", "F00.2", "F00.4", "F01.1", "F01.2", "F01.4", "F01.5", "F02.1", "F02.3",
            "F02.4", "F02.5", "F03.2", "F03.3", "F03.4", "F04.2", "F05.1", "F05.3", "F05.4", "F05.6",
            "F05.7", "F06.1", "F06.3", "F06.4", "F06.5",
            "R02.3", "R02.5", "R02.7", "R03.1", "R03.2", "R03.3", "R03.4", "R05.2", "R05.4", "R05.6",
            "R05.7", "R06.1", "R06.2", "R06.3", "R06.4", "R22.1",
        })

    def test_every_scoped_acceptance_names_a_remainder_that_exists(self):
        ledger = self.ledger()
        names = {e["id"] for e in CHECKPOINT.ledger_entries(ledger)}
        scoped = CHECKPOINT.semantic_acceptance(ledger)["scoped"]
        self.assertEqual({e["id"] for e in scoped},
                         {"R00.2", "R00.3", "R04.1", "R05.1", "R05.3", "R05.5", "F05.5", "F00.3"})
        for entry in scoped:
            remainder = [name.strip() for name in entry["remainder"].split(",")]
            self.assertTrue(remainder)
            self.assertLessEqual(set(remainder), names, entry["id"])

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
        accepted = {e["id"]: e["acceptance"] for e in CHECKPOINT.ledger_entries(ledger)}
        self.assertEqual(accepted, {"R09.1": None, "R09.2": "accepted", "R09.3": None})
        semantic = CHECKPOINT.semantic_acceptance(ledger)
        self.assertEqual({e["id"]: e["acceptance"] for e in semantic["open"]},
                         {"R09.1": "unclassified", "R09.3": "unclassified"})

    def test_an_acceptance_line_that_does_not_follow_the_title_is_not_read(self):
        ledger = (
            "- **R09.4 — Complete** (depends on: none)\n"
            "  A paragraph first.\n"
            "  Acceptance: accepted\n"
        )
        self.assertIsNone(CHECKPOINT.ledger_entries(ledger)[0]["acceptance"])

    def test_a_heading_states_its_acceptance_on_the_line_after_it(self):
        ledger = (
            "### F09.1 — Complete: Example\n"
            "Acceptance: accepted\n"
            "\n"
            "### F09.2 — Partial: Example\n"
            "Acceptance: not accepted\n"
        )
        entries = {e["id"]: e["acceptance"] for e in CHECKPOINT.ledger_entries(ledger)}
        self.assertEqual(entries, {"F09.1": "accepted", "F09.2": "not accepted"})

    def test_every_reference_in_an_accepted_entry_resolves_in_the_tree(self):
        # The PHP sources below are the locked external inputs the catalog builder reads and
        # digest-checks from the restored PHP tree; they are the only names allowed to be absent.
        external = {"zend_compile.c", "zend_vm_def.h"}
        root = CHECKPOINT.HERE.parents[3]
        try:
            tracked = subprocess.run(["git", "ls-files"], cwd=root, capture_output=True, text=True,
                                     check=True).stdout.split()
        except (OSError, subprocess.CalledProcessError):
            self.skipTest("the repository is not a git checkout")
        sources = "\n".join((root / name).read_text(errors="ignore") for name in tracked
                            if name.endswith((".rs", ".py", ".sh", ".yml", ".toml"))
                            and (root / name).stat().st_size < 2_000_000)
        paths = "\n".join(tracked)
        ledger = self.ledger()
        blocks = {}
        current = None
        for line in ledger.split("\n"):
            if line.startswith("## ") or line.startswith("#### "):
                current = None
                continue
            match = CHECKPOINT.ENTRY_TITLE.match(line)
            if match:
                current = match.group(1) or match.group(3)
                blocks[current] = []
            if current:
                blocks[current].append(line)
        accepted = [e["id"] for e in CHECKPOINT.ledger_entries(ledger) if e["acceptance"] == "accepted"]
        unresolved = []
        for entry_id in accepted:
            for token in set(re.findall(r"`([^`\n]+)`", "\n".join(blocks[entry_id]))):
                token = token.strip()
                if re.fullmatch(r"[0-9a-f]{64}", token) or token in external:
                    continue
                if "/" in token or re.search(r"\.(rs|py|json|md|toml|php|expect|sh|yml|h|c|cpp|txt|tsv)$", token):
                    if not any(name == token or name.endswith("/" + token) for name in tracked):
                        unresolved.append((entry_id, token))
                elif re.fullmatch(r"[a-z][a-z0-9_]{9,}", token) and "_" in token:
                    if not re.search(r"\b" + re.escape(token) + r"\b", sources) and token not in paths:
                        unresolved.append((entry_id, token))
        self.assertEqual(unresolved, [])

if __name__ == "__main__":
    unittest.main()
