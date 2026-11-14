# MCP context and identity contract

## Progressive discovery

1. Call `context` once for repository orientation. Cache only for the same repository revision.
2. Use `repomap` for ranked symbol/location discovery, or `impact` for candidate graph traversal.
3. Call `get_symbol` with `include_source: false` to inspect relationships and resolution evidence.
4. Get the source only for the selected definition. Use the `name` and the `start_line` of one suggestion of an `ambiguous` error.

When a name agrees with more than one definition, `get_symbol` and
`task_context` give an error with the code `ambiguous`. They do not select a
definition. `error.suggestions` holds one object for each choice, with the
qualified `name` and the `start_line`. For `get_symbol`, each object also has
the `signature`. `start_line` is an integer from 1 to 4294967295. It must be
equal to the line on which the definition starts. When no definition starts on
that line, the tool gives an error with the code `not_found`. The file and the
line identify a definition in the current checkout, not after an edit.

## Arguments, errors, and the envelope

One `ToolSpec` in `src/mcp/tools/` is the source of each tool. The schema in
`tools/list`, the validation of arguments, and the list of tool names come from
these specifications.

### Arguments

The server validates the arguments of each call before it runs the tool. Each
input schema has `additionalProperties: false`. The server does not replace an
incorrect value with a default. One call reports all its problems.

The schema shows each bound that the validation applies. A number with a zero
fraction, such as `30.0`, is a valid integer. The schema shows a `default` only
when the value is the same for each server. When a default comes from the
configuration, the description of the parameter names the configuration key.

### The same answer as the CLI

A tool and its CLI command give the same data for the same repository and the
same options. An argument that is absent takes the value that the command
takes: the value in the configuration, or the built-in default.

| Tool | Parameter | Value when the argument is absent |
|---|---|---|
| `churn` | `days` | `churn.since` (built-in default `6m`) |
| `clones` | `min_tokens` | `duplicates.min_tokens` (50) |
| `clones` | `similarity` | `duplicates.min_similarity` (0.9) |
| `clones` | `include_tests` | `false`; `duplicates.exclude_tests` then applies |
| `defect` | `days` | `defect.churn_days` (30) |
| `hotspot` | `days` | `hotspot.days` (90) |
| `flags` | `provider` | `feature_flags.providers` |
| `flags` | `stale_days` | `feature_flags.stale_days`; when that value is 0, the age is 14 days |

`changes` reads `changes.days`, and `satd` reads `satd.custom_markers`. They
have no parameter for these values.

`tests/parity.rs` compares each tool with its command on a fixture repository,
with and without a configuration file. Its tables list each permitted
difference, each parameter with its flag, and each parameter or flag that only
one side has. These differences are permitted:

- `generated_at`, because the two sides run at different times.
- `repository_root` of `churn`: the server makes the path canonical.
- The default page: `limit` has the default 50, and most commands have no
  default for `--top`.

| Code | Cause |
|---|---|
| `missing_argument` | A required argument is absent. |
| `invalid_argument` | A value has the wrong type, is out of its bounds, or is not in its enum. A `null` value is also invalid. |
| `unknown_argument` | The tool has no argument with that name. `suggestions` holds the nearest valid names. |

### Errors

A JSON-RPC error object is only for a protocol fault:

- A request that is not valid JSON (`-32700`).
- An unknown method (`-32601`).
- An unknown tool, or a `tools/call` request without a tool name (`-32602`).

Each other failure is a tool result with `isError: true`. The text of the
result and its `structuredContent` hold the same JSON:

```json
{
  "error": {
    "code": "invalid_argument",
    "message": "The value of `direction` is not correct.",
    "param": "direction",
    "expected": "one of: callers, callees, both",
    "received": "caller",
    "suggestions": ["callers", "callees"],
    "next": "Send the call again with a correct value for `direction`."
  }
}
```

Each key of `error` is always present. A key without a value is `null`, and
`suggestions` without a value is an empty array. When one call has more than
one problem, `errors` holds all of them and `error` is the first.

The other codes are:

| Code | Cause |
|---|---|
| `not_found` | A path does not exist, or no definition agrees with a symbol name. For a symbol, `suggestions` holds near names. |
| `ambiguous` | A symbol name agrees with more than one definition. `suggestions` holds the choices. |
| `path_outside_root` | A path is not in the MCP root. Start the server with `--allow-external-paths` to permit such a path. |
| `git_unavailable` | The tool needs git data that git cannot give. |
| `internal` | All other failures of the server or of an analyzer. |

### Envelope

Each tool gives its result in the same envelope:

| Key | Value |
|---|---|
| `tool` | The tool name. |
| `total_items` | The size of the primary list before `limit` and `offset`, and after the cuts of the tool. `null` when the result has no list. |
| `returned` | The number of items in the primary list of this result. |
| `offset` | The offset that the call gave. |
| `has_more` | `true` when a next page exists. |
| `next_offset` | The `offset` of the next page, or `null` when `has_more` is `false`. |
| `truncated` | One `{path, total, returned}` object for each list in `result` that does not hold all its items. |
| `result` | The data of the tool. |
| `git_skipped_reason` | Present only when an analyzer ran without git data. |

`path` in `truncated` is a JSON pointer from `result`. The segment `*` means
each element of an array, such as `/files/*/functions`.

`truncated` also shows the data that a tool cut before the pagination:

- `task_context`: each list that `max_bytes` cut, and `/syntax_facts` with the
  total 1 when the budget removed that object.
- `get_symbol`: `/source`, in lines, when `max_source_lines` cut the source.
- `context`: each list that `max_tokens` cut.
- `semantic_search` and `semantic_search_hyde`: `/results` when `top_k` cut the
  results that the filters accepted.

One rule applies to each tool. `total_items` is the size of the primary list
before `limit` and `offset`, and after the cuts of the tool. Each cut of a tool
has an entry in `truncated` with the total before the cut.

These limits do not appear in `truncated`, because the server does not know
the total without them: `max_symbols` and `max_risks` of `context`,
`max_symbols` of `repomap`, and `depth` of `impact` and `task_context`.

The search tools apply `min_score`, `files`, and `max_complexity` first, and
then `top_k`. An empty `include_projects` is the same as no `include_projects`.
With `include_projects`, the `file` of each result has a `[label] ` before the
path. `files` then accepts two forms. A path applies to each project. A path
with a `[label] ` before it, as a result gives it, applies to that project
only. A label that is not the label of a project of the search is an
`invalid_argument` error, with the valid labels in `suggestions`.

### Identifiers go from one tool to a different tool

A tool accepts each identifier and each path that a different tool gives:

- A `qualified_name` is a valid `name` of `get_symbol` and `task_context`, and
  a valid `symbol` of `impact`. `get_symbol` and `impact` give the key of the
  symbol, `file#Container.name`. The other tools give `file:name`. Each of
  these tools accepts the two forms. Do not parse the identifier.
- A `file` without a `[label] ` is a valid `file` of `outline`, a valid `path`
  of each tool, and the file part of a `file:name`.
- The `file` of a search result is a valid entry of `files`, with or without a
  `[label] `.

Each path is from the `path` argument, which has the MCP root as its default.
`outline` gives paths in this form also. When `path` is a file, a tool reads
that one file.

A `file` with a `[label] ` is valid only in `files` of a search with the same
`include_projects`. A label is the name of a project directory, with a number
when two projects have one name. Thus only that search call can change the
label back into a path, and the other tools do not accept it. Each result with
a label has two more fields for the other tools:

| Field | Value | Use |
|---|---|---|
| `project` | The path of the project. It is `.` for the MCP root, a path from the MCP root for a project in it, and an absolute path for a project out of it. | The `path` argument. |
| `project_file` | The `file` without the `[label] `. | The `file` of `outline`, or the file part of a `file:name`, with `project` as `path`. |

For example, the result `{"file": "[sub] simple.rs", "project": "sub",
"project_file": "simple.rs"}` goes to `outline` as `{"path": "sub", "file":
"simple.rs"}`. The path confinement of the server applies to `project` as to
each other path.

`limit` has the default 50, and the value 0 removes the limit. When `has_more`
is `true`, send the same call with `offset` set to `next_offset`. The pages
together hold each item one time.

The primary list of `score` is the `components` object. `limit` and `offset`
apply to its entries in the sequence of their keys. `skipped_components` and
the overall score are complete on each page.

`context` with `format: "markdown"` gives text without an envelope. `limit` and
`offset` then apply to the symbols in the text.

`task_context` has no `limit` and no `offset`, because `max_bytes` controls its
size. Its `total_items` and `returned` are the number of symbols in the result,
after the byte budget. Its `has_more` is always `false`. `truncated` shows the
lists that the byte budget cut, with the totals before the cut.

`initialize` gives an `instructions` string that tells an agent which tool to
call first, and how to use the pagination and the errors.

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
