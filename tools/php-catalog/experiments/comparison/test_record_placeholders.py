import tempfile
import unittest
from pathlib import Path

import run


class PlaceholderTests(unittest.TestCase):
    def test_cache_and_checkout_paths_round_trip_through_their_placeholders(self):
        cache = run.CACHE_ROOT / "root" / "php-src" / "Zend" / "zend_API.c"
        checkout = run.REPOSITORY_ROOT / "tools" / "php-catalog" / "results" / "lock.json"
        self.assertEqual(run.record_text(cache), "<cache>/root/php-src/Zend/zend_API.c")
        self.assertEqual(run.resolve_record(run.record_text(cache)), str(cache))
        self.assertEqual(run.record_text(checkout), "<repo>/tools/php-catalog/results/lock.json")
        self.assertEqual(run.resolve_record(run.record_text(checkout)), str(checkout))

    def test_a_sibling_that_only_shares_a_prefix_is_not_a_placeholder(self):
        sibling = str(run.CACHE_ROOT) + "-other/file.c"
        self.assertEqual(run.record_text(sibling), sibling)
        self.assertEqual(run.resolve_record(sibling), sibling)

    def test_unrelated_paths_and_arguments_are_unchanged(self):
        self.assertEqual(run.record_text("-O0"), "-O0")
        self.assertEqual(run.record_text("/usr/bin/clang-21"), "/usr/bin/clang-21")

    def test_recorded_rewrites_every_string_in_a_nested_record(self):
        record = {"command": ["clang", str(run.REPOSITORY_ROOT / "cmp" / "x.c")],
                  "objects": [{"source": str(run.CACHE_ROOT / "php.c"), "imported": True}]}
        out = run.recorded(record)
        self.assertEqual(out["command"], ["clang", "<repo>/cmp/x.c"])
        self.assertEqual(out["objects"][0]["source"], "<cache>/php.c")
        self.assertIs(out["objects"][0]["imported"], True)

    def test_a_runner_record_names_its_paths_by_placeholder_and_resolves_to_the_real_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            runner = run.Runner(Path(temporary))
            self.assertTrue(runner.run("probe", ["true"], cwd=run.REPOSITORY_ROOT))
            [record] = runner.measurements
            self.assertEqual(record["cwd"], "<repo>")
            stdout = Path(run.resolve_record(record["stdout_path"]))
            self.assertEqual(stdout, (Path(temporary) / "probe.out").resolve())
            self.assertTrue(stdout.is_file())


if __name__ == "__main__":
    unittest.main()
