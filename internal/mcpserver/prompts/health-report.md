---
name: health-report
title: Health Report
description: Generate an interactive HTML repository health report with visualizations. Use for stakeholder presentations, quarterly reviews, or comprehensive codebase health dashboards.
arguments:
  - name: target
    description: "Repository to analyze: local path, GitHub shorthand (owner/repo), or GitHub URL"
    required: false
    default: "."
  - name: days
    description: Days of git history for churn analysis
    required: false
    default: "30"
  - name: trend_period
    description: "Trend analysis period: daily, weekly, monthly"
    required: false
    default: "monthly"
  - name: trend_since
    description: "How far back for trend analysis: 3m, 6m, 1y, 2y, 10y"
    required: false
    default: "10y"
---

# Repository Health Report

Generate an interactive HTML repository health report for: {{.target}}

## Overview

Create a single self-contained HTML file with embedded CSS and JavaScript (using Chart.js from CDN) that provides a comprehensive visualization of repository health metrics.

## Target Repository

This prompt works with both local and remote repositories:

- **Local**: Current directory (default `.`) or specify a path
- **Remote**: GitHub shorthand (`owner/repo`), full URL (`https://github.com/owner/repo`), or with ref (`owner/repo@branch`)

When a remote repository is specified, omen will clone it to a temporary directory for analysis.

## Required Analyses

Run the following analyses and incorporate their results:

### Step 1: Repository Score
```bash
omen score {{.target}} -f json
```
Get overall health score (0-100) and component breakdown.

### Step 2: Complexity Analysis
```bash
omen analyze complexity {{.target}} -f json
```
Function-level cyclomatic and cognitive complexity metrics.

### Step 3: Hotspot Analysis
```bash
omen analyze hotspot {{.target}} --days {{.days}} -f json
```
Files with high churn combined with high complexity.

### Step 4: Churn Analysis
```bash
omen analyze churn {{.target}} --days {{.days}} -f json
```
Recent file change patterns and contributor activity.

### Step 5: Ownership Analysis
```bash
omen analyze ownership {{.target}} -f json
```
Bus factor and knowledge silo identification.

### Step 6: SATD Analysis
```bash
omen analyze satd {{.target}} -f json
```
Self-admitted technical debt (TODO/FIXME/HACK markers).

### Step 7: Duplication Analysis
```bash
omen analyze duplicates {{.target}} -f json
```
Code clone detection and duplication ratio.

### Step 8: Trend Analysis
```bash
omen analyze trend {{.target}} --since {{.trend_since}} --period {{.trend_period}} -f json
```
Historical score trends over time.

### Step 9: Historical Event Investigation

For each significant score change (>=2 points) identified in the trend data:
```bash
git log --oneline --since="<date-1week>" --until="<date+1week>"
```
Correlate score changes with releases and code changes.

## Report Structure

### Header Section
- Repository name and generation date
- Large circular score visualization (0-100)
- Key metadata: files analyzed, functions analyzed, threshold status
- Overall trend indicator (improving/declining/stable)

### Component Scores Section
For each component (Complexity, Duplication, Cohesion, TDG, SATD, Coupling, Smells):
- Score with color coding (green >80, yellow 60-80, red <60)
- Progress bar visualization
- Key metrics specific to that component
- Definition box explaining what the metric measures

### Historical Trends Section
- Line chart showing overall score over time with trend line
- Annotations on the chart marking significant score changes
- Component trends chart showing individual metrics over time
- Statistical summary: start score, end score, slope, R-squared
- **Historical Events Table**: For each significant score change (>=2 points):
  - Period and magnitude of change
  - Primary driver (which component changed most)
  - Key releases identified from git history
  - Color-coded rows (red for drops, green for improvements)
- **Component Events Table**: Major individual component changes
- Insights box explaining patterns

### Hotspots Section
- Summary stats: total hotspots, max score, average score
- Table of top 15 hotspots with file path and score
- Definition and importance explanation

### Code Churn Section
- Summary stats: total changes, unique files, lines added/deleted
- Table of highest-churning files
- Top contributors by commit volume

### Ownership Section
- Bus factor score
- Knowledge silo count and ratio
- Chart showing top contributors by code ownership
- Average contributors per file

### SATD Section
- Total items, files affected, critical/high counts
- Doughnut charts for severity and category distribution
- Table listing critical and high-priority items with file locations

### Duplication Section
- Duplication ratio percentage
- Clone group statistics
- Bar chart showing clone groups by size

### Recommendations Section
Three cards:
- **High Priority**: Issues needing immediate attention
- **Medium Priority**: Issues to address in upcoming sprints
- **Ongoing Maintenance**: Metrics to monitor

### Glossary Section
Definition boxes for all technical terms used in the report

## Design Requirements

### Visual Style
- Dark theme with GitHub-inspired color palette:
  - Background: #0d1117 (primary), #161b22 (secondary), #21262d (tertiary)
  - Text: #e6edf3 (primary), #8b949e (secondary)
  - Accents: #3fb950 (green), #d29922 (yellow), #f85149 (red), #58a6ff (blue), #a371f7 (purple)
- Cards with subtle borders (#30363d)
- Responsive design with mobile breakpoints

### Charts (Chart.js)
- Load from CDN: `https://cdn.jsdelivr.net/npm/chart.js`
- Load annotation plugin: `https://cdn.jsdelivr.net/npm/chartjs-plugin-annotation`
- Use annotations for marking significant events on trend charts
- Consistent color scheme matching each component
- Tooltips showing detailed values
- Proper axis labels and legends

### Tables
- Hover highlighting
- Badge components for severity levels (critical/high/medium/low)
- Truncated file paths for readability

### Interactive Elements
- Collapsible sections where appropriate
- Tab interfaces for switching between related views

## Output

Save the report as `repository-health-report.html` in the repository root.

The file must be completely self-contained:
- All CSS embedded in `<style>` tags
- All JavaScript embedded in `<script>` tags
- Chart.js and plugins loaded from CDN
- All analysis data embedded as JavaScript objects
- Opens directly in any modern browser
- Prints cleanly for PDF export
