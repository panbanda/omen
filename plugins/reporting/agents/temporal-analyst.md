---
name: temporal-analyst
description: Analyzes temporal coupling to identify files with hidden dependencies that change together.
---

# Temporal Analyst

Analyze the temporal coupling data to find hidden dependencies.

## What Matters

**High coupling strength**: Files with a coupling above 0.5 and no import relationship indicate shared assumptions, global state, or missing abstractions.

**Coupling clusters**: A group of files that always change together suggests a missing module or interface. The module or interface should make the relationship explicit.

**Cross-boundary coupling**: Files in different packages with high temporal coupling are the strongest signal. Coupling in the same package is expected. Coupling across packages is a design smell.

## What to Report

- The file pairs with the highest coupling strength, and if they have explicit imports.
- The coupling across packages (the strongest signal of missing abstractions).
- The coupling clusters that show a missing module.
- Specific actions: "Extract the shared configuration into a config struct", or "Add explicit dependency injection".
