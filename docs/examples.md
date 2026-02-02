---
sidebar_position: 3
---

# Example Reports

Omen can generate comprehensive HTML health reports for any repository. These reports run all analyzers in parallel, invoke LLM analyst agents for narrative insights, and render an interactive report covering complexity, technical debt, hotspots, architecture, ownership, and more.

## Analyzed Repositories

| Repository | Language | Description | Report |
|---|---|---|---|
| [BerriAI/litellm](https://github.com/BerriAI/litellm) | Python | LLM proxy server -- call 100+ LLM APIs in the OpenAI format | [View Report](pathname:///omen/reports/litellm.html) |
| [discourse/discourse](https://github.com/discourse/discourse) | Ruby | Community discussion platform used by thousands of organizations | [View Report](pathname:///omen/reports/discourse.html) |
| [Gusto/apollo-federation-ruby](https://github.com/Gusto/apollo-federation-ruby) | Ruby | Apollo Federation implementation for Ruby GraphQL backends | [View Report](pathname:///omen/reports/apollo-federation-ruby.html) |
| [zed-industries/zed](https://github.com/zed-industries/zed) | Rust | High-performance multiplayer code editor | [View Report](pathname:///omen/reports/zed.html) |

## Generating Your Own Report

There are two ways to generate reports:

### CLI

```bash
# Generate JSON data files from all analyzers
omen report generate

# Render data + insights into a self-contained HTML file
omen report render

# Or serve with live re-render
omen report serve
```

The CLI runs all analyzers in parallel and renders an HTML report from the raw data.

### Claude Code Skill (recommended)

```
/omen-reporting:generate-report
```

The Claude Code skill goes beyond the CLI by having LLM analyst agents interpret the data. Each section of the report gets a dedicated agent that provides narrative analysis -- explaining what the numbers mean, identifying patterns across metrics, and producing prioritized recommendations. The result is a report with both raw data and expert-level commentary.

Reports are saved to `.omen/report/` by default.

## What the Reports Cover

Each report includes the following sections:

- **Executive Summary** -- overall health score with key findings
- **Complexity Analysis** -- functions exceeding cyclomatic and cognitive thresholds
- **Churn Analysis** -- most frequently changed files with contributor breakdown
- **Hotspot Analysis** -- files where high complexity meets high churn
- **Technical Debt Gradient** -- per-file TDG scores with letter grades (A-F)
- **Code Clone Detection** -- duplicate code groups with similarity percentages
- **Self-Admitted Technical Debt** -- categorized TODO/FIXME/HACK comments
- **Temporal Coupling** -- files that change together revealing hidden dependencies
- **Code Ownership** -- bus factor and knowledge concentration risk
- **Dependency Graph** -- module coupling, circular dependencies, centrality metrics
- **Architectural Smells** -- god classes, feature envy, cyclic dependencies
- **Feature Flags** -- flag inventory with staleness tracking
- **Score Trends** -- health score changes over time
