---
sidebar_position: 5
---

# Architectural Smell Detection

```bash
omen smells
```

The smells analyzer detects structural patterns in code that indicate design problems. These are not bugs -- the code works -- but they are patterns that make the code harder to change, more likely to accumulate defects, and more expensive to maintain. Martin Fowler and Kent Beck popularized the term "code smell" in *Refactoring* (1999) to describe exactly this: surface indicators that usually correspond to deeper problems in the design.

Omen detects five categories of architectural smells using AST analysis and, where applicable, dependency graph construction.

## God Class

**What it is:** A class that has absorbed too many responsibilities. It has too many methods, too many fields, and is involved in too many of the system's operations. It tends to grow over time because "it already knows about everything, so I'll just add this here."

**How Omen detects it:** A class is flagged as a God Class when it exceeds thresholds on multiple indicators simultaneously:

- High WMC (Weighted Methods per Class): many methods with significant complexity
- Low LCOM4 (Lack of Cohesion): methods that share few or no instance variables, indicating the class is doing unrelated things
- High number of fields and methods relative to other classes in the codebase

**Why it matters:** God Classes are the most common source of merge conflicts in large codebases. Every change to the system seems to touch the God Class, because it is entangled with everything. Testing is painful because the class has many dependencies and many states. When a bug is found in a God Class, fixing it risks breaking unrelated functionality that happens to live in the same class.

**How to fix it:** Extract cohesive groups of methods and their associated fields into separate classes. The LCOM4 metric from the [cohesion analyzer](./cohesion.md) directly identifies which groups of methods belong together: each connected component in the LCOM4 graph is a candidate for extraction.

## Data Clumps

**What it is:** Groups of fields or parameters that appear together repeatedly across the codebase. For example, `street`, `city`, `state`, `zip` appearing as four separate parameters in multiple function signatures, or `x`, `y`, `z` stored as three fields in several classes rather than as a `Point` or `Vector3`.

**How Omen detects it:** The analyzer examines function signatures and class field declarations across the codebase and identifies groups of names/types that co-occur in three or more locations. It uses fuzzy matching on parameter names and types to catch variations like `streetAddr`/`street_address`/`street`.

**Why it matters:** Data clumps are a missed abstraction. The group of values belongs together conceptually, but the lack of a unifying type means:

- Every function that works with the group must accept all its members individually, inflating parameter lists.
- Validation logic for the group is duplicated wherever the group appears.
- Adding a new member to the group (e.g., `country` to the address) requires updating every function signature.

**How to fix it:** Introduce a class, struct, or record to represent the group. Replace individual parameters with the new type. This reduces parameter counts, centralizes validation, and creates a named concept in the codebase.

## Divergent Change

**What it is:** A file that is modified for multiple, unrelated reasons. If `UserService` changes when you modify the authentication flow, when you update the billing integration, and when you add a new reporting feature, it is exhibiting divergent change. Each of those changes is driven by a different concern, but they all land in the same file.

**How Omen detects it:** Omen analyzes Git history to find files that appear in commits with diverse commit messages or that change in conjunction with files from multiple unrelated modules. It also uses AST analysis to detect methods within a class that depend on non-overlapping sets of external types, suggesting the class serves multiple masters.

**Why it matters:** Divergent change is a direct violation of the Single Responsibility Principle. Files with divergent change:

- Accumulate merge conflicts as independent work streams collide.
- Become hard to reason about because their behavior spans multiple domains.
- Resist refactoring because each change set has different stakeholders and test requirements.

**How to fix it:** Split the class along the lines of its distinct responsibilities. Each reason for change should correspond to a separate class. This often aligns with the LCOM4 connected components.

## Feature Envy

**What it is:** A method that uses data from another class more than from its own class. The method "envies" the other class's features and would be more naturally defined there.

**How Omen detects it:** For each method, the analyzer counts how many times it accesses fields and methods of its own class versus fields and methods of other classes. If the ratio of external accesses to internal accesses exceeds a threshold, the method is flagged.

```java
// Feature envy: this method belongs on Order, not ReportGenerator
class ReportGenerator {
    String generateOrderSummary(Order order) {
        return order.getCustomer().getName() + ": " +
               order.getItems().size() + " items, $" +
               order.getTotal() + " (tax: $" +
               order.getTax() + ")";
    }
}
```

In this example, `generateOrderSummary` makes five accesses to `Order` and zero accesses to `ReportGenerator`'s own state. It envies `Order`.

**Why it matters:** Feature envy indicates that behavior is in the wrong place. It creates unnecessary coupling: `ReportGenerator` must know the internal structure of `Order`. If `Order` changes (e.g., `getTotal()` is renamed), `ReportGenerator` breaks even though it has nothing to do with order processing.

**How to fix it:** Move the method to the class whose data it primarily uses. If the method genuinely needs data from both classes, consider whether the data it needs could be passed as a parameter or extracted into a shared abstraction.

## Cyclic Dependencies

**What it is:** A set of modules or classes that depend on each other in a cycle. Module A imports B, B imports C, and C imports A. This creates a situation where none of the modules can be understood, tested, or compiled independently.

**How Omen detects it:** Omen builds a directed dependency graph from import/use/require statements across the codebase and runs **Tarjan's Strongly Connected Components (SCC) algorithm** to find cycles. Tarjan's algorithm identifies all maximal sets of mutually reachable nodes in a single linear-time traversal of the graph. Every SCC with more than one node is a dependency cycle.

The algorithm works by performing a depth-first traversal of the dependency graph and maintaining a stack of visited nodes. When a back-edge is found (a dependency pointing to a node already on the stack), a cycle is identified. The algorithm runs in O(V + E) time, where V is the number of modules and E is the number of dependencies.

**Why it matters:** Cyclic dependencies are a structural trap:

- **Build order:** In compiled languages, cycles can prevent compilation entirely. In interpreted languages, they cause subtle import-order bugs.
- **Testing:** You cannot unit test any module in the cycle in isolation. Mocking one dependency brings in the others.
- **Change propagation:** A change anywhere in the cycle potentially affects every other module in the cycle.
- **Cognitive load:** Understanding module A requires understanding B and C, which requires understanding A.

**How to fix it:**

- **Dependency Inversion:** Introduce an interface in the lower-level module and have the higher-level module depend on the interface rather than the concrete implementation.
- **Extract shared code:** If A and B both depend on each other because they share some logic, extract that logic into a new module C that both depend on.
- **Merge:** If two modules are so tightly coupled that separating them creates a cycle, they may belong as one module.

## Output

```bash
# Default output
omen smells

# JSON output
omen -f json smells

# Analyze a specific directory
omen -p ./src/services smells
```

The output groups findings by smell type and lists each instance with the affected file, class or function name, line numbers, and a severity indicator. For cyclic dependencies, the output shows the full cycle path (A -> B -> C -> A).

## References

- Fowler, M. (1999). *Refactoring: Improving the Design of Existing Code*. Addison-Wesley.
- Tarjan, R.E. (1972). "Depth-first search and linear graph algorithms." *SIAM Journal on Computing*, 1(2), 146-160.
- Martin, R.C. (2003). *Agile Software Development: Principles, Patterns, and Practices*. Prentice Hall.
- Suryanarayana, G., Samarthyam, G., & Sharma, T. (2014). *Refactoring for Software Design Smells*. Morgan Kaufmann.
