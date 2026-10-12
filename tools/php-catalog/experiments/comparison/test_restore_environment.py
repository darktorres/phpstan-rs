import importlib.util
import io
import contextlib
import os
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


SPEC = importlib.util.spec_from_file_location(
    "restore_environment", Path(__file__).with_name("restore_environment.py"))
assert SPEC is not None and SPEC.loader is not None
RESTORE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESTORE)


class RestoreVerifyOnlyTests(unittest.TestCase):
    def test_an_empty_root_is_refused_by_name_without_building(self):
        with tempfile.TemporaryDirectory() as temporary:
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                status = RESTORE.main(["--root", os.path.join(temporary, "root"), "--verify-only"])
            self.assertEqual(status, 1)
            self.assertIn("package not present", errors.getvalue())

    def test_a_digest_mismatch_names_the_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            name = next(iter(RESTORE.LOCK["downloaded_packages_sha256"]))
            (root / name).write_bytes(b"not the pinned package")
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                status = RESTORE.main(["--root", str(root), "--verify-only"])
            self.assertEqual(status, 1)
            self.assertIn(f"package digest differs from the lock: {name}", errors.getvalue())


class CanaryTests(unittest.TestCase):
    def test_a_manifest_digest_that_the_toolchain_does_not_reproduce_stops_the_canary(self):
        import json as _json
        root = Path.home() / ".cache/php-catalog-evaluation/root"
        if not (root / "php-cli" / "php-src" / "sapi" / "cli" / "php").is_file():
            self.skipTest("the restored tree is not present")
        real = _json.loads((HERE / "results" / "evaluation" / "link-manifest.json").read_text())
        emitted = [o for o in real["objects"] if o.get("imported") and o.get("module_sha256")][:2]
        with tempfile.TemporaryDirectory() as temporary:
            tampered = Path(temporary) / "manifest.json"
            for record in emitted:
                record["module_sha256"] = "0" * 64
            tampered.write_text(_json.dumps({"objects": emitted}))
            with self.assertRaisesRegex(RESTORE.RestoreError, "differs from its manifest digest"):
                RESTORE.canary(root, units=2, manifest_path=tampered)

    def test_the_checked_in_manifest_reproduces_under_the_restored_tree(self):
        root = Path.home() / ".cache/php-catalog-evaluation/root"
        if not (root / "php-cli" / "php-src" / "sapi" / "cli" / "php").is_file():
            self.skipTest("the restored tree is not present")
        RESTORE.canary(root, units=2)


if __name__ == "__main__":
    unittest.main()
