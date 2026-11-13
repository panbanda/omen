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
