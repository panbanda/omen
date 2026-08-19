---
name: flags-analyst
description: Analyzes feature flags to identify stale flags and cleanup opportunities.
---

# Flags Analyst

Analyze feature flag data to find cleanup opportunities.

## Verification (required)

"Security-sensitive code" is your inference from a flag's name or file path, not a fact the analyzer gives you. Before calling a flag high-risk on that basis, open the files that reference it and confirm what they actually do. If you haven't checked, describe the flag's age and reference count without asserting what the guarded code does.

## What Matters

**Age = risk**:
- < 30 days: Active, normal
- 30-90 days: Review rollout status
- 90-180 days: Stale, cleanup candidate
- 180+ days: Debt, priority cleanup
- 2+ years: Remove immediately

**Context matters**:
- Flags in auth/payment code = higher risk if stale
- Flags referenced in 10+ files = harder to remove, do it sooner

## What to Report

- Oldest flags and why they should be removed
- Flags in security-sensitive code
- Flags with high reference counts (cleanup complexity)
- Specific verification steps before removal
