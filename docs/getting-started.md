---
sidebar_position: 2
---

# Getting Started

## Installation

### Homebrew (macOS and Linux)

```bash
brew install panbanda/brews/omen
```

### Build from Source

Requires a Rust toolchain (1.92+).

```bash
git clone https://github.com/panbanda/omen.git
cd omen
cargo build --release
# Binary at target/release/omen
```

### Docker

```bash
docker pull ghcr.io/panbanda/omen:latest
```

To analyze the current directory:

```bash
docker run --rm -v "$(pwd):/repo" ghcr.io/panbanda/omen:latest -p /repo all
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
omen graph
omen clones
omen smells
omen deadcode
omen stubs
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

### Create a Configuration File

Omen supports project-level configuration through `omen.toml`. There is no `omen init` command to scaffold one; instead, copy the annotated example and customize it:

```bash
curl -O https://raw.githubusercontent.com/panbanda/omen/main/omen.example.toml
mv omen.example.toml omen.toml
```

If you use Claude Code, the `setup-config` skill can analyze your repository and generate an `omen.toml` with intelligent defaults for your tech stack instead. See [Configuration](./configuration.md) for the full list of accepted sections.

### Filter by Language

```bash
omen complexity --language rust
omen clones --language typescript
```

### Exclude Files From Analysis

Most analyzers accept a repeatable `-e`/`--exclude` flag for glob patterns, in addition to whatever is configured in `omen.toml`:

```bash
omen complexity -e "**/vendor/**" -e "**/*.pb.go"
```

### Pipeline Integration

All commands support minified JSON with the global `--compact` flag, which pairs well with CI logging:

```bash
omen -f json score --compact
```

A typical CI step that fails if the repository score drops below 60:

```bash
SCORE=$(omen -f json score | jq '.overall_score')
if [ "$(echo "$SCORE < 60" | bc)" -eq 1 ]; then
  echo "Repository score $SCORE is below threshold (60)"
  exit 1
fi
```

## What to Explore Next

- [Repository Score](./repository-score.md) -- how the composite score is calculated
- [Semantic Search](./semantic-search.md) -- natural language code discovery
