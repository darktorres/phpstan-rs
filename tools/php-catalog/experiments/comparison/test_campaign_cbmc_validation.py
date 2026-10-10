import copy
import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name('campaign.py')
sys.path.insert(0, str(SCRIPT.parent))
SPEC = importlib.util.spec_from_file_location('campaign', SCRIPT)
assert SPEC is not None and SPEC.loader is not None
CAMPAIGN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAMPAIGN)


class CbmcValidationTests(unittest.TestCase):
    def setUp(self):
        self.references = {
            name: json.loads((SCRIPT.parent / 'results/evaluation' / (name + '.out')).read_text())
            for name in CAMPAIGN.CBMC_EXPECTATIONS
        }

    def test_all_recorded_runs_match_their_expected_results(self):
        for name, document in self.references.items():
            with self.subTest(name=name):
                CAMPAIGN.validate_cbmc_run(
                    name,
                    document,
                    document,
                    CAMPAIGN.CBMC_EXPECTATIONS[name]['command_exit_code'],
                )

    def test_missing_property_is_rejected(self):
        document = copy.deepcopy(self.references['cbmc-contracts'])
        for block in document:
            if block.get('result'):
                block['result'].pop()
                break
        with self.assertRaisesRegex(ValueError, 'properties|property inventory/status changed'):
            CAMPAIGN.validate_cbmc_run(
                'cbmc-contracts', document, self.references['cbmc-contracts'], 0
            )

    def test_unexpected_failure_is_rejected(self):
        document = copy.deepcopy(self.references['cbmc-contracts'])
        for block in document:
            for prop in block.get('result', []):
                if prop['property'] == 'main.assertion.1':
                    prop['status'] = 'FAILURE'
                    break
        with self.assertRaisesRegex(ValueError, 'property inventory|property inventory/status changed'):
            CAMPAIGN.validate_cbmc_run(
                'cbmc-contracts', document, self.references['cbmc-contracts'], 0
            )

    def test_unexpected_exit_code_is_rejected(self):
        document = self.references['cbmc-contracts']
        with self.assertRaisesRegex(ValueError, 'command exit'):
            CAMPAIGN.validate_cbmc_run(
                'cbmc-contracts', document, document, 1
            )

    def test_reference_inventory_change_is_rejected(self):
        reference = copy.deepcopy(self.references['cbmc-contracts'])
        for block in reference:
            for prop in block.get('result', []):
                if prop['property'] == 'main.assertion.1':
                    prop['property'] = 'main.assertion.renamed'
                    break
        with self.assertRaisesRegex(ValueError, 'inventory digest'):
            CAMPAIGN.validate_cbmc_run(
                'cbmc-contracts', self.references['cbmc-contracts'], reference, 0
            )

    def test_unknown_run_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'No reviewed CBMC expectation'):
            CAMPAIGN.validate_cbmc_run('cbmc-new-mode', [], [], True)

    def test_campaign_runner_checks_each_command_exit_and_transcript(self):
        class TranscriptRunner:
            def __init__(self, output, exit_codes=None):
                self.output = output
                self.measurements = []
                self.calls = []
                self.exit_codes = exit_codes or {}

            def run(inner, name, command, **kwargs):
                inner.calls.append((name, command, kwargs))
                source = SCRIPT.parent / 'results/evaluation' / (name + '.out')
                shutil.copyfile(source, inner.output / (name + '.out'))
                inner.measurements.append({
                    'exit_code': inner.exit_codes.get(
                        name, CAMPAIGN.CBMC_EXPECTATIONS[name]['command_exit_code']
                    )
                })
                return inner.measurements[-1]['exit_code'] == 0

        with tempfile.TemporaryDirectory() as temporary:
            runner = TranscriptRunner(Path(temporary))
            CAMPAIGN.run_cbmc_campaign(Path('/locked/cache'), runner)
            self.assertEqual(
                [name for name, _, _ in runner.calls],
                list(CAMPAIGN.CBMC_EXPECTATIONS),
            )
            self.assertEqual(
                [entry['exit_code'] for entry in runner.measurements],
                [0, 10, 10],
            )
            for (name, command, kwargs) in runner.calls:
                with self.subTest(name=name):
                    self.assertIn('--json-ui', command)
                    self.assertEqual(kwargs['timeout'], 300)
                    self.assertEqual(kwargs['env']['LD_LIBRARY_PATH'], '/locked/cache/deps/usr/lib')

        with tempfile.TemporaryDirectory() as temporary:
            runner = TranscriptRunner(Path(temporary), {'cbmc-invalid-composition': 1})
            with self.assertRaisesRegex(ValueError, 'command exit'):
                CAMPAIGN.run_cbmc_campaign(Path('/locked/cache'), runner)


if __name__ == '__main__':
    unittest.main()
