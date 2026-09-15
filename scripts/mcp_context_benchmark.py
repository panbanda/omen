#!/usr/bin/env python3
"""Exercise real MCP stdio responses; prove table round trips and measure tokens.

Requires tiktoken==0.14.0. Never executes code from benchmark repositories.
"""
import argparse
import copy
import hashlib
import json
import pathlib
import statistics
import subprocess
import time
import tempfile
import tiktoken


def expand(envelope):
    result = copy.deepcopy(envelope)
    if result.get('encoding') != 'omen.tables.v1':
        return result
    for pointer in result.pop('table_paths'):
        parts = [p.replace('~1', '/').replace('~0', '~') for p in pointer.split('/')[1:]]
        parent, key = result, 'result'
        for part in parts:
            parent = parent[key]
            key = int(part) if isinstance(parent, list) else part
        table = parent[key]
        parent[key] = [dict(zip(table['columns'], row, strict=True)) for row in table['rows']]
    result.pop('encoding')
    return result


def call(binary, root, tool, arguments):
    request = {'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
               'params': {'name': tool, 'arguments': arguments}}
    started = time.perf_counter()
    proc = subprocess.run([binary, '-p', str(root), 'mcp'],
                          input=json.dumps(request) + '\n', text=True,
                          capture_output=True, check=True, timeout=180)
    elapsed = (time.perf_counter() - started) * 1000
    response = json.loads(proc.stdout)
    if 'error' in response or response['result'].get('isError'):
        raise ValueError(response)
    text = response['result']['content'][0]['text']
    return text, json.loads(text), elapsed


def stable(envelope):
    envelope = copy.deepcopy(envelope)
    # Repomap emits wall-clock metadata, independent of the code facts.
    envelope.get('result', {}).pop('generated_at', None)
    return envelope


def identity_checks(baseline, candidate):
    fixtures = {
        'ruby': ('same.rb', 'class A\n  def helper\n    1\n  end\nend\nclass B\n  def helper\n    2\n  end\nend\n', [2, 7]),
        'typescript': ('same.ts', 'class A {\n  helper() { return 1; }\n}\nclass B {\n  helper() { return 2; }\n}\n', [2, 5]),
        'rust': ('same.rs', 'struct A;\nstruct B;\nimpl A {\n fn helper() -> i32 { 1 }\n}\nimpl B {\n fn helper() -> i32 { 2 }\n}\n', [4, 7]),
        'go': ('same.go', 'package main\ntype A struct {}\ntype B struct {}\nfunc (a A) helper() int { return 1 }\nfunc (b B) helper() int { return 2 }\n', [4, 5]),
    }
    rows = []
    for language, (filename, source, lines) in fixtures.items():
        with tempfile.TemporaryDirectory(prefix='omen-identity-') as tmp:
            root = pathlib.Path(tmp)
            (root / filename).write_text(source)
            correct = [0, 0]
            for mode, binary in enumerate([baseline, candidate]):
                for line in lines:
                    _, envelope, _ = call(binary, root, 'get_symbol',
                                          {'name': f'{filename}:helper', 'start_line': line})
                    correct[mode] += envelope['result'].get('start_line') == line
            _, unselected, _ = call(candidate, root, 'get_symbol', {'name': f'{filename}:helper'})
            safe = (unselected['result'].get('ambiguous') is True
                    and 'source' not in unselected['result'] and 'callees' not in unselected['result']
                    and sorted(c['start_line'] for c in unselected['result']['choices']) == lines)
            rows.append({'language': language, 'correct_definitions': correct, 'total': 2,
                         'safe_ambiguity': safe})
    return rows


def schema_text(binary):
    request = {'jsonrpc': '2.0', 'id': 1, 'method': 'tools/list'}
    proc = subprocess.run([binary, 'mcp'], input=json.dumps(request) + '\n',
                          text=True, capture_output=True, check=True, timeout=30)
    return json.dumps(json.loads(proc.stdout)['result'], separators=(',', ':'), sort_keys=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--baseline', required=True)
    parser.add_argument('--real-root', type=pathlib.Path, default=pathlib.Path('..'))
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.repetitions < 2:
        parser.error('at least two repetitions required')
    binary = str(pathlib.Path(args.binary).resolve())
    baseline = str(pathlib.Path(args.baseline).resolve())
    encoders = {name: tiktoken.get_encoding(name) for name in ['cl100k_base', 'o200k_base']}
    schemas = [schema_text(b) for b in [baseline, binary]]
    schema_tokens = {name: [len(enc.encode(s, disallowed_special=())) for s in schemas]
                     for name, enc in encoders.items()}
    corpus = json.loads(pathlib.Path('tests/fixtures/quality/real.json').read_text())
    repos = {case['directory']: case for case in corpus['cases']}
    rows = []
    for directory, case in repos.items():
        root = (args.real_root / directory).resolve()
        revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
        if revision != case['revision']:
            raise ValueError(f'{directory}: revision mismatch')
        if subprocess.check_output(['git', '-C', str(root), 'status', '--porcelain']):
            raise ValueError(f'{directory}: dirty benchmark repository')
        for tool, arguments in [('context', {'max_tokens': 8000}),
                                ('repomap', {'max_symbols': 100, 'limit': 0}),
                                ('impact', {'symbol': case['query'], 'depth': 2, 'limit': 0}),
                                ('get_symbol', {'name': case['query'], 'include_source': False, 'limit': 0})]:
            pairs = []
            times = [[], []]
            for repeat in range(args.repetitions):
                pair = [None, None]
                for mode in ([0, 1] if repeat % 2 == 0 else [1, 0]):
                    text, envelope, elapsed = call(binary, root, tool, {**arguments, 'compact': bool(mode)})
                    pair[mode] = (text, envelope)
                    times[mode].append(elapsed)
                if stable(expand(pair[1][1])) != stable(pair[0][1]):
                    raise ValueError(f'{directory}/{tool}: lossy encoding')
                pairs.append(pair)
            if any([stable(expand(item[1])) for item in pair] !=
                   [stable(expand(item[1])) for item in pairs[0]] for pair in pairs):
                raise ValueError(f'{directory}/{tool}: nondeterministic output')
            before, after = pairs[0]
            counts = {name: [len(enc.encode(text, disallowed_special=())) for text in (before[0], after[0])]
                      for name, enc in encoders.items()}
            rows.append({'repository': case['repository'], 'revision': revision, 'tool': tool,
                         'bytes': [len(before[0].encode()), len(after[0].encode())],
                         'tokens': counts, 'lossless': True,
                         'elapsed_ms': times,
                         'median_ms': [statistics.median(t) for t in times]})
    totals = {name: [sum(row['tokens'][name][i] for row in rows) for i in [0, 1]] for name in encoders}
    identities = identity_checks(baseline, binary)
    # Charge the entire added schema cost once against compact-mode savings.
    # This is an amortization check, not a historical coding-session replay.
    session_totals = {name: [totals[name][0], totals[name][1] +
                            max(0, schema_tokens[name][1] - schema_tokens[name][0])]
                      for name in encoders}
    report = {'binary_sha256': hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),
              'baseline_sha256': hashlib.sha256(pathlib.Path(baseline).read_bytes()).hexdigest(),
              'tiktoken_version': tiktoken.__version__, 'repetitions': args.repetitions,
              'scope': 'Same-binary full vs compact ablation; lossless code data, not model task success. Only result.generated_at excluded from cross-call equality.',
              'rows': rows, 'total_tokens': totals, 'identity_checks': identities,
              'schema_tokens': schema_tokens, 'full_vs_compact_plus_schema_delta': session_totals,
              'gate': all(after <= before for row in rows for before, after in row['tokens'].values())
                      and all(after < before for before, after in totals.values())
                      and all(after < before for before, after in session_totals.values())
                      and all(r['safe_ambiguity'] and r['correct_definitions'][1] == r['total']
                              for r in identities)}
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'total_tokens': totals, 'gate': report['gate']}, indent=2))
    if not report['gate']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
