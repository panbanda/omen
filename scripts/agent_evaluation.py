#!/usr/bin/env python3
"""Fail-closed scoring of registered, paired coding-agent outcome records.

This does not run models, execute patches, or certify submitted test logs.
"""
import argparse
import hashlib
import json
import math
import pathlib
import random
import re
import statistics

VARIANTS = {'baseline', 'candidate'}
MIN_PAIRS = 30
MIN_REPOSITORIES = 5
RESOURCE_MARGIN = 0.05


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def sha(value, size=64):
    return isinstance(value, str) and re.fullmatch(f'[0-9a-f]{{{size}}}', value) is not None


def score(registration, runs):
    if registration.get('version') != 1 or registration.get('split') not in {'held_out', 'development'}:
        raise ValueError('invalid registration version/split')
    model = registration.get('model_snapshot')
    prompt = registration.get('prompt_sha256')
    budget = registration.get('token_budget')
    if not isinstance(model, str) or not model or not sha(prompt) or type(budget) is not int or budget <= 0:
        raise ValueError('register immutable model, prompt and positive token budget')
    cases = registration.get('cases', [])
    if not isinstance(cases, list) or not cases or any(not isinstance(c, dict) or not isinstance(c.get('id'), str) or not c['id'] for c in cases):
        raise ValueError('each registered case needs a non-empty string id')
    if len({c['id'] for c in cases}) != len(cases):
        raise ValueError('missing/duplicate registered cases')
    planned = {c['id']: c for c in cases}
    development = registration.get('development_repositories')
    if not isinstance(development, list) or any(not isinstance(repo, str) for repo in development):
        raise ValueError('declare development repositories explicitly')
    for case in cases:
        if not sha(case.get('revision'), 40) or not case.get('repository') or not case.get('language'):
            raise ValueError('each case needs repository, revision and language')
        if registration['split'] == 'held_out' and case['repository'] in development:
            raise ValueError('held-out repository overlaps development data')
    registration_hash = digest(registration)
    paired = {}
    for run in runs:
        case = planned.get(run.get('case_id'))
        if case is None or run.get('variant') not in VARIANTS:
            raise ValueError('unexpected case or variant')
        if any(run.get(k) != case[k] for k in ['repository', 'revision', 'language']):
            raise ValueError('case identity mismatch')
        for key, expected in [('model_snapshot', model), ('prompt_sha256', prompt),
                              ('token_budget', budget), ('registration_sha256', registration_hash)]:
            if run.get(key) != expected:
                raise ValueError(f'confounded pair: {key}')
        for key in ['patch_sha256', 'test_log_sha256', 'tool_snapshot_sha256']:
            if not sha(run.get(key)):
                raise ValueError(f'missing evidence hash: {key}')
        if run.get('status') not in {'completed', 'error', 'timeout'}:
            raise ValueError('unknown run status')
        for key in ['tests_pass', 'patch_applies']:
            if type(run.get(key)) is not bool:
                raise ValueError(f'{key} must be boolean')
        for key in ['input_tokens', 'output_tokens', 'unrelated_edits', 'seed']:
            if type(run.get(key)) is not int or run[key] < 0:
                raise ValueError(f'invalid {key}')
        for key in ['elapsed_ms', 'cost_usd']:
            if type(run.get(key)) not in (int, float) or not math.isfinite(run[key]) or run[key] < 0:
                raise ValueError(f'invalid {key}')
        tokens = run['input_tokens'] + run['output_tokens']
        if (tokens == 0 and run['status'] == 'completed') or tokens > budget or run['elapsed_ms'] <= 0:
            raise ValueError('invalid/exceeded resource budget')
        if run['status'] != 'completed' and run['tests_pass']:
            raise ValueError('failed/timed-out run cannot claim passing tests')
        key = run['case_id']
        variants = paired.setdefault(key, {})
        if run['variant'] in variants:
            raise ValueError('duplicate observation')
        variants[run['variant']] = run
    if set(paired) != set(planned) or any(set(p) != VARIANTS for p in paired.values()):
        raise ValueError('missing registered observations; failures must not be dropped')

    by_repo = {}
    by_language = {}
    rows = []
    new_cost_from_zero = False
    new_tokens_from_zero = False
    def success(run):
        return int(run['status'] == 'completed' and run['tests_pass'] and run['patch_applies'] and run['unrelated_edits'] == 0)
    for case_id, pair in sorted(paired.items()):
        b, c = pair['baseline'], pair['candidate']
        if b['seed'] != c['seed']:
            raise ValueError('paired seeds differ')
        quality = success(c) - success(b)
        by_language.setdefault(b['language'], []).append(quality)
        if b['cost_usd'] == 0 and c['cost_usd'] > 0:
            new_cost_from_zero = True
        baseline_tokens = b['input_tokens'] + b['output_tokens']
        candidate_tokens = c['input_tokens'] + c['output_tokens']
        if baseline_tokens == 0 and candidate_tokens > 0:
            new_tokens_from_zero = True
        metrics = [quality,
                   candidate_tokens / baseline_tokens if baseline_tokens else 1.0,
                   c['elapsed_ms'] / b['elapsed_ms'],
                   c['cost_usd'] / b['cost_usd'] if b['cost_usd'] else 1.0]
        by_repo.setdefault(b['repository'], []).append(metrics)
        rows.append({'case_id': case_id, 'baseline_success': success(b), 'candidate_success': success(c),
                     'candidate_unrelated_edits': c['unrelated_edits']})
    clusters = [[statistics.mean(m[i] for m in cases) for i in range(4)] for cases in by_repo.values()]
    rng = random.Random(0)
    draws = [[], [], [], []]
    for _ in range(2000):
        selected = [rng.choice(clusters) for _ in clusters]
        for i in range(4):
            draws[i].append(statistics.mean(c[i] for c in selected))
    # Four predeclared comparisons; Bonferroni-adjusted two-sided intervals.
    tail = 0.05 / (2 * 4)
    intervals = []
    for sample in draws:
        sample.sort()
        intervals.append([sample[int(len(sample) * tail)], sample[min(len(sample)-1, math.ceil(len(sample)*(1-tail))-1)]])
    adequate = len(rows) >= MIN_PAIRS and len(clusters) >= MIN_REPOSITORIES and registration['split'] == 'held_out'
    gate = (adequate and intervals[0][0] > 0 and
            all(interval[1] <= 1 + RESOURCE_MARGIN for interval in intervals[1:]) and
            not new_cost_from_zero and not new_tokens_from_zero and all(sum(deltas) >= 0 for deltas in by_language.values()) and
            all(row['candidate_unrelated_edits'] == 0 for row in rows))
    return {'registration_sha256': registration_hash, 'pairs': len(rows), 'repositories': len(clusters),
            'adequate_held_out_sample': adequate, 'reported_outcome_gate': gate,
            'outcomes_independently_verified': False,
            'caveat': 'Input outcome claims and artifact hashes must be independently verified. This scorer does not run models or tests. Bootstrap intervals do not establish benchmark representativeness.',
            'repository_weighted_intervals': {'success_delta': intervals[0], 'token_ratio': None if new_tokens_from_zero else intervals[1],
                                             'elapsed_ratio': intervals[2], 'cost_ratio': None if new_cost_from_zero else intervals[3]},
            'per_language_success_delta': {k: sum(v) for k, v in by_language.items()}, 'cases': rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--registration', type=pathlib.Path, required=True)
    parser.add_argument('--runs', type=pathlib.Path, required=True, help='JSON array of all registered runs')
    parser.add_argument('--output', type=pathlib.Path, required=True)
    args = parser.parse_args()
    result = score(json.loads(args.registration.read_text()), json.loads(args.runs.read_text()))
    args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
    print(json.dumps({'reported_outcome_gate': result['reported_outcome_gate'], 'pairs': result['pairs']}))
    if not result['reported_outcome_gate']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
