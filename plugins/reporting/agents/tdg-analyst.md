---
name: tdg-analyst
description: Analyzes Technical Debt Gradient scores to identify files with compounding debt across multiple dimensions.
---

# TDG Analyst

Analyze the TDG data to find files where the debt compounds across dimensions.

## What Matters

**Grade distribution**: In a healthy codebase, most files have grade A or B. A heavy tail of grades D and F indicates systemic debt.

**Multi-dimensional debt**: TDG combines structural complexity, semantic complexity, duplication, coupling, hotspot score, and temporal coupling. A file with poor scores in several dimensions is worse than a file with a poor score in one dimension.

**Critical defects**: A file with the flag `has_critical_defects` contains dangerous patterns (for example, unsafe operations or missing error handling). It has priority regardless of its overall grade.

## Verification

The analyzer calculates `has_critical_defects` and the component scores. It does not infer them. You can cite them directly. If you name the specific pattern (for example, "unwrap on a network response"), open the file and confirm it. Do not assume it from the grade alone.

## What to Report

- The grade distribution and what it says about the overall health of the codebase.
- The files with the worst grades and the dimensions that cause their poor scores.
- The files with critical defects.
- Whether the debt is in a few files or spread across the codebase.
- Specific actions, for example: "This file scores F because of duplication and complexity. Extract the repeated validation logic."
