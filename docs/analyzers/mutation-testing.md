---
sidebar_position: 19
---

# Mutation Testing

```bash
omen mutation
```

The mutation testing analyzer evaluates test suite effectiveness by introducing small, controlled changes (mutations) to source code and checking whether the test suite detects them. A mutation that causes a test failure is "killed" -- the tests caught the change. A mutation that passes all tests is a "survivor" -- a gap in test coverage that line-coverage metrics would miss entirely.

## Why Mutation Testing Matters

Line coverage and branch coverage answer the question "was this code executed during tests?" Mutation testing answers a harder question: "would the tests actually catch a bug here?"

A test suite can achieve 100% line coverage without meaningfully asserting anything. If every line is executed but no assertions validate the behavior, mutations will survive. Mutation testing quantifies this gap.

Jia and Harman (2011) conducted a comprehensive survey of mutation testing research and found it to be the strongest available test adequacy criterion. Their analysis showed that test suites optimized for mutation coverage consistently outperform those optimized for structural coverage at detecting real faults.

Papadakis et al. (2019) confirmed this with a large-scale empirical study: mutation testing subsumes other test adequacy criteria. A test suite that kills most mutants will also achieve high branch coverage, but the reverse is not true.

## Mutation Operators

Omen implements 21 mutation operators organized into five language families. The core operators apply to all supported languages. Language-specific operators target idioms unique to Rust, Go, TypeScript, Python, and Ruby.

### Core Operators (All Languages)

These 10 operators cover the universal mutation categories defined in the literature.

| Operator | Code | What It Mutates | Example |
|----------|------|-----------------|---------|
| Constant Replacement | CRR | Replaces constants with boundary values | `0` -> `1`, `""` -> `"mutant"` |
| Relational Operator | ROR | Swaps relational operators | `<` -> `<=`, `==` -> `!=` |
| Arithmetic Operator | AOR | Swaps arithmetic operators | `+` -> `-`, `*` -> `/` |
| Conditional Operator | COR | Swaps logical operators | `&&` -> `\|\|`, `!x` -> `x` |
| Unary Operator | UOR | Removes or swaps unary operators | `-x` -> `x`, `!b` -> `b` |
| Statement Deletion | SDL | Removes a statement entirely | `return result;` -> `` |
| Return Value | RVR | Replaces return values | `return true;` -> `return false;` |
| Boundary Value | BVO | Shifts boundary conditions | `x > 0` -> `x > 1`, `x >= 10` -> `x >= 11` |
| Bitwise Operator | BOR | Swaps bitwise operators | `&` -> `\|`, `<<` -> `>>` |
| Assignment Operator | ASR | Swaps compound assignments | `+=` -> `-=`, `*=` -> `/=` |

### Rust-specific Operators

| Operator | What It Mutates | Example |
|----------|-----------------|---------|
| BorrowOperator | Borrow semantics | `&x` -> `&mut x`, removes borrows |
| OptionOperator | Option handling | `Some(x)` -> `None`, `unwrap()` -> `unwrap_or_default()` |
| ResultOperator | Result handling | `Ok(x)` -> `Err(...)`, `?` operator removal |

### Go-specific Operators

| Operator | What It Mutates | Example |
|----------|-----------------|---------|
| GoErrorOperator | Error handling patterns | `if err != nil` -> `if err == nil`, removes error checks |
| GoNilOperator | Nil checks and returns | `return nil` -> `return &T{}`, `!= nil` -> `== nil` |

### TypeScript-specific Operators

| Operator | What It Mutates | Example |
|----------|-----------------|---------|
| TSEqualityOperator | Strict/loose equality | `===` -> `==`, `!==` -> `!=` |
| TSOptionalOperator | Optional chaining and nullish coalescing | `?.` -> `.`, `??` -> `\|\|` |

### Python-specific Operators

| Operator | What It Mutates | Example |
|----------|-----------------|---------|
| PythonIdentityOperator | Identity and membership operators | `is` -> `is not`, `in` -> `not in` |
| PythonComprehensionOperator | Comprehension mutations | Modifies filter predicates, replaces with empty |

### Ruby-specific Operators

| Operator | What It Mutates | Example |
|----------|-----------------|---------|
| RubyNilOperator | Nil handling | `nil?` -> `!nil?`, `&.` -> `.` |
| RubySymbolOperator | Symbol/string interchange | `:symbol` -> `"symbol"` |

## Execution Modes

### Fast Mode

```bash
omen mutation --mode fast
```

Runs a subset of operators (ROR, COR, RVR, SDL) that historically kill the most mutants per time invested. Suitable for CI pipelines where full mutation analysis is too slow.

### Thorough Mode

```bash
omen mutation --mode thorough
```

Runs all 21 operators. Takes significantly longer but provides the most complete picture of test suite effectiveness. Best run overnight or on dedicated CI agents.

### Dry Run

```bash
omen mutation --mode dry-run
```

Generates all mutations and reports them without executing the test suite. Useful for reviewing what mutations would be created, estimating runtime, and validating configuration.

## Coverage Integration

Omen uses existing coverage data to skip mutating code that is not covered by any test. This is a significant performance optimization: there is no point in creating a mutant for a line that is never executed by tests -- it will survive by definition.

Supported coverage formats:

| Format | Languages | Source |
|--------|-----------|--------|
| LLVM-cov | Rust, C, C++ | `cargo llvm-cov`, `llvm-cov export` |
| Istanbul | JavaScript, TypeScript | `nyc`, `c8`, `jest --coverage` |
| coverage.py | Python | `coverage json` |
| Go coverage | Go | `go test -coverprofile` |

Omen auto-detects coverage files in standard locations. To specify a coverage file explicitly:

```bash
omen mutation --coverage ./coverage/lcov.info
```

## Parallel Execution

Mutation testing is inherently parallelizable: each mutation is independent and can be tested in its own process. Omen uses a work-stealing scheduler to distribute mutations across available CPU cores.

```bash
# Use all available cores (default)
omen mutation

# Limit to 4 parallel workers
omen mutation --jobs 4
```

The work-stealing approach adapts to uneven test durations: if one mutation's test run finishes quickly, that worker immediately picks up the next unprocessed mutation rather than waiting for slower peers.

## Incremental Mode

Full mutation testing can take minutes to hours on large codebases. Incremental mode limits mutation to files that have changed since the last run:

```bash
omen mutation --incremental
```

This is designed for CI integration: on each pull request, only the changed files are mutated. The full suite can be run on a nightly schedule.

## Equivalent Mutant Detection

Some mutations produce code that is semantically identical to the original -- they can never be killed because they don't change behavior. These "equivalent mutants" inflate the denominator of the mutation score, making results look worse than they are.

Omen includes an ML-based equivalent mutant detector that learns from historical mutation results. The workflow:

### 1. Collect Training Data

```bash
omen mutation --record
```

Runs mutation testing normally but saves detailed results (mutation type, location, code context, outcome) to a training dataset.

### 2. Train the Model

```bash
omen mutation train
```

Trains a classifier on the recorded data to predict which future mutations are likely to be equivalent. The model uses features from the mutation type, surrounding AST context, and historical outcomes.

### 3. Skip Predicted Equivalents

```bash
omen mutation --skip-predicted
```

Uses the trained model to filter out mutations predicted to be equivalent before running the test suite. This reduces total test runs and produces a more accurate mutation score.

The model is stored locally and improves with more data. It is project-specific -- patterns that produce equivalent mutants vary by codebase.

## Mutation Score

The mutation score is the primary metric:

```
Mutation Score = Killed Mutants / (Total Mutants - Equivalent Mutants)
```

### Score Interpretation

| Score | Interpretation |
|-------|----------------|
| > 80% | Excellent. The test suite catches the vast majority of potential faults. |
| 60-80% | Good. Most critical paths are well-tested, but some gaps exist. |
| 40-60% | Moderate. Significant testing gaps. Bugs in mutated areas would likely go undetected. |
| < 40% | Poor. The test suite provides limited fault-detection capability. |

These thresholds are based on empirical data from Papadakis et al. (2019). In practice, achieving 100% is nearly impossible due to equivalent mutants and edge cases. A score above 80% indicates a mature, effective test suite.

## Configuration

```toml
# omen.toml
[mutation]
# Execution mode: "fast", "thorough", "dry-run"
mode = "fast"

# Staleness threshold for incremental mode (in commits)
incremental_lookback = 1

# Maximum number of parallel test workers
jobs = 0  # 0 = use all available cores

# Test command to run for each mutation
test_command = "cargo test"

# Timeout per mutation test run (seconds)
timeout = 60

# Operators to include (empty = all applicable operators for detected languages)
operators = []

# Operators to exclude
exclude_operators = []

# Files/directories to exclude from mutation
exclude = ["tests/", "test_helpers/", "fixtures/"]

# Coverage file path (empty = auto-detect)
coverage_path = ""
```

### Customizing the Test Command

Omen needs to know how to run your test suite. The `test_command` is the shell command that will be executed for each mutation. It should:

- Run the relevant tests (not necessarily the entire suite)
- Exit with code 0 on success, non-zero on failure
- Be as fast as possible (mutations multiply the total runtime)

```toml
# Rust
test_command = "cargo test --lib"

# JavaScript/TypeScript
test_command = "npm test"

# Python
test_command = "pytest tests/ -x --no-header -q"

# Go
test_command = "go test ./..."

# Ruby
test_command = "bundle exec rspec --fail-fast"
```

The `-x` / `--fail-fast` flags (where available) are recommended: once a test fails, the mutation is killed and there is no need to run remaining tests.

## Output

```bash
# Table output with summary
omen mutation

# JSON output for CI integration
omen -f json mutation

# Dry run to see what would be mutated
omen mutation --mode dry-run

# Incremental mode for PRs
omen mutation --incremental

# Specific directory
omen -p ./src/core mutation
```

The JSON output includes per-file breakdowns with individual mutation details:

```json
{
  "score": 0.78,
  "total_mutants": 342,
  "killed": 267,
  "survived": 61,
  "equivalent": 14,
  "timeout": 0,
  "files": [
    {
      "path": "src/parser.rs",
      "mutants": 45,
      "killed": 38,
      "survived": 7,
      "score": 0.844
    }
  ]
}
```

## Practical Use

### CI Integration

Run fast-mode mutation testing on every PR:

```yaml
# .github/workflows/mutation.yml
name: Mutation Testing
on: [pull_request]
jobs:
  mutation:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run mutation tests
        run: |
          SCORE=$(omen -f json mutation --mode fast --incremental | jq '.score')
          if [ "$(echo "$SCORE < 0.6" | bc)" -eq 1 ]; then
            echo "Mutation score $SCORE is below threshold (0.6)"
            exit 1
          fi
```

### Identify Weak Tests

Find files with the lowest mutation scores to focus testing efforts:

```bash
omen -f json mutation | jq '[.files[] | select(.score < 0.5)] | sort_by(.score)'
```

### Compare Before and After

Use mutation testing to validate that a refactoring didn't weaken the test suite:

```bash
# Before refactoring
omen -f json mutation > before.json

# After refactoring
omen -f json mutation > after.json

# Compare scores
jq -s '.[0].score as $before | .[1].score as $after |
  {before: $before, after: $after, delta: ($after - $before)}' before.json after.json
```

## References

- Jia, Y., & Harman, M. (2011). "An Analysis and Survey of the Development of Mutation Testing." *IEEE Transactions on Software Engineering*, 37(5), 649-678.
- Papadakis, M., Kintis, M., Zhang, J., Jia, Y., Le Traon, Y., & Harman, M. (2019). "Mutation Testing Advances: An Analysis and Survey." *Advances in Computers*, Vol. 112, 275-378.
- Offutt, A.J., & Untch, R.H. (2001). "Mutation 2000: Uniting the Orthogonal." *Mutation Testing for the New Century*, 34-44.
