---
sidebar_position: 18
---

# Feature Flag Detection

```bash
omen flags
```

The feature flag analyzer identifies feature flag usage across a codebase, reports which flags are active, where they are referenced, and which ones are stale. Stale feature flags -- toggles that were introduced months or years ago and never cleaned up -- are a well-documented source of complexity, dead code, and operational risk.

## Why Feature Flags Become a Problem

Feature flags are a deployment tool: they decouple releases from deployments by wrapping new functionality behind toggles that can be enabled or disabled at runtime. Used well, they enable trunk-based development, gradual rollouts, and safe experimentation. The problem is cleanup.

Rahman et al. (2018) studied Google Chrome's codebase and found over 12,000 feature toggles, many of which had been active for years with no clear owner. Stale flags accumulate conditional logic that everyone is afraid to remove because nobody knows what the flag controls or whether removing it will break something.

Meinicke et al. (2020) measured the impact of feature flags on code complexity and found that flag-guarded code has measurably higher cyclomatic complexity and is harder to test. Each flag doubles the potential configuration space: 10 flags mean 1,024 possible combinations, most of which are never tested.

Omen detects flags from popular feature flag providers, identifies all reference locations, and uses Git history to determine when each flag was last modified. Flags that haven't been touched in a configurable period (default: 90 days) are reported as stale.

## Built-in Providers

Omen can recognize feature flag patterns from six providers -- `launchdarkly`, `flipper`, `split`, `unleash`, `generic`, and `env` -- covering the most common platforms across JavaScript/TypeScript, Python, and Ruby ecosystems. Detection is opt-in: `[feature_flags] providers` in `omen.toml` defaults to an empty list, so no built-in provider runs until you enable it. See [Configuration](#configuration) later in this page.

### LaunchDarkly

| Language | Pattern |
|----------|---------|
| JavaScript/TypeScript | `variation("flag-key", ...)` |
| JavaScript/TypeScript | `boolVariation("flag-key", ...)` |
| JavaScript/TypeScript | `stringVariation("flag-key", ...)` |
| JavaScript/TypeScript | `numberVariation("flag-key", ...)` |
| JavaScript/TypeScript | `jsonVariation("flag-key", ...)` |

```typescript
const showNewDashboard = ldClient.variation("new-dashboard-ui", user, false);

if (showNewDashboard) {
  return <NewDashboard />;
}
return <LegacyDashboard />;
```

### Split

| Language | Pattern |
|----------|---------|
| JavaScript/TypeScript | `getTreatment("flag-key")` |

```javascript
const treatment = splitClient.getTreatment("checkout-redesign");

if (treatment === "on") {
  showNewCheckout();
}
```

### Unleash

| Language | Pattern |
|----------|---------|
| JavaScript/TypeScript | `isEnabled("flag-key")` |
| Python | `is_enabled("flag-key")` |

```python
if unleash_client.is_enabled("dark-mode"):
    apply_dark_theme(user)
```

### Flipper

| Language | Pattern |
|----------|---------|
| Ruby | `Flipper[:flag_name]` |
| Ruby | `Flipper.enabled?(:flag_name)` |
| Ruby | `feature_enabled?(:flag_name)` |

```ruby
if Flipper[:new_pricing].enabled?(current_user)
  render_new_pricing_page
else
  render_legacy_pricing_page
end
```

### Environment-based Flags

| Language | Pattern |
|----------|---------|
| Ruby | `ENV["FEATURE_*"]` |
| JavaScript/TypeScript | `process.env.FEATURE_*` |
| Python | `os.environ["FEATURE_*"]`, `os.getenv("FEATURE_*")` |

```typescript
if (process.env.FEATURE_NEW_SEARCH === "true") {
  enableElasticsearch();
} else {
  useLegacySearch();
}
```

Environment-based flags are detected by matching environment variable access where the variable name starts with `FEATURE_`. This convention is common in applications that use environment variables for simple boolean toggles without a dedicated feature flag service.

## Output

For each detected flag, Omen reports:

| Field | Description |
|-------|-------------|
| Flag key | The flag identifier (e.g., `new-dashboard-ui`, `FEATURE_NEW_SEARCH`) |
| Provider | Which provider was detected (LaunchDarkly, Split, Unleash, Flipper, ENV) |
| References | All file paths and line numbers where the flag appears |
| First seen | Date of the earliest Git commit that introduced this flag |
| Last modified | Date of the most recent Git commit that touched code referencing this flag |
| Stale | Whether the flag exceeds the staleness threshold |
| Age (days) | Number of days since the flag was last modified |

```bash
# Table output (default)
omen flags

# JSON output
omen -f json flags

# Filter to a specific language
omen flags --language typescript

# Analyze a specific directory
omen -p ./src flags
```

## Staleness Detection

A flag is considered stale when the most recent commit touching any line that references the flag is older than the staleness threshold. The default threshold is 90 days.

This is a conservative heuristic: a flag that was introduced 6 months ago and hasn't been touched since is very likely either fully rolled out (and the flag can be removed) or abandoned (and the flag-guarded code is dead weight).

Lower thresholds are more aggressive about flagging staleness. Higher thresholds tolerate longer-lived flags, which may be appropriate for organizations with longer release cycles or flags that are intentionally long-lived (like kill switches).

## Configuration

The `[feature_flags]` section in `omen.toml` controls which built-in providers run and the staleness threshold:

```toml
# omen.toml
[feature_flags]
stale_days = 90
providers = ["launchdarkly"]
```

`providers` defaults to an empty list -- no built-in detection runs until you list the providers you want (`launchdarkly`, `flipper`, `split`, `unleash`, `generic`, `env`).

### Custom Provider Configuration

For internal feature flag systems or providers not covered by the built-in set, Omen supports custom provider definitions in `omen.toml`. Custom providers are defined using tree-sitter query patterns and are checked alongside built-in providers.

```toml
# omen.toml
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

Each custom provider requires:

- **name**: A human-readable name for the provider (appears in output).
- **languages**: Which languages this provider applies to.
- **query**: A tree-sitter query pattern. The query must include a capture named `@flag_key` that matches the string or symbol literal containing the flag key.

### Writing Tree-sitter Queries

Tree-sitter queries use S-expression syntax to match AST node patterns. The key elements:

- Node types are in parentheses: `(call_expression ...)`
- Named children use field labels: `function:`, `arguments:`
- String predicates filter by value: `(#eq? @capture "value")`
- `@name` creates a capture that extracts the matched node's text

To develop custom queries, use `tree-sitter parse <file>` to inspect the AST structure of your source code and identify the node types involved in your flag-checking pattern.

## Practical Use

### Audit Stale Flags

List all stale flags for cleanup:

```bash
omen -f json flags | jq '[.flags[] | select(.stale == true)]'
```

### Track Flag Proliferation

Count total flags and stale flags over time to monitor whether flags are being cleaned up:

```bash
omen -f json flags | jq '{total: (.flags | length), stale: ([.flags[] | select(.stale)] | length)}'
```

### CI Gate for Flag Count

Prevent flag accumulation by failing the build when stale flag count exceeds a limit:

```bash
STALE=$(omen -f json flags | jq '[.flags[] | select(.stale)] | length')
if [ "$STALE" -gt 10 ]; then
  echo "Found $STALE stale feature flags (limit: 10). Clean up old flags before adding new ones."
  exit 1
fi
```

### Find All References for a Specific Flag

```bash
omen -f json flags | jq '.flags[] | select(.key == "new-dashboard-ui")'
```

This returns all locations where the flag is referenced, useful when planning flag removal.

## References

- Meinicke, J., Wong, C.P., Kastner, C., Thum, T., & Saake, G. (2020). "Exploring the Complexity of Feature Toggles." *ACM/IEEE 42nd International Conference on Software Engineering (ICSE)*, 1-12.
- Rahman, M.T., Rigby, P.C., & Shang, W. (2018). "Release Engineering Practices and Pitfalls." *IEEE/ACM 40th International Conference on Software Engineering: Software Engineering in Practice (ICSE-SEIP)*, 285-294.
- Hodgson, P. (2017). "Feature Toggles (aka Feature Flags)." *martinfowler.com*.
