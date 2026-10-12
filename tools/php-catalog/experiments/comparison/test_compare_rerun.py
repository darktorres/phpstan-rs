import json
import tempfile
import unittest
from pathlib import Path

import compare_rerun as CR


class CompareRerunTests(unittest.TestCase):
    def make_run(self, root, name, record, text=None):
        run = root / name
        run.mkdir()
        (run / 'measurements.json').write_text(json.dumps(record))
        if text is not None:
            (run / 'out.txt').write_text(text)
        return run

    def test_timings_and_samples_may_differ(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            a = self.make_run(root, 'a', [{'name': 's', 'exit_code': 0, 'elapsed_seconds': 1.0, 'max_rss_kib': 10}])
            b = self.make_run(root, 'b', [{'name': 's', 'exit_code': 0, 'elapsed_seconds': 9.0, 'max_rss_kib': 99}])
            self.assertEqual(CR.compare(a, b), [])

    def test_an_exit_code_difference_is_reported(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            a = self.make_run(root, 'a', [{'name': 's', 'exit_code': 0}])
            b = self.make_run(root, 'b', [{'name': 's', 'exit_code': 1}])
            self.assertEqual(CR.compare(a, b), ['measurements.json: differs'])

    def test_a_run_directory_inside_text_is_not_a_difference(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            a = self.make_run(root, 'a', [], text=f'from {root / "a"}/x.c\n')
            b = self.make_run(root, 'b', [], text=f'from {root / "b"}/x.c\n')
            self.assertEqual(CR.compare(a, b), [])

    def test_a_file_present_in_one_run_is_reported(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            a = self.make_run(root, 'a', [], text='x')
            b = self.make_run(root, 'b', [])
            self.assertIn('out.txt: present in only one run', CR.compare(a, b))


if __name__ == '__main__':
    unittest.main()
