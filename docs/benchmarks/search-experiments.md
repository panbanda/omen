# Search: two experiments on the TF-IDF engine

This file records two experiments before the first run. Omen keeps TF-IDF for
`search` (see `search-bm25f.md`). Neither experiment was measured on that
engine. The text of "Pre-registration" does not change after the first run.

## Pre-registration

### Base and data

The base is commit `3624771`. The data is the retrieval corpus
`tests/fixtures/retrieval/corpus.json`, with the ledger and the rank baseline
of the base. We do not move a query to `retired`. We do not tune a constant
after the first run.

### Adoption rule

An experiment is adopted only if all of these are true:

- `cargo test --test retrieval` passes with the ledger and the rank baseline of
  the base, unchanged.
- `ledger_never_grows` passes against a copy of `tests/fixtures` of the base
  (made with `git archive`), with no new accepted entry.
- No query has a rank worse than its rank on the base.

A gain does not pay for a loss. The report gives recall@1, recall@5, recall@10,
and MRR for each language and each query type. It never pools them. The report
gives the full table if the experiment is not adopted.

### Experiment A: documents for each kind of symbol

Today the engine has one document for each function, method, and constructor.
Experiment A adds one document for each symbol of these kinds: class, struct,
interface, trait, enum, type alias, constant, field, module, and macro. The
kind `variable` stays out.

The change:

- A new document has the same form as the document of a function: the path, the
  name, and the lines of the definition, split at 500 characters as before.
  The lines of a class include the lines of its methods.
- The `symbol_type` of the document is the kind of the symbol. A function
  keeps `function`.
- A new document has no complexity values.
- Each constant of the scorer, the tokenizer, and the order of the results stay
  the same.
- The index version in `meta` goes from 1 to 2.

### Experiment B: a bounded graph prior

B goes on top of A if A is adopted. If A is not adopted, B goes on the base.

The change:

- The prior of a symbol is its PageRank in the call graph of the shared index,
  divided by the largest PageRank of the index. The prior is in the range 0 to
  1. A symbol that is not a node of the graph has prior 0.
- The graph has one node for each function, method, and constructor. An edge is
  a call with the basis `DEFAULT_MIN_BASIS` or a stronger basis. The damping is
  0.85, the limit is 100 iterations, and the tolerance is 1e-6. These are the
  values of `repomap`.
- The weight `w` is 0.1. The new score is `score * (1 + w * prior)`.
- The order stays total: the score, high to low, by `total_cmp`, and then the
  existing key of the symbol. The same input gives the same output.
- A document that has score 0 stays out of the results.
- The index version in `meta` goes up by one, if the stored data changes.

## Results

Not yet measured.
