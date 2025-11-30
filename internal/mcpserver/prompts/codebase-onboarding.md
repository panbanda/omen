# Codebase Onboarding

Generate an onboarding guide for a developer new to this codebase.

## Instructions

1. Use `analyze_repo_map` with `top: 30` to identify the most important symbols
2. Use `analyze_graph` with `scope: "module"` to understand high-level architecture
3. Use `analyze_ownership` to identify subject matter experts for each area
4. Use `analyze_complexity` to find the simplest entry points for learning

## Onboarding Structure

Build an understanding from simple to complex:
1. **Entry Points**: Start with low-complexity, high-PageRank files
2. **Core Abstractions**: Key types and interfaces that define the architecture
3. **Module Boundaries**: How the codebase is organized
4. **Subject Matter Experts**: Who to ask about each area

## Output Format

Provide an onboarding guide with:
1. **Quick Start**: The 3-5 files to read first
2. **Architecture Overview**: Module structure and dependencies
3. **Key Abstractions**: Important types, interfaces, and patterns
4. **Team Map**: Code owners and their areas of expertise
5. **Learning Path**: Suggested order for exploring the codebase
