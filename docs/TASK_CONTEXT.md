# Task context: bounded facts, not a safety certificate

Call `task_context` with `name` (prefer `file:name`), optional exact `start_line`,
`depth` (0–4, default 2), `max_bytes` (1024–1048576, default 12000), and optional
`include_history` (default false). Ambiguity returns choices without selecting a
definition. Free-text task/diff interpretation is not implemented yet.

The response combines a seed-first candidate call neighborhood, signatures and
locations, seed call-resolution evidence, test/framework convention hints,
bounded AST identifier occurrences, parse-error counts, and a content fingerprint.
Optional 30-day co-change hints are separate from semantic edges. History absence
is explicit. Omission counters disclose budget trimming; the seed is never silently
discarded. A budget too small for required metadata returns an error. The byte
budget covers the JSON `result`, not the RPC wrapper or tokenizer cost. Generic
`limit`/`offset` pagination is not advertised for this tool.

## Incremental work and freshness

`task_context` and `get_symbol` share a session-local cache. Every request reads
and hashes the current files. Unchanged snapshots reuse the graph; changed files
alone are reparsed, followed by a full graph/symbol rebuild. Changes, removals,
renames, and repository switches invalidate the relevant state. Parse inputs
come from the same bytes used for hashing, not a second disk read. Cache retention
is limited to 32 MiB of source and 10000 files; expanded AST/graph memory is not
hard-bounded by that source limit. Large requests can still require substantial
transient memory. This is not dependency-local incremental graph rebuilding.

Snapshot hashes include normalized relative paths and content hashes. They are
not an atomic checkout guarantee during concurrent cross-file edits. Existing
`get_symbol` source slicing still reads the current file separately. An agent
must re-fetch after an edit; a stale position is not a persistent symbol ID.

## Proof

Run `cargo test --test task_context`. Tests cover cache hits, same-size edits,
deletions, fresh-rebuild equality, repository isolation and escape rejection,
four-language two-hop test-candidate discovery, seed retention, Unicode byte
budgets and omission counts, ambiguous selection, nested syntax-scope exclusion,
directory-role labeling, missing history, and co-change hints remaining separate
from graph edges.

```sh
python scripts/task_context_benchmark.py --baseline /path/to/aa8aafe/omen \
  --candidate /path/to/candidate/omen --real-root .. \
  --output docs/benchmarks/task-context.json
```

The committed raw report records pinned repository revisions and binary hashes.
Four language fixtures specify four expected neighborhood definitions each:
one `get_symbol` call retrieves 3/4; `task_context` retrieves 4/4, with no irrelevant
definitions. This is a comparison of different one-call contracts, not model task
success or general retrieval quality.

The richer result costs **528–532 o200k_base tokens**, versus **201–202** for
`get_symbol` in these fixtures. Tokens per retrieved definition therefore worsen;
this is a retrieval-depth/provenance tradeoff, not a compression win. Use the
smaller lookup when only a direct symbol report is needed.

Across Rack (98 source files), Zod (521), ripgrep (113), and Cobra (36), cold
requests parse 768 files total. Thirty follow-up requests per repository perform
zero parses and zero graph rebuilds, with identical code facts after removing
cache counters. Timings are raw shared-host debug measurements, not statistically
validated release speedups. Reads and hashing still occur on every request.

## Boundaries by improvement area

- Test roles: path/name candidates, not measured execution coverage. Do not use
  this to skip all other tests.
- Data flow: syntax occurrences only, not binding-resolved def-use, taint paths,
  return/parameter propagation, or branch feasibility.
- Frameworks: controller/job/schema/etc. directory conventions only, not proven
  routing, dependency injection, ORM, or middleware relationships.
- History: observed co-change evidence, not a dependency or demonstrated ranking
  improvement on historical bug fixes.
- Confidence: categorical provenance and parse diagnostics, not calibrated
  probabilities or compiler-proven edges.
- Caching: validated parse reuse and snapshot graph reuse, not full incremental
  analysis or a release latency/memory improvement claim.

Those unfinished areas require independent labels and additional implementation.
