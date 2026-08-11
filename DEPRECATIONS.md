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
