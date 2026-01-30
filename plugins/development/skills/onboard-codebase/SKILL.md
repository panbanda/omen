---
name: onboard-codebase
description: Generate onboarding guide with key symbols, architecture overview, and subject matter experts. Use when joining a new project, onboarding teammates, or creating project documentation.
---

# Onboard Codebase

Generate a comprehensive onboarding guide for a new developer joining the project, identifying key code, architecture, and people to talk to.

## Prerequisites

Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Identify Key Symbols

Run the repomap analysis to find the most important code:

```bash
omen -f json repomap
```

PageRank-ranked symbols show what's central to the codebase.

### Step 2: Map the Architecture

Run the dependency graph analysis to understand structure:

```bash
omen -f json graph
```

This reveals how the codebase is organized and how modules relate.

### Step 3: Identify Subject Matter Experts

Run the ownership analysis to find who knows what:

```bash
omen -f json ownership
```

This shows who has expertise in each area of the code.

### Step 4: Find Complexity Hotspots

Run the complexity analysis to identify tricky areas:

```bash
omen -f json complexity
```

New developers should be warned about complex areas.

## Output Format

Generate an onboarding guide:

```markdown
# Onboarding Guide: <Project Name>

## Quick Start
1. Clone the repo: `git clone ...`
2. Install dependencies: `...`
3. Run tests: `...`
4. Start dev server: `...`

## Architecture Overview

### Module Structure
```
src/
├── core/       # Core business logic
├── api/        # HTTP handlers
├── storage/    # Database layer
└── utils/      # Shared utilities
```

### Key Dependencies
- `core/` depends on `storage/`
- `api/` depends on `core/`
- `utils/` is shared by all

## Important Code to Understand

### Core Symbols (by importance)
1. `core.Engine` - Main processing engine, heart of the system
2. `api.Router` - HTTP routing and middleware
3. `storage.Repository` - Database abstraction
4. ...

### Entry Points
- `cmd/server/main.go` - Main server entry point
- `cmd/worker/main.go` - Background job processor

## Subject Matter Experts

| Area | Expert | Notes |
|------|--------|-------|
| Core engine | alice@example.com | Original author |
| API layer | bob@example.com | Recent maintainer |
| Storage | charlie@example.com | Database specialist |

## Areas to Be Careful With

### High Complexity
- `core/processor.go` - Cognitive complexity: 45
  - Talk to alice before modifying
  - Has many edge cases

- `api/middleware.go` - Cyclomatic complexity: 28
  - Authentication logic is tricky
  - Well-tested but fragile

### Knowledge Silos
- `legacy/importer.go` - Only one contributor
  - Documentation is sparse
  - Ask alice for context

## First Tasks Suggestions

Good starter tasks to learn the codebase:
1. Add a new API endpoint (learn api/ patterns)
2. Write tests for an untested function (learn testing patterns)
3. Fix a documentation issue (explore while reading)
4. Address a simple TODO/FIXME (learn code style)

## Resources

- Design docs: `docs/design/`
- API docs: `docs/api/`
- Team wiki: <link>
```
