# Contributing to Omen

Thank you for your help with Omen. This page explains how the project is built, tested, and released. Use it to make a change that merges without problems.

## Prerequisites

- Rust 1.92 or later (`rustup update stable`)
- Git
- [lefthook](https://github.com/evilmartians/lefthook) for the pre-push hooks (optional, but we recommend it)

## Setup

```bash
git clone https://github.com/panbanda/omen.git
cd omen
cargo build
cargo test --all-features
```

Optional: `lefthook install` adds a pre-push hook. The hook runs `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo check --all-features`.

`cargo run -- --help` lists the analyzers. `cargo run -- -p . score` runs the health score on this repository.

## Development workflow

1. Read the open [issues](https://github.com/panbanda/omen/issues) and [pull requests](https://github.com/panbanda/omen/pulls). Do not repeat work that exists.
2. For a large change, open an issue first. Describe the approach in the issue.
3. Make a branch from `main` in your fork.
4. Keep the pull request to one concern.

Before you open the pull request, run the checks that CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

CI also enforces 80% line coverage with `cargo llvm-cov --all-features`, and it runs `cargo audit`. The optional lefthook hook checks the format, clippy, and `cargo check`. It does not run the tests.

## Commit messages

Release-please makes the releases from the commit messages. Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

The types are `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, and `chore`. A `feat` increases the minor version. A `fix` increases the patch version. A `!` after the type marks a breaking change.

Examples:

- `feat(analyzers): add Kotlin support to complexity`
- `fix(parser): handle empty files`
- `docs: update installation instructions`

## Project layout

```
src/
├── main.rs        CLI entry point
├── lib.rs         Public library surface (the omen crate)
├── cli/           Argument parsing and subcommand dispatch
├── commands/      One handler for each command
├── index/         Shared index of symbols, imports, and calls
├── changeset/     Changes between two commits and the pull request comment
├── task_context/  Bounded context for one definition
├── eval/          Scoring of paired coding-agent outcome records
├── benchmark/     Paired quality benchmark of two omen binaries
├── analyzers/     One module per analyzer (complexity, satd, churn, hotspot, ...)
├── parser/        Tree-sitter wrapper and language detection
├── mcp/           MCP server exposing the analyzers as tools
├── output/        Table, JSON and markdown formatting
├── report/        HTML health report
├── score/         Repository health score
├── semantic/      TF-IDF index and search
├── git/           Git history, churn and ownership
├── config/        omen.toml loading
└── core/          Shared types and file processing
plugins/           Claude Code plugins (omen-development, omen-reporting)
action.yml         GitHub Action
```

## Add an analyzer

1. Add a module under `src/analyzers/`. Follow an existing module, such as `stubs.rs` or `satd.rs`. Register it in `src/analyzers/mod.rs`.
2. Add the subcommand in `src/cli/`.
3. Register an MCP tool for the analyzer in `src/mcp/`.
4. Add tests next to the code. If the analyzer reads source files, add fixtures in more than one language.
5. Document the command in `README.md`.

## Add a language

1. Add the tree-sitter grammar crate to `Cargo.toml`.
2. Extend the language detection and the node-type mappings in `src/parser/`.
3. Add tree-sitter queries where an analyzer needs them. The feature-flag queries are an example.
4. Add fixture files in the new language. Extend the analyzer tests.

## Tests

```bash
cargo test --all-features                   # everything
cargo test --all-features complexity        # tests matching a name
cargo test --all-features -- --nocapture    # show output
cargo llvm-cov --all-features               # coverage report
```

## Report an issue

Include these items in the report:

- The Omen version (`omen --version`).
- The operating system.
- The command that you ran.
- The result that you expected.
- The result that you got.
- If possible, a small repository or snippet that shows the problem.

For a feature request, describe the use case.

## License

Your contributions have the license of the project, Apache-2.0 (see [LICENSE](LICENSE)).
