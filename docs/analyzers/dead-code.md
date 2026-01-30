---
sidebar_position: 14
---

# Dead Code Detection

The dead code analyzer identifies code that exists in the repository but is never executed: unused functions, unreferenced variables, unreachable statements, and orphaned imports. Dead code inflates the codebase, confuses developers, and correlates with other quality problems.

## How It Works

Omen uses tree-sitter to parse each file and extract declarations (functions, variables, classes, constants) and references (calls, accesses, imports). It then cross-references declarations against references across the entire file set to identify symbols that are declared but never used.

The analyzer detects four categories of dead code:

| Category | Description | Example |
|----------|-------------|---------|
| Unused functions | Functions or methods that are never called anywhere in the analyzed scope. | A helper function written speculatively that was never integrated. |
| Unused variables | Variables that are assigned but never read. | A result captured from a function call but never used. |
| Unreachable code | Statements that appear after an unconditional `return`, `break`, `continue`, `throw`, or `exit`. | Debug logging left after a `return` statement. |
| Unused imports | Import statements that bring in symbols not referenced in the file. | An import left behind after the code that used it was deleted. |

## Command

```bash
omen deadcode
```

### Common Options

```bash
# Analyze a specific directory
omen -p ./src deadcode

# JSON output
omen -f json deadcode

# Filter by language
omen deadcode --language go

# Remote repository
omen -p tokio-rs/tokio deadcode
```

## Language-Specific Behavior

### Visibility rules

Not all unexported symbols are dead code. Omen applies language-specific visibility rules:

| Language | Entry Points and Exemptions |
|----------|----------------------------|
| Go | `main()`, `init()`, exported symbols (capitalized names), test functions (`Test*`, `Benchmark*`), interface implementations |
| Python | `__main__` blocks, `__init__` methods, decorated functions (`@app.route`, `@pytest.fixture`), dunder methods |
| TypeScript/JavaScript | Default exports, named exports, React components, event handlers |
| Rust | `main()`, `#[test]` functions, public items in library crates, trait implementations |
| Java | `main(String[])`, `@Override` methods, annotated classes (`@Component`, `@Bean`) |
| Ruby | Entry point scripts, methods used via `send`/`method_missing` (limited detection) |

Functions that are exported or public are not flagged as dead code because they may be consumed by code outside the analyzed scope.

### Rust-specific handling

For Rust projects, Omen can leverage `cargo check` output for more reliable dead code detection. The Rust compiler's own dead code analysis accounts for conditional compilation (`#[cfg(...)]`), trait bounds, and macro expansions that tree-sitter parsing alone cannot resolve.

## Example Output

```
Dead Code Analysis
==================

Unused Functions:
  src/utils/legacy.py:42       calculate_old_rate()
  src/helpers/format.ts:18     padLeft()
  src/core/engine.go:156       deprecatedHandler()

Unused Variables:
  src/api/handler.go:23        unused variable 'tempResult'
  src/models/user.py:67        unused variable 'cached_value'

Unreachable Code:
  src/auth/token.ts:45         code after return statement (lines 45-52)
  src/core/processor.go:89     code after return statement (line 89)

Unused Imports:
  src/api/routes.py:3          import 'json' is unused
  src/utils/format.ts:1        import { parse } from './parse' is unused

Summary: 3 unused functions, 2 unused variables, 2 unreachable blocks, 2 unused imports
```

In JSON format:

```json
{
  "unused_functions": [
    {
      "path": "src/utils/legacy.py",
      "line": 42,
      "name": "calculate_old_rate",
      "visibility": "private"
    }
  ],
  "unused_variables": [
    {
      "path": "src/api/handler.go",
      "line": 23,
      "name": "tempResult"
    }
  ],
  "unreachable_code": [
    {
      "path": "src/auth/token.ts",
      "line": 45,
      "end_line": 52,
      "reason": "after_return"
    }
  ],
  "unused_imports": [
    {
      "path": "src/api/routes.py",
      "line": 3,
      "symbol": "json"
    }
  ]
}
```

## Why Dead Code Matters

Dead code is more than clutter. It has measurable effects on development velocity and code quality.

### It confuses developers

A developer reading a codebase for the first time cannot easily distinguish dead code from live code. They may spend time understanding a function that is never called, or worse, modify it under the assumption that it matters. Dead code inflates the apparent complexity of a system.

### It increases build times

Unused imports pull in modules that need to be compiled, linked, and sometimes bundled. In languages like TypeScript and JavaScript where tree-shaking is imperfect, dead imports can increase bundle sizes. In compiled languages, dead code still consumes compilation time.

### It can hide bugs

Dead code that was once live may contain assumptions about system state that are no longer valid. If someone reactivates it (removes the early return, re-enables the code path), those stale assumptions become bugs.

### It wastes maintenance effort

Automated refactors, linter fixes, dependency upgrades, and code reviews all spend time on dead code. Every rename, type change, or API migration that touches dead code is wasted effort.

### It correlates with other problems

Dead code is rarely an isolated issue. Files with significant dead code tend to have other quality problems: high complexity, poor test coverage, and higher defect rates. Romano et al. (2020) found that the presence of dead code is a reliable predictor of broader code quality issues in the same file.

## Limitations

### Dynamic dispatch

Languages with dynamic dispatch (Ruby's `send`, Python's `getattr`, JavaScript's bracket notation) can invoke functions by name at runtime. Omen's static analysis cannot detect these references, so it may flag functions as unused when they are actually called dynamically.

### Reflection and metaprogramming

Frameworks that use reflection (Java Spring, Ruby on Rails) or metaprogramming (Python decorators, Rust macros) may reference symbols in ways that tree-sitter cannot see. Omen applies heuristics for common frameworks but cannot cover all cases.

### Cross-repository consumers

A public function in a library may have no callers within the repository but be consumed by external code. Omen only analyzes the files within the target scope, so it does not flag exported or public symbols as dead code. This is a deliberate trade-off to avoid false positives.

### Conditional compilation

Code behind feature flags, `#[cfg(...)]` attributes, or `#ifdef` blocks may appear dead in one configuration but be live in another. Omen's tree-sitter analysis sees the code as written, not as compiled. For Rust projects, using `cargo check` integration provides better accuracy.

## Practical Applications

### Cleanup sprints

Run `omen deadcode` periodically and remove confirmed dead code. This reduces cognitive load, speeds up builds, and removes potential sources of confusion. Pair with `omen clones` to also eliminate duplicated dead code.

### Post-refactor validation

After a refactoring that changes or removes interfaces, run dead code detection to identify any callers or implementations that were missed.

### Dependency pruning

Unused imports often indicate dependencies that can be removed from the project entirely. This reduces the attack surface and build complexity.

### CI integration

```bash
DEAD=$(omen -f json deadcode | jq '.unused_functions | length')
if [ "$DEAD" -gt 0 ]; then
  echo "Found $DEAD unused functions"
  exit 1
fi
```

## Research Background

**Romano, Scanniello, Sartiani, and Risi (2020).** "On the Use of Dead Code as a Smell: An Empirical Investigation" -- Published at IEEE SANER, this study analyzed the relationship between dead code and other code quality indicators across multiple open-source projects. The authors found that files containing dead code have significantly higher defect density, higher complexity, and lower cohesion than files without dead code. Dead code is not just a cosmetic issue; it is a statistical predictor of the broader quality problems in a file.

Their findings suggest that dead code detection is valuable not only for cleanup but also as a triage signal: files flagged for dead code are worth examining for other problems even if the dead code itself is harmless.
