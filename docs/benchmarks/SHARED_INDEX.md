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

The litellm number includes 4,123 files that are not Python. Most of them are
real source, for example the TypeScript of the dashboard, the Rust crates, and
the tests. About 190 files are build output, for example
`litellm/proxy/_experimental/out/_next/static/chunks/*.js`. The default file
set now drops 188 of them. The rule for the `_next` directory and for the
`-bundle` name suffix drops them. The numbers above are from before this change.

## Call edges of the two indexes

The test `call_edges_of_the_two_indexes` compares the edges. One row is one
caller and one callee name.

| Repository | Same | Only shared index | Only old index | Different target |
| --- | --- | --- | --- | --- |
| rack | 307 | 259 | 276 | 4 |
| zod | 1,449 | 1,087 | 489 | 56 |
| ripgrep | 1,200 | 2,542 | 6 | 6 |
| cobra | 1,311 | 203 | 190 | 0 |
| litellm, Python files | 175,929 | 169,823 | 4,661 | 70 |

A person read the first rows of each class in sorted order and compared them
with the source. Most rows that only the old index has are a name of the
standard library that the old index gave to a function of the repository with
the same name: `next`, `partial`, `err.Error()`, `drop`.

## Calls with name-only candidates

A call for which only the name gives a definition is not an edge. It lists
the definitions with the name as candidates. `--min-basis name_only` makes an
edge of each such call that has one candidate.

| Repository | Calls with name-only candidates | Of these, with one candidate |
| --- | --- | --- |
| rack | 3,071 | 1,118 |
| zod | 9,478 | 321 |
| ripgrep | 2,336 | 905 |
| cobra | 114 | 112 |
| litellm, Python files | 62,477 | 11,000 |

## Labelled cases

`tests/fixtures/quality/real.json` has 42 cases on these repositories. Each
label comes from the source at the pinned revision, not from the output of an
index. `omen benchmark` ran the cases with two binaries:

| Binary | Cases that pass |
| --- | --- |
| Baseline: main at a233332, before the shared index | 15 of 42 |
| Candidate: this branch | 42 of 42 |

No case passes with the baseline and fails with the candidate.
[shared-index-real.json](shared-index-real.json) has the full report.

The command, with a clean git checkout of each repository at its revision in
`<real-root>/real-<name>`:

```sh
omen -f json benchmark \
  --corpus tests/fixtures/quality/real.json \
  --real-root <real-root> \
  --baseline <omen of main at a233332> \
  --candidate <omen of this branch> \
  --repetitions 2 --timeout 900
```

The baseline binary is a debug build and the candidate binary is a release
build. Thus the times in the report do not compare the two indexes. Use the
table of the cost above for the time.

The report of `symbol` does not list a definition as its own callee. Two
cobra cases have a recursive call in the source, and their labels do not
include it.
