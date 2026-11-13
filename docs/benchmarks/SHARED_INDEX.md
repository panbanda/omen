# Shared index: cost and call edges

This page gives descriptive numbers from one host. They are not a guarantee
and not a gate. The host ran other builds during the session, thus a
difference of a few milliseconds has no meaning.

The old index is `analyzers::repomap::build_index`. The shared index is
`index::build::CodeIndex`. The commands `symbol` and `impact` and the MCP tools
`get_symbol` and `impact` read the shared index.

## Repositories

Each repository is at a pinned revision. The measurements read the files. They
do not build or run the code of a repository.

| Repository | Language | Revision |
| --- | --- | --- |
| rack/rack | Ruby | a9833c8f3bd6b6d1e0ab35de00a1f1a16b5095f5 |
| colinhacks/zod | TypeScript | 59bbc03e10c636b9eb3c393dfeb552819774ec21 |
| BurntSushi/ripgrep | Rust | 3fce3b5bb0236da2df6d99672afb8a719642eca7 |
| spf13/cobra | Go | adbc8813901bba65827259daa8e22ff94ec1f30e |
| BerriAI/litellm | Python | 0a78ce9cc59dcc91e184f5897146de1492fc84a9 |

Java, C#, C, C++, and PHP have no repository in this list. Their rules have
unit tests and fixture labels, and no measurement on a real repository.

## Cost of one index build

The test `cost_of_one_index` in `tests/index_differential.rs` builds one index
in one process. Each number is from five processes of the release test binary:
the median and the maximum of the wall time, and the peak resident memory.

| Repository | Files | Old index | Shared index |
| --- | --- | --- | --- |
| litellm | 13,297 | 6,687 ms / 7,093 ms, 2.17 GB | 3,528 ms / 3,762 ms, 2.94 GB |
| zod | 521 | 71 ms / 79 ms, 47 MB | 86 ms / 89 ms, 94 MB |
| ripgrep | 110 | 42 ms / 47 ms, 48 MB | 88 ms / 89 ms, 58 MB |

On litellm the shared index takes 0.53 of the time of the old index and 1.35 of
its memory. On ripgrep it takes 2.1 of the time. That time is the parse and the
extraction of one file of 7,700 lines, which one thread does.

The litellm number includes 4,123 files that are not Python. The default file
set takes in the build output of the repository, for example
`litellm/proxy/_experimental/out/_next/static/chunks/*.js`.

## Call edges of the two indexes

The test `call_edges_of_the_two_indexes` compares the edges. One row is one
caller and one callee name.

| Repository | Same | Only shared index | Only old index | Different target |
| --- | --- | --- | --- | --- |
| rack | 307 | 259 | 276 | 4 |
| zod | 1,451 | 1,089 | 487 | 56 |
| ripgrep | 1,200 | 2,542 | 6 | 6 |
| cobra | 1,311 | 203 | 190 | 0 |
| litellm, Python files | 175,949 | 169,853 | 4,641 | 70 |

A person read the first rows of each class in sorted order and compared them
with the source. Most rows that only the old index has are a name of the
standard library that the old index gave to a function of the repository with
the same name: `next`, `partial`, `err.Error()`, `drop`.

## Labelled cases

`tests/fixtures/quality/real.json` has 42 cases on these repositories. Each
label comes from the source at the pinned revision, not from the output of an
index. Checked against the reports of the comparison test, by the file and the
name of each callee:

| Index | Cases that pass |
| --- | --- |
| Old index | 13 of 42 |
| Shared index | 42 of 42 |

This check did not go through `omen benchmark`. That command needs a git
checkout of each repository at its revision, and the measurements above used
archives of the same revisions.
