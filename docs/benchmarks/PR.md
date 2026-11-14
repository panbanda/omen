# feat(analysis): expose call evidence and reproducible improvement gates

Omen currently turns ambiguous names into arbitrary graph edges. It also attributes the calls in nested function bodies to the enclosing function. Coding agents can follow these false relationships when they navigate code or estimate the impact of an edit.

This rebuild exposes call candidates that have source locations, with the status unique, ambiguous, or unresolved. It keeps duplicate query matches in the same file. It restricts name candidates to language families. It stops nested bodies from leaking calls into their parents. The `symbol` and `impact` responses explain the limits of these name-based edges.

A review of real code also found that omen did not recognize the Rust call `self.method()`. This change adds that form, with regression coverage. It keeps generic field receivers out of the new handling. It does not implement compiler binding or receiver-type binding.

The improvement standard and the paired Python harness keep these items: raw results, hashes, errors, validated warmups, alternating run order, and per-case regression gates.

## Evidence

- 28 development cases across Rust, TypeScript, Ruby, and Go:
  - Incorrect edges: 17 to 4.
  - Correct edges: 8 to 17.
  - Missed edges: 16 to 7.
  - Precision: 32% to 81%.
  - Recall: 33.3% to 70.8%.
  - No case regresses, so the semantic gate passes.
- The response size increases from 9,710 bytes to 15,669 bytes across the fixed cases. This resource regression is disclosed separately.
- Six partially labeled real-code queries on pinned revisions of Rack, Zod, ripgrep, and Cobra: two improve, four do not change, and none regress. The fully satisfied reviewed cases increase from 4/6 to 6/6.
- Both final experiments use 30 paired repetitions. This change makes no claim about model editing, actual tokens, memory, held-out generalization, or release performance.

## Validation

- 2,281 Rust tests pass, and six are ignored. Six Python scorer tests pass. The format check passes.
- Cross-language regression coverage: 14 direct-call variants and 13 applicable nested-scope variants. Also tests for duplicates, determinism, and evidence ownership.
- Strict Clippy fails on existing diagnostics in files that do not differ from the baseline. The coverage was not measured locally.

See `docs/benchmarks/README.md` and the raw JSON reports for reproduction steps and limits.

The semantic gate and the partial real-repository non-regression gates pass. These items remain for follow-up work: alias handling, lexical parameter binding, response budgeting, and held-out model-editing studies. This change is not proof of improvement in every dimension.
