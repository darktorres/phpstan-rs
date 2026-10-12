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

    def test_a_timing_line_in_stdout_may_differ_but_a_count_may_not(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            a = self.make_run(root, 'a', [], text='Nodes 12\nLLVMIRTime 0.02\n')
            b = self.make_run(root, 'b', [], text='Nodes 12\nLLVMIRTime 5.9e-05\n')
            self.assertEqual(CR.compare(a, b), [])
            c = self.make_run(root, 'c', [], text='Nodes 13\nLLVMIRTime 0.02\n')
            self.assertEqual(CR.compare(a, c), ['out.txt: differs'])

    def test_stderr_differs_only_in_timestamps_or_crash_diagnostics(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            a = self.make_run(root, 'a', [], text='one\n')
            b = self.make_run(root, 'b', [])
            (b / 'out.txt').write_text('one\n')
            (a / 'binary.err').write_text('2026-10-12 09:26:37,929 | log line\n')
            (b / 'binary.err').write_text('2026-10-12 10:01:02,111 | log line\n')
            self.assertEqual(CR.compare(a, b), [])
            (a / 'gcc.err').write_text('a real warning in one run\n')
            (b / 'gcc.err').write_text('a different warning in the other\n')
            self.assertEqual(CR.compare(a, b), ['gcc.err: differs'])
            (a / 'cir.err').write_text('Stack dump:\n#0 0x1234567890\n')
            (b / 'cir.err').write_text('Stack dump:\n#0 0x9999999999\n')
            self.assertEqual(CR.compare(a, b), ['gcc.err: differs'])

    def test_a_file_present_in_one_run_is_reported(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            a = self.make_run(root, 'a', [], text='x')
            b = self.make_run(root, 'b', [])
            self.assertIn('out.txt: present in only one run', CR.compare(a, b))


if __name__ == '__main__':
    unittest.main()
