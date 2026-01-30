---
sidebar_position: 2
---

# Complexity Analysis

```bash
omen complexity
```

The complexity analyzer measures two complementary aspects of function complexity: **cyclomatic complexity** (how many execution paths exist) and **cognitive complexity** (how hard the code is for a human to understand). Both are computed per function via tree-sitter AST analysis.

## Cyclomatic Complexity

Cyclomatic complexity counts the number of linearly independent paths through a function's control flow graph. It was introduced by Thomas McCabe in 1976 as a quantitative measure of program testing difficulty.

The formula is straightforward: start at 1 (the function's baseline path) and add 1 for every decision point:

- `if` / `else if`
- `for` / `while` / `loop`
- `match` / `switch` arms
- `&&` / `||` (short-circuit boolean operators)
- `catch` / `except` blocks
- Ternary expressions (`? :`)

A function with no branches has a cyclomatic complexity of 1. Each branch adds exactly one path.

### Example

```python
def process_order(order):           # +1 baseline
    if not order.is_valid():        # +1
        return None
    if order.is_priority():         # +1
        if order.amount > 1000:     # +1
            apply_discount(order)
        else:
            apply_surcharge(order)
    for item in order.items:        # +1
        if item.is_backordered():   # +1
            notify_warehouse(item)
    return order                    # total: 6
```

### Why It Matters

McCabe's original paper established that functions with cyclomatic complexity above 10 are significantly harder to test exhaustively. Each independent path requires at least one test case for full path coverage. A function with complexity 25 has at least 25 distinct execution paths, making comprehensive testing impractical and bugs more likely to hide in untested branches.

Empirical studies have consistently confirmed the correlation between cyclomatic complexity and defect density. Basili and Perricone (1984) found that modules with complexity above 10 had significantly higher fault rates. Subsequent large-scale analyses at NASA and other organizations reinforced this threshold as a practical boundary.

## Cognitive Complexity

Cognitive complexity measures how difficult a function is for a human to read and understand. It was developed by SonarSource as a response to cyclomatic complexity's blindness to readability concerns.

The key insight: not all control flow is equally hard to follow. A simple `if/else` is easy to read. A deeply nested `if` inside a `for` inside a `try` block is much harder, even if the cyclomatic complexity is similar.

Cognitive complexity applies three rules:

1. **Increment for each break in linear flow.** Each `if`, `for`, `while`, `switch`, `catch`, ternary, and logical operator chain adds 1.

2. **Increment for each level of nesting.** A decision nested inside another decision adds an additional penalty equal to its nesting depth. An `if` at the top level adds 1. An `if` nested inside a `for` adds 2 (1 for the `if` + 1 for being nested one level deep). An `if` nested two levels deep adds 3.

3. **No increment for shorthand structures that reduce complexity.** `else` and `else if` do not add to nesting penalties because they don't require the reader to mentally backtrack -- they continue an existing flow.

### Example

```python
def process_order(order):
    if not order.is_valid():          # +1 (if)
        return None
    if order.is_priority():           # +1 (if)
        if order.amount > 1000:       # +2 (if + 1 nesting)
            apply_discount(order)
        else:                         # +1 (else, no nesting penalty)
            apply_surcharge(order)
    for item in order.items:          # +1 (for)
        if item.is_backordered():     # +2 (if + 1 nesting)
            notify_warehouse(item)
                                      # total: 8
```

Compare this to the cyclomatic complexity of 6 for the same function. The cognitive score is higher because it accounts for the nesting, which makes the code harder to follow even though the path count is moderate.

### Nesting Depth

In addition to the complexity scores, the analyzer reports maximum nesting depth per function. Deeply nested code is hard to read regardless of the type of nesting. The default threshold flags functions that exceed 4 levels of nesting.

## How Omen Computes It

Omen parses each source file into an AST using tree-sitter grammars. It walks the tree, identifies function nodes, and then traverses each function body counting decision-point nodes for cyclomatic complexity and applying the SonarSource nesting rules for cognitive complexity.

Because the analysis is AST-based, it works correctly across all 13 supported languages without language-specific regex patterns. The tree-sitter grammar for each language defines what constitutes a decision point (e.g., `if_expression` in Rust vs. `if_statement` in Python), and the analyzer maps these to the appropriate complexity increments.

## Thresholds

Default thresholds can be overridden in `omen.toml`:

| Metric | Warning | Error | Config Key |
|--------|---------|-------|------------|
| Cyclomatic complexity | 10 | 20 | `cyclomatic_warn`, `cyclomatic_error` |
| Cognitive complexity | 15 | 30 | `cognitive_warn`, `cognitive_error` |
| Nesting depth | 4 | -- | `max_nesting` |

These defaults are based on widely adopted industry thresholds. The cyclomatic warning at 10 follows McCabe's original recommendation. The cognitive thresholds at 15/30 follow SonarSource's recommended defaults. The nesting depth limit of 4 reflects the Linux kernel coding standard and is commonly adopted elsewhere.

### Customizing Thresholds

```toml
# omen.toml
[complexity]
cyclomatic_warn = 10
cyclomatic_error = 20
cognitive_warn = 15
cognitive_error = 30
max_nesting = 4
```

## Output

```bash
# Table output (default)
omen complexity

# JSON output
omen -f json complexity

# Filter to a specific language
omen complexity --language python

# Analyze a specific directory
omen -p ./src/core complexity
```

The output lists every function that exceeds a threshold, sorted by complexity score descending. Each entry includes the file path, function name, line number, cyclomatic score, cognitive score, and nesting depth.

## References

- McCabe, T.J. (1976). "A Complexity Measure." *IEEE Transactions on Software Engineering*, SE-2(4), 308-320.
- Campbell, G.A. (2018). "Cognitive Complexity: A new way of measuring understandability." SonarSource whitepaper.
- Basili, V.R., & Perricone, B.T. (1984). "Software Errors and Complexity: An Empirical Investigation." *Communications of the ACM*, 27(1), 42-52.
