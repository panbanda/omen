---
sidebar_position: 16
---

# Configuration

Omen automatically discovers configuration by searching for `omen.toml` and then `.omen/omen.toml` under the analyzed repository. An explicit `-c`/`--config <PATH>` accepts TOML (the default for unrecognized extensions), YAML (`.yaml`/`.yml`), or JSON (`.json`) -- automatic discovery only looks for TOML files.

`OMEN_`-prefixed environment variables override file values. Use a double underscore for nested keys, for example `OMEN_COMPLEXITY__CYCLOMATIC_ERROR=25`.

All options are optional. When a value is not specified, Omen uses sensible defaults. You do not need a configuration file to run any analyzer.

Configuration structs use `#[serde(deny_unknown_fields)]`, so unknown keys -- including unknown nested keys -- are rejected with an error instead of being silently ignored. This means a typo in a config file fails loudly rather than being ignored.

## Creating a Configuration File

There is no `omen init` command. Copy the annotated example and customize it:

```bash
curl -O https://raw.githubusercontent.com/panbanda/omen/main/omen.example.toml
mv omen.example.toml omen.toml
```

If you use Claude Code, the `setup-config` skill can analyze your repository and generate an `omen.toml` with intelligent defaults for your tech stack, including detected feature flag providers and language-specific exclude patterns.

## Using a Custom Config Path

```bash
omen -c ./config/omen.toml complexity
omen --config /etc/omen/global.toml score
```

The `-c` flag works with all subcommands and, unlike automatic discovery, accepts TOML, YAML, or JSON.

## Accepted Top-Level Keys

The accepted top-level keys are `exclude`, `exclude_built_assets`, `complexity`, `satd`, `churn`, `duplicates`, `hotspot`, `score`, `feature_flags`, `temporal`, and `changes`. Any other top-level key is rejected.

### `exclude`

An array of glob patterns for files and directories that should be excluded from analysis, in addition to `.gitignore` rules.

```toml
exclude = [
    # Test files
    "*_test.rs",
    "*_test.go",
    "**/*_test.py",
    "**/*.test.ts",
    "**/*.test.js",
    "**/*.spec.ts",
    "**/*.spec.js",
    "tests/**",
    "test/**",
    "**/testdata/**",

    # Generated code
    "**/mocks/**",
    "**/*.pb.go",
    "**/*.gen.go",
    "**/*.generated.*",

    # Dependencies
    "vendor/**",
    "node_modules/**",
    "**/site-packages/**",

    # Build/output directories
    "target/**",
    "dist/**",
    "build/**",
    "bin/**",
    ".git/**",
    ".omen/**",

    # Lock files
    "Cargo.lock",
    "go.sum",
    "package-lock.json",
    "yarn.lock",
    "poetry.lock",
]
```

Patterns use standard glob syntax. `**` matches any number of directories. Patterns are matched against the file path relative to the analysis root. The same patterns can be passed ad hoc on the command line with a repeatable `-e`/`--exclude` flag, which most analyzers accept:

```bash
omen complexity -e "**/vendor/**" -e "**/*.pb.go"
```

### `[complexity]`

Controls thresholds for cyclomatic and cognitive complexity. These thresholds determine which functions are flagged as errors and affect the repository score.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `cyclomatic_error` | integer | `20` | Cyclomatic complexity at or above this triggers an error |
| `cognitive_error` | integer | `30` | Cognitive complexity at or above this triggers an error |

```toml
[complexity]
cyclomatic_error = 20
cognitive_error = 30
```

Cyclomatic complexity counts linearly independent paths through a function (branches, loops, logical operators). Cognitive complexity measures how difficult a function is for a human to understand, penalizing nested control flow more heavily. See [Research References](./research.md) for the academic foundations of both metrics.

### `[satd]`

Configures Self-Admitted Technical Debt detection.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `custom_markers` | array of strings | `[]` | Additional comment markers to treat as SATD, beyond the built-in set (TODO, FIXME, HACK, BUG, and others) |

```toml
[satd]
custom_markers = []
```

### `[churn]`

Controls the time window and output limits for churn analysis, which measures how frequently files are modified.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `since` | string | `"6m"` | How far back in Git history to look |
| `top` | integer | `20` | Number of highest-churn files to include in output |

Valid values for `since`:

| Value | Meaning |
|-------|---------|
| `1m` | 1 month |
| `3m` | 3 months |
| `6m` | 6 months |
| `1y` | 1 year |
| `2y` | 2 years |
| `all` | Entire repository history |

```toml
[churn]
since = "6m"
top = 20
```

Shorter windows focus on recent activity and are faster to compute. Longer windows provide a more complete picture but include historical noise from files that may have been stable for a long time.

### `[duplicates]`

Configures the code clone detector (MinHash + LSH), which identifies duplicated code blocks across the codebase.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `min_tokens` | integer | `50` | Minimum token count for a code block to be considered a clone candidate |
| `min_similarity` | float | `0.9` | Similarity threshold (0.0 to 1.0) for two blocks to be reported as clones |

```toml
[duplicates]
min_tokens = 50
min_similarity = 0.9
```

Lower `min_tokens` values will detect smaller duplicated fragments but increase noise. Lower `min_similarity` values will catch Type 3 clones (similar but not identical blocks) at the cost of more false positives. A similarity of `1.0` restricts detection to exact (Type 1) clones only.

### `[hotspot]`

Controls the hotspot analyzer, which identifies files that combine high complexity with high change frequency.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `top` | integer | `20` | Number of hotspot files to include in output |

```toml
[hotspot]
top = 20
```

### `[score]`

Controls the composite repository score behavior.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `fail_under` | float | none (no gate) | Exit with a non-zero code if the score is below this value |

```toml
[score]
fail_under = 70.0
```

The `fail_under` threshold is useful in CI pipelines and pre-push hooks. When set and the computed score is below this value, `omen score` exits with a non-zero code, which can be used to fail a build or block a merge. Achieving a score of 100 is nearly impossible for real-world codebases -- run `omen score` to see your current score, then set the threshold slightly below it and raise it over time. See [Repository Score](./repository-score.md) for how the composite score is calculated.

### `[feature_flags]`

Configures detection of feature flags in the codebase.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `stale_days` | integer | `90` | Days before a flag is considered stale |
| `providers` | array of strings | `[]` | Built-in providers to enable: `launchdarkly`, `flipper`, `split`, `unleash`, `generic`, `env`. If empty, no built-in detection runs. |
| `custom_providers` | array of tables | `[]` | Custom provider definitions using tree-sitter queries |

```toml
[feature_flags]
stale_days = 90
providers = ["launchdarkly"]

[[feature_flags.custom_providers]]
name = "my_feature_system"
languages = ["ruby", "python"]
query = '''
(call
  receiver: (constant) @receiver
  (#eq? @receiver "Feature")
  method: (identifier) @method
  (#match? @method "^enabled\\?$")
  arguments: (argument_list
    .
    (simple_symbol) @flag_key))
'''
```

Custom providers let you define tree-sitter queries to detect project-specific feature flag patterns that the built-in detectors do not cover.

### `[temporal]` and `[changes]`

`temporal` (temporal coupling) and `changes` (JIT commit-level risk) are also accepted top-level configuration sections. Most projects can rely on the defaults for these analyzers; consult `omen <command> --help` for the current set of CLI flags each one accepts.

## Complete Example

`omen.example.toml` in the repository root is kept up to date with every accepted section and is the canonical starting point -- copy it rather than hand-writing a config from scratch. Its contents are `exclude`, `[complexity]`, `[satd]`, `[churn]`, `[duplicates]`, `[hotspot]`, `[score]`, and `[feature_flags]`, matching the sections documented above.
