# Search: BM25F against TF-IDF

## Decision

Omen keeps TF-IDF for `search`. Omen does not adopt BM25F.

The plan adopts a new scorer only if the retrieval gate passes. BM25F did not
pass. It loses queries that TF-IDF wins, in each of the three document sets
that we measured. We did not change the weights after the first run. We did
not move the lost queries to `retired`. Both actions would hide the loss.

## What we measured

The BM25F scorer gives a score from the text of five fields of a document:
`name`, `location` (the container and the path), `signature`, `doc`, and `body`.
Each symbol of the shared index of each kind gives one document for each chunk
of its body. Each file gives one more document.

We chose these constants before the first run:

| Constant | Value |
|---|---|
| `K1` | 1.2 |
| Weight of `name` | 5.0 |
| Weight of `location` | 1.0 |
| Weight of `signature` | 2.0 |
| Weight of `doc` | 2.0 |
| Weight of `body` | 1.0 |
| Length normalization `b` of `name` | 0.3 |
| Length normalization `b` of `location` | 0.5 |
| Length normalization `b` of `signature`, `doc`, and `body` | 0.75 |

The score of a document is the sum, over the terms of the query in ascending
order, of `idf * weighted_tf / (K1 + weighted_tf)`. `idf` is
`ln(1 + (N - df + 0.5) / (df + 0.5))`. The scorer divides the sum by the
largest sum that a document can have for the terms of the query, so the score
is in the range 0 to 1. The tokenizer is version 3. A body chunk has at most
500 characters, and a chunk ends at a line end.

The corpus is `tests/fixtures/retrieval/corpus.json`. The base is the TF-IDF
engine of the base commit, with the ledger `expected_failures.json` and the
rank baseline `ranks.json`.

We measured three document sets:

1. All symbol kinds and the file documents.
2. All symbol kinds, without the file documents.
3. Functions, methods, and constructors only, without the file documents.

## Result

| Document set | Queries worse than TF-IDF at hit@1 | Ranks worse | Ranks better |
|---|---|---|---|
| 1 | 14 | 26 | 45 |
| 2 | 14 | 26 | 45 |
| 3 | 7 | 10 | 31 |

BM25F also passes 73 (sets 1 and 2) and 43 (set 3) ledger entries that
TF-IDF fails. Most of these entries are queries for a symbol kind that
TF-IDF did not index. This is a gain, but the ledger rule does not permit a
gain to pay for a loss. A query of the base corpus must not get worse.

### Queries that get worse (sets 1 and 2)

All of them are at hit@1. The best acceptable symbol has rank 2 or 3, and
TF-IDF gives it rank 1.

| Query | Expected | BM25F rank 1 |
|---|---|---|
| `cpp/c-push-back` | `main.cpp:external` | `other.cpp:push_back` |
| `csharp/n-increment` | `Program.cs:Next` | `Program.cs:count` |
| `csharp/n-width-height` | `Shapes.cs:Total` | `Shapes.cs:Size` |
| `csharp/c-side-squared` | `Shapes.cs:Area` | `Shapes.cs:side` |
| `java/n-distance` | `Point.java:norm` | `Point.java:Point` |
| `java/n-increment` | `Main.java:next` | `Main.java:count` |
| `jsx/c-render-app` | `main.jsx:render` | `components.jsx:App` |
| `php/c-radius-times-two` | `shapes.php:diameter` | `shapes.php:radius` |
| `python/c-side-squared` | `shapes.py:area` | `shapes.py:side` |
| `ruby/c-side-squared` | `shapes.rb:area` | `shapes.rb:initialize` |
| `ruby/c-block` | `main.rb:callback` | `main.rb:twice_local` |
| `tsx/c-render-app` | `main.tsx:render` | `components.tsx:App` |
| `tsx/c-side-squared-twice` | `shapes.ts:doubled` | `shapes.ts:side` |
| `typescript/c-side-squared-twice` | `shapes.ts:doubled` | `shapes.ts:side` |

A field, a class, or a different function that shares terms with the query
takes rank 1 in most of these cases.

### Queries that get worse (set 3)

Seven queries stay worse when only callable symbols are documents. All seven
are `code_sketch` queries: `cpp/c-push-back`, `csharp/c-side-squared`,
`jsx/c-render-app`, `tsx/c-render-app`, `php/c-radius-times-two`,
`ruby/c-side-squared`, and `ruby/c-block`. Thus the scorer causes a part of
the loss, and the added symbol kinds cause the rest.

### hit@1 by language and query type

Columns: TF-IDF, then BM25F set 1, set 2, and set 3.

| Language | Type | TF-IDF | Set 1 | Set 2 | Set 3 |
|---|---|---|---|---|---|
| bash | code_sketch | 1.000 | 0.857 | 1.000 | 1.000 |
| bash | identifier | 0.714 | 1.000 | 1.000 | 1.000 |
| bash | natural_language | 0.167 | 0.167 | 0.167 | 0.167 |
| c | code_sketch | 0.714 | 1.000 | 1.000 | 0.714 |
| c | identifier | 0.714 | 1.000 | 1.000 | 0.857 |
| c | natural_language | 0.167 | 0.167 | 0.167 | 0.167 |
| cpp | code_sketch | 0.571 | 0.714 | 0.714 | 0.714 |
| cpp | identifier | 0.500 | 1.000 | 1.000 | 0.875 |
| cpp | natural_language | 0.000 | 0.167 | 0.167 | 0.167 |
| csharp | code_sketch | 0.857 | 0.857 | 0.857 | 0.714 |
| csharp | identifier | 0.750 | 1.000 | 1.000 | 0.875 |
| csharp | natural_language | 0.333 | 0.000 | 0.000 | 0.333 |
| go | code_sketch | 1.000 | 1.000 | 1.000 | 1.000 |
| go | identifier | 0.875 | 1.000 | 1.000 | 1.000 |
| go | natural_language | 0.333 | 0.333 | 0.333 | 0.500 |
| java | code_sketch | 0.714 | 0.857 | 0.857 | 0.714 |
| java | identifier | 0.750 | 1.000 | 1.000 | 0.875 |
| java | natural_language | 0.333 | 0.000 | 0.000 | 0.333 |
| javascript | code_sketch | 1.000 | 1.000 | 1.000 | 1.000 |
| javascript | identifier | 1.000 | 1.000 | 1.000 | 1.000 |
| javascript | natural_language | 0.286 | 0.429 | 0.429 | 0.429 |
| jsx | code_sketch | 1.000 | 0.857 | 0.857 | 0.857 |
| jsx | identifier | 0.875 | 1.000 | 1.000 | 1.000 |
| jsx | natural_language | 0.333 | 0.500 | 0.500 | 0.500 |
| php | code_sketch | 0.875 | 0.750 | 0.750 | 0.750 |
| php | identifier | 0.889 | 1.000 | 1.000 | 0.889 |
| php | natural_language | 0.143 | 0.143 | 0.143 | 0.143 |
| python | code_sketch | 1.000 | 0.833 | 0.833 | 1.000 |
| python | identifier | 0.714 | 0.857 | 0.857 | 0.857 |
| python | natural_language | 0.000 | 0.000 | 0.000 | 0.000 |
| ruby | code_sketch | 1.000 | 0.750 | 0.750 | 0.750 |
| ruby | identifier | 0.750 | 1.000 | 1.000 | 0.875 |
| ruby | natural_language | 0.143 | 0.143 | 0.143 | 0.143 |
| rust | code_sketch | 0.833 | 0.833 | 0.833 | 0.833 |
| rust | identifier | 0.714 | 0.857 | 0.857 | 0.714 |
| rust | natural_language | 0.000 | 0.167 | 0.167 | 0.167 |
| tsx | code_sketch | 0.857 | 0.714 | 0.714 | 0.714 |
| tsx | identifier | 0.667 | 1.000 | 1.000 | 0.778 |
| tsx | natural_language | 0.333 | 0.333 | 0.333 | 0.333 |
| typescript | code_sketch | 0.857 | 0.857 | 0.857 | 0.857 |
| typescript | identifier | 1.000 | 1.000 | 1.000 | 1.000 |
| typescript | natural_language | 0.143 | 0.286 | 0.286 | 0.286 |

Each cell has 6 to 9 queries. A change of one query changes a cell by 0.11 to
0.17. Do not read a small difference as a result.

## Reading of the result

BM25F is better for identifier queries, mostly because it indexes more symbol
kinds. It is worse for `code_sketch` queries in the typed languages and in
Ruby, because a field or a class that shares terms takes rank 1.

We can try two changes: a prior by symbol kind, and new weights. Each one
needs a new pre-registered measurement on a corpus that is not the one that
showed the loss.

## How to reproduce

The scorer and the documents are in commit `347e2c0`, on the branch
`feat/omen-5-c`. They are not in the main line.

1. Run `git cherry-pick 347e2c0`.
2. Run `cargo test --test retrieval_bm25f -- --ignored --nocapture`. This is
   set 1. The test prints the table, the ledger differences, and the rank
   differences.
3. Set `MEASURE_NO_FILE_DOCS=1` for set 2.
4. Set `MEASURE_ONLY_CALLABLE=1` for set 3.

The TF-IDF table comes from `cargo test --test retrieval -- --nocapture`.
