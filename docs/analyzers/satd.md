---
sidebar_position: 17
---

# Self-Admitted Technical Debt (SATD)

```bash
omen satd
```

The SATD analyzer detects comments where developers explicitly acknowledge shortcuts, known defects, incomplete implementations, and other forms of technical debt. These comments are a direct signal from the author that something is wrong or incomplete -- and research shows they tend to stay in codebases far longer than anyone intends.

## What Is Self-Admitted Technical Debt?

Self-admitted technical debt is any comment where a developer documents that the current implementation is intentionally suboptimal. Unlike latent technical debt (which requires analysis tools or expert judgment to identify), SATD is already known to the team. The problem is that it accumulates silently: each individual comment seems minor, but the aggregate effect can be substantial.

Potdar and Shihab (2014) studied four large open-source projects and found that SATD comments persist in codebases for years. In their dataset, the median survival time of a SATD comment was over 1,000 days. Many were never resolved -- they simply became part of the code's permanent landscape.

Maldonado and Shihab (2015) classified SATD into five categories and found that design debt (comments about shortcuts in architecture or implementation approach) is by far the most common type, accounting for 42-84% of all SATD depending on the project. It is also the most dangerous: design debt comments tend to indicate structural problems that grow worse as surrounding code evolves.

## Detection Categories

Omen classifies SATD into six categories based on marker keywords found in comments. Each category captures a different type of acknowledged debt.

### Design Debt

Shortcuts in implementation approach, architectural compromises, and acknowledged code quality issues.

| Marker | Typical Usage |
|--------|---------------|
| `HACK` | Workaround that bypasses proper design |
| `KLUDGE` | Inelegant solution known to be fragile |
| `SMELL` | Code that violates design principles |
| `REFACTOR` | Code that needs structural improvement |
| `UGLY` | Acknowledged poor quality implementation |
| `WORKAROUND` | Temporary bypass for a deeper issue |

```python
# HACK: bypassing the validation layer because the schema
# doesn't support nested arrays yet. Fix when schema v2 lands.
data = json.loads(raw_input)
```

### Defect Debt

Known bugs, broken behavior, and issues that haven't been fixed.

| Marker | Typical Usage |
|--------|---------------|
| `BUG` | Known defect in the code |
| `FIXME` | Something that is broken and needs repair |
| `BROKEN` | Feature or path that doesn't work correctly |
| `DEFECT` | Identified defect not yet resolved |

```rust
// FIXME: this panics when the input contains non-UTF-8 bytes.
// We need to handle the error case instead of unwrapping.
let name = String::from_utf8(bytes).unwrap();
```

### Requirement Debt

Missing features, incomplete implementations, and deferred work.

| Marker | Typical Usage |
|--------|---------------|
| `TODO` | Work that needs to be done |
| `FEAT` | Feature that is planned but not implemented |
| `MISSING` | Functionality that should exist but doesn't |
| `NEEDED` | Required capability not yet built |

```typescript
// TODO: add pagination support -- currently returns all results
// which will be a problem once we have more than ~1000 users
async function listUsers(): Promise<User[]> {
  return db.query("SELECT * FROM users");
}
```

### Test Debt

Tests that are skipped, disabled, or known to be failing.

| Marker | Typical Usage |
|--------|---------------|
| `FAILING` | Test that is known to fail |
| `SKIP` | Test intentionally skipped |
| `DISABLED` | Test turned off |
| `FLAKY` | Test with intermittent failures |

```python
# SKIP: this test depends on an external API that's been
# unreliable. Disabled until we add proper mocking.
@pytest.mark.skip(reason="external API flaky")
def test_payment_processing():
    ...
```

### Performance Debt

Known performance issues and optimization opportunities.

| Marker | Typical Usage |
|--------|---------------|
| `SLOW` | Code known to have performance issues |
| `OPTIMIZE` | Code that should be optimized |
| `PERF` | Performance concern noted |
| `N+1` | Known N+1 query or similar inefficiency |

```ruby
# SLOW: this does a full table scan on every request.
# Should add an index on (user_id, created_at).
def recent_orders(user)
  Order.where(user_id: user.id).order(created_at: :desc)
end
```

### Security Debt

Unresolved security concerns, known vulnerabilities, and unsafe patterns.

| Marker | Typical Usage |
|--------|---------------|
| `SECURITY` | Security concern acknowledged |
| `VULN` | Known vulnerability |
| `UNSAFE` | Code using unsafe patterns intentionally |
| `INSECURE` | Known insecure implementation |
| `XXE` | XML External Entity risk |
| `SQLI` | SQL injection risk |
| `XSS` | Cross-site scripting risk |

```go
// SECURITY: user input is interpolated directly into the query.
// This is vulnerable to SQL injection. Must switch to parameterized queries.
query := fmt.Sprintf("SELECT * FROM users WHERE name = '%s'", userName)
```

## Severity Weighting

Not all SATD categories carry equal risk. Omen assigns severity multipliers that affect how each item contributes to the repository score:

| Category | Severity Multiplier | Rationale |
|----------|---------------------|-----------|
| Security | 4x | Unresolved security issues represent the highest risk |
| Defect | 2x | Known bugs that remain unfixed actively harm users |
| Design | 1x | Structural shortcuts compound over time |
| Performance | 1x | Performance issues degrade user experience |
| Test | 0.5x | Skipped tests reduce coverage but are lower immediate risk |
| Requirement | 0.25x | Missing features are tracked debt, not active harm |

A codebase with 5 security SATD items scores worse than one with 80 TODO comments. This reflects the actual risk profile: an unresolved SQL injection vulnerability is categorically more dangerous than a missing feature reminder.

## Output

Each detected SATD item includes:

- **File path** and **line number** for the comment
- **Category** (design, defect, requirement, test, performance, security)
- **Marker** that triggered the detection (e.g., HACK, TODO, FIXME)
- **Severity** based on the category multiplier
- **Comment text** for context

```bash
# Table output (default)
omen satd

# JSON output
omen -f json satd

# Filter to a specific language
omen satd --language python

# Analyze a specific directory
omen -p ./src satd
```

The output is sorted by severity (highest first), then by file path. This puts security and defect items at the top where they get attention first.

## Configuration

Default markers can be extended or replaced in `omen.toml`:

```toml
[satd]
# Add custom markers to existing categories
[satd.markers]
design = ["HACK", "KLUDGE", "SMELL", "REFACTOR", "UGLY", "WORKAROUND", "CLEANUP"]
defect = ["BUG", "FIXME", "BROKEN", "DEFECT", "REGRESSION"]
requirement = ["TODO", "FEAT", "MISSING", "NEEDED", "PLACEHOLDER"]
test = ["FAILING", "SKIP", "DISABLED", "FLAKY", "PENDING"]
performance = ["SLOW", "OPTIMIZE", "PERF", "N+1", "BOTTLENECK"]
security = ["SECURITY", "VULN", "UNSAFE", "INSECURE", "XXE", "SQLI", "XSS"]

# Exclude specific directories from SATD scanning
exclude = ["vendor", "third_party", "generated"]
```

When custom markers are specified, they replace the defaults for that category entirely. If you want to add markers without removing the defaults, include the default markers in your list.

## How Omen Detects SATD

Omen uses tree-sitter to identify comment nodes in the AST. This is important because it avoids false positives from strings, variable names, or other non-comment text that might contain marker keywords.

For each comment node, Omen:

1. Extracts the comment text, stripping comment delimiters (`//`, `#`, `/* */`, etc.).
2. Searches for marker keywords using case-insensitive matching.
3. Classifies the comment into the highest-severity matching category (if a comment contains both `TODO` and `SECURITY`, it is classified as security debt).
4. Records the file, line, category, marker, severity, and full comment text.

Because detection is AST-based, it works correctly across all 13 supported languages regardless of comment syntax differences.

## Practical Use

### Triage by Severity

Use SATD output to prioritize debt reduction. Start with security and defect items, which represent active risk:

```bash
omen -f json satd | jq '.items[] | select(.category == "security" or .category == "defect")'
```

### Track Debt Over Time

Combine with Git history to see whether SATD is growing or shrinking:

```bash
omen -f json satd | jq '.summary'
```

The summary includes total counts per category, making it easy to track in a dashboard or CI artifact.

### CI Quality Gate

Fail the build if security-related SATD exceeds a threshold:

```bash
SECURITY_DEBT=$(omen -f json satd | jq '[.items[] | select(.category == "security")] | length')
if [ "$SECURITY_DEBT" -gt 0 ]; then
  echo "Found $SECURITY_DEBT unresolved security debt items"
  exit 1
fi
```

## References

- Potdar, A., & Shihab, E. (2014). "An Exploratory Study on Self-Admitted Technical Debt." *IEEE 30th International Conference on Software Maintenance and Evolution (ICSME)*, 91-100.
- Maldonado, E.S., & Shihab, E. (2015). "Detecting and Quantifying Different Types of Self-Admitted Technical Debt." *IEEE 7th International Workshop on Managing Technical Debt (MTD)*, 9-15.
- Bavota, G., & Russo, B. (2016). "A Large-Scale Empirical Study on Self-Admitted Technical Debt." *IEEE/ACM 13th Working Conference on Mining Software Repositories (MSR)*, 315-326.
