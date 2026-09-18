# AGENTS.md

Guidance for AI agents working in this repository. See `CLAUDE.md` for build
commands, architecture, and the TDD workflow.

## Do not add Python

Omen is a Rust project. Do not add Python to this repository.

This covers shipped code, helper scripts, benchmarks, evaluation harnesses, and
one-off automation. Tooling that would once have been a script in `scripts/`
belongs in Rust instead:

- A module under `src/`, exposed as a CLI subcommand (see `src/eval/` and the
  `eval` command for a worked example).
- Tests as ordinary `#[cfg(test)]` modules or targets under `tests/`, run by
  `cargo test` — not a separate interpreter invocation in CI.

If a task appears to call for a Python script, write it in Rust or ask first.
Do not add a Python step to a GitHub Actions workflow.

The sole exception is parser test-fixture source: files under `tests/fixtures/`
that exist to be *parsed* by omen's tree-sitter grammars (for example
`tests/fixtures/sample.py`). These are input data, never tooling, and are never
executed.
