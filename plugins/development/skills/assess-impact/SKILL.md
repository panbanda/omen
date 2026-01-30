---
name: assess-impact
description: Analyze blast radius before changing specific files. Use before refactoring core components, making breaking API changes, or modifying shared code to identify affected files and stakeholders.
---

# Assess Impact

Understand the potential blast radius of changes before making them by analyzing dependencies, coupling patterns, and ownership.

## Prerequisites

Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Map Dependencies

Run the dependency graph analysis:

```bash
omen -f json graph
```

Identify:
- Direct dependents (files that import the target)
- Transitive dependents (files that import the dependents)

### Step 2: Check Temporal Coupling

Run the temporal coupling analysis:

```bash
omen -f json temporal
```

Files with high temporal coupling to the target often need to change together even without explicit imports.

### Step 3: Identify Stakeholders

Run the ownership analysis:

```bash
omen -f json ownership
```

Identify:
- Primary owner of the target file
- Owners of dependent files
- Subject matter experts to consult

## Impact Categories

Classify impact by type:

| Category | Description | Action |
|----------|-------------|--------|
| Direct | Explicit imports/calls | Will break if signature changes |
| Implicit | Temporal coupling | May break due to shared assumptions |
| Behavioral | Same owner/team | Likely understands the change |
| Unknown | No coupling, different owner | Needs extra review |

## Output Format

Present impact analysis as:

```markdown
# Change Impact: `target/file.go:FunctionName`

## Direct Dependencies (will break)
- `consumer/a.go` - calls FunctionName directly
- `consumer/b.go` - uses returned type
- `test/target_test.go` - tests the function

## Implicit Dependencies (may break)
- `related/cache.go` - 0.85 temporal coupling
  - Always changes with target
  - Likely shares state or assumptions
- `related/config.go` - 0.72 temporal coupling
  - Often changes together
  - May depend on same configuration

## Stakeholders to Notify
- alice@example.com - Primary owner of target (85% of commits)
- bob@example.com - Owns consumer/a.go
- charlie@example.com - Owns related/cache.go

## Risk Assessment
- **Blast Radius**: 5 files directly, 12 files transitively
- **Coupling Risk**: 2 files with implicit dependencies
- **Team Impact**: 3 developers should be notified

## Recommended Approach
1. Coordinate with alice (primary owner)
2. Update tests in target_test.go first
3. Check related/cache.go for shared assumptions
4. Notify bob and charlie before merging
```

## Reducing Impact

Strategies to minimize blast radius:

1. **Add an adapter**: Wrap changes behind a stable interface
2. **Deprecate first**: Add deprecation warnings before removing
3. **Feature flag**: Gate changes behind a flag for gradual rollout
4. **Split the change**: Break into smaller, reviewable pieces
