---
sidebar_position: 16
---

# Configuration

Omen reads configuration from `omen.toml` in the project root, or from `.omen/omen.toml`. YAML (`omen.yaml`) and JSON (`omen.json`) are also supported. Omen searches for configuration files in this order and uses the first one found:

1. Path specified with `-c` / `--config`
2. `omen.toml` in the target directory
3. `.omen/omen.toml` in the target directory

All options are optional. When a value is not specified, Omen uses sensible defaults. You do not need a configuration file to run any analyzer.

## Generating a Configuration File

```bash
omen init
```

This creates an `omen.toml` in the current directory with all sections and their default values. Edit it to match your project's standards.

## Using a Custom Config Path

```bash
omen -c ./config/omen.toml complexity
omen --config /etc/omen/global.toml score
```

The `-c` flag works with all subcommands.

## Configuration Sections

### `exclude_patterns`

An array of glob patterns for files and directories that should be excluded from analysis. These patterns are applied in addition to `.gitignore` rules.

```toml
exclude_patterns = [
    "**/node_modules/**",
    "**/vendor/**",
    "**/target/**",
    "**/.git/**",
    "**/dist/**",
    "**/build/**",
    "**/*_test.go",
    "**/*_test.rs",
    "**/*_spec.rb",
    "**/*_test.py",
    "**/test_*.py",
    "**/*.test.ts",
    "**/*.test.js",
    "**/*.spec.ts",
    "**/*.spec.js",
    "**/__tests__/**",
    "**/__pycache__/**",
    "**/bin/**",
    "**/obj/**",
    "**/*.min.js",
    "**/*.generated.*",
]
```

Patterns use standard glob syntax. `**` matches any number of directories. Patterns are matched against the file path relative to the analysis root.

### `[complexity]`

Controls thresholds for cyclomatic complexity, cognitive complexity, and nesting depth. These thresholds determine which functions appear in warnings and errors, and affect the repository score.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `cyclomatic_warn` | integer | `10` | Cyclomatic complexity at or above this triggers a warning |
| `cyclomatic_error` | integer | `20` | Cyclomatic complexity at or above this triggers an error |
| `cognitive_warn` | integer | `15` | Cognitive complexity at or above this triggers a warning |
| `cognitive_error` | integer | `30` | Cognitive complexity at or above this triggers an error |
| `max_nesting` | integer | `4` | Maximum nesting depth before flagging |

```toml
[complexity]
cyclomatic_warn = 10
cyclomatic_error = 20
cognitive_warn = 15
cognitive_error = 30
max_nesting = 4
```

Cyclomatic complexity counts linearly independent paths through a function (branches, loops, logical operators). Cognitive complexity measures how difficult a function is for a human to understand, penalizing nested control flow more heavily. See [Research References](./research.md) for the academic foundations of both metrics.

### `[satd]`

Configures Self-Admitted Technical Debt detection. SATD analysis scans comments for markers that indicate developers have knowingly left behind suboptimal code.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `categories` | array of strings | `["DEFECT", "DESIGN", "IMPLEMENTATION", "DOCUMENTATION", "TEST", "REQUIREMENT"]` | SATD categories to detect |
| `custom_markers` | array of strings | `[]` | Additional comment markers to treat as SATD |

```toml
[satd]
categories = ["DEFECT", "DESIGN", "IMPLEMENTATION", "DOCUMENTATION", "TEST", "REQUIREMENT"]
custom_markers = ["KLUDGE", "REFACTOR", "TECHDEBT", "CLEANUP"]
```

The six default categories correspond to the SATD taxonomy from Maldonado and Shihab (2015). Built-in markers include `TODO`, `FIXME`, `HACK`, `XXX`, `WORKAROUND`, and `TEMPORARY`. Custom markers extend this list with project-specific terms.

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

Configures the code clone detector, which identifies duplicated code blocks across the codebase.

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

Controls the hotspot analyzer, which identifies files that combine high complexity with high change frequency -- the intersection where bugs are most likely.

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
| `fail_under` | float | `70.0` | Exit with a non-zero code if the score is below this value |

```toml
[score]
fail_under = 70.0
```

The `fail_under` threshold is useful in CI pipelines. When the computed score is below this value, `omen score` exits with code 1, which can be used to fail a build or block a merge.

### `[score.thresholds]`

Per-category thresholds that define what score each dimension must reach to be considered healthy. Each value is a score from 0 to 100. Lower thresholds are more lenient; higher thresholds demand cleaner code.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `complexity` | float | `85` | Target score for the complexity dimension |
| `duplication` | float | `65` | Target score for the duplication dimension |
| `satd` | float | `80` | Target score for the SATD dimension |
| `tdg` | float | `90` | Target score for the technical debt gradient dimension |
| `coupling` | float | `70` | Target score for the coupling dimension |
| `smells` | float | `90` | Target score for the architectural smells dimension |
| `cohesion` | float | `80` | Target score for the cohesion (CK metrics) dimension |

```toml
[score.thresholds]
complexity = 85
duplication = 65
satd = 80
tdg = 90
coupling = 70
smells = 90
cohesion = 80
```

These thresholds influence how each analyzer's raw results are normalized into the 0-100 composite score. See [Repository Score](./repository-score.md) for details on how the composite score is calculated.

### `[feature_flags]`

Configures detection of feature flags in the codebase. Omen can identify usage of common feature flag SDKs and detect potentially stale flags.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `stale_days` | integer | `90` | Number of days after which an unmodified feature flag is considered stale |
| `providers` | array of strings | `[]` | Feature flag providers to detect (e.g., `"launchdarkly"`, `"split"`, `"unleash"`, `"flipper"`) |
| `custom_providers` | array of tables | `[]` | Custom provider definitions using tree-sitter queries |

```toml
[feature_flags]
stale_days = 90
providers = ["launchdarkly", "split", "unleash"]

[[feature_flags.custom_providers]]
name = "internal_flags"
language = "typescript"
query = '(call_expression function: (member_expression object: (identifier) @obj property: (property_identifier) @prop) (#eq? @obj "FeatureFlags") (#eq? @prop "isEnabled"))'
```

Custom providers let you define tree-sitter queries to detect project-specific feature flag patterns that the built-in detectors do not cover.

### `[semantic_search]`

Configures the semantic search engine used by `omen search`.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `max_results` | integer | `20` | Maximum number of results returned by a query |
| `min_score` | float | `0.3` | Minimum cosine similarity score for a result to be included |

```toml
[semantic_search]
max_results = 20
min_score = 0.3
```

### `[semantic_search.provider]`

Controls which embedding model is used for semantic search.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `type` | string | `"candle"` | Embedding provider: `"candle"`, `"open_ai"`, `"cohere"`, or `"voyage"` |
| `model` | string | varies | Model identifier (provider-specific) |
| `api_key_env` | string | none | Environment variable containing the API key (external providers only) |

```toml
[semantic_search.provider]
type = "candle"
# model = "all-MiniLM-L6-v2"   # default for candle

# OpenAI alternative:
# type = "open_ai"
# model = "text-embedding-3-small"
# api_key_env = "OPENAI_API_KEY"

# Cohere alternative:
# type = "cohere"
# model = "embed-english-v3.0"
# api_key_env = "COHERE_API_KEY"

# Voyage alternative (code-optimized):
# type = "voyage"
# model = "voyage-code-2"
# api_key_env = "VOYAGE_API_KEY"
```

The default `candle` provider runs locally with no API key. External providers require a valid API key in the specified environment variable. See [Semantic Search](./semantic-search.md) for details on each provider.

## Complete Example

Below is a complete `omen.toml` showing all sections with their default values.

```toml
exclude_patterns = [
    "**/node_modules/**",
    "**/vendor/**",
    "**/target/**",
    "**/.git/**",
    "**/dist/**",
    "**/build/**",
    "**/*_test.go",
    "**/*_test.rs",
    "**/*_spec.rb",
    "**/*_test.py",
    "**/test_*.py",
    "**/*.test.ts",
    "**/*.test.js",
    "**/*.spec.ts",
    "**/*.spec.js",
    "**/__tests__/**",
    "**/__pycache__/**",
    "**/bin/**",
    "**/obj/**",
    "**/*.min.js",
    "**/*.generated.*",
]

[complexity]
cyclomatic_warn = 10
cyclomatic_error = 20
cognitive_warn = 15
cognitive_error = 30
max_nesting = 4

[satd]
categories = ["DEFECT", "DESIGN", "IMPLEMENTATION", "DOCUMENTATION", "TEST", "REQUIREMENT"]
custom_markers = []

[churn]
since = "6m"
top = 20

[duplicates]
min_tokens = 50
min_similarity = 0.9

[hotspot]
top = 20

[score]
fail_under = 70.0

[score.thresholds]
complexity = 85
duplication = 65
satd = 80
tdg = 90
coupling = 70
smells = 90
cohesion = 80

[feature_flags]
stale_days = 90
providers = []

[semantic_search]
max_results = 20
min_score = 0.3

[semantic_search.provider]
type = "candle"
```

## Environment Variables

Some configuration values can be set or overridden through environment variables:

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | API key for OpenAI embedding provider |
| `COHERE_API_KEY` | API key for Cohere embedding provider |
| `VOYAGE_API_KEY` | API key for Voyage AI embedding provider |

API key environment variables are only required when using the corresponding external embedding provider for semantic search.
