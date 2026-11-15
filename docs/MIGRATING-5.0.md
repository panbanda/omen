# Migrating from omen 4.x to 5.0

Omen 5.0 has breaking changes. This guide lists each change and the action that you must do. `DEPRECATIONS.md` has the full tables, and `docs/MCP_CONTEXT.md` has the MCP contract.

Omen 5.0 has no alias and no shim for a removed item. A removed flag gives a clap error. A removed configuration key gives an error that names the new key.

## Quick checklist

1. Change `--check` to `--gate error`.
2. Change `search query -k` and `--top-k` to `-n` or `--top`.
3. Change the configuration file to TOML. Change each removed key (see "Configuration").
4. Change each MCP call to `semantic_search` to `search`. Replace each call to `semantic_search_hyde`.
5. Read `error.code` of an MCP tool result. Do not read `result.found` or `result.ambiguous`.
6. Change each key `path` and `file_path` in the JSON output to `file`.
7. Do not parse a symbol identifier. Give it back as it is.
8. Remove `--ref` from a history command.

## Command-line flags

Each flag in this table does not exist in 5.0.

| Removed | Replacement |
|---|---|
| `--check` (`complexity`, `score`, `mutation`) | `--gate error` |
| `--top-k` and `-k` (`search query`) | `--top` and `-n` |
| `--output` (`report generate`) | `--data` and `-d` |
| `--output-survivors` (`mutation`) | `--output` and `-o`. The file has the full JSON report. |
| `--jobs` of `mutation` | The global `--jobs` and `-j` |
| `--path` and `-p` of `mutation train` | The global `--path` and `-p` |
| `-p` as the short form of `--period` (`score trend`) | `--period` |
| `-p` as the short form of `--port` (`report serve`) | `--port` |

The flags `-p`, `-f`, `-c`, `-v`, `-j`, and `--compact` are global. On each command, `-p` is the repository path. If the `-p` value is not a directory, the command stops with an error. The error names `--period` or `--port` when you use the old short form.

Before:

```sh
omen complexity --check
omen search query "parse config" -k 5
omen report generate --output ./data
```

After:

```sh
omen complexity --gate error
omen search query "parse config" -n 5
omen report generate --data ./data
```

The flag `--days` is removed from `churn`, `hotspot`, `defect`, and `report generate`. Use `--since` with a duration. `--days 30` becomes `--since 30d`. For `report generate`, one window applies to the metadata, the churn, and the trend.

### Gates

The commands `complexity`, `score`, `stubs`, and `mutation` take `--gate off|warn|error`. The command `security` takes it also. If you do not give the flag, the `gate` key in the table of the command gives the mode.

A gated command writes the normal report to stdout. The gate writes the violations to stderr. Before 5.0, a gate on `complexity` or `score` replaced the report with a `violations` document. Read the violations from stderr.

The gate of `score` applies only to the bare `score` command. `score trend` refuses `--gate`.

### Paging flags

- `outline`: `--top` and `--offset` page the `files` list.
- `symbol`: the flags page only the `callers` list. `callees` and `call_resolutions` stay complete.
- `graph`: the flags page the `nodes` list. An edge stays only if its two nodes stay. `nodes_omitted` and `edges_omitted` give the counts.
- `all`: the flags apply to each analyzer section.
- SARIF output: the flags do not apply.

### Output of `all`

`all` obeys `-f`. Before 5.0, it always wrote JSON. The default format is Markdown, so `omen all` now writes Markdown. Use `omen -f json all` to get JSON.

The analyzer name `duplicates` is now `clones` in the `analyzers` array. The array also has an entry for `security`.

## Output keys

The path of a file is the key `file` in each output.

| Command | Removed | Replacement |
|---|---|---|
| `complexity` | `files[].path` | `files[].file` |
| `churn` | `files[].path`, `files[].relative_path` | `files[].file` |
| `cohesion` | `classes[].path` | `classes[].file` |
| `ownership` | `files[].path` | `files[].file` |
| `graph` | `nodes[].path` | `nodes[].file` |
| `defect` | `files[].file_path` | `files[].file` |
| `tdg` | `files[].file_path` | `files[].file` |
| `search query` | `results[].file_path` | `results[].file` |
| `mutation` | `files[].path` | `files[].file` |
| `score trend` | `component_trends.<name>.Slope` | `component_trends.<name>.slope` |
| `score trend` | `component_trends.<name>.Correlation` | `component_trends.<name>.correlation` |

The `file` value of `churn` is relative to the repository root and has no `./` prefix. The MCP tools give the same keys as the commands.

These lists now have a stable order: `items` of `deadcode`, `unique_authors` of `churn`, `coupled_classes` of `cohesion`, and `groups` of `clones`.

### Report artifacts

The command `report generate` writes one JSON file for each analyzer. Two file names and two `--skip` values changed.

| Removed | Replacement |
|---|---|
| `duplicates.json` | `clones.json` |
| `hotspots.json` | `hotspot.json` |
| `--skip duplicates` | `--skip clones` |
| `--skip hotspots` | `--skip hotspot` |

`report generate` also writes `security.json`. It writes each file to a temporary file and then renames it. This includes `metadata.json` and `score.json`. If a write fails, the command fails and names each file that it could not write. The report renderer reads the new names.

## Symbol identifiers

The commands `symbol`, `impact`, `repomap`, and `context`, and the MCP tools with the same results, use the key of the symbol from the shared index.

| Before 5.0 | From 5.0 |
|---|---|
| `src/git/log.rs:is_since_all` | `src/git/log.rs#is_since_all` |
| `src/cli/mod.rs:from_str` | `src/cli/mod.rs#ChurnWindow.from_str` |

The format is `path#Container.name`. A second definition of the same name in the same container has the suffix `~2`, then `~3`, and so on.

- Do not parse the identifier. Give it back as it is.
- The commands `symbol` and `impact`, and the MCP tools `get_symbol` and `task_context`, also accept the old `file:name` form as input. `task_context` is an MCP tool only. It has no CLI command.
- `start_line` is now the line of the declaration, after annotations and decorators.
- A call that has only a name as evidence is a candidate, not an edge. Use `--min-basis name_only` (MCP: `min_basis`) to get an edge for a call with one such candidate.

## Configuration

### File format

Omen reads TOML only. Automatic discovery loads `omen.toml` and `.omen/omen.toml`. An explicit `--config` path must be a TOML file. A path that ends in `.yaml`, `.yml`, or `.json` is an error. Convert such a file to TOML.

Environment variables with the prefix `OMEN_` still override file values. A variable that sets a removed key gives the same error as a file.

Config types reject unknown keys. A typing error in a key is an error.

### Removed and renamed keys

Each removed key gives an error. For example: `gate.complexity was removed in 5.0; use complexity.gate`.

| Removed | Replacement |
|---|---|
| `[gate]` table | The `gate` key in the table of each command |
| `gate.complexity`, `gate.score`, `gate.stubs`, `gate.mutation` | `complexity.gate`, `score.gate`, `stubs.gate`, `mutation.gate` |
| `complexity.cyclomatic_error` | `complexity.cyclomatic_max` |
| `complexity.cognitive_error` | `complexity.cognitive_max` |
| `score.fail_under` | `score.min` |
| `satd.custom_markers` | `satd.markers` |
| `exclude_built_assets` | `include_generated` (opposite meaning) |
| `duplicates.exclude_tests`, `[temporal]` table | `exclude_tests` at the top level |
| `hotspot.days` | `hotspot.since` |
| `defect.churn_days` | `defect.since` |
| `changes.days` | `changes.since` |
| `churn.top`, `hotspot.top` | The `--top` flag |

### Gate

Before:

```toml
[gate]
complexity = "error"
score = "warn"
```

After:

```toml
[complexity]
gate = "error"

[score]
gate = "warn"
```

The modes are `off`, `warn`, and `error`. The default is `off`. The `--gate` flag overrides the key.

### Complexity limits

Before:

```toml
[complexity]
cyclomatic_error = 20
cognitive_error = 30
```

After:

```toml
[complexity]
cyclomatic_max = 20
cognitive_max = 30
```

### Score threshold

Before:

```toml
[score]
fail_under = 80
```

After:

```toml
[score]
min = 80.0
```

The default of `score.min` is 80. The `--fail-under` flag still overrides it.

### History windows

A window is a `since` duration, such as `30d`, `6m`, or `1y`. It has the same syntax as the `--since` flag. The value `all` is valid for `churn`, `hotspot`, and `defect`. It is not valid for `changes`. A window of 0 is an error. A text that is not a window, such as `nonsense`, is an error. Before 5.0, such text meant all the history.

Before:

```toml
[hotspot]
days = 90

[defect]
churn_days = 30

[changes]
days = 30
```

After:

```toml
[hotspot]
since = "90d"

[defect]
since = "30d"

[changes]
since = "30d"
```

The built-in defaults are `6m` for `churn`, `90d` for `hotspot`, and `30d` for `defect` and `changes`.

### Row limits

The keys `churn.top` and `hotspot.top` do not exist. Use the `--top` flag. The default is 20 rows, as a constant in the command.

Before:

```toml
[churn]
top = 20
```

After:

```sh
omen churn --top 20
```

### Custom markers

Before:

```toml
[satd]
custom_markers = ["REVIEW"]
```

After:

```toml
[satd]
markers = ["REVIEW"]
```

### Test files

One top-level key controls test files for `clones` and `temporal`. The default is `true`. `clones --include-tests` overrides it for one run. Before 5.0, `clones` and `temporal` had separate keys.

Before:

```toml
[duplicates]
exclude_tests = false

[temporal]
exclude_tests = false
```

After:

```toml
exclude_tests = false
```

### Generated files

`include_generated` replaces `exclude_built_assets`. The meaning is the opposite. The default `false` gives the old default behavior: omen skips minified and bundled files, and files in directories such as `node_modules`, `dist`, and `vendor`.

Before:

```toml
exclude_built_assets = false
```

After:

```toml
include_generated = true
```

### Feature flags

`stale_days` has the default 90. The CLI flag `--stale-days` had the default 90 before, and it hid the configured value. Now the flag has no default, and the configured value applies.

Before, a configuration value of 0 or no value made the analyzer use 14 days. The value 0 is now an error in the configuration and in `--stale-days`. The MCP tool `flags` has the same rule.

Before:

```toml
[feature_flags]
stale_days = 0
```

After:

```toml
[feature_flags]
stale_days = 90
```

### Template

`omen.example.toml` is the one template. Each value in it is the default value. A test checks that it equals the defaults.

## `--ref`

Each command that reads source now reads the files of the commit that `--ref` gives. This includes `repomap`, `context`, `graph`, and `smells`, which read the files on the disk before 5.0.

The walk of a commit skips a path with a component that starts with a dot, as the walk of the working tree does. Thus `.github/` and `.config/` are not analyzed under `--ref`.

A command that reads the history from `HEAD`, or that cannot read a commit, gives an error with `--ref`. Before 5.0, it read the working tree and did not tell you. These commands give the error: `churn`, `hotspot`, `defect`, `temporal`, `ownership`, `changes`, `diff`, `tdg`, `score`, `all`, `search`, `mutation`, and `mcp`.

Remove `--ref` from these commands, or check out the commit.

## MCP server

### Renamed and removed tools

| Before 5.0 | From 5.0 |
|---|---|
| `semantic_search` | `search`. It matches terms and does not use embeddings. The arguments are the same. |
| `semantic_search_hyde` | Removed. A call gives the JSON-RPC error `-32602`. Call `search` with the code snippet as `query`. |

Also, `files` and `include_projects` of `search` are arrays of strings. Send `["a.rs", "b.rs"]`, not `"a.rs,b.rs"`.

### Validation

The server validates each call against the schema of the tool. It does not replace a wrong value with a default.

| Case | Before 5.0 | From 5.0 |
|---|---|---|
| Unknown argument | Ignored | Error `unknown_argument` |
| Wrong type, or `null` | Default | Error `invalid_argument` |
| Value out of the enum or the bounds | Default or the value | Error `invalid_argument` |
| Missing required argument | JSON-RPC error `-32602` | Error `missing_argument` |

One call reports all its problems.

### Errors

A JSON-RPC error is only for a protocol fault: a parse error (`-32700`), an unknown method (`-32601`), an unknown tool, or a `tools/call` request without `params` or without a tool name (`-32602`). Each other failure is a tool result with `isError: true`. Its text and `structuredContent` have this JSON:

```json
{
  "error": {
    "code": "invalid_argument",
    "message": "...",
    "param": "direction",
    "expected": "one of: callers, callees, both",
    "received": "caller",
    "suggestions": ["callers", "callees"],
    "next": "..."
  }
}
```

The eight codes are `missing_argument`, `invalid_argument`, `unknown_argument`, `not_found`, `ambiguous`, `path_outside_root`, `git_unavailable`, and `internal`.

| Case | Before 5.0 | From 5.0 |
|---|---|---|
| Ambiguous name in `get_symbol` and `task_context` | Success with `ambiguous: true` | Error `ambiguous`. Send `name` and the `start_line` of one suggestion. |
| Unknown name in `get_symbol`, `task_context`, and `impact` | Success with `found: false`, or an empty result | Error `not_found`, with near names in `suggestions` |

### Envelope

Each tool, `task_context` included, gives `tool`, `total_items`, `returned`, `offset`, `has_more`, `next_offset`, `truncated`, `hints`, `index`, and `result`. Before 5.0, there was no `has_more`, `next_offset`, `truncated`, `hints`, or `index`. Each tool has `limit` (default 50) and `offset`. `task_context` has a byte budget instead.

One exception exists. If you call `context` with `format: "markdown"`, the result is text and has no envelope.

### Defaults

An argument that is absent takes the value of the related command: the value in the configuration, or the built-in default. Some defaults changed.

| Tool | Argument | Before 5.0 | From 5.0 |
|---|---|---|---|
| `churn` | `days` | 30 | `churn.since`. The default is `6m`. The argument `days` is removed. |
| `clones` | `similarity` | 0.7 | `duplicates.min_similarity`. The default is 0.9. |

Set the old value in the configuration to keep the old behavior. The arguments `days` of the tools `churn`, `defect`, `hotspot`, and `temporal` are removed. A call that sends `days` gets the error `unknown_argument`. Use the argument `since` with a duration. `days: 30` becomes `since: "30d"`. The value `all` reads the whole history. A value that is not a window gives the error `invalid_argument`. Without `since`, the tools `churn`, `defect`, and `hotspot` read `churn.since`, `defect.since`, and `hotspot.since`. The tool `temporal` uses `30d`. The command `temporal` has the new flag `--since`, with the same default. The MCP tools `clones`, `changes`, and `satd` now read their configuration keys.

## Other changes

### `deadcode`

`deadcode` reads the shared index. It reports fewer false positives, and more dead items in some languages. The file `DEPRECATIONS.md` lists each rule.

### `search`

The default of `--min-score` of `search query` is now 0. It was 0.3, and a value below 0.3 had no effect.

### `outline`

`outline --file` gives the path from the repository root. It gave an absolute path before 5.0.

## New in 5.0

### `omen security`

The command `omen security` reports sinks, entry points, and the paths between them. It reports facts. It does not report a vulnerability, and an empty result does not mean that the code is safe. The sections are `sinks`, `surface`, and `paths`. The formats include `-f sarif`. The format `-f mermaid` works only for `omen security paths`. The other sections give an error.

```sh
omen security
omen security paths --entry KEY
omen security --gate error
```

Configure it in the `[security]` table of `omen.toml`. `--gate` compares the branch with its base, and it counts the new sinks. The default gate is `off`. The category `hardcoded_credential` is experimental. Use `--experimental` to include it. See `docs/SECURITY.md`.

The MCP tool `security` has the same sections. The tools `symbol`, `impact`, and `context` have a `security` field. `all` runs the security analysis. `omen diff` adds a Security section to the comment.

### Entry points in `deadcode`

`deadcode` treats each entry point as alive, with its callees. This includes the entry points that you add with `[[security.entry_points]]`. A function that a Rust serde attribute names (`default`, `skip_serializing_if`, `serialize_with`, `deserialize_with`, `with`) is alive.

### `unresolved_paths`

The summary of `deadcode` has the new count `unresolved_paths`. It counts the serde paths that omen cannot resolve. For each such path, omen keeps each function with the last name of the path alive. The result can miss a dead function. It does not report a live function.

### A file that changes during analysis

Omen reads a file again after it indexes the file. It records a hash of each file. If a later read fails or gives a different hash, the command stops with this error:

```text
<file> changed or became unreadable during analysis; run the command again
```

Run the command again. Do not edit the repository while omen runs.

### Cohesion for TSX and JSX

`cohesion` now measures the classes of `.tsx` and `.jsx` files. Before 5.0, it skipped them.

### Cohesion on the shared index

`cohesion` reads the symbols, the calls, and the member facts of the shared index. It does not parse source. `--ref` selects the content of the metrics through the content source. Some values change, and each change is a correction:

- WMC: WMC is the sum of the cyclomatic complexity of the methods, as `omen complexity` gives it. Omen counted each `if`, `for`, and `while` keyword two times. It now counts each keyword one time. It also counts `do` loops and the logical operators `&&`, `||`, `and`, and `or`. WMC uses the cyclomatic complexity rules of omen, so it counts each construct that `omen complexity` counts, for example a Python `with` statement.
- LCOM: the value is LCOM4. Two methods are connected when they share a field or when one calls the other on the object itself. A method name is not a field. Java, C#, C++, PHP, and Ruby had no field uses before. They have them now. A class without a declared field can connect methods through a field that it uses, as a Rust `self.0` does.
- RFC: RFC is the size of the union of the methods of the class and the called methods. A call of a method of the same class does not count two times. Omen counts each called method one time, and it counts a call in all languages. A constructor call is a called method. Before, Java, Python, PHP, and Ruby calls were not counted.
- CBO: a class does not couple to itself. Python counts the names in type annotations, the base classes, and the calls of classes of the repository. Before, it counted each identifier. C# and JavaScript now count type names and `new` expressions.
- A nested class has its own metrics. Its methods, fields, calls, and types are not in the outer class.
- A method is a function that has its own name. A field and a function value that a variable holds are not methods. In C++, a declaration and its definition are one method. A Go or C++ method in a different file of the same directory belongs to its type.
- The classes of Rust modules and functions, abstract TypeScript classes, and Java and C# records are now classes. C# structs, Java enums, Rust enums and traits, and PHP traits are also classes. A C# record parameter is a field of the record. A Go type with a lower-case name now couples.
- A base type has its simple name. A qualifier and generic arguments are not in the name, as in `pkg.Base` and `Base<T>`. PHP enums are classes. A Go type with a name of one character couples.
- `parent_class` is the first base type. A Ruby parent no longer has the text `< `. JavaScript and C# parents are now found. A C# interface of the repository is not a parent. DIT is the longest path over all base types. NOC counts the direct children of each base type.
- `start_line` is the line of the declaration. The lines of annotations and attributes before it are not in the range.

C and Bash have no class, and `cohesion` reports nothing for them.

### GitHub Action input `binary`

The action has the new input `binary`. It is the path of an omen binary, relative to the workflow workspace. The file must be executable and its name must be `omen`. If you set it, the action does not install a release, and it ignores `version`.

```yaml
- uses: panbanda/omen@<release tag>
  with:
    binary: target/release/omen
```
