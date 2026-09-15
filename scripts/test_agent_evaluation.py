import contextlib
import io
import json
import pathlib
import tempfile
import unittest
from unittest import mock

import agent_evaluation
from agent_evaluation import digest, score


def fixture(count=30):
    registration = {'version': 1, 'split': 'held_out', 'development_repositories': [], 'model_snapshot': 'fixed-test-snapshot',
                    'prompt_sha256': 'a'*64, 'token_budget': 2000,
                    'cases': [{'id': str(i), 'repository': f'repo-{i%5}', 'revision': 'b'*40,
                               'language': ['rust', 'go', 'ruby', 'typescript'][i%4]} for i in range(count)]}
    runs = []
    for case in registration['cases']:
        for variant in ['baseline', 'candidate']:
            runs.append({**{k: case[k] for k in ['repository', 'revision', 'language']},
                         'case_id': case['id'], 'variant': variant, 'registration_sha256': digest(registration),
                         'model_snapshot': registration['model_snapshot'], 'prompt_sha256': 'a'*64,
                         'token_budget': 2000, 'seed': 0, 'patch_sha256': 'c'*64,
                         'test_log_sha256': 'd'*64, 'tool_snapshot_sha256': 'e'*64,
                         'status': 'completed', 'tests_pass': variant == 'candidate', 'patch_applies': True,
                         'input_tokens': 500, 'output_tokens': 200, 'elapsed_ms': 1000, 'cost_usd': .01,
                         'unrelated_edits': 0})
    return registration, runs


class EvaluationTests(unittest.TestCase):
    def test_complete_synthetic_gain_passes_reported_gate_not_independent_verification(self):
        report = score(*fixture())
        self.assertTrue(report['reported_outcome_gate'])
        self.assertFalse(report['outcomes_independently_verified'])

    def test_missing_duplicate_and_unregistered_rows_rejected(self):
        plan, runs = fixture()
        for altered in [runs[:-1], runs + [runs[0]], [dict(runs[0], case_id='missing')] + runs[1:]]:
            with self.assertRaises(ValueError): score(plan, altered)

    def test_model_prompt_seed_revision_and_budget_confounds_rejected(self):
        for key, value in [('model_snapshot', 'other'), ('prompt_sha256', 'f'*64), ('seed', 3),
                           ('revision', 'f'*40), ('token_budget', 3000)]:
            plan, runs = fixture(); runs[0][key] = value
            with self.assertRaises(ValueError): score(plan, runs)

    def test_bad_resources_and_missing_artifacts_rejected(self):
        for key, value in [('elapsed_ms', float('nan')), ('input_tokens', -1), ('output_tokens', True),
                           ('test_log_sha256', ''), ('cost_usd', float('inf')), ('input_tokens', 9999)]:
            plan, runs = fixture(); runs[0][key] = value
            with self.assertRaises(ValueError): score(plan, runs)

    def test_small_or_development_samples_never_prove_gain(self):
        self.assertFalse(score(*fixture(4))['reported_outcome_gate'])
        plan, runs = fixture(); plan['split'] = 'development'
        for run in runs: run['registration_sha256'] = digest(plan)
        self.assertFalse(score(plan, runs)['reported_outcome_gate'])

    def test_timeouts_remain_failures_not_dropped(self):
        plan, runs = fixture()
        for run in runs:
            if run['variant'] == 'candidate': run.update(status='timeout', tests_pass=False)
        self.assertFalse(score(plan, runs)['reported_outcome_gate'])

    def test_resource_regression_blocks_quality_gain(self):
        plan, runs = fixture()
        for run in runs:
            if run['variant'] == 'candidate': run['elapsed_ms'] = 2000
        self.assertFalse(score(plan, runs)['reported_outcome_gate'])

    def test_unrelated_edits_cannot_be_hidden_by_aggregate_gains(self):
        plan, runs = fixture(); runs[1]['unrelated_edits'] = 1
        self.assertFalse(score(plan, runs)['reported_outcome_gate'])

    def test_new_cost_from_zero_cannot_pass(self):
        plan, runs = fixture()
        for run in runs:
            if run['variant'] == 'baseline': run['cost_usd'] = 0
        report = score(plan, runs)
        self.assertFalse(report['reported_outcome_gate'])
        self.assertIsNone(report['repository_weighted_intervals']['cost_ratio'])

    def test_development_repository_cannot_be_called_held_out(self):
        plan, runs = fixture(); plan['development_repositories'] = ['repo-0']
        for run in runs: run['registration_sha256'] = digest(plan)
        with self.assertRaises(ValueError): score(plan, runs)

    def test_candidate_unrelated_edits_rejected_even_when_baseline_matches(self):
        plan, runs = fixture()
        for run in runs:
            if run['case_id'] == '0': run['unrelated_edits'] = 1
        report = score(plan, runs)
        self.assertFalse(report['reported_outcome_gate'])
        self.assertEqual(report['cases'][0]['candidate_unrelated_edits'], 1)

    def test_unapplied_patch_is_not_success(self):
        plan, runs = fixture()
        for run in runs:
            if run['variant'] == 'candidate': run['patch_applies'] = False
        self.assertFalse(score(plan, runs)['reported_outcome_gate'])

    def test_token_regression_blocks_quality_gain(self):
        plan, runs = fixture()
        for run in runs:
            if run['variant'] == 'candidate': run['input_tokens'] = 1000
        self.assertFalse(score(plan, runs)['reported_outcome_gate'])

    def test_cost_regression_blocks_quality_gain(self):
        plan, runs = fixture()
        for run in runs:
            if run['variant'] == 'candidate': run['cost_usd'] = .02
        self.assertFalse(score(plan, runs)['reported_outcome_gate'])

    def test_malformed_cases_rejected_as_invalid_evidence(self):
        plan, runs = fixture()
        for cases in [[{}], 'not-a-list', [{'id': ''}], [None]]:
            with self.assertRaises(ValueError): score(dict(plan, cases=cases), runs)

    def test_zero_token_connection_failure_is_retained(self):
        plan, runs = fixture()
        runs[0].update(status='error', tests_pass=False, input_tokens=0, output_tokens=0)
        report = score(plan, runs)
        self.assertEqual(report['pairs'], 30)
        self.assertFalse(report['reported_outcome_gate'])
        self.assertIsNone(report['repository_weighted_intervals']['token_ratio'])


class MainTests(unittest.TestCase):
    def run_main(self, plan, runs):
        with tempfile.TemporaryDirectory() as directory:
            paths = [pathlib.Path(directory, name) for name in ['plan.json', 'runs.json', 'out.json']]
            paths[0].write_text(json.dumps(plan))
            paths[1].write_text(json.dumps(runs))
            argv = ['agent_evaluation.py', '--registration', str(paths[0]),
                    '--runs', str(paths[1]), '--output', str(paths[2])]
            printed = io.StringIO()
            with mock.patch('sys.argv', argv), contextlib.redirect_stdout(printed):
                error = None
                try:
                    agent_evaluation.main()
                except SystemExit as exit_error:
                    error = exit_error
            return error, json.loads(printed.getvalue()), json.loads(paths[2].read_text())

    def test_passing_gate_writes_report_and_exits_zero(self):
        error, summary, report = self.run_main(*fixture())
        self.assertIsNone(error)
        self.assertTrue(summary['reported_outcome_gate'])
        self.assertEqual(summary['pairs'], 30)
        self.assertFalse(report['outcomes_independently_verified'])

    def test_failing_gate_exits_nonzero_after_writing_report(self):
        plan, runs = fixture(4)
        error, summary, report = self.run_main(plan, runs)
        self.assertEqual(error.code, 1)
        self.assertFalse(summary['reported_outcome_gate'])
        self.assertFalse(report['adequate_held_out_sample'])


if __name__ == '__main__':
    unittest.main()
