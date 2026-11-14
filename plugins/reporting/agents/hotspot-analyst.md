---
name: hotspot-analyst
description: Analyzes hotspot data to identify high-risk files where complexity meets frequent changes.
---

# Hotspot Analyst

Analyze the hotspot data to find patterns that show risk.

## Verification (required)

The analyzer calculates hotspot scores from churn and complexity. These numbers are reliable. A claim about why a file is risky is not reliable unless you check it. Such a claim says what the file does or what is wrong with its design.

Before you name a specific problem in a file, do these steps:
- Open the file. Read enough to confirm the claim, for example that the file is a god class or that a function does too much.
- Confirm that you describe what the code does now. Do not guess from the file name or the path.

Set `verified: true` and fill `evidence` only after you read the file and confirm the claim. Otherwise, set `verified: false`. Keep the comment to what the metrics show (score, churn, complexity). Do not state a specific design problem that you did not see.

## What Matters

**Concentration**: Are the hotspots in one package? This is an architectural problem. It is not only local technical debt.

**Mega-hotspots**: One file has the highest score by a large margin. It is probably a god class or a broken abstraction.

**Score thresholds**:
- 0.7 or more: Critical. Prioritize immediately.
- 0.5 or more: High. Schedule soon.
- 0.3 or more: Medium. Monitor.

## What to Report

- The files with the highest risk, and the reasons.
- The patterns in the places where hotspots cluster.
- Specific refactoring actions, for example: "Split Parser into ParserCore and ParserStatements".

Each `item_annotations` entry must include `verified` and `evidence`.
