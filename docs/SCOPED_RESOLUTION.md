# Scoped resolution: development proof

This PR advances lexical scope, import identity, and resolution provenance. It
does not implement a compiler type checker or a complete alias/re-export graph.

## Changes

- Function-body byte ranges constrain nested definitions to their enclosing
  function, including same-line source. The nearest visible nested definition
  shadows a file-level candidate.
- Top-level Rust `use ... as ...` and TS/JS named import aliases preserve both
  local and original names. Nested Rust aliases are deliberately not promoted
  to file-wide bindings.
- Relative TS/JS imports resolve against the caller directory, not arbitrary
  files with the same basename. Source `.ts` for runtime `.js` imports and
  directory `index` files are recognized.
- `self`/`this` evidence retains the receiver and narrows candidates by the
  enclosing type name extracted by the parser. This is syntactic evidence,
  not runtime dispatch proof.
- New `lexical_scope`, `imported_alias`, and `enclosing_type` resolution bases
  disclose why a candidate was selected. Existing uncertainty is retained.

## Reproduction and results

The four initial regression tests failed on the parent before implementation.
Seven expanded tests now check shadowing, sibling invisibility, alias targets,
relative path collisions, enclosing-type hints, nested shadowing of imports,
and exclusion of block-local aliases. Location assertions distinguish same-name
targets; the edge benchmark alone cannot do that.

Eight source-reviewed development fixtures in `tests/fixtures/quality/scoped.json`
are scored with the existing harness (three repetitions, alternating order):

| Direct edges | Baseline | Candidate |
| --- | ---: | ---: |
| Correct | 1 | 7 |
| False | 1 | 0 |
| Missed | 6 | 0 |

Baseline is the archived `aa8aafe` binary whose hash is recorded in the previous
benchmark. PR #508 did not change its CLI graph semantics. Candidate hashes and
all case-level measurements are in `docs/benchmarks/scoped-*.json`.

```sh
cargo test --test scoped_resolution --test call_resolution
python scripts/quality_benchmark.py --baseline /path/to/baseline \
  --candidate /path/to/candidate --baseline-label aa8aafe \
  --candidate-label scoped-resolution --corpus tests/fixtures/quality/scoped.json \
  --repetitions 3 --enforce
```

Repeat with `corpus.json` and `real.json` for existing regression protection.
Tests are development evidence, not independent held-out accuracy. Arbitrary
receiver types, namespace-qualified type identity, lexical variables/parameters,
conditional rebinding, wildcard/default imports, re-exports, Go package aliases,
Ruby dynamic constants, reflection and overload/trait dispatch remain incomplete.
Signatures and graph evidence can still contain unresolved or incorrect candidates;
never interpret absent edges as proof of change safety.
