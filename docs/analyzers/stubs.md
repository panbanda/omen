---
sidebar_position: 17
---

# Stubs Detection

```bash
omen stubs
```

The stubs analyzer finds incomplete or placeholder implementations: code that was never finished, as opposed to code a developer deliberately marked as debt. SATD is debt a developer chose to keep; a stub is work that was left mid-task -- the kind of thing an agent or human leaves behind before coming back to finish it (and sometimes never does).

Detection is entirely AST-based via tree-sitter, so trigger words that appear inside string literals or comments unrelated to a function body are never false positives.

## Pattern Types

Omen classifies each stub finding into one of three pattern types.

### `not_implemented` (High severity)

Explicit not-implemented idioms -- code that unambiguously declares itself unfinished.

| Language | Example |
|----------|---------|
| Rust | `todo!()`, `unimplemented!()` |
| Python | `raise NotImplementedError` |
| JavaScript/TypeScript | `throw new Error("not implemented")` |
| Go | `panic("not implemented")` |
| Java/C# | `NotImplementedException`, `UnsupportedOperationException` (with an unfinished-work message) |

```rust
fn calculate_discount(order: &Order) -> f64 {
    todo!()
}
```

### `elision` (Medium severity)

Comments that admit work was skipped, inside a function body -- the author documented that something is missing rather than writing it.

```python
def process_payment(order):
    # ... rest of the implementation
    pass
```

```javascript
function validateInput(data) {
  // your code here
}
```

Typical phrases: "rest of the implementation", "for brevity", "placeholder", "your code here".

### `empty_body` (Medium severity)

A function or method with an empty body that cannot legitimately be empty: it has a non-void return type, or it contains an elision comment. An empty body that is a deliberate no-op (matching a suppressed idiom -- see below) is not flagged.

```go
func (s *Service) FetchUser(id string) (*User, error) {
}
```

## What Omen Does Not Flag

Several idioms look like stubs but are legitimate, and Omen suppresses them:

- **`unreachable!()`** in Rust -- this asserts a code path is provably impossible, not unfinished.
- **`.pyi` stub files, `@overload`, `@abstractmethod`, `Protocol`, and `ABC`** in Python -- these are intentional interface declarations, not incomplete implementations.
- **`UnsupportedOperationException`** in Java without an "unfinished" style message -- this is a legitimate way to reject an operation a class deliberately does not support (for example, an immutable collection rejecting mutation).

Because detection is tree-sitter AST-based rather than string matching, a string literal containing the text `"todo!()"` or a comment describing what `NotImplementedError` does is never mistaken for an actual stub site.

Each stub site produces exactly one finding, regardless of how many trigger patterns it happens to match.

## Output

Each detected stub includes:

- **File path** and **line number**
- **Pattern type** (`not_implemented`, `elision`, `empty_body`)
- **Severity** (high or medium, per the pattern type)
- **Snippet** for context

```bash
# Table output (default)
omen stubs

# JSON output
omen -f json stubs

# Analyze a specific directory
omen -p ./src stubs
```

## CI Gate

`omen stubs` supports a gate independent of the repository score, similar in spirit to a linter's `--deny`:

```bash
# Report only, exit 0 regardless of findings (default)
omen stubs --gate off

# Warn but do not fail the build
omen stubs --gate warn

# Fail CI if any stub is found
omen stubs --gate error

# Only fail on high-severity stubs (todo!()-style idioms); warn on the rest
omen stubs --gate error --gate-severity high
```

`--gate error` exits with code 2 when a stub at or above `--gate-severity` (default: `low`, i.e. everything) is found. The JSON report is still written to stdout regardless of the gate outcome, so CI logs retain the full finding list even when the build fails.

## MCP

The `stubs` MCP tool is read-only: it reports findings with the same pagination (`limit`/`offset`) as every other tool, but it does not expose the CLI's `--gate` option -- gating is a CI concern, not something an LLM client should trigger mid-conversation.

## How It Feeds the Health Score

Stubs feed into the composite health score's SATD/debt component alongside self-admitted technical debt. If a stub site also happens to contain a SATD marker comment (for example a `// TODO: implement this` immediately above a `todo!()`), the two detectors deduplicate against each other so a single site is not counted twice against the score.

## Why It Matters

Unfinished work that looks complete -- it compiles, it's committed, it may even pass a shallow review -- is more dangerous than code that's obviously missing, because nothing signals it needs attention until it's called in production. This risk has grown with AI-assisted development: an agent can leave a `todo!()` or an elided function body behind mid-task, and unlike a human contributor, it will not necessarily flag that the work is incomplete in the PR description.

## Practical Use

### CI Quality Gate

Block merges that leave unfinished work behind:

```yaml
- name: Check for unfinished stubs
  run: omen stubs --gate error
```

### Triage by Severity

Start with `not_implemented` findings, which are unambiguous:

```bash
omen -f json stubs | jq '[.items[] | select(.pattern_type == "not_implemented")]'
```

### Combine With SATD

Since stubs and SATD both surface unfinished-work signals but from different sources (explicit markers vs. AST idioms), running both together gives a more complete picture of what's left to do in a codebase:

```bash
omen satd
omen stubs
```
