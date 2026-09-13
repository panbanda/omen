# feat(analysis): expose call evidence and reproducible improvement gates

Omen currently turns ambiguous names into arbitrary graph edges and attributes
calls in nested function bodies to the enclosing function. Coding agents can
follow those false relationships when navigating code or estimating edit impact.

This rebuild exposes source-located call candidates and unique/ambiguous/unresolved
status, preserves same-file duplicate query matches, restricts name candidates to
language families, and stops nested bodies from leaking calls into their parents.
Symbol and impact responses explain the limits of these name-based edges.

Real-code inspection also found that Rust `self.method()` was not recognized.
This adds that form with regression coverage while keeping generic field receivers
out of the new handling. It does not implement compiler or receiver-type binding.

The improvement standard and paired Python harness retain raw results, hashes,
errors, validated warmups, alternating run order and per-case regression gates.

## Evidence

- 28 development cases across Rust, TypeScript, Ruby and Go: incorrect edges
  17 → 4, correct edges 8 → 8, missed edges 16 → 16. Precision 32% → 66.7%.
- Four import cases regress; the aggregate semantic non-regression gate fails.
- Response bytes increase 9,710 → 15,650 across the fixed cases.
- Six partially labeled real-code queries on pinned Rack, Zod, ripgrep and Cobra
  revisions: one improvement, five unchanged, zero regressions. Fully satisfied
  reviewed cases increase 4/6 → 5/6. Rack receiver binding remains wrong.
- Both final experiments use 30 paired repetitions. No model editing, actual
  tokens, memory, held-out generalization or release performance claim is made.

## Validation

- 2,279 Rust tests pass; six ignored. Six Python scorer tests pass. Formatting passes.
- Cross-language regression coverage: 14 direct-call variants and 13 applicable
  nested-scope variants, plus duplicates, determinism and evidence ownership.
- Strict Clippy fails on existing diagnostics in files unchanged from baseline;
  coverage was not measured locally.

See `docs/benchmarks/README.md` and the raw JSON reports for reproduction and limits.

Keep this PR in draft: import/alias binding and response budgeting are required
before claiming overall non-regression. The proposed change is measured progress,
not proof that the approach is better in every dimension.
