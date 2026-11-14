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
`symbol`, the two flags page only the `callers` list, as `limit` and `offset`
of the MCP tool `get_symbol` do. `callees` and `call_resolutions` stay
complete. Before 5.0, the two flags cut each list.

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

### Symbol identifiers of `symbol` and `impact`

The commands `symbol`, `impact`, `repomap`, `context`, `graph`, and `smells`, and
the MCP tools that give the same results, read the shared index.

| Area | Before 5.0 | From 5.0 | Change in a client |
|---|---|---|---|
| `qualified_name` in the output | `file:name`. Two definitions with one name in one file had the same identifier. | The key of the symbol: `file#Container.name`, with `~2` for a second definition of the name in one container. | Do not parse the identifier. Give it back as it is. The four tools accept the two forms as input, and the other tools accept the new form. |
| `start_line` | The first line of the node, which could be an annotation. | The line of the declaration, after annotations, attributes, and decorators. | Use `start_line` with the name to select a definition. |
| `call_resolutions` of `symbol` and `get_symbol` | One entry for each called name. `basis` was always a string. | One entry for each call site, with its `line`. `basis` is `null` when no rule applied. `status` can be `external`. | Read `line`. Accept `null` for `basis`. |
| `resolution_summary` of `impact` | `unique_candidate`, `ambiguous`, `unresolved`. | Also `external`. The counts include each call site of the input files, also a call that is not in a definition. | None, unless the client rejects unknown keys. |
| A call for which only the name gives a definition | An edge to the one definition with that name in the repository. | No edge. The call has the status `ambiguous` and the basis `name_only`, and it lists each definition with the name that the caller can see. | Read `candidates` of `call_resolutions`. Use `--min-basis name_only`, or the MCP parameter `min_basis`, to get an edge for a call with one such candidate. |
| `--min-basis` of `symbol` and `impact`, `min_basis` of `get_symbol` and `impact` | Not available. | The weakest basis of a call that gives a caller or a callee. The default is `same_package`, which excludes `name_only`. | None. |
| `--ref` with `symbol` and `impact` on a local repository | The commands read the files on the disk. | The commands read the files of that commit. | None. |
| `truncated` of the MCP tools `context` and `repomap` | Only the cut of the token budget. | Also the cut of `max_symbols` and `max_risks`, with the total before the cut. | Read `truncated`. |
| `truncated` of the MCP tool `impact` | Always empty. | `/callers/*/symbols` and `/callees/*/symbols` have the number of definitions without the `depth` limit and the number in the result. | Read `truncated` to know that `depth` cut the result. |
| A key of the shared index as the `name` of `task_context` | Not applicable. The tool did not accept the key. | The tool selects the definition by the file and by the last name of the key. It does not use the container or the `~N` suffix. A file with two definitions of that name gives the `ambiguous` error. | Send the `start_line` of one choice with the `name`. |

### Commands that read the shared index

The commands `repomap`, `context`, `graph`, and `smells`, and the MCP tool
`task_context`, now read the shared index. The old call resolver and the old
import resolver do not exist.

| Area | Before 5.0 | From 5.0 | Change in a client |
|---|---|---|---|
| `qualified_name` of `repomap`, `context`, and `task_context` | `file:name`. | The key of the symbol: `file#Container.name`. | Give the identifier back as it is. |
| `kind` of `repomap` symbols | `Function` for each symbol. | `Method` for a method. | Accept `Method`. |
| `repomap` and `context` edges | A call whose only evidence was the name gave an edge to the one definition with that name. | No edge. The ranking uses the calls that have the basis `same_package` or a stronger basis. | None. Expect lower `in_degree` and `out_degree` for calls without an import. |
| `--ref` with `repomap`, `context`, `graph`, and `smells` | `repomap` and `context` read the files on the disk. | The commands read the files of that commit. | None. |
| Test files in `repomap` | The index did not hold them, thus a call from a test file did not exist. | The index holds them, but the map ignores the symbols and the calls of test files unless `skip_test_files` is false. A call that has a test file as the only other definition with a name can now resolve. | None. |
| Edges of `graph` and `smells` | The old import resolver matched the import path to a file by the stem of the path. | The module map of the shared index resolves the import. An import that more than one file can satisfy gives no edge. A Go import of a package gives an edge to each file of the package. | Expect a different edge count. Both commands still give the same edges and the same cycles. |
| `call_evidence` of `task_context` | `basis` was always a string. | `basis` can be `null`, and each entry has the `line` of the call. | Read `line`. Accept `null` for `basis`. |

### `outline` and `search` read the shared index

The command `outline`, the MCP tool `outline`, and the chunks of `search index`
now come from the definitions of the shared index.

| Area | Before 5.0 | From 5.0 | Change in a client |
|---|---|---|---|
| `classes` of `outline` | Classes and structs. Rust and Go listed only structs. | Also interfaces, traits, and enums. A Ruby module is a class. | Accept more entries. |
| `imports` of `outline` | The parser gave the paths. C# `using` was not an import. | The shared index gives each path one time, in source order. C# `using` is an import. | None. |
| Methods of a type in `outline` and in `search` | The parser found them in the body of the type, and in the Rust `impl` blocks. | The index also finds Go receiver methods, and C++ methods that a class declares and that a different file defines. | None. |
| `parent_name` of a search result | Rust `impl` blocks and class bodies. Go methods had no parent. | The receiver type of a Go method is the parent. | None. |
| `signature` of a search result | The first line of the declaration, with the opening brace when the line had one. | The declaration without the opening brace. | None. |
| Declarations without a body in `search` | Not indexed. | Not indexed. | None. |
| `search index` cache | The cache holds chunks that the parser made. | A file that did not change keeps its old chunks. | Delete `.omen/search.db` to build all chunks again. |

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
| `files` and `include_projects` of `search` | One string with commas. | An array of strings. | Send `["a.rs", "b.rs"]`, not `"a.rs,b.rs"`. |
| Envelope of `task_context` | Only `tool` and `result`. | The same envelope as each other tool. | None, unless the client rejects unknown keys. |
| Envelope of each tool | `tool`, `total_items`, `returned`, `offset`, `result`. | Also `has_more`, `next_offset`, `truncated`, `hints`, and `index`. | None, unless the client rejects unknown keys. |
| Tool `semantic_search` | The tool searched with TF-IDF. | The tool has the name `search`. It matches terms and does not use embeddings. | Call `search` with the same arguments. |
| Tool `semantic_search_hyde` | The tool searched with a code snippet in `hypothetical_document`. | The tool does not exist. A call gives a JSON-RPC error `-32602`. | Call `search` with the code snippet as `query`. |

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
| Default of `--min-score` and of `min_score` in the search tools | 0.3. A correct result with a lower score was lost, and the score changed when other code was added. | 0. Omen never gives a symbol that has no term of the query. |

### A time window that is not valid

`churn --since`, `report generate --since`, and `[churn] since` read text that
is not a time window, such as `nonsense`, as all the history. Such text is now
an error that names the flag or the key and the value. A valid window is a
duration such as `30d`, `3m`, or `1y`, or the word `all`. The configuration
check applies when omen loads the configuration, thus each command and the MCP
server reject a configuration with an incorrect `churn.since`.
