---
name: flag-audit
title: Feature Flag Audit
description: Identify stale feature flags and provide actionable cleanup recommendations
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: provider
    description: "Filter by provider: launchdarkly, split, unleash, posthog, flipper, or all"
    required: false
    default: "all"
  - name: stale_days
    description: Days after which a flag is considered stale
    required: false
    default: "30"
---

# Feature Flag Audit

Analyze feature flags and provide cleanup recommendations for: {{.paths}}

## When to Use

- Monthly feature flag hygiene reviews
- Before major releases to clean up temporary flags
- After experiments conclude to remove test flags
- Sprint planning to allocate flag cleanup work
- Identifying technical debt from abandoned flags

## Workflow

### Step 1: Detect All Flags
```
analyze_flags:
  paths: {{.paths}}
  provider: {{.provider}}
```
Find all feature flag references in the codebase with staleness information.

### Step 2: Identify Ownership
```
analyze_ownership:
  paths: {{.paths}}
```
Understand who owns the code containing flags - they should own cleanup.

### Step 3: Check Coupling
```
analyze_temporal_coupling:
  paths: {{.paths}}
  days: 90
```
Find files that change together with flag-containing files.

## Flag Lifecycle Reference

| Flag Type | Expected Lifespan | Action if Stale |
|-----------|-------------------|-----------------|
| Release | 7-14 days | Remove after 100% rollout |
| Experiment | 30-90 days | Remove after analysis complete |
| Ops/Kill Switch | Permanent | Document, don't remove |
| Permission | Permanent | Document, audit access |

## Output

### Feature Flag Audit Report

**Scope**: {{.paths}}
**Provider Filter**: {{.provider}}
**Stale Threshold**: {{.stale_days}} days
**Audit Date**: [date]

---

### Executive Summary

| Metric | Count |
|--------|-------|
| Total flags | |
| Stale flags (>{{.stale_days}} days) | |
| Critical priority | |
| High priority | |
| Files affected | |

**Estimated Cleanup Effort**: [hours/days]

### Flag Inventory

#### By Provider

| Provider | Flags | Stale | Cleanup Priority |
|----------|-------|-------|------------------|
| | | | |

#### By Priority

| Priority | Count | Criteria |
|----------|-------|----------|
| CRITICAL | | >90 days stale, high file spread |
| HIGH | | >60 days stale or in hotspot files |
| MEDIUM | | >30 days stale |
| LOW | | Recently added or low file spread |

---

### Stale Flags (Immediate Action Required)

Flags that have been in the codebase longer than expected:

| Flag Key | Provider | Age (days) | References | Files | Owner | Action |
|----------|----------|------------|------------|-------|-------|--------|
| | | | | | | Remove/Keep/Investigate |

### Flag Details

For each stale flag:

#### `[flag_key]`

**Provider**: [provider]
**First Seen**: [date]
**Last Modified**: [date]
**Age**: [days] days
**Priority**: [CRITICAL/HIGH/MEDIUM/LOW]

**References** ([count] total):
| File | Line | Context |
|------|------|---------|
| | | |

**Coupled Flags** (often used together):
- [other_flag_1]
- [other_flag_2]

**Owner**: [primary contributor to these files]

**Recommendation**:
- [ ] **Remove**: Flag appears to be serving 100% or 0%
- [ ] **Keep**: Document as permanent (ops/permission flag)
- [ ] **Investigate**: Check flag service for current state

**Cleanup Steps**:
1. Verify flag state in [provider] dashboard
2. If serving 100%: Remove flag checks, keep enabled code path
3. If serving 0%: Remove flag checks AND disabled code path
4. Remove flag from [provider] dashboard
5. Update tests that reference this flag

---

### Cleanup Roadmap

#### Immediate (This Sprint)

High-confidence removals - flags clearly at 100% or 0%:

| Flag | Files to Modify | Estimated Effort | Risk |
|------|-----------------|------------------|------|
| | | | Low |

#### Short-term (Next 2-4 Sprints)

Flags requiring investigation before removal:

| Flag | Blocker | Owner | Next Step |
|------|---------|-------|-----------|
| | Need to verify in dashboard | | Check analytics |

#### Document (Permanent Flags)

Flags that should remain but need documentation:

| Flag | Type | Purpose | Documentation Location |
|------|------|---------|----------------------|
| | Kill switch | Emergency disable for X | |
| | Permission | Feature access control | |

### Code Quality Impact

#### Files with Multiple Flags

Files with high flag density may indicate:
- Feature branching complexity
- Potential dead code paths
- Testing complexity

| File | Flag Count | Complexity | Recommendation |
|------|------------|------------|----------------|
| | | | Consolidate/Refactor |

#### Flag-Related Technical Debt

| Issue | Count | Impact |
|-------|-------|--------|
| Nested flag checks | | Hard to reason about |
| Flags in hot paths | | Performance overhead |
| Untested flag combinations | | Risk of bugs |

### Prevention Recommendations

1. **Flag Ownership**: Assign owner when creating flag
2. **Expiration Dates**: Set expected removal date in flag service
3. **Automated Alerts**: Alert when flags exceed expected lifespan
4. **CI Checks**: Fail builds if stale flag count exceeds threshold
5. **Sprint Ritual**: Review flag inventory monthly

### Metrics to Track

| Metric | Current | Target | Trend |
|--------|---------|--------|-------|
| Total flags | | | |
| Flags per 1K LOC | | <5 | |
| Average flag age | | <30 days | |
| Stale flag % | | <10% | |

---

**Next Audit**: [suggested date, typically 30 days]
**Owner**: [team/person responsible for flag hygiene]
