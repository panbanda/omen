# Deprecations

Single source of truth for everything scheduled for removal. When cutting a
breaking (major) release, work this list top to bottom and remove each entry
plus its code.

Every deprecation in the codebase is tagged with a greppable marker so you can
find all sites for a given removal milestone:

```sh
rg "DEPRECATED(remove-in: 5.0)"
```

The marker format is `DEPRECATED(remove-in: <version>): <what to use instead>`,
placed in a doc comment or code comment at each site.

## Scheduled for removal

No entries.

Note: `--days` (churn) is a **kept** alias for `--since`, not scheduled for
removal, so it carries no marker.

## Removed in 5.0

Omen 5.0 removed each item in this section. There is no alias and no shim.
Use the replacement.

### CLI flags

| Removed | Replacement |
|---|---|
| `--check` (`complexity`, `score`, `mutation`) | `--gate error` |
| `--top-k` / `-k` (`search query`) | `--top` / `-n` |
| `--output` (`report generate`) | `--data` / `-d` |
| `--output-survivors` (`mutation`) | `--output` / `-o`, which writes the full JSON report |
| `--jobs` of `mutation` | the global `--jobs` / `-j` |
| `--path` / `-p` of `mutation train` | the global `--path` / `-p` |
| `-p` as the short form of `--period` (`score trend`) | `--period` |
| `-p` as the short form of `--port` (`report serve`) | `--port` |

The flags `-p`, `-f`, `-c`, `-v`, and `-j` are global. On each command, `-p` is
the repository path. `stubs --gate` has no default value in the CLI. If the
flag is absent, the `[gate]` table of the configuration gives the mode.

If the `-p` value is not a directory, each command that reads the repository
stops with an error before the analysis. `mcp manifest`, `eval`, `benchmark`,
`report validate`, and `report render` do not read the repository. Thus `score trend -p monthly` and `report serve -p 3000` fail, and
the message names `--period` or `--port`.

The gate of `score` applies only to the bare `score` command. `score trend` has
no gate, and it refuses the `--gate` flag.

The SARIF format keeps each finding. The `--top` and `--offset` flags do not
apply to it, also on `search query`.

On `outline`, `--top` and `--offset` page the `files` list in each format. On
`symbol`, the two flags page each list, for example `callers` and `callees`.

### `--top` on `graph`

On `graph`, `--top` and `--offset` page the `nodes` list in each format. An
edge stays only if its two nodes stay. `nodes_omitted` and `edges_omitted` give
the counts. Before 5.0, the two flags cut the members of each cycle, and they
did nothing if the graph had no cycle.

### Output of a gated command

`complexity`, `score`, `stubs`, and `mutation` write the normal report to stdout
for each gate mode. The gate writes the violations to stderr. `warn` exits with
status 0, and `error` exits with status 2. Before 5.0, a gate on `complexity`
or `score` replaced the report: stdout had a `violations` JSON document, or
nothing for the other formats. Read the violations from stderr, or calculate
them from the report.

### Output of `all`

`all` obeys `-f`. Before 5.0, it wrote JSON for each format, also for the
default format. The default format is Markdown, so `omen all` now writes
Markdown. Use `omen -f json all` to get JSON.

`--top` and `--offset` apply to each analyzer section of `all`. Before 5.0, the
two flags cut the list of analyzers.

### Path options of `mutation`

A relative path that you give on the command line is relative to the working
directory. This applies to `--model`, `--history`, and `--output`. A default
path is relative to the repository at `-p`. The defaults are
`.omen/mutation-model.json` and `.omen/mutation-history.jsonl`.

### Output keys

| Removed | Replacement |
|---|---|
| `all`: analyzer name `duplicates` | `clones` |
| `complexity`: `files[].path` | `files[].file` |
| `churn`: `files[].path` and `files[].relative_path` | `files[].file` |
| `cohesion`: `classes[].path` | `classes[].file` |
| `ownership`: `files[].path` | `files[].file` |
| `graph`: `nodes[].path` | `nodes[].file` |
| `defect`: `files[].file_path` | `files[].file` |
| `tdg`: `files[].file_path` | `files[].file` |
| `search query`: `results[].file_path` | `results[].file` |
| `mutation`: `files[].path` | `files[].file` |
| `score trend`: `component_trends.<name>.Slope` | `component_trends.<name>.slope` |
| `score trend`: `component_trends.<name>.Correlation` | `component_trends.<name>.correlation` |

The `file` value of `churn` is relative to the repository root and has no `./`
prefix. The `hotspot_files` and `stable_files` lists of the `churn` summary
use the same form. The MCP tools give the same keys as the commands.

### Report artifacts

| Removed | Replacement |
|---|---|
| `duplicates.json` (`report generate`) | `clones.json` |
| `hotspots.json` (`report generate`) | `hotspot.json` |
| `--skip duplicates` (`report generate`) | `--skip clones` |
| `--skip hotspots` (`report generate`) | `--skip hotspot` |
## Changed in 5.0

These changes have no compatibility shim. Each row gives the old behaviour, the
new behaviour, and the necessary change in a client.

### MCP server

`docs/MCP_CONTEXT.md` has the full contract for arguments, errors, and the
envelope.

| Area | Before 5.0 | From 5.0 | Change in a client |
|---|---|---|---|
| Unknown argument | The server ignored the argument. | The call gives an `unknown_argument` error. | Send only the arguments in the input schema of the tool. |
| Value of the wrong type, or `null` | The server used the default. | The call gives an `invalid_argument` error. | Send each value with the type in the input schema. Do not send `null`. |
| Value that is not in its enum (`direction` of `impact`, `format` of `context`) | The server used the default. | The call gives an `invalid_argument` error. | Send one of the enum values. |
| Value out of its bounds | The server used the value, or the default. | The call gives an `invalid_argument` error. | Obey the `minimum` and `maximum` in the input schema. An integer cannot be larger than 4294967295. |
| Missing required argument | JSON-RPC error `-32602`. | Tool result with `isError: true` and the code `missing_argument`. | Read `error.code` of the tool result. |
| Text of a tool error | One sentence. | JSON with an `error` object. `structuredContent` holds the same JSON. | Parse the JSON, and read `error.code`. |
| Ambiguous name in `get_symbol` and `task_context` | Success result with `ambiguous: true` and `choices`. | `ambiguous` error with the choices in `error.suggestions`. | Send the call again with the `name` and the `start_line` of one suggestion. |
| Unknown name in `get_symbol` and `task_context` | Success result with `found: false`. | `not_found` error with near names in `error.suggestions`. | Read `error.code`, not `result.found`. |
| Unknown symbol in `impact` | Empty success result with `suggestions`. | `not_found` error with the names in `error.suggestions`. | Read `error.code`. |
| `files` and `include_projects` of `semantic_search` and `semantic_search_hyde` | One string with commas. | An array of strings. | Send `["a.rs", "b.rs"]`, not `"a.rs,b.rs"`. |
| Envelope of `task_context` | Only `tool` and `result`. | The same envelope as each other tool. | None, unless the client rejects unknown keys. |
| Envelope of each tool | `tool`, `total_items`, `returned`, `offset`, `result`. | Also `has_more`, `next_offset`, and `truncated`. | None, unless the client rejects unknown keys. |

These bounds are new:

- `days` of `churn`, `defect`, `hotspot`, and `temporal`: 1 to 36500.
- `stale_days` of `flags`: 0 to 36500.
- `min_tokens` of `clones`, `top_k` of the search tools, and `max_tokens` of
  `context`: 1 or more.
- `similarity` of `clones` and `min_score` of the search tools: 0 to 1.

#### Defaults that are now the same as the CLI

A tool argument that is absent takes the value that the related CLI command
takes: the value in the configuration, or the built-in default. Before, some
tools used a fixed value and did not read the configuration.

| Tool | Setting | Before 5.0 | From 5.0 | Change in a client |
|---|---|---|---|---|
| `churn` | `days` | 30 | `churn.since` of the configuration. The built-in default is `6m`, which is 180 days. | Send `days: 30` to keep the old window. |
| `clones` | `similarity` | 0.7 | `duplicates.min_similarity` of the configuration. The built-in default is 0.9. | Send `similarity: 0.7` to keep the old threshold. |
| `clones` | `min_tokens` | 50 | `duplicates.min_tokens` of the configuration. The built-in default is 50. | None, unless the configuration sets the value. |
| `clones` | Test code | Not read. | `duplicates.exclude_tests` of the configuration applies. The new `include_tests` argument reads test code. | None, unless the configuration sets `exclude_tests = false`. |
| `changes` | Days of history | 30 | `changes.days` of the configuration. The built-in default is 30. | None, unless the configuration sets the value. |
| `satd` | Custom markers | Not read. | `satd.custom_markers` of the configuration applies. | None, unless the configuration sets markers. |

### Default of `flags --stale-days`

`--stale-days` had the built-in default 90. That default had priority over
`[feature_flags] stale_days`, thus the configured value had no effect unless
the command got `--stale-days 0`.

| | Before 5.0 | From 5.0 |
|---|---|---|
| `flags` without `--stale-days` | 90 days. The configuration had no effect. | `[feature_flags] stale_days`. When that value is absent or 0, the age is 14 days. |
| `flags --stale-days 0` | The configured value, or 14 days. | 14 days. |

To keep the old result, give `--stale-days 90`, or set `stale_days = 90` in
`[feature_flags]`. The MCP tool `flags` did not change: its `stale_days`
argument has the same rule as the flag.

No other flag has a built-in default that hides a configuration key.

### Stable order and paths in the output

These changes apply to the CLI and to the MCP server.

| Output | Before 5.0 | From 5.0 |
|---|---|---|
| `items` of `deadcode` | The sequence was different in each run. | Sorted by file, line, and name. |
| `unique_authors` of `churn` | The sequence was different in each run. | Sorted by name. |
| `coupled_classes` of `cohesion` | The sequence was different in each run. | Sorted by name. |
| `groups` and `clones` of `clones`, and each `group_id` | The sequence and the identifiers were different in each run. | The sequence of the first fragment of each group. |
| `entropy` of `changes` | The last digit was different in some runs. | The same in each run. |
| `file` of `outline --file` | An absolute path. | The path from the repository root, as without `--file`. Omen makes the two paths canonical before it compares them. A file out of the repository has its canonical absolute path. |
| `search query --min-score` below 0.3 | No effect, because 0.3 always applied. | The value applies. |

### A time window that is not valid

`churn --since`, `report generate --since`, and `[churn] since` read text that
is not a time window, such as `nonsense`, as all the history. Such text is now
an error that names the flag or the key and the value. A valid window is a
duration such as `30d`, `3m`, or `1y`, or the word `all`. The configuration
check applies when omen loads the configuration, thus each command and the MCP
server reject a configuration with an incorrect `churn.since`.
