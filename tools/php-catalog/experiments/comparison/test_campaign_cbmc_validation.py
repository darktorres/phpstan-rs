import copy
import importlib.util
import json
import sys
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


if __name__ == '__main__':
    unittest.main()
