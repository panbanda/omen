---
sidebar_position: 12
---

# Code Ownership and Bus Factor

The ownership analyzer examines Git blame data to determine how knowledge and authorship are distributed across the codebase. It identifies files owned primarily by a single contributor, calculates bus factor, and highlights areas where knowledge concentration creates risk.

## How It Works

Omen runs `git blame` (via gix) on each file and attributes every line to the author of its most recent modification. From this data it calculates:

- **Primary owner**: The contributor who authored the most lines in the file.
- **Ownership ratio**: The percentage of lines attributed to the primary owner.
- **Contributor count**: The total number of distinct authors who have modified the file.
- **Bus factor**: The minimum number of contributors who collectively own more than 50% of the code. A contributor is considered "major" if they authored more than 5% of the file's lines.

## Command

```bash
omen ownership
```

### Common Options

```bash
# Analyze a specific directory
omen -p ./src ownership

# JSON output
omen -f json ownership

# Filter by language
omen ownership --language python

# Remote repository
omen -p rust-lang/rust ownership
```

## Risk Levels

Ownership concentration is a proxy for knowledge risk. When one person owns most of a file, that file's maintenance depends on their availability.

| Ownership Ratio | Risk Level | Interpretation |
|-----------------|------------|----------------|
| > 90% | High | Single point of failure. If the primary owner leaves, no one else has meaningful context on this code. |
| 70--90% | Medium | Limited knowledge sharing. One person dominates, but others have some familiarity. |
| 50--70% | Low | Healthy distribution. Multiple contributors have significant ownership. |
| < 50% | Very low | Broad ownership. No single person dominates. May indicate highly collaborative or frequently refactored code. |

### The sweet spot

Research suggests that 2 to 4 significant contributors per module is optimal. Fewer than 2 creates bus factor risk. More than 6 or 7 can indicate diffuse ownership where no one feels responsible, which correlates with higher defect rates (the "tragedy of the commons" in code ownership).

## Example Output

```
Code Ownership Analysis
=======================

  File                        Primary Owner    Ownership   Contributors   Bus Factor   Risk
  src/core/engine.py          alice            94%         2              1            High
  src/api/handler.go          bob              78%         4              2            Medium
  src/utils/format.ts         carol            52%         6              3            Low
  src/models/user.rb          dave             41%         8              4            Very Low
  src/auth/token.go           alice            100%        1              1            High
```

In JSON format:

```json
{
  "files": [
    {
      "path": "src/core/engine.py",
      "primary_owner": "alice",
      "ownership_ratio": 0.94,
      "contributors": 2,
      "bus_factor": 1,
      "risk": "high"
    }
  ]
}
```

## Bus Factor

Bus factor is the minimum number of people who would need to be unavailable (the grim metaphor: "hit by a bus") before a file or module loses all knowledgeable contributors.

Omen calculates bus factor per file by:

1. Sorting contributors by their line count in descending order.
2. Accumulating ownership percentages from the top.
3. Counting how many contributors are needed to exceed 50% of the file.

A bus factor of 1 means a single person owns a majority of the code. A bus factor of 3 means three people collectively account for more than half. Higher is generally better, up to the point where ownership becomes too diffuse.

### Bus factor at the project level

While Omen reports bus factor per file, you can aggregate these results to identify project-level risk. If a large fraction of files have bus factor 1 and the same primary owner, the project's effective bus factor is 1.

## Practical Applications

### Identifying knowledge silos

Files with ownership above 90% and a single contributor are knowledge silos. These files should be prioritized for:

- Code review by additional team members.
- Pair programming sessions to spread context.
- Documentation of design decisions and non-obvious behavior.

### Planning for team changes

Before a team member leaves or transitions to a different project, ownership analysis identifies which files they dominate. This enables targeted knowledge transfer rather than hoping documentation covers everything.

### Code review assignment

Ownership data can inform review routing. If a change touches a file primarily owned by Alice, Alice should review it (she has the most context). If Alice is the one making the change, someone else should review it (to spread knowledge).

### Correlating ownership with defect rates

Files with very high ownership concentration tend to have fewer bugs (the owner knows the code well) but are fragile in the long term (the owner eventually moves on). Files with very low ownership concentration can have more bugs because no one feels fully responsible. The relationship is U-shaped: moderate ownership concentration produces the best outcomes.

## Interpreting Results

### High ownership is not always bad

A file with 95% ownership by one person is only risky if that person is the bottleneck. In a small team or early-stage project, concentrated ownership may be efficient. The risk grows as the team scales or the project's lifespan extends.

### Low ownership is not always good

A file modified by 15 different people with no one owning more than 10% can indicate churn without stewardship. No one feels accountable for the file's design integrity, and changes may accumulate without coherent direction.

### Blame data has limitations

Git blame attributes each line to its last modifier, not its original author. A developer who reformats a file or runs a linter will appear as the "owner" of lines they did not meaningfully write. Large-scale refactors, automated code modifications, and merge commits can all distort blame data.

## Research Background

**Bird et al. (2011).** "Don't Touch My Code! Examining the Effects of Ownership on Software Quality" -- Published at IEEE FSE, this study analyzed ownership patterns at Microsoft and found that files with many minor contributors (those contributing less than 5% of the code) had significantly more pre-release and post-release defects. The number of minor contributors was a stronger predictor of defect density than the number of major contributors.

**Nagappan, Murphy, and Basili (2008).** "The Influence of Organizational Structure on Software Quality" -- Also from Microsoft Research, this study demonstrated that organizational metrics (including ownership distribution) predict defects more accurately than traditional code metrics like complexity, lines of code, or code coverage. The key finding was that the organizational structure around the code -- who writes it, how many people touch it, how distributed the team is -- matters more for quality than the code's structural properties.

**Rahman and Devanbu (2011).** "Ownership, Experience and Defects: A Fine-Grained Study of Authorship" -- Confirmed that concentrated ownership is beneficial up to a point, but that the experience of the owner matters as much as the concentration level. An inexperienced sole owner is worse than distributed ownership among experienced contributors.

These studies collectively establish that code ownership metrics are among the strongest predictors of software quality, and that the relationship between ownership patterns and defect rates is well-understood and reproducible across organizations.
