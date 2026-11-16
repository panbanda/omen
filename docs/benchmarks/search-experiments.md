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

## Decision

Omen adopts neither experiment. Both experiments failed the first condition of
the adoption rule. Thus we did not run `ledger_never_grows` for either one.
We did not change a constant, and we did not move a query to `retired`.

| Experiment | Queries with a worse rank | Queries that lose a hit@1 | Decision |
|---|---|---|---|
| A: documents for each symbol kind | 22 | 6 | Not adopted |
| B: graph prior, on the base | 1 | 1 | Not adopted |

The code of each experiment is in a commit that a later commit reverts. The
base `search` code is unchanged.

## Result of experiment A

24 queries got a hit in the first 10 results where the base had none, and 2
queries lost one. Thus the net change of hit@10 is 22. The queries that gained
are for types, constants, and fields that the base does not index. The gate does not let a gain pay for a loss.

These 22 queries have a worse rank:

| Query | Base rank | Rank of A |
|---|---|---|
| `cpp/n-times` | 2 | 3 |
| `csharp/n-around-square` | 4 | 6 |
| `csharp/n-distance` | 3 | 7 |
| `csharp/n-increment` | 1 | 2 |
| `csharp/n-twice-radius` | 7 | 11 |
| `csharp/n-width-height` | 1 | 2 |
| `go/c-interface-method` | 1 | 2 |
| `go/n-make-circle` | 2 | 4 |
| `go/n-upper-case` | 1 | 2 |
| `java/n-around-square` | 4 | 5 |
| `java/n-distance` | 1 | 2 |
| `java/n-increment` | 1 | 2 |
| `java/n-twice-radius` | 7 | 11 |
| `javascript/n-unit-circle` | 5 | 6 |
| `php/n-twice-radius` | 5 | 7 |
| `php/n-unit-circle` | 6 | 8 |
| `python/i-part-side` | 3 | 5 |
| `python/n-shape-name` | 2 | 3 |
| `python/n-square-from-length` | 6 | 7 |
| `python/n-unit-circle` | 3 | 5 |
| `ruby/n-unit-circle` | 7 | 9 |
| `rust/n-make-circle` | 2 | 4 |

The queries `csharp/n-twice-radius` and `java/n-twice-radius` also lose their
hit@10. The six queries `csharp/n-increment`, `csharp/n-width-height`,
`go/n-upper-case`, `go/c-interface-method`, `java/n-distance`, and
`java/n-increment` lose their hit@1.

A class document contains the lines of all its methods. Thus a class matches
many queries for its methods, and it can take a rank before the method. This
is the same cause as in `search-bm25f.md`. A second experiment that uses the
signature of a type as its document, and not all its lines, needs its own
pre-registration.

## Result of experiment B

The prior changes one query: `go/i-part-circle` falls from rank 1 to rank 2.
Its expected symbol is `Circle` or `NewCircle`, and `Area` takes rank 1
because the call graph gives `Area` a higher prior. The prior changes no other
rank. A weight of 0.1 changes the order only when two scores are near, and it
is wrong in the one case where it changes the order. The fixtures have 5 to 20
functions, so their call graphs have few edges. A real repository can give a
different result. This corpus does not measure it.

## Tables

Each cell has hit@1, hit@5, hit@10, and MRR (all results), in this order. The
column `Queries` has the number of positive queries. Each group also has one
negative query, which the column does not count. Each cell has 6 to 9 queries, so one query changes a cell by
0.11 to 0.17.

| Language | Type | Queries | Base | A | B on the base |
|---|---|---|---|---|---|
| bash | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| bash | identifier | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 0.714 / 0.714 / 0.714 / 0.714 | 0.714 / 0.714 / 0.714 / 0.714 |
| bash | natural_language | 6 | 0.167 / 0.167 / 0.167 / 0.167 | 0.167 / 0.167 / 0.167 / 0.167 | 0.167 / 0.167 / 0.167 / 0.167 |
| c | code_sketch | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 1.000 / 1.000 / 1.000 / 1.000 | 0.714 / 0.714 / 0.714 / 0.714 |
| c | identifier | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 0.857 / 0.857 / 0.857 / 0.857 | 0.714 / 0.714 / 0.714 / 0.714 |
| c | natural_language | 6 | 0.167 / 0.167 / 0.167 / 0.167 | 0.333 / 0.333 / 0.333 / 0.333 | 0.167 / 0.167 / 0.167 / 0.167 |
| cpp | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| cpp | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.875 / 0.875 / 0.875 / 0.875 | 0.750 / 0.750 / 0.750 / 0.750 |
| cpp | natural_language | 6 | 0.000 / 0.333 / 0.500 / 0.167 | 0.000 / 0.500 / 0.667 / 0.194 | 0.000 / 0.333 / 0.500 / 0.167 |
| csharp | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 1.000 / 1.000 / 1.000 / 1.000 | 0.857 / 0.857 / 0.857 / 0.857 |
| csharp | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.875 / 0.875 / 0.875 / 0.875 | 0.750 / 0.750 / 0.750 / 0.750 |
| csharp | natural_language | 6 | 0.333 / 0.667 / 0.833 / 0.454 | 0.000 / 0.333 / 0.667 / 0.233 | 0.333 / 0.667 / 0.833 / 0.454 |
| go | code_sketch | 6 | 1.000 / 1.000 / 1.000 / 1.000 | 0.833 / 1.000 / 1.000 / 0.917 | 1.000 / 1.000 / 1.000 / 1.000 |
| go | identifier | 8 | 0.875 / 0.875 / 0.875 / 0.875 | 0.875 / 0.875 / 0.875 / 0.875 | 0.750 / 0.875 / 0.875 / 0.812 |
| go | natural_language | 6 | 0.333 / 0.667 / 0.667 / 0.500 | 0.167 / 0.667 / 0.667 / 0.375 | 0.333 / 0.667 / 0.667 / 0.500 |
| java | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 1.000 / 1.000 / 1.000 / 1.000 | 0.857 / 0.857 / 0.857 / 0.857 |
| java | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.750 / 0.875 / 0.875 / 0.812 | 0.750 / 0.750 / 0.750 / 0.750 |
| java | natural_language | 6 | 0.333 / 0.500 / 0.667 / 0.399 | 0.167 / 0.667 / 0.667 / 0.382 | 0.333 / 0.500 / 0.667 / 0.399 |
| javascript | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| javascript | identifier | 9 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| javascript | natural_language | 7 | 0.286 / 0.714 / 0.714 / 0.433 | 0.286 / 0.571 / 0.714 / 0.429 | 0.286 / 0.714 / 0.714 / 0.433 |
| jsx | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| jsx | identifier | 8 | 0.875 / 0.875 / 0.875 / 0.875 | 0.875 / 0.875 / 0.875 / 0.875 | 0.875 / 0.875 / 0.875 / 0.875 |
| jsx | natural_language | 6 | 0.333 / 0.667 / 0.667 / 0.431 | 0.333 / 0.667 / 0.667 / 0.431 | 0.333 / 0.667 / 0.667 / 0.431 |
| php | code_sketch | 8 | 0.875 / 0.875 / 0.875 / 0.875 | 1.000 / 1.000 / 1.000 / 1.000 | 0.875 / 0.875 / 0.875 / 0.875 |
| php | identifier | 9 | 0.889 / 0.889 / 0.889 / 0.889 | 1.000 / 1.000 / 1.000 / 1.000 | 0.889 / 0.889 / 0.889 / 0.889 |
| php | natural_language | 7 | 0.143 / 0.286 / 0.429 / 0.195 | 0.286 / 0.286 / 0.571 / 0.324 | 0.143 / 0.286 / 0.429 / 0.195 |
| python | code_sketch | 6 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| python | identifier | 7 | 0.714 / 0.857 / 0.857 / 0.762 | 0.714 / 0.857 / 1.000 / 0.761 | 0.714 / 0.857 / 0.857 / 0.762 |
| python | natural_language | 6 | 0.000 / 0.333 / 0.500 / 0.167 | 0.000 / 0.333 / 0.500 / 0.113 | 0.000 / 0.333 / 0.500 / 0.167 |
| ruby | code_sketch | 8 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| ruby | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.875 / 0.875 / 0.875 / 0.875 | 0.750 / 0.750 / 0.750 / 0.750 |
| ruby | natural_language | 7 | 0.143 / 0.143 / 0.429 / 0.184 | 0.143 / 0.143 / 0.429 / 0.179 | 0.143 / 0.143 / 0.429 / 0.184 |
| rust | code_sketch | 6 | 0.833 / 0.833 / 0.833 / 0.833 | 1.000 / 1.000 / 1.000 / 1.000 | 0.833 / 0.833 / 0.833 / 0.833 |
| rust | identifier | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 0.857 / 0.857 / 0.857 / 0.857 | 0.714 / 0.714 / 0.714 / 0.714 |
| rust | natural_language | 6 | 0.000 / 0.167 / 0.167 / 0.083 | 0.167 / 0.333 / 0.333 / 0.208 | 0.000 / 0.167 / 0.167 / 0.083 |
| tsx | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 1.000 / 1.000 / 1.000 / 1.000 | 0.857 / 0.857 / 0.857 / 0.857 |
| tsx | identifier | 9 | 0.667 / 0.667 / 0.667 / 0.667 | 0.778 / 0.778 / 0.778 / 0.778 | 0.667 / 0.667 / 0.667 / 0.667 |
| tsx | natural_language | 6 | 0.333 / 0.500 / 0.500 / 0.389 | 0.333 / 0.667 / 0.667 / 0.444 | 0.333 / 0.500 / 0.500 / 0.389 |
| typescript | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 1.000 / 1.000 / 1.000 / 1.000 | 0.857 / 0.857 / 0.857 / 0.857 |
| typescript | identifier | 8 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| typescript | natural_language | 7 | 0.143 / 0.571 / 0.571 / 0.310 | 0.286 / 0.714 / 0.714 / 0.452 | 0.143 / 0.571 / 0.571 / 0.310 |

## How to reproduce

The code of experiment A is in commit `c4471ad`. The code of experiment B is in
commit `5264585`. B in that commit goes on the base, not on A.

1. Run `git cherry-pick <commit>`.
2. Run `cargo test --test retrieval -- --nocapture`.
3. The test prints the table. Then it lists each query that differs from the
   ledger and from the rank baseline.

For A, the unit tests of `chunking.rs` that count chunks also fail. They count
the documents of types as chunks. We did not correct them, because A is not
adopted.

## Experiment C: the identifier tokenizer for TF-IDF

This section is written before any code and before any run of the experiment.
The text of "Pre-registration of experiment C" does not change after the first
run. The results go in a later section.

### Pre-registration of experiment C

The base is commit `2d9bb49`. The data is the retrieval corpus
`tests/fixtures/retrieval/corpus.json`, with the ledger and the rank baseline
of the base. We do not move a query to `retired`. We do not tune a constant
after the first run.

The reason: `search` uses a private tokenizer. It finds the runs of `\w+`,
changes them to lowercase, and adds bigrams. The identifier tokenizer in
`src/semantic/tokenize.rs` divides `parseFile` and `parse_file` into `parse`
and `file`, and it keeps the whole identifier. We measured it only inside
BM25F, and BM25F was not adopted. We did not measure it with TF-IDF alone.

The change:

- The engine tokenizes each document and each query with
  `semantic::tokenize::tokenize`. The private tokenizer is deleted.
- Bigrams are the pairs of adjacent tokens in the list that `tokenize` gives.
  The list has the parts of each identifier, then its whole form, as the
  tokenizer defines. A bigram is the two tokens, joined by one space. This is
  the same rule as on the base, applied to the new list.
- The vocabulary limit, the IDF formula, the sublinear TF, the L2
  normalization, the order of the results, the documents, and the chunking
  stay the same.
- The change has no flag.

The metrics are recall@1, recall@5, recall@10, and MRR, for each language and
each query type, as in the tables above. The report never pools them.

The adoption rule is the rule of "Adoption rule" above, without change:

- `cargo test --test retrieval` passes with the ledger and the rank baseline of
  the base, unchanged.
- `ledger_never_grows` passes against a copy of `tests/fixtures` of the base
  (made with `git archive`), with no new accepted entry.
- No query has a rank worse than its rank on the base.

A gain does not pay for a loss. If experiment C is not adopted, a later commit
reverts the code, and this file keeps the full tables and the rejection.

### Result of experiment C

The result of experiment C is on the base `2d9bb49`. The code is in the
commit `6896825`. The commit that follows it reverts it.

Experiment C fails the first and the third condition of the adoption rule.
`cargo test --test retrieval` fails, and four queries have a worse rank. Thus
we did not run `ledger_never_grows`. We did not write the rank baseline or the
ledger again. We did not change a constant, and we did not move a query to
`retired`.

| Measure | Queries |
|---|---|
| Queries with a worse rank | 4 |
| Queries that lose a hit@1 | 3 |
| Queries that gain a rank where the base has none | 12 |
| Queries with a better rank, where the base has a rank | 5 |

These 4 queries have a worse rank:

| Query | Base rank | Rank of C |
|---|---|---|
| `cpp/c-push-back` | 1 | 2 |
| `jsx/i-counter` | 1 | 2 |
| `php/n-twice-radius` | 5 | 6 |
| `tsx/i-counter` | 1 | 2 |

The queries `cpp/c-push-back`, `jsx/i-counter`, and `tsx/i-counter` lose their
hit@1. The query `php/n-twice-radius` loses its hit@5 and keeps its hit@10.

The 12 queries that gain a hit are the queries `bash/i-part-local`,
`bash/i-part-receiver`, `c/i-part-area`, `cpp/i-part-overload`,
`csharp/i-part-real`, `go/i-part-here`, `java/i-part-real`, `jsx/i-part-click`,
`ruby/i-part-parens`, `rust/i-part-here`, `rust/n-count-map`, and
`tsx/i-part-click`. Eleven of them are `i-part` queries. These queries use a
part of an identifier, such as `real` for `realValue`. The new tokenizer gives
that part as a token, and the base tokenizer does not.

The 5 queries `javascript/n-times-three`, `jsx/n-add-to-count`,
`jsx/n-times-three`, `python/i-part-side`, and `typescript/n-times-three` have
a better rank.

In `jsx/i-counter` and `tsx/i-counter`, the function `App` takes rank 1 before
`Counter`. The new tokens and the bigrams of tokens change the weights of the
terms, and `App` now scores higher. We did not find the cause of the change
in `cpp/c-push-back` and `php/n-twice-radius`. The gain of `jsx/i-part-click`
and `tsx/i-part-click` keeps the hit@1 of the `identifier` type in `jsx` and
`tsx` at the same value as on the base. The cell hides the loss of
`i-counter`. The gate looks at each query, so it sees the loss.

The cells that are lower than on the base are `cpp` `code_sketch` (hit@1 and
MRR) and `php` `natural_language` (hit@5 and MRR). The gate does not let a gain
pay for a loss, so experiment C is not adopted.

The tokenizer gives a real gain for queries with a part of an identifier. A
second experiment, for example new tokens for the name of a symbol only, needs
its own pre-registration.

### Tables of experiment C

Each cell has hit@1, hit@5, hit@10, and MRR (all results), in this order.

| Language | Type | Queries | Base | C |
|---|---|---|---|---|
| bash | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| bash | identifier | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 1.000 / 1.000 / 1.000 / 1.000 |
| bash | natural_language | 6 | 0.167 / 0.167 / 0.167 / 0.167 | 0.167 / 0.167 / 0.167 / 0.167 |
| c | code_sketch | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 0.714 / 0.714 / 0.714 / 0.714 |
| c | identifier | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 0.857 / 0.857 / 0.857 / 0.857 |
| c | natural_language | 6 | 0.167 / 0.167 / 0.167 / 0.167 | 0.167 / 0.167 / 0.167 / 0.167 |
| cpp | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 0.857 / 1.000 / 1.000 / 0.929 |
| cpp | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.875 / 0.875 / 0.875 / 0.875 |
| cpp | natural_language | 6 | 0.000 / 0.333 / 0.500 / 0.167 | 0.000 / 0.333 / 0.500 / 0.167 |
| csharp | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 0.857 / 0.857 / 0.857 / 0.857 |
| csharp | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.875 / 0.875 / 0.875 / 0.875 |
| csharp | natural_language | 6 | 0.333 / 0.667 / 0.833 / 0.454 | 0.333 / 0.667 / 0.833 / 0.454 |
| go | code_sketch | 6 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| go | identifier | 8 | 0.875 / 0.875 / 0.875 / 0.875 | 1.000 / 1.000 / 1.000 / 1.000 |
| go | natural_language | 6 | 0.333 / 0.667 / 0.667 / 0.500 | 0.333 / 0.667 / 0.667 / 0.500 |
| java | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 0.857 / 0.857 / 0.857 / 0.857 |
| java | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.875 / 0.875 / 0.875 / 0.875 |
| java | natural_language | 6 | 0.333 / 0.500 / 0.667 / 0.399 | 0.333 / 0.500 / 0.667 / 0.399 |
| javascript | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| javascript | identifier | 9 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| javascript | natural_language | 7 | 0.286 / 0.714 / 0.714 / 0.433 | 0.286 / 0.714 / 0.714 / 0.457 |
| jsx | code_sketch | 7 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| jsx | identifier | 8 | 0.875 / 0.875 / 0.875 / 0.875 | 0.875 / 1.000 / 1.000 / 0.938 |
| jsx | natural_language | 6 | 0.333 / 0.667 / 0.667 / 0.431 | 0.333 / 0.667 / 0.667 / 0.472 |
| php | code_sketch | 8 | 0.875 / 0.875 / 0.875 / 0.875 | 0.875 / 0.875 / 0.875 / 0.875 |
| php | identifier | 9 | 0.889 / 0.889 / 0.889 / 0.889 | 0.889 / 0.889 / 0.889 / 0.889 |
| php | natural_language | 7 | 0.143 / 0.286 / 0.429 / 0.195 | 0.143 / 0.143 / 0.429 / 0.190 |
| python | code_sketch | 6 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| python | identifier | 7 | 0.714 / 0.857 / 0.857 / 0.762 | 0.714 / 0.857 / 0.857 / 0.786 |
| python | natural_language | 6 | 0.000 / 0.333 / 0.500 / 0.167 | 0.000 / 0.333 / 0.500 / 0.167 |
| ruby | code_sketch | 8 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| ruby | identifier | 8 | 0.750 / 0.750 / 0.750 / 0.750 | 0.875 / 0.875 / 0.875 / 0.875 |
| ruby | natural_language | 7 | 0.143 / 0.143 / 0.429 / 0.184 | 0.143 / 0.143 / 0.429 / 0.184 |
| rust | code_sketch | 6 | 0.833 / 0.833 / 0.833 / 0.833 | 0.833 / 0.833 / 0.833 / 0.833 |
| rust | identifier | 7 | 0.714 / 0.714 / 0.714 / 0.714 | 0.714 / 0.857 / 0.857 / 0.786 |
| rust | natural_language | 6 | 0.000 / 0.167 / 0.167 / 0.083 | 0.167 / 0.333 / 0.333 / 0.250 |
| tsx | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 0.857 / 0.857 / 0.857 / 0.857 |
| tsx | identifier | 9 | 0.667 / 0.667 / 0.667 / 0.667 | 0.667 / 0.778 / 0.778 / 0.722 |
| tsx | natural_language | 6 | 0.333 / 0.500 / 0.500 / 0.389 | 0.333 / 0.500 / 0.500 / 0.389 |
| typescript | code_sketch | 7 | 0.857 / 0.857 / 0.857 / 0.857 | 0.857 / 0.857 / 0.857 / 0.857 |
| typescript | identifier | 8 | 1.000 / 1.000 / 1.000 / 1.000 | 1.000 / 1.000 / 1.000 / 1.000 |
| typescript | natural_language | 7 | 0.143 / 0.571 / 0.571 / 0.310 | 0.143 / 0.571 / 0.571 / 0.333 |

To reproduce, run `git cherry-pick 6896825`, and then run `cargo test --test retrieval -- --nocapture`.
