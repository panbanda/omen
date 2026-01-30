---
sidebar_position: 2
---

# Getting Started

## Installation

### Homebrew (macOS and Linux)

```bash
brew install panbanda/omen/omen
```

### Cargo

Requires a Rust toolchain (1.70+).

```bash
cargo install omen-cli
```

### Docker

```bash
docker pull ghcr.io/panbanda/omen:latest
```

To analyze the current directory:

```bash
docker run --rm -v "$(pwd):/repo" ghcr.io/panbanda/omen:latest all
```

To analyze a specific path inside the container:

```bash
docker run --rm -v "/path/to/project:/repo" ghcr.io/panbanda/omen:latest -p /repo complexity
```

### Build from Source

```bash
git clone https://github.com/panbanda/omen.git
cd omen
cargo build --release
```

The binary will be at `target/release/omen`. Move it to a directory on your `PATH` or run it directly.

## Verifying the Installation

```bash
omen --version
omen --help
```

## Quick Start

### Run All Analyzers

The simplest way to get a full picture of a codebase:

```bash
omen all
```

This runs every analyzer against the current directory and prints a summary to stdout.

### Run a Single Analyzer

Each analyzer is a top-level subcommand:

```bash
omen complexity
omen coupling
omen clones
omen smells
omen dead-code
```

### Check the Repository Score

```bash
omen score
```

This produces a composite health score from 0 to 100 based on a weighted combination of analyzer results.

### JSON Output

All commands support JSON output for scripting and CI integration:

```bash
omen -f json score
omen -f json complexity
omen -f json all
```

## Common Workflows

### Analyze a Remote Repository

Omen can analyze any public Git repository directly. Pass the owner/repo shorthand with `-p`:

```bash
omen -p facebook/react complexity
omen -p rust-lang/rust score
omen -p expressjs/express all
```

Omen clones the repository to a temporary directory, runs the analysis, and cleans up afterward.

### Analyze a Specific Directory

```bash
omen -p ./src/api complexity
omen -p /absolute/path/to/project score
```

### Generate a Configuration File

Omen supports project-level configuration through `omen.toml`. Generate a default config:

```bash
omen init
```

This creates an `omen.toml` in the current directory with default thresholds, analyzer settings, and output preferences. Edit it to customize behavior for your project.

### Filter by Language

```bash
omen complexity --language rust
omen clones --language typescript
```

### Pipeline Integration

A typical CI step that fails if the repository score drops below 60:

```bash
SCORE=$(omen -f json score | jq '.score')
if [ "$(echo "$SCORE < 60" | bc)" -eq 1 ]; then
  echo "Repository score $SCORE is below threshold (60)"
  exit 1
fi
```

## What to Explore Next

- [How It Works](./how-it-works.md) -- architecture and design of the analysis pipeline
- [Repository Score](./repository-score.md) -- how the composite score is calculated
- [Semantic Search](./semantic-search.md) -- natural language code discovery
