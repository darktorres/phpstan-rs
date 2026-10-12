import importlib.util
import io
import contextlib
import os
import tempfile
import unittest
from pathlib import Path


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


if __name__ == "__main__":
    unittest.main()
