---
sidebar_position: 15
---

# Code Clone Detection

The code clone analyzer detects duplicated code blocks across the codebase using MinHash and Locality-Sensitive Hashing (LSH). It identifies exact copies, structurally identical code with renamed identifiers, and near-duplicates with minor modifications.

## How It Works

Omen's clone detection pipeline has three stages:

1. **Tokenization.** Each source file is parsed with tree-sitter and converted into a sequence of tokens. Whitespace, comments, and formatting are stripped so that superficial differences do not prevent detection.

2. **MinHash signature generation.** Each token sequence is shingled (broken into overlapping subsequences) and hashed using multiple hash functions to produce a compact signature. The MinHash signature approximates the Jaccard similarity between any two code blocks in constant time.

3. **LSH bucketing.** Signatures are grouped into buckets using Locality-Sensitive Hashing. Code blocks whose signatures hash to the same bucket are candidate clone pairs. Only these candidates are compared in detail, avoiding the O(n^2) cost of comparing every pair.

This approach scales to large codebases because the expensive pairwise comparison is limited to blocks that are already likely to be similar.

## Clone Types

The software engineering literature defines three types of code clones, all of which Omen detects:

| Type | Name | Description | Example |
|------|------|-------------|---------|
| Type 1 | Exact clones | Identical code after removing whitespace, comments, and formatting differences. | Copy-paste of a function with different indentation. |
| Type 2 | Parameterized clones | Same syntactic structure with different identifier names, literal values, or type names. | Two functions with identical logic but different variable names. |
| Type 3 | Near-miss clones | Similar structure with some statements added, removed, or modified. | A function copied and then partially adapted for a different use case. |

Type 1 and Type 2 clones are detected with high reliability. Type 3 detection depends on the similarity threshold -- clones with substantial modifications may fall below the threshold.

## Command

```bash
omen clones
```

### Common Options

```bash
# Analyze a specific directory
omen -p ./src clones

# JSON output
omen -f json clones

# Filter by language
omen clones --language python

# Remote repository
omen -p django/django clones
```

## Configuration

Clone detection parameters are configured in `omen.toml` under the `[duplicates]` section:

```toml
[duplicates]
min_tokens = 50
min_similarity = 0.9
```

| Parameter | Default | Description |
|-----------|---------|-------------|
| `min_tokens` | 50 | Minimum number of tokens for a code block to be considered. Blocks shorter than this are ignored. Lowering this value finds smaller clones but increases noise. |
| `min_similarity` | 0.9 | Minimum Jaccard similarity (0.0 to 1.0) for two blocks to be reported as clones. Lower values find more distant clones (Type 3) but increase false positives. |

### Tuning guidance

- **For strict duplicate detection** (Type 1 only): set `min_similarity = 1.0`.
- **For finding renamed copies** (Type 1 and 2): use the default `min_similarity = 0.9`.
- **For finding near-duplicates** (Type 1, 2, and 3): lower `min_similarity` to 0.7 or 0.8. Expect more results, some of which may be coincidental structural similarity.
- **For ignoring trivial clones**: increase `min_tokens` to 80 or 100. Small code blocks (a 3-line null check, a standard error return) are often duplicated legitimately and not worth flagging.

## Example Output

```
Code Clone Analysis
===================

Clone Group 1 (similarity: 0.95, Type 2):
  src/api/users.go:34-58         (25 lines)
  src/api/products.go:41-65      (25 lines)

Clone Group 2 (similarity: 1.00, Type 1):
  src/utils/validate.ts:12-28    (17 lines)
  src/helpers/check.ts:5-21      (17 lines)

Clone Group 3 (similarity: 0.87, Type 3):
  src/services/email.py:89-120   (32 lines)
  src/services/sms.py:45-78      (34 lines)

Summary: 3 clone groups, 6 files involved, 149 duplicated lines
```

In JSON format:

```json
{
  "clone_groups": [
    {
      "similarity": 0.95,
      "clone_type": 2,
      "instances": [
        {
          "path": "src/api/users.go",
          "start_line": 34,
          "end_line": 58,
          "lines": 25
        },
        {
          "path": "src/api/products.go",
          "start_line": 41,
          "end_line": 65,
          "lines": 25
        }
      ]
    }
  ],
  "summary": {
    "clone_groups": 3,
    "files_involved": 6,
    "duplicated_lines": 149
  }
}
```

## Why Clones Matter

### Inconsistent bug fixes

The most significant risk from code clones: when a bug is found and fixed in one copy, the fix is often not applied to the other copies. Juergens et al. (2009) found that cloned code has significantly more bugs than non-cloned code, primarily because fixes are applied inconsistently.

This is not a hypothetical risk. Their study of multiple industrial codebases showed that roughly half of all changes to cloned code were applied to only one instance, leaving the other instances with the original bug.

### Maintenance burden

Duplicated code multiplies the cost of every change that affects the duplicated logic. A change that should touch one file instead touches three or five. This increases development time, code review load, and testing surface.

### Codebase inflation

Cloned code inflates lines of code, complexity metrics, and build times without adding functionality. Removing clones typically reduces the codebase size without any loss of capability.

## Practical Applications

### Extracting shared abstractions

When Omen reports a clone group, examine whether the duplicated code can be extracted into a shared function, module, or base class. Type 2 clones (same structure, different names) are particularly good candidates -- the differences are often just the parameters of a generic solution.

```go
// Before: two clone instances
func validateUser(u User) error {
    if u.Name == "" { return errors.New("name required") }
    if u.Email == "" { return errors.New("email required") }
    return nil
}

func validateProduct(p Product) error {
    if p.Name == "" { return errors.New("name required") }
    if p.Price == 0 { return errors.New("price required") }
    return nil
}

// After: extracted validation pattern
type Validatable interface {
    Validate() error
}
```

### Identifying accidental duplication

Not all duplication is intentional. Developers working in different parts of a codebase may independently implement the same utility function. Clone detection surfaces these cases so the team can consolidate to a single implementation.

### Tracking duplication over time

Run `omen clones` in CI and track the number of clone groups and duplicated lines over time. An upward trend indicates that duplication is accumulating faster than it is being resolved.

### Combining with other analyzers

Clone analysis is most useful when combined with other signals:

- **Clones + hotspots**: Duplicated code that is also frequently modified is a high-priority cleanup target.
- **Clones + defect prediction**: Duplicated code in high-defect-risk files is dangerous because bugs in one copy are likely to exist in all copies.
- **Clones + temporal coupling**: If two clone instances always change together, they should be consolidated. If they change independently, the copies may have diverged intentionally.

## Algorithm Details

### MinHash

MinHash (Broder, 1997) estimates the Jaccard similarity between two sets without computing the full intersection. For code clone detection, each code block is represented as a set of token shingles (overlapping subsequences of k tokens). The MinHash signature is a fixed-size summary of this set, computed by applying multiple independent hash functions and keeping the minimum hash value from each.

The probability that two MinHash signatures agree on any given hash function equals the Jaccard similarity of the underlying sets. By using many hash functions (typically 100-200), the similarity estimate becomes reliable.

### Locality-Sensitive Hashing

LSH (Indyk and Motwani, 1998) reduces the clone detection problem from O(n^2) pairwise comparisons to approximately O(n). MinHash signatures are divided into bands, and each band is hashed into a bucket. Two code blocks are compared in detail only if they hash to the same bucket in at least one band.

The band size controls the trade-off between recall (finding all clones) and performance (limiting comparisons). Omen's defaults are tuned to find clones above the configured `min_similarity` threshold with high probability while keeping the false negative rate low.

## Research Background

**Juergens, Deissenboeck, Hummel, and Wagner (2009).** "Do Code Clones Matter?" -- Published at IEEE ICSE, this study is the definitive empirical investigation of whether code clones cause real harm. The authors analyzed multiple industrial codebases and found that cloned code contains significantly more defects than non-cloned code. The primary mechanism is inconsistent changes: when one clone instance is fixed, the other instances are often overlooked. Their conclusion: code clones matter, and clone detection should be part of regular quality assurance.

**Broder (1997).** "On the Resemblance and Containment of Documents" -- Introduced the MinHash technique for estimating document similarity, which forms the basis of Omen's clone detection algorithm.

**Roy, Cordy, and Koschke (2009).** "Comparison and Evaluation of Code Clone Detection Techniques and Tools: A Qualitative Approach" -- A comprehensive survey of clone detection methods, providing the Type 1/2/3 taxonomy used throughout the clone detection literature.
