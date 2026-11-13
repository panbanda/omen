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

## Remove in 5.0

### CLI flags

| Deprecated | Replacement | Sites |
|---|---|---|
| `--check` (complexity, score, mutation) | `--gate error` | `src/cli/mod.rs` (the three `pub check` fields), `resolve_gate_mode` in `src/main.rs` maps a bare `--check` to `--gate error` |
| `--top-k` / `-k` (search query) | `--top` / `-n` | `src/cli/mod.rs` (`top_k` field, `alias = "top-k"`) |

Note: `--days` (churn) is a **kept** alias for `--since`, not scheduled for
removal, so it carries no marker. `all`-payload key `duplicates` and report
artifact names `duplicates.json` / `hotspots.json` are deferred to the output
schema change (they are part of the JSON contract); see `TODO(output-schema)`
comments in `src/main.rs`.

<!-- Future work (output-schema standardization) will add entries here for the
     duplicated path fields (`path`/`file_path` emitted alongside canonical
     `file`), `AnalysisSummary` alias, etc., using the same marker convention. -->

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
