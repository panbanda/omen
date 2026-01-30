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
| [vercel/next.js](https://github.com/vercel/next.js) | TypeScript | The React framework for production -- server rendering, static generation, and more | [View Report](pathname:///omen/reports/next-js.html) |
| [kubernetes/kubernetes](https://github.com/kubernetes/kubernetes) | Go | Production-grade container orchestration | [View Report](pathname:///omen/reports/kubernetes.html) |

## Generating Your Own Report

Reports are generated using the Claude Code skill:

```
/omen-reporting:generate-report
```

This skill:

1. Runs all applicable analyzers in parallel via the Omen MCP server
2. Spawns 14 LLM analyst agents to produce narrative insights for each section
3. Renders an interactive standalone HTML report with charts, tables, and prioritized recommendations

Reports are saved to `.omen/report.html` by default. Run `omen report view` to open the generated report in your browser.

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
