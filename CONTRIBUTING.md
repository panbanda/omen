# Contributing to Omen

Thank you for your interest in contributing to Omen. This document provides guidelines and information for contributors.

## Code of Conduct

Be respectful and constructive in all interactions. We welcome contributors of all experience levels.

## Getting Started

### Prerequisites

- Go 1.25 or later
- [Task](https://taskfile.dev/) (task runner)
- Git

### Setup

```bash
# Clone the repository
git clone https://github.com/panbanda/omen.git
cd omen

# Install git hooks
task setup

# Build the project
go build -o omen ./cmd/omen

# Run tests
task test
```

## Development Workflow

### Before Making Changes

1. Check existing [issues](https://github.com/panbanda/omen/issues) and [pull requests](https://github.com/panbanda/omen/pulls) to avoid duplicate work
2. For significant changes, open an issue first to discuss the approach
3. Fork the repository and create a feature branch from `main`

### Making Changes

1. Write code following the patterns established in the codebase
2. Add tests for new functionality
3. Run the full test suite: `task test`
4. Run the linter: `task lint`
5. Format and tidy: `task tidy`

### Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `chore`

Examples:
- `feat(analyzer): add support for Kotlin`
- `fix(parser): handle empty files gracefully`
- `docs: update installation instructions`

### Pull Requests

1. Keep PRs focused on a single concern
2. Update documentation if needed
3. Ensure all CI checks pass
4. Request review from maintainers

## Project Structure

```
omen/
├── cmd/omen/          # CLI entry point
├── pkg/               # Public API (stable)
│   ├── parser/        # Tree-sitter wrapper
│   ├── models/        # Data structures
│   └── config/        # Configuration loading
├── internal/          # Implementation details
│   ├── analyzer/      # Analysis implementations
│   ├── fileproc/      # Concurrent file processing
│   ├── scanner/       # File discovery
│   ├── cache/         # Result caching
│   ├── output/        # Output formatting
│   ├── mcpserver/     # MCP server
│   └── vcs/           # Git operations
└── skills/            # Claude Code skills
```

## Adding New Features

### Adding a New Analyzer

1. Create a new file in `internal/analyzer/` following the existing pattern:
   - Constructor: `NewXxxAnalyzer()`
   - Single file: `AnalyzeFile(path)`
   - Project-wide: `AnalyzeProject(files)` or `AnalyzeProjectWithProgress(files, progressFn)`
   - Cleanup: `Close()`

2. Add result types to `pkg/models/`

3. Register the CLI command in `cmd/omen/`

4. Add MCP tool registration in `internal/mcpserver/`

5. Write tests covering the new functionality

### Adding Language Support

1. Update `pkg/parser/parser.go`:
   - Add to `DetectLanguage()` extension mapping
   - Add to `GetTreeSitterLanguage()`
   - Add to `getFunctionNodeTypes()` and `getClassNodeTypes()`

2. Add tree-sitter query files if needed in `internal/analyzer/featureflags/queries/<lang>/`

3. Add test files in the new language

### Adding Feature Flag Provider Support

1. Create query file: `internal/analyzer/featureflags/queries/<lang>/<provider>.scm`
2. Query must capture `@flag_key` for the flag identifier
3. Add provider detection logic
4. Add tests with sample code

## Testing

```bash
# Run all tests
task test

# Run specific test
go test ./internal/analyzer -run TestComplexity

# Run with verbose output
go test -v ./internal/analyzer/...

# Run with race detection
go test -race ./...
```

## Reporting Issues

When reporting bugs, include:

- Omen version (`omen --version`)
- Go version (`go version`)
- Operating system
- Steps to reproduce
- Expected vs actual behavior
- Relevant log output

For feature requests, describe the use case and expected behavior.

## Questions

Open a [GitHub Discussion](https://github.com/panbanda/omen/discussions) for questions or ideas that aren't bug reports or feature requests.

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (see [LICENSE](LICENSE)).
