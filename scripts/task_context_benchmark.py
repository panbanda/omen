#!/usr/bin/env python3
"""Real stdio cache measurements and source-labelled context retrieval; no model calls."""
import argparse
import hashlib
import json
import pathlib
import selectors
import subprocess
import tempfile
import time
import tiktoken


class Session:
    def __init__(self, binary, root):
        self.proc = subprocess.Popen([binary, '-p', str(root), 'mcp'], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)

    def call(self, tool, args):
        request = {'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call',
                   'params': {'name': tool, 'arguments': args}}
        start = time.perf_counter()
        self.proc.stdin.write(json.dumps(request) + '\n')
        self.proc.stdin.flush()
        with selectors.DefaultSelector() as selector:
            selector.register(self.proc.stdout, selectors.EVENT_READ)
            if not selector.select(60):
                raise TimeoutError('MCP response timed out')
        response = json.loads(self.proc.stdout.readline())
        elapsed = (time.perf_counter() - start) * 1000
        if 'error' in response or response['result'].get('isError'):
            raise ValueError(response)
        text = response['result']['content'][0]['text']
        return json.loads(text)['result'], text, elapsed

    def close(self):
        self.proc.stdin.close()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
            raise
        if self.proc.returncode != 0:
            raise RuntimeError(f'MCP exited {self.proc.returncode}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True)
    parser.add_argument('--candidate', required=True)
    parser.add_argument('--real-root', type=pathlib.Path, default=pathlib.Path('..'))
    parser.add_argument('--output', type=pathlib.Path, required=True)
    args = parser.parse_args()
    before, after = [str(pathlib.Path(p).resolve()) for p in [args.baseline, args.candidate]]
    encoder = tiktoken.get_encoding('o200k_base')
    fixtures = {
        'rust': ('a.rs', 'fn helper(){}\nfn service(){helper();}\nfn wrapper(){service();}\nfn test_service(){wrapper();}'),
        'typescript': ('a.ts', 'function helper(){}\nfunction service(){helper();}\nfunction wrapper(){service();}\nfunction test_service(){wrapper();}'),
        'ruby': ('a.rb', 'def helper; end\ndef service; helper(); end\ndef wrapper; service(); end\ndef test_service; wrapper(); end'),
        'go': ('a.go', 'package main\nfunc helper(){}\nfunc service(){helper()}\nfunc wrapper(){service()}\nfunc TestService(){wrapper()}'),
    }
    retrieval = []
    for language, (file, source) in fixtures.items():
        with tempfile.TemporaryDirectory(prefix='omen-task-proof-') as tmp:
            root = pathlib.Path(tmp)
            (root / file).write_text(source)
            expected = {f'{file}:{name}' for name in ['service', 'helper', 'wrapper',
                        'TestService' if language == 'go' else 'test_service']}
            reports = []
            token_counts = []
            for binary, tool in [(before, 'get_symbol'), (after, 'task_context')]:
                session = Session(binary, root)
                try:
                    report, text, _ = session.call(tool, {'name': 'service', 'include_source': False})
                finally:
                    session.close()
                if tool == 'get_symbol':
                    found = {report['qualified_name']} | {s['qualified_name'] for s in report['callers'] + report['callees']}
                else:
                    found = {s['location']['qualified_name'] for s in report['symbols']}
                reports.append({'correct': len(found & expected), 'irrelevant': len(found - expected),
                                'missing': len(expected - found)})
                token_counts.append(len(encoder.encode(text, disallowed_special=())))
            retrieval.append({'language': language, 'one_call_retrieval': reports,
                              'response_tokens': token_counts, 'expected_definitions': sorted(expected)})
    corpus = json.loads(pathlib.Path('tests/fixtures/quality/real.json').read_text())
    repos = {case['directory']: case for case in corpus['cases']}
    cache_rows = []
    for directory, case in repos.items():
        root = (args.real_root / directory).resolve()
        revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
        if revision != case['revision'] or subprocess.check_output(['git', '-C', str(root), 'status', '--porcelain']):
            raise ValueError('benchmark checkout revision/cleanliness mismatch')
        session = Session(after, root)
        try:
            # Resolve an ambiguous query explicitly before measuring the selected context.
            report, _, _ = session.call('task_context', {'name': case['query']})
            query = {'name': case['query']}
            if report.get('ambiguous'):
                choice = report['choices'][0]
                query = {'name': choice['qualified_name'], 'start_line': choice['line']}
        finally:
            session.close()
        session = Session(after, root)
        observations = []
        try:
            expected_data = None
            for repeat in range(31):
                report, text, elapsed = session.call('task_context', query)
                cache = report.pop('cache')
                stable = json.dumps(report, sort_keys=True)
                if expected_data is None:
                    expected_data = stable
                elif stable != expected_data:
                    raise ValueError('warm-cache facts changed')
                if repeat and (cache['parsed_files'] != 0 or cache['graph_rebuilt']):
                    raise ValueError('unchanged snapshot reparsed/rebuilt')
                observations.append({'elapsed_ms': elapsed, 'cache': cache,
                                     'tokens': len(encoder.encode(text, disallowed_special=()))})
        finally:
            session.close()
        cache_rows.append({'repository': case['repository'], 'revision': revision, 'observations': observations})
    result = {'baseline_sha256': hashlib.sha256(pathlib.Path(before).read_bytes()).hexdigest(),
              'candidate_sha256': hashlib.sha256(pathlib.Path(after).read_bytes()).hexdigest(),
              'retrieval': retrieval, 'cache': cache_rows,
              'scope': 'Development one-call neighborhood recall and deterministic cache work counts. Extra facts can cost more tokens. Debug cold/warm times are descriptive, not release speedup proof.',
              'gate': all(r['one_call_retrieval'][1] == {'correct': 4, 'irrelevant': 0, 'missing': 0} for r in retrieval)}
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'gate': result['gate'], 'retrieval': retrieval}, indent=2))
    if not result['gate']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
