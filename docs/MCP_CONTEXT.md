# MCP context and identity contract

## Progressive discovery

1. Call `context` once for repository orientation. Cache only for the same repository revision.
2. Use `repomap` for ranked symbol/location discovery, or `impact` for candidate graph traversal.
3. Call `get_symbol` with `include_source: false` to inspect relationships and resolution evidence.
4. Fetch source only for the selected definition. Use `file:name` and `start_line` from an ambiguity choice.

An ambiguous `get_symbol` query now returns `found: true`, `ambiguous: true`,
`choices`, and a navigation hint. It does **not** return an arbitrarily selected
definition's source, callers, or callees. Choices contain qualified name, exact
start line, and signature. `start_line` is a positive u32 and is matched exactly,
not as a containing source range. A nonexistent line returns `found: false`.
File and line identify a definition within the current checkout, not across edits.
The CLI's existing first-match behavior is unchanged for compatibility.

## Lossless compact tables

`context`, `repomap`, `impact`, and `get_symbol` advertise `compact: true`.
The default JSON representation remains unchanged. Context's direct markdown
format bypasses this JSON encoding.

When profitable, the response envelope adds `encoding: "omen.tables.v1"` and
`table_paths`. Each path is a JSON Pointer **relative to `result`**, pointing to
an object with `columns` and `rows`. Zip each row with the columns to reconstruct
the original array of records. Only the listed paths are tables; do not infer
tables from ordinary source text or arbitrary `columns`/`rows` objects.

Only homogeneous flat records are packed. Field names are transmitted once;
values, nulls, booleans, signatures, source locations, scores, call resolution
status, and candidate evidence are retained. Heterogeneous records and nested
relationships retain their existing structure. JSON Pointer escaping applies.
Pagination is applied **before** encoding and its metadata is unchanged.
Compaction does not recover information omitted by pagination or context budgets.

Encoding falls back to ordinary JSON unless the **complete envelope**, including
encoding metadata, saves at least 128 UTF-8 bytes. This headroom rejects marginal
byte savings that can increase tokens. This runtime check is not a promise
of fewer tokens for every tokenizer or response. Exact token counts are evaluated
separately by the benchmark. Small responses may remain unchanged.

## What is guaranteed

The lossless encoding is guaranteed in this repository, not by a benchmark.
`src/mcp/compact.rs` ships `encode` and its inverse `decode`, and its tests
assert `decode(encode(x)) == x` over escaped JSON Pointer keys, every scalar type
including nulls, and a root-level table. A declared table path that is missing,
or a row whose arity disagrees with its columns, is rejected rather than silently
reshaped. `decode` is public as `omen::mcp::decode`, so a client reconstructs
records with the same code these tests cover.

The token measurements below are a historical record, not a reproducible result.
They came from a Python harness that was removed along with the rest of the
repository's Python tooling, together with the raw reports it wrote. It ran
against baseline commit `aa8aafecc45d1a579238a10bafca9301957980ed`, counted
tokens with `tiktoken==0.14.0`, and sent real MCP stdio requests over 16
repository/tool combinations in alternating order. Nothing in the repository
reproduces those numbers today.

These were development checks, not held-out coding-agent task results. Token
counts do not prove better model comprehension. No claim is made about model
task success, latency, peak memory, compiler-level binding, or complete graph
recall. Existing call-edge accuracy gates must also pass before adoption.

## Measured development results (2026-09-14)

Baseline: merged PR #506 (`aa8aafe`). The baseline binary SHA-256 matched the
candidate recorded by that PR's synthetic benchmark. The three raw reports that
held the binary hashes, pinned revisions, observations and per-case gates were
removed along with the Python tooling that produced them, so the figures below
cannot be re-derived from this repository.

| Check | Full / before | Compact / after |
| --- | ---: | ---: |
| Response tokens, cl100k_base | 42,941 | 32,543 (-24.2%) |
| Response tokens, o200k_base | 43,152 | 32,985 (-23.6%) |
| UTF-8 response bytes | 152,312 | 107,358 (-29.5%) |
| Exact duplicate-definition retrieval | 4/8 | 8/8 |
| Explicit ambiguity without false attribution | Not enforced | 4/4 |

All 16 real-repository/tool cases are token-non-regressing on both tokenizers;
all 48 pairs reconstruct identical code facts. Graph traversal cases with small
outputs remain unchanged. This proves graph-data retention, not improved graph
recall. The added tool schema costs 207/212 tokens respectively. Charging that
increase once still leaves savings of 10,191/9,955 tokens over these 16 responses.

The first candidate **failed** the predeclared per-case token gate: Cobra's
`get_symbol` saved 16 bytes but added 3/7 tokens. The implementation was revised
to require 128 bytes of envelope savings; the unchanged zero-token-regression
gate then passed. These repositories are consequently development data, not a
held-out validation set.

The existing synthetic call-edge gate and all six source-reviewed real-repository
relationship checks remain non-regressing against PR #506. `cargo test
--all-targets` passed 2,283 tests (five ignored), including benchmark smoke runs;
the Python benchmark/scorer suites passed 11 tests. Default JSON compatibility is
preserved except for intentionally explicit ambiguous MCP lookups. Clients that
assumed every found name has a source report must handle the new choices response.
