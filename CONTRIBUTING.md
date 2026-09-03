# Contributing to Omen

Thanks for helping with Omen. This page covers how the project is built, tested, and released so a change lands without surprises.

## Prerequisites

- Rust 1.92 or later (`rustup update stable`)
- Git
- [lefthook](https://github.com/evilmartians/lefthook) for the pre-push hooks (optional but recommended)

## Setup

```bash
git clone https://github.com/panbanda/omen.git
cd omen
lefthook install          # runs fmt, clippy and cargo check before every push
cargo build
cargo test --all-features
```

`cargo run -- --help` lists the analyzers. `cargo run -- -p . score` runs the health score against this repository.

## Development workflow

1. Check open [issues](https://github.com/panbanda/omen/issues) and [pull requests](https://github.com/panbanda/omen/pulls) so you don't duplicate work.
2. For a large change, open an issue first and describe the approach.
3. Branch from `main` in your fork.
4. Keep the pull request to one concern.

Before pushing, the hooks run the same checks CI does:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-features
cargo test --all-features
```

CI also enforces 80% line coverage with `cargo llvm-cov --all-features` and runs `cargo audit`.

## Commit messages

Releases are cut by release-please from commit messages, so use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `chore`. A `feat` bumps the minor version, a `fix` bumps the patch version, and a `!` after the type marks a breaking change.

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
├── analyzers/     One module per analyzer (complexity, satd, churn, hotspot, ...)
├── parser/        Tree-sitter wrapper and language detection
├── mcp/           MCP server exposing the analyzers as tools
├── output/        Table, JSON and markdown formatting
├── report/        HTML health report
├── score/         Repository health score
├── semantic/      Embeddings and semantic search
├── git/           Git history, churn and ownership
├── config/        omen.toml loading
└── core/          Shared types and file processing
plugins/           Claude Code plugins (omen-development, omen-reporting)
action.yml         GitHub Action
```

## Adding an analyzer

1. Add a module under `src/analyzers/` following an existing one such as `stubs.rs` or `satd.rs`, and register it in `src/analyzers/mod.rs`.
2. Add the subcommand in `src/cli/`.
3. Register an MCP tool for it in `src/mcp/`.
4. Add tests next to the code and, when the analyzer reads source files, fixtures in more than one language.
5. Document the command in `README.md`.

## Adding a language

1. Add the tree-sitter grammar crate to `Cargo.toml`.
2. Extend language detection and the node-type mappings in `src/parser/`.
3. Add tree-sitter queries where an analyzer needs them (for example the feature-flag queries).
4. Add fixture files in the new language and extend the analyzer tests.

## Testing

```bash
cargo test --all-features                   # everything
cargo test --all-features complexity        # tests matching a name
cargo test --all-features -- --nocapture    # show output
cargo llvm-cov --all-features               # coverage report
```

## Reporting issues

Include the Omen version (`omen --version`), operating system, the command you ran, what you expected, what happened, and a small repository or snippet that reproduces it when possible. For feature requests, describe the use case.

## License

Contributions are licensed under the project's Apache-2.0 license (see [LICENSE](LICENSE)).
