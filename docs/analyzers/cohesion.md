---
sidebar_position: 3
---

# CK Metrics (Cohesion)

```bash
omen cohesion
```

The cohesion analyzer computes the Chidamber-Kemerer (CK) object-oriented metrics suite. These six metrics quantify different dimensions of class design: how much work a class does, how tightly it is coupled to other classes, how cohesive its methods are, and how deep its inheritance hierarchy runs.

The CK suite was introduced by Chidamber and Kemerer in 1994 and has been validated extensively in empirical studies as a predictor of fault-proneness, maintainability cost, and change impact. Basili et al. (1996) confirmed that several CK metrics -- particularly WMC, CBO, and RFC -- are statistically significant predictors of which classes will contain faults.

## The Six Metrics

### WMC -- Weighted Methods per Class

**What it measures:** The sum of the complexities of all methods defined in a class.

In Omen's implementation, each method's weight is its cyclomatic complexity. A class with 5 methods, each with complexity 3, has a WMC of 15. A class with 2 methods of complexity 1 each has a WMC of 2.

**What it tells you:** WMC is a proxy for the effort required to develop, test, and maintain a class. High WMC means the class does a lot of complex work. Classes with WMC above 20 tend to be candidates for decomposition.

**Threshold:** `< 20`

### CBO -- Coupling Between Objects

**What it measures:** The number of distinct classes that a given class references. If class `OrderProcessor` uses `Order`, `Customer`, `PaymentGateway`, and `Logger`, its CBO is 4.

Only direct dependencies count. If `OrderProcessor` uses `Customer` and `Customer` uses `Address`, the `Address` dependency does not contribute to `OrderProcessor`'s CBO.

**What it tells you:** High CBO means a class is entangled with many other classes. Changes to any of those classes may force changes here. High-CBO classes are harder to reuse, harder to test in isolation, and more likely to propagate defects.

**Threshold:** `< 10`

### RFC -- Response for Class

**What it measures:** The total number of methods that can potentially be invoked in response to a message received by an object of the class. This includes the class's own methods plus all methods directly called by those methods (one level deep).

For example, if a class has 5 methods and those methods collectively call 12 distinct external methods, the RFC is 17.

**What it tells you:** RFC captures the "blast radius" of a class. A high RFC means that understanding the behavior of the class requires tracing through many method calls. Testing and debugging become more expensive because there are more code paths to consider.

**Threshold:** `< 50`

### LCOM4 -- Lack of Cohesion in Methods

**What it measures:** The number of connected components in the graph where methods are nodes and edges connect methods that share at least one instance variable.

LCOM4 = 1 means all methods in the class are connected through shared fields: the class is cohesive. LCOM4 = 3 means there are three independent groups of methods that share no state with each other. Those groups are essentially separate classes forced into one.

Omen uses the LCOM4 variant (Henderson-Sellers) rather than the original LCOM1 from the CK paper, because LCOM1 produces misleading results for classes with accessor methods. LCOM4 builds an undirected graph and counts connected components, which is more robust.

**What it tells you:** A class with LCOM4 > 1 is doing multiple unrelated things. It should probably be split. LCOM4 is one of the strongest signals for the God Class smell.

**Threshold:** `< 3`

### DIT -- Depth of Inheritance Tree

**What it measures:** The length of the longest path from a class to the root of its inheritance hierarchy. A class that extends nothing has DIT = 0. A class that extends `Base` has DIT = 1. A class that extends `Mid`, which extends `Base`, has DIT = 2.

**What it tells you:** Deep inheritance trees create tight coupling between layers. A change to a base class can ripple down through many descendants, often in ways that are difficult to predict. DIT above 5 suggests the hierarchy should be flattened, potentially replacing inheritance with composition.

The Liskov Substitution Principle becomes harder to maintain as DIT grows. Each additional level adds behavioral expectations that subclasses must honor.

**Threshold:** `< 5`

### NOC -- Number of Children

**What it measures:** The number of classes that directly extend a given class (immediate subclasses only, not transitive).

**What it tells you:** A class with many children has a large influence on the system. Changes to its interface or behavior affect all subclasses. High NOC also suggests that the class may be too general or that the inheritance hierarchy should be replaced with a different abstraction (interfaces, traits, composition).

However, high NOC is not always bad. Framework base classes and abstract interfaces are designed to be extended. The metric is most useful for identifying classes that have accumulated many subclasses without that being an intentional design decision.

**Threshold:** `< 6`

## Language Support

The CK metrics are most meaningful for languages with class-based OO semantics. Omen computes them for:

- **Java** -- full support (classes, interfaces, abstract classes)
- **C++** -- classes and structs with methods
- **C#** -- classes, interfaces, abstract classes
- **Ruby** -- classes and modules
- **Python** -- classes (including dataclasses)

For languages without class constructs (C, Bash), or where classes are less central to the programming model (Go, Rust), the cohesion analyzer either skips the file or adapts the metrics to the closest equivalent (e.g., Rust `impl` blocks).

## How Omen Computes It

Omen parses each file with tree-sitter and identifies class-like nodes. For each class, it:

1. **Enumerates methods** and computes their cyclomatic complexity (for WMC).
2. **Extracts field accesses** per method to build the LCOM4 graph.
3. **Identifies type references** in method signatures, bodies, and field declarations (for CBO).
4. **Traces one level of method calls** from each method (for RFC).
5. **Resolves inheritance** within the file and across the analyzed scope (for DIT and NOC).

Because tree-sitter provides concrete syntax trees rather than fully resolved type information, some cross-file resolution (e.g., determining whether an imported name is a class) is approximate. The metrics are most precise within a single file and degrade slightly for cross-module analysis.

## Thresholds

| Metric | Threshold | What Exceeding It Means |
|--------|-----------|------------------------|
| WMC | < 20 | Class is too complex; consider splitting |
| CBO | < 10 | Class is too coupled; reduce dependencies |
| RFC | < 50 | Class has too large a response set; simplify interactions |
| LCOM4 | < 3 | Class lacks cohesion; likely doing multiple unrelated things |
| DIT | < 5 | Inheritance too deep; consider composition |
| NOC | < 6 | Too many subclasses; consider interfaces or composition |

## Output

```bash
# Default table output
omen cohesion

# JSON for scripting
omen -f json cohesion

# Analyze only Java files
omen cohesion --language java

# Analyze a specific module
omen -p ./src/models cohesion
```

The output lists each class with its six metric values and flags metrics that exceed their thresholds.

## References

- Chidamber, S.R., & Kemerer, C.F. (1994). "A Metrics Suite for Object Oriented Design." *IEEE Transactions on Software Engineering*, 20(6), 476-493.
- Basili, V.R., Briand, L.C., & Melo, W.L. (1996). "A Validation of Object-Oriented Design Metrics as Quality Indicators." *IEEE Transactions on Software Engineering*, 22(10), 751-761.
- Henderson-Sellers, B. (1996). *Object-Oriented Metrics: Measures of Complexity*. Prentice Hall.
