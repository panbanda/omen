---
name: assess-impact
description: Analyze blast radius before changing specific files. Use before refactoring core components, making breaking API changes, or modifying shared code to identify affected files and stakeholders.
---

# Assess Impact

Find the potential blast radius of a change before you make it. Analyze the dependencies, the coupling patterns, and the ownership.

## Prerequisites

The Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Check the Symbol Blast Radius

Use the `impact` command to get the transitive callers and callees in one call:

```bash
omen -f json impact <symbol-name> --depth 2
```

The command returns the affected files, the caller and callee BFS levels, and the total count. This is much faster than a manual analysis.

If more than one definition has the name, the command does not select one. It writes an `ambiguous` error to stdout and exits with the status 1. Each item of `error.suggestions` has the `kind`, the `container`, the `file`, the `start_line`, and a `command`. Run the `command` of the correct definition. If no definition has the name, the error code is `not_found`, and `error.suggestions` holds near names. If the code is `not_found` and `error.param` is `start_line`, no definition of the name starts on that line. Run the `command` of one suggestion.

To inspect a specific symbol first, use this command:

```bash
omen -f json symbol <symbol-name>
```

### Step 2: Map the File-Level Dependencies

Run the dependency graph analysis for the coupling between files:

```bash
omen -f json graph --compact
```

Identify these items:
- Direct dependents (the files that import the target).
- Transitive dependents (the files that import the dependents).

### Step 3: Check the Temporal Coupling

Run the temporal coupling analysis:

```bash
omen -f json temporal
```

A file with high temporal coupling to the target often must change together with the target. This is true also if there are no explicit imports.

### Step 4: Identify the Stakeholders

Run the ownership analysis:

```bash
omen -f json ownership
```

Identify these items:
- The primary owner of the target file.
- The owners of the dependent files.
- The subject matter experts to consult.

## Impact Categories

Classify the impact by type:

| Category | Description | Action |
|----------|-------------|--------|
| Direct | Explicit imports/calls | Will break if signature changes |
| Implicit | Temporal coupling | May break due to shared assumptions |
| Behavioral | Same owner/team | Likely understands the change |
| Unknown | No coupling, different owner | Needs extra review |

## Output Format

Show the impact analysis in this format:

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

## Reduce the Impact

Use these strategies to make the blast radius smaller:

1. **Add an adapter**: Put the changes behind a stable interface.
2. **Deprecate first**: Add deprecation warnings before you remove an item.
3. **Use a feature flag**: Put the change behind a flag for a gradual rollout.
4. **Split the change**: Break the change into smaller pieces that are easy to review.
