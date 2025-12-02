---
name: onboard-codebase
title: Onboard Codebase
description: Generate onboarding guide with key symbols, architecture overview, and subject matter experts. Use when joining a new project, onboarding teammates, or creating project documentation.
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: role
    description: "Developer role: frontend, backend, fullstack, devops, or general"
    required: false
    default: "general"
  - name: depth
    description: "Guide depth: quick, standard, or comprehensive"
    required: false
    default: "standard"
  - name: top
    description: Maximum items to return
    required: false
    default: "20"
---

# Codebase Onboarding Guide

Generate an onboarding guide for: {{.paths}}

## When to Use

- When a new developer joins the team
- When switching to a new project
- When exploring an unfamiliar codebase
- To document institutional knowledge

## Workflow

### Step 1: Important Symbols
```
analyze_repo_map:
  paths: {{.paths}}
  top: 30
```
Get PageRank-ranked symbols - the most important functions, types, and classes.

### Step 2: Architecture Overview
```
analyze_graph:
  paths: {{.paths}}
  scope: module
  include_metrics: true
```
Understand high-level module structure and dependencies.

### Step 3: Subject Matter Experts
```
analyze_ownership:
  paths: {{.paths}}
  top: {{.top}}
```
Identify who knows what - who to ask about each area.

### Step 4: Entry Points
```
analyze_complexity:
  paths: {{.paths}}
  functions_only: false
```
Find the simplest files (low complexity) that are also important (high PageRank) - good starting points.

### Step 5: Technical Debt Awareness
```
analyze_satd:
  paths: {{.paths}}
  strict_mode: false
```
Surface known issues and caveats new developers should know about.

## Output

### Onboarding Guide for {{.paths}}

**Target Role**: {{.role}}
**Guide Depth**: {{.depth}}
**Generated**: [date]

---

### Quick Start

Read these files first (low complexity, high importance):

| Priority | File | Why Start Here |
|----------|------|----------------|
| 1 | | Entry point / core abstraction |
| 2 | | Key types and interfaces |
| 3 | | Main business logic |
| 4 | | Configuration and setup |
| 5 | | Common utilities |

### Architecture Overview

```
[Module dependency diagram - simplified]
```

**Core Modules**:
| Module | Purpose | Key Files |
|--------|---------|-----------|
| | | |

**Module Dependencies**:
| From | To | Relationship |
|------|-----|--------------|
| | | imports/uses |

### Key Abstractions

The most important types and interfaces to understand:

| Symbol | File | Purpose | Used By |
|--------|------|---------|---------|
| | | | |

### Code Patterns

Common patterns used in this codebase:

| Pattern | Example Location | When Used |
|---------|------------------|-----------|
| | | |

### Team Map

Who to ask about each area:

| Area | Expert | Backup | Notes |
|------|--------|--------|-------|
| | | | |

**Bus Factor Warnings**:
- [Files with single owner that are critical]

### Learning Path

Suggested order for exploring the codebase:

**Week 1: Foundations**
1. [ ] Read [entry point files]
2. [ ] Understand [core abstractions]
3. [ ] Run the test suite
4. [ ] Make a small change (typo fix, logging)

**Week 2: Core Domain**
1. [ ] Study [main business logic modules]
2. [ ] Trace a request through the system
3. [ ] Review recent PRs
4. [ ] Pair with [expert] on a task

**Week 3: Deep Dive**
1. [ ] Explore [complex subsystems]
2. [ ] Understand [integration points]
3. [ ] Review [architectural decisions]
4. [ ] Take on a small feature

### Known Gotchas

Things that might trip you up:

| Gotcha | Location | Workaround |
|--------|----------|------------|
| | | |

### Technical Debt Awareness

Known issues documented in the code:

| Category | Count | Examples |
|----------|-------|----------|
| TODO | | |
| FIXME | | |
| HACK | | |

### Development Setup

Key commands and configurations:
```
[Build commands]
[Test commands]
[Common tasks]
```

### Resources

- [ ] Architecture docs: [location]
- [ ] API docs: [location]
- [ ] Runbooks: [location]
- [ ] Slack channels: [channels]

---

**Next Steps**: After completing this guide, use `change-impact` before making changes and `code-review-focus` when reviewing PRs.
