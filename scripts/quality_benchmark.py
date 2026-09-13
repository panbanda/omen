#!/usr/bin/env python3
"""Paired, dependency-free development benchmark. Does not execute fixture code."""
import argparse
import hashlib
import json
import pathlib
import statistics
import subprocess
import tempfile
import time


def digest(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def score(predicted, expected):
    predicted, expected = set(predicted), set(expected)
    tp, fp, fn = len(predicted & expected), len(predicted - expected), len(expected - predicted)
    return dict(tp=tp, fp=fp, fn=fn,
                precision=tp / (tp + fp) if tp + fp else None,
                recall=tp / (tp + fn) if tp + fn else None)


def observe(binary, root, query, timeout):
    started = time.perf_counter()
    result = dict(error=None, predicted=[], output_bytes=0)
    try:
        proc = subprocess.run([binary, '-p', str(root), '-f', 'json', '--compact',
                               'symbol', query, '--no-source'], capture_output=True,
                              timeout=timeout, check=True)
        result['output_bytes'] = len(proc.stdout)
        report = json.loads(proc.stdout)
        if not isinstance(report, dict) or not isinstance(report.get('callees'), list):
            raise ValueError('missing callees array')
        predicted = []
        for entry in report['callees']:
            if not isinstance(entry, dict) or not isinstance(entry.get('qualified_name'), str):
                raise ValueError('invalid callee')
            predicted.append(entry['qualified_name'])
        canonical = json.dumps(report, sort_keys=True, separators=(',', ':')).encode()
        result.update(predicted=sorted(set(predicted)),
                      report_sha256=hashlib.sha256(canonical).hexdigest())
    except (subprocess.SubprocessError, ValueError, OSError) as exc:
        result['error'] = str(exc)
    result['elapsed_ms'] = (time.perf_counter() - started) * 1000
    return result


def reliable(observations):
    return (all(o['error'] is None for o in observations)
            and all(o.get('report_sha256') == observations[0].get('report_sha256')
                    for o in observations))


def gate(before, after, healthy):
    return healthy and after['fp'] <= before['fp'] and after['fn'] <= before['fn']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True)
    parser.add_argument('--candidate', required=True)
    parser.add_argument('--corpus', default='tests/fixtures/quality/corpus.json')
    parser.add_argument('--repetitions', type=int, default=7)
    parser.add_argument('--timeout', type=float, default=30)
    parser.add_argument('--baseline-label', required=True)
    parser.add_argument('--candidate-label', required=True)
    parser.add_argument('--real-root', type=pathlib.Path, default=pathlib.Path('..'))
    parser.add_argument('--enforce', action='store_true')
    args = parser.parse_args()
    if args.repetitions < 2 or args.timeout <= 0:
        parser.error('require repetitions >= 2 and positive timeout')
    corpus = json.loads(pathlib.Path(args.corpus).read_text())
    if corpus.get('version') != 1 or not corpus.get('cases'):
        parser.error('require version 1 and nonempty cases')
    ids = [c['id'] for c in corpus['cases']]
    if len(ids) != len(set(ids)):
        parser.error('duplicate case IDs')
    binaries = [str(pathlib.Path(p).resolve()) for p in (args.baseline, args.candidate)]
    results = []
    for case_index, case in enumerate(corpus['cases']):
        with tempfile.TemporaryDirectory(prefix='omen-quality-') as tmp:
            root = pathlib.Path(tmp)
            if 'repository' in case:
                root = (args.real_root / case['directory']).resolve()
                actual = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
                if actual != case['revision']:
                    raise ValueError(f"revision mismatch: {case['id']}")
                if subprocess.check_output(['git', '-C', str(root), 'status', '--porcelain']):
                    raise ValueError(f"dirty real repository: {case['id']}")
            else:
                for name, source in case['files'].items():
                    path = pathlib.PurePosixPath(name)
                    if path.is_absolute() or '..' in path.parts or '\\' in name:
                        raise ValueError('unsafe fixture path')
                    dest = root / name
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_text(source)
            observations = [[], []]
            # First observation is a validated warmup, retained for reliability.
            for rep in range(args.repetitions + 1):
                order = (0, 1) if (rep + case_index) % 2 == 0 else (1, 0)
                for variant in order:
                    observations[variant].append(observe(binaries[variant], root,
                                                        case.get('query', 'caller'), args.timeout))
            variants = []
            for obs in observations:
                variant = dict(
                    reliable=reliable(obs),
                    elapsed_ms=[o['elapsed_ms'] for o in obs],
                    errors=[o['error'] for o in obs],
                    output_bytes=[o['output_bytes'] for o in obs],
                    report_sha256=[o.get('report_sha256') for o in obs],
                    predicted=obs[1]['predicted'],
                    p50_ms=statistics.median(o['elapsed_ms'] for o in obs[1:]),
                )
                if 'expected' in case:
                    variant['score'] = score(obs[1]['predicted'], case['expected'])
                else:
                    # Partial real-code labels: do not treat unreviewed edges as false.
                    predicted = set(obs[1]['predicted'])
                    variant['checks'] = dict(
                        missing=sorted(set(case.get('required', [])) - predicted),
                        forbidden=sorted(set(case.get('forbidden', [])) & predicted))
                variants.append(variant)
            healthy = all(v['reliable'] for v in variants)
            if 'expected' in case:
                passed = gate(variants[0]['score'], variants[1]['score'], healthy)
            else:
                passed = healthy and all(
                    set(variants[1]['checks'][key]) <= set(variants[0]['checks'][key])
                    for key in ('missing', 'forbidden'))
            results.append(dict(id=case['id'], language=case['language'],
                                non_regression=passed, baseline=variants[0], candidate=variants[1]))
    report = dict(standard='omen-improvement-v1', corpus_sha256=digest(args.corpus),
                  baseline=dict(label=args.baseline_label, sha256=digest(binaries[0])),
                  candidate=dict(label=args.candidate_label, sha256=digest(binaries[1])),
                  repetitions=args.repetitions, warmups=1, order='alternating',
                  broad_improvement_proven=False,
                  unmeasured=['model editing', 'actual tokens/cost', 'peak RSS',
                              'release performance', 'compiler binding', 'held-out generalization'],
                  cases=results, non_regression=all(r['non_regression'] for r in results))
    print(json.dumps(report, indent=2))
    return 2 if args.enforce and not report['non_regression'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
