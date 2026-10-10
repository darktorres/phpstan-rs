import copy
import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


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

    def test_property_description_and_source_change_are_rejected(self):
        for field, value in [('description', 'different assertion with the same ID'),
                             ('line', '999')]:
            document = copy.deepcopy(self.references['cbmc-contracts'])
            for block in document:
                for prop in block.get('result', []):
                    if prop['property'] == 'main.assertion.1':
                        if field == 'line':
                            prop['sourceLocation'][field] = value
                        else:
                            prop[field] = value
                        break
            with self.subTest(field=field):
                with self.assertRaisesRegex(ValueError, 'property records'):
                    CAMPAIGN.validate_cbmc_run(
                        'cbmc-contracts', document, self.references['cbmc-contracts'], 0
                    )

    def test_counterexample_trace_change_is_rejected(self):
        document = copy.deepcopy(self.references['cbmc-invalid-composition'])
        for block in document:
            for prop in block.get('result', []):
                if prop['property'] == 'main.assertion.11':
                    prop['trace'][0]['stepType'] = 'changed-step'
        with self.assertRaisesRegex(ValueError, 'property records'):
            CAMPAIGN.validate_cbmc_run(
                'cbmc-invalid-composition',
                document,
                self.references['cbmc-invalid-composition'],
                10,
            )

    def test_campaign_runner_rejects_unlocked_cbmc_inputs(self):
        source = SCRIPT.parent / 'contracts.c'
        source_digest = CAMPAIGN.digest(source)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / 'cache'
            cbmc = cache / 'deps/usr/bin/cbmc'
            cbmc.parent.mkdir(parents=True)
            cbmc.write_bytes(b'pinned-test-cbmc')
            lock = root / 'lock.json'
            lock.write_text(json.dumps({
                'followup_cbmc_validation': {
                    'cbmc_binary_sha256': CAMPAIGN.digest(cbmc),
                    'runs': {
                        'cbmc-contracts': {
                            'source': 'contracts.c',
                            'source_sha256': source_digest,
                            'reviewed_reference_sha256': CAMPAIGN.digest(
                                SCRIPT.parent / 'results/evaluation/cbmc-contracts.out'
                            ),
                        }
                    },
                }
            }))
            runs = [('cbmc-contracts', 'contracts.c', ['--unwind', '4'])]
            CAMPAIGN.validate_cbmc_inputs(cache, runs, lock)

            record = json.loads(lock.read_text())
            record['followup_cbmc_validation']['runs']['cbmc-contracts']['source_sha256'] = '0' * 64
            lock.write_text(json.dumps(record))
            with self.assertRaisesRegex(ValueError, 'source digest'):
                CAMPAIGN.validate_cbmc_inputs(cache, runs, lock)

            record['followup_cbmc_validation']['runs']['cbmc-contracts']['source_sha256'] = source_digest
            record['followup_cbmc_validation']['runs']['cbmc-contracts']['reviewed_reference_sha256'] = '0' * 64
            lock.write_text(json.dumps(record))
            with self.assertRaisesRegex(ValueError, 'result transcript'):
                CAMPAIGN.validate_cbmc_inputs(cache, runs, lock)

            record['followup_cbmc_validation']['runs']['cbmc-contracts']['reviewed_reference_sha256'] = CAMPAIGN.digest(
                SCRIPT.parent / 'results/evaluation/cbmc-contracts.out'
            )
            lock.write_text(json.dumps(record))

            cbmc.write_bytes(b'changed-cbmc')
            with self.assertRaisesRegex(ValueError, 'CBMC binary digest'):
                CAMPAIGN.validate_cbmc_inputs(cache, runs, lock)

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
            with patch.object(CAMPAIGN, 'validate_cbmc_inputs') as validate_inputs:
                CAMPAIGN.run_cbmc_campaign(Path('/locked/cache'), runner)
            validate_inputs.assert_called_once()
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
            with patch.object(CAMPAIGN, 'validate_cbmc_inputs'):
                with self.assertRaisesRegex(ValueError, 'command exit'):
                    CAMPAIGN.run_cbmc_campaign(Path('/locked/cache'), runner)


if __name__ == '__main__':
    unittest.main()
