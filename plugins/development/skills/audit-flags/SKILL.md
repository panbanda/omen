---
name: audit-flags
description: Identify stale feature flags and prioritize cleanup. Use when cleaning up flags after launches, during monthly hygiene reviews, or before major releases.
---

# Audit Flags

Identify the stale feature flags. Give actionable cleanup recommendations.

## Prerequisites

The Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Detect All Flags

Run the flags analysis:

```bash
omen -f json flags
```

The command returns all the feature flag references with staleness information. It groups them by provider.

### Step 2: Identify the Ownership

Run the ownership analysis:

```bash
omen -f json ownership
```

Find who owns the code that contains the flags. The owners must do the cleanup.

### Step 3: Check the Coupling

Run the temporal coupling analysis:

```bash
omen -f json temporal
```

Find the files that change together with the files that contain flags.

## Flag Lifecycle Reference

| Flag Type | Expected Lifespan | Action if Stale |
|-----------|-------------------|-----------------|
| Release | 7-14 days | Remove after 100% rollout |
| Experiment | 30-90 days | Remove after analysis complete |
| Ops/Kill Switch | Permanent | Document, don't remove |
| Permission | Permanent | Document, audit access |

## Priority Matrix

| Priority | Criteria | Action |
|----------|----------|--------|
| CRITICAL | >90 days stale, high file spread | Immediate removal |
| HIGH | >60 days stale or in hotspot files | This sprint |
| MEDIUM | >30 days stale | Next sprint |
| LOW | Recently added or low file spread | Monitor |

## Output Format

Make a flag audit report in this format:

```markdown
# Feature Flag Audit Report

## Summary

| Metric | Count |
|--------|-------|
| Total flags | 23 |
| Stale (>30 days) | 8 |
| Critical priority | 2 |
| High priority | 3 |

## Stale Flags (Action Required)

### `enable_new_checkout` (CRITICAL)
- **Provider**: LaunchDarkly
- **Age**: 95 days
- **References**: 12 across 5 files
- **Owner**: alice (payment team)
- **Action**: Remove - likely at 100%

### `experiment_pricing_v2` (HIGH)
- **Provider**: Split
- **Age**: 67 days
- **References**: 4 across 2 files
- **Owner**: bob (growth team)
- **Action**: Check experiment results, then remove

## Cleanup Roadmap

### This Sprint
1. Remove `enable_new_checkout` - 2 hours
2. Investigate `experiment_pricing_v2` - 1 hour

### Next Sprint
1. Clean up remaining HIGH priority flags

## Permanent Flags (Document)

| Flag | Type | Purpose |
|------|------|---------|
| `kill_switch_payments` | Ops | Emergency disable |
| `beta_access` | Permission | Feature gating |
```

## Cleanup Steps

For each stale flag, do these steps:

1. Verify the state of the flag in the provider dashboard.
2. If the flag serves 100%: remove the flag checks. Keep the enabled code path.
3. If the flag serves 0%: remove the flag checks and the disabled code path.
4. Remove the flag from the provider dashboard.
5. Update the tests that reference the flag.
