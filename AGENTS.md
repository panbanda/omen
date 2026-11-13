# AGENTS.md

Guidance for AI agents working in this repository. See `CLAUDE.md` for build
commands, architecture, and the TDD workflow.

## Write in Simplified Technical English

Write all prose in this repository to ASD-STE100, Simplified Technical English.
This applies to documentation, decision records, code comments, doc comments,
commit messages, pull request text, CLI help, MCP tool descriptions, and error
messages.

The specification is free on request from ASD (<https://www.asd-ste100.org>).
It is copyrighted, so do not copy its dictionary into this repository. Obey
these rules, which summarize its writing rules:

- Use one word for one meaning. Do not use a different word for the same thing.
- Use approved words in their approved part of speech. If you are not sure that
  a word is approved, use a simpler word.
- Software terms are technical names. Examples: `call graph`, `tree-sitter`,
  `parse`, `symbol`, `merge`. Use them as nouns or as the verbs that the
  software field uses. Put identifiers in backticks.
- Write a maximum of 20 words in each instruction and 25 words in each
  descriptive sentence.
- Give one instruction in each sentence. Give one topic in each paragraph, and
  write a maximum of six sentences in each paragraph.
- Use the active voice. Use the imperative for instructions.
- Use the simple present, simple past, and simple future tenses. Do not use a
  perfect or progressive tense.
- Do not use the `-ing` form of a verb, unless it is part of a technical name.
- Use the articles `the`, `a`, and `an`. Do not write in telegraphic style.
- Write a maximum of three nouns together.
- Put a condition before the instruction that it controls.
- Use a vertical list for three or more items.
- Do not use contractions, idioms, or figures of speech.

Three items are exempt:

- Text that omen must read or write exactly, such as fixture source, expected
  output, and quoted messages from other tools.
- Text that a format controls, such as the type prefix of a Conventional Commit.
- `CHANGELOG.md`, which release-please generates.

When you change a file, make its prose obey these rules in the lines that you
change. Do not rewrite other lines in the same change.

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
