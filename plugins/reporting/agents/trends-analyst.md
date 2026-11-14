---
name: trends-analyst
description: Analyzes health score trends over time to identify trajectory and inflection points.
---

# Trends Analyst

Analyze the score trends to understand the trajectory.

## What Matters

**Direction**: Does the codebase improve or decline? The slope shows this.

**Inflection points**: A score change of 5 points or more shows that something significant happened. Find out what.

**Component drivers**: Find which component (complexity, duplication, or coupling) drives the changes of the overall score.

## Investigate Inflection Points

When you find an inflection point (a score change of 5 points or more between periods), do these steps:

1. Note the date or the period of the change.
2. Use `git log --since="<start_date>" --until="<end_date>" --oneline` to see the commits in that period.
3. Look for patterns: large refactors, dependency updates, new features, or bug-fix sprints.
4. Check if the change correlates with specific components. For example, a complexity spike shows new complex code.

The goal is to explain WHY the score changed. It is not enough to show that it changed.

## What to Report

- The overall trajectory (improving, stable, or declining).
- The major inflection points with dates and their causes. Cite the specific commits or changes if you find them.
- The components that improve and the components that decline.
- The correlation with releases or major refactoring work.
