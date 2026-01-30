---
sidebar_position: 11
---

# Temporal Coupling

Temporal coupling analysis reveals hidden dependencies between files by examining which files change together in the same commits. Unlike import-based dependency analysis, temporal coupling captures relationships that exist in practice -- logical dependencies, shared assumptions, and coordination requirements -- regardless of whether they appear in the code's module structure.

## How It Works

Omen walks the Git commit history and records which files appear together in each commit. For every pair of files that co-occur, it calculates a coupling strength based on how frequently they change together relative to how frequently each changes independently.

The coupling metric is:

```
coupling(A, B) = commits_containing_both(A, B) / commits_containing_either(A, B)
```

This is the Jaccard similarity coefficient applied to commit sets. A value of 1.0 means the two files have never changed independently -- every commit that touches one also touches the other. A value near 0 means the files rarely appear in the same commit.

## Command

```bash
omen temporal
```

### Common Options

```bash
# Analyze a specific directory
omen -p ./src temporal

# JSON output for scripting
omen -f json temporal

# Analyze a remote repository
omen -p facebook/react temporal
```

## Interpreting Coupling Strength

| Coupling | Interpretation | Action |
|----------|---------------|--------|
| > 80% | Tight dependency. These files almost always change together. | Investigate whether they should be merged, or document the dependency explicitly. |
| 50--80% | Frequently coupled. A real relationship likely exists. | Review whether the coupling reflects a design dependency or a process artifact. |
| 20--50% | Moderate coupling. May be coincidental. | Worth noting, but may reflect broad refactors or shared configuration changes. |
| < 20% | Weak coupling. Probably independent. | No action needed unless other signals (like import analysis) suggest otherwise. |

## What Temporal Coupling Reveals

### Hidden dependencies not visible in imports

Two files may have no direct import relationship yet still be tightly coupled. Common examples:

- A database migration file and the model code it supports.
- A configuration file and the module that reads it.
- A test file and the code it tests (these should be coupled, but the coupling direction matters).
- An API handler and the frontend component that calls it, in a monorepo.

These dependencies are invisible to static analysis of import graphs. Temporal coupling surfaces them from observed behavior.

### Logical coupling

When a change to module A requires a corresponding change to module B due to shared assumptions (data format, protocol, naming convention), the files are logically coupled even if they share no code. Temporal coupling detects this pattern because developers make both changes in the same commit.

### Accidental coupling

Not all temporal coupling is meaningful. Some common sources of noise:

- **Bulk reformatting commits.** A linter or formatter run touches many files simultaneously, inflating coupling scores.
- **Copy-paste duplication.** If file B was created by copying file A, they may carry correlated changes until the duplication is resolved.
- **Large feature branches squashed into single commits.** Files that are part of the same feature but not logically dependent appear coupled.

Omen's coupling scores should be interpreted alongside other signals. High temporal coupling combined with no import dependency is the most interesting case -- it suggests a hidden relationship worth investigating.

## Example Output

```
Temporal Coupling Analysis
==========================

  File A                    File B                    Coupling   Co-commits   Total
  src/api/handler.go        src/api/middleware.go      0.85       34           40
  src/db/schema.sql          src/models/user.go         0.72       18           25
  src/config/settings.py     src/core/engine.py         0.65       13           20
  pkg/auth/token.go          pkg/auth/validate.go       0.91       42           46
  src/utils/format.ts        src/utils/parse.ts         0.33       5            15
```

In JSON format:

```json
{
  "pairs": [
    {
      "file_a": "src/api/handler.go",
      "file_b": "src/api/middleware.go",
      "coupling": 0.85,
      "co_commits": 34,
      "total_commits": 40
    }
  ]
}
```

## Practical Applications

### Identifying extraction candidates

Files with coupling above 80% that live in different packages or modules may belong together. If `auth/token.go` and `auth/validate.go` change together 91% of the time, they are effectively a single unit. Merging them or extracting a shared abstraction can simplify the codebase.

### Detecting shotgun surgery

When a single logical change requires modifications to many files, it indicates that a concern is spread across the codebase. Temporal coupling highlights which files form these change sets, pointing to opportunities to consolidate.

### Validating module boundaries

If files within a module are tightly coupled with each other but weakly coupled with files outside it, the module boundary is well-drawn. If cross-module coupling is high, the boundary may need adjustment.

### Onboarding

Temporal coupling data tells new team members: "when you change this file, you probably also need to change these other files." This knowledge is otherwise acquired only through experience or tribal knowledge.

## Configuration

Temporal coupling analysis uses the Git history available in the repository. The depth of history analyzed depends on the number of commits present. For large repositories, you can limit the scope by targeting a subdirectory:

```bash
omen -p ./src/core temporal
```

## Research Background

Temporal coupling as a software analysis technique originates from two key studies:

**Ball et al. (1997).** "If Your Version Control System Could Talk..." -- Research conducted at AT&T Bell Laboratories demonstrating that co-change patterns in version control history reveal architectural dependencies that are not captured by static analysis. The authors showed that files frequently modified together tend to share design dependencies, and that these patterns can predict future changes.

**Beyer and Noack (2005).** "Clustering Software Artifacts Based on Frequent Common Changes" -- Extended the concept by showing that temporal coupling patterns can be used to cluster related files and that these clusters predict which files will need to change together in the future. Their work demonstrated that historical co-change data is a reliable predictor of future co-change, often more accurate than structural dependency analysis alone.

Adam Tornhill's work on behavioral code analysis (2015, 2018) popularized these techniques for practitioners, demonstrating their application in codebases at scale and combining temporal coupling with other historical signals like code churn and ownership patterns.
