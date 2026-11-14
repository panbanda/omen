# Memory of the shared index

This page records the peak memory of the commands that build the shared index.
It gives the causes of the memory use, the changes, and the results.

## Result

The table shows the peak resident set size (RSS) and the wall time. The repository
is litellm (13,294 files). The command is `omen -f json <command>`.

| Command | Before: RSS (MiB) | Before: time (s) | After: RSS (MiB) | After: time (s) |
| --- | ---: | ---: | ---: | ---: |
| `symbol get_llm_provider` | 12,985 | 8.44 | 2,408 | 5.42 |
| `impact get_llm_provider` | 12,983 | 8.67 | 2,429 | 5.33 |
| `deadcode` | 14,221 | 18.27 | 3,938 | 12.39 |
| `repomap` | 2,976 | 3.43 | 1,915 | 3.46 |
| `all` | 16,492 | 34.31 | 5,002 | 26.78 |

The small repositories use less memory also.

| Repository and command | Before: RSS (MiB) | After: RSS (MiB) |
| --- | ---: | ---: |
| zod, `symbol parse` | 254 | 105 |
| zod, `impact parse` | 254 | 103 |
| ripgrep, `symbol search` | 136 | 74 |
| ripgrep, `impact search` | 135 | 76 |

The output of each command is the same, byte for byte. The one exception is the
field `generated_at` of `repomap`, which holds the time of the run.

The wall time did not get worse. It is lower in each row. The machine had a high
load in all runs, thus the times have a large error. Use the times only to see
that the change does not make the commands slower.

### Baseline before omen 5.0 part 3

I made the first measure on the commit `c73bcb5`, before the security analysis ran
in `symbol` and `impact`. The load average at the start was 3.48, 4.07, 4.21.

| Repository and command | RSS (MiB) | Time (s) |
| --- | ---: | ---: |
| litellm, `symbol get_llm_provider` | 4,285 | 12.75 |
| litellm, `impact get_llm_provider` | 4,300 | 3.91 |
| litellm, `deadcode` | 9,311 | 16.07 |
| litellm, `repomap` | 2,888 | 3.55 |
| litellm, `all` | 10,665 | 28.27 |
| zod, `symbol parse` | 109 | 0.09 |
| ripgrep, `symbol search` | 76 | 0.11 |

The first row has a time of 12.75 s because the first run of a new binary stalled.
With the changes of this page, the same commit gave 2,106 MiB and 3.50 s for the
first row.

## Method

- The binary `before` is the commit `38f87d4` (main, after the parts 1 to 3 of
  omen 5.0). The binary `after` is the same commit with the changes of this page.
- Both are release builds. Each row is the median of five runs, or of three runs
  for `deadcode`, `repomap`, and `all`. The first launch of a new binary is not
  in the numbers, because macOS can stall it.
- `/usr/bin/time -l` gives the peak RSS (`maximum resident set size`).
- The data is the checkout of litellm that the benchmark corpus holds. Omen only
  reads it. The corpus is not built and not run.
- The machine load (`uptime`, load average for 1, 5, and 15 minutes) at the start and
  at the end of each table is below.

| Table | Start | End |
| --- | --- | --- |
| Before | 15.12, 10.96, 10.81 | 24.44, 15.36, 12.64 |
| After | 12.44, 9.82, 8.87 | 21.60, 15.97, 11.69 |

To find the causes, a temporary global allocator counted the live bytes of the Rust
heap at each phase of the build. A temporary probe cleared one field of all files
at a time and read the difference. The tree-sitter library allocates the syntax
trees with the C allocator, and the counter does not see them. The difference
between the RSS and the Rust heap is the size of the trees and of the allocator
overhead. The probe is not in the repository.

The probe slows a run, because each allocation changes a shared counter. Use the
probe only for sizes. Do not use it for times.

## Causes

The numbers are from `symbol get_llm_provider` on litellm. The facts of the index:

- 13,109 files with 341,596 symbols
- 1,342,124 references
- 5,811,156 candidate keys, and the keys are 500 MB of text
- 69,361 references with more than one candidate, and a maximum of 3,529
- 838,534 local names and 1,958,872 arguments

### Cause 1: each candidate key was a copy of a string

A reference held its candidates as `Vec<String>`. A reference held the key of
its enclosing symbol and its file path as `String` also. A key can have 100 bytes.
The 5.8 million candidates were 5.8 million allocations. The resolution step added
about 600 MiB. The measure before this change, on the commit `c73bcb5`, was:

| Phase | Live heap (MiB) |
| --- | ---: |
| After extraction | 1,229 |
| After resolution of the references | 1,854 |
| After the view | 3,490 |

### Cause 2: the view copied the location of each candidate

For each call site, `CallView` held a `CallEvidence`. The `CallEvidence` held a
`SymbolLocation` for the caller and one for each candidate. Each location has two
`String` values. The view needed 1,484 MiB for these copies. The commands read
the evidence of one symbol only.

### Cause 3: the security analysis kept all syntax trees

Since omen 5.0 part 3, the command `symbol` also runs the security analysis. The
analysis parsed all files and kept all trees at one time. It also made a copy of the
facts of all files. On litellm the Rust heap peak was 2.9 GiB, but the RSS was 8.7 GiB.
The syntax trees are the difference of about 5.8 GiB. The `deadcode` command did
the same, and it kept the trees for the entry points.

### What stays

After the changes, the extraction facts are the largest part of the live heap, at
1,009 MiB. The probe cleared one field of all files at a time:

| Field | Size (MiB) |
| --- | ---: |
| `references` | 285 |
| `symbols` | 176 |
| `reference_facts` (without `arguments`) | 161 |
| `locals` | 154 |
| `imports` | 85 |
| `link_facts` | 80 |
| `arguments` of the references | 63 |

These fields hold names that repeat in many files. An interner for the names is the
next step. It changes many types, and it is not a part of this change.

## Changes

1. `SharedStr` (`src/index/model.rs`) is a string that clones without a copy. It
   is serialized as a plain string. `Symbol::key`, `Reference::file`,
   `Reference::enclosing`, and the candidate lists of `Resolution` use it. A
   reference now holds a pointer to the key of the symbol. It does not hold a copy.
2. `CallView` keeps each call site with the positions of the definitions. It makes
   a `CallEvidence` when a caller asks for it with `calls_of` or `call_resolutions`.
3. The `deadcode` analyzer uses `SharedStr` for the keys, and it moves the per-file
   results into the global tables. It does not copy them.
4. The security analysis (`src/index/security/analysis.rs`) parses each file in
   the thread that uses it. It finds the sinks, the suppression comments, and the
   entry points with that tree, and it drops the tree. When an entry point rule
   needs the tree of a different file, `OnDemandTrees` parses that file and keeps it.
5. The `deadcode` analyzer does the same. It keeps only the trees of Rust files,
   because the resolution of Rust paths needs them.

If a file has no tree, the analysis uses the old path that keeps all trees. The old
path gives the same result as the streaming path when every file has a tree. A file
has no tree only when the file cannot be read or parsed a second time.

## Tests

- `test_references_share_the_text_of_the_keys_and_the_path` (build) shows that a
  candidate, an enclosing key, and a path share the text of the symbol key.
- `test_shared_string_clone_shares_the_text` and
  `test_shared_string_is_a_plain_string_in_json` (model) pin the type.
- `test_stored_call_holds_positions_and_no_locations` and
  `test_all_calls_are_the_calls_of_each_definition_in_order` (view) pin the
  compact call sites.
- `one_tree_at_a_time_gives_the_findings_of_all_trees_at_once` and
  `a_file_without_a_tree_sends_the_analysis_to_the_slow_path` (security analysis)
  show that the two paths give the same sinks, entry points, and comments.
- The truth, parity, and snapshot tests did not change.

## Not changed

`ViewCache::load_with` (MCP server) keeps the bytes of all source files and of the
manifest files, so that a view always has the content that its hashes name. On
litellm this is 291 MB. The `symbol` and `impact` commands of the CLI do not use
it. The header re-parse of C++ files reads a file a second time, and thus a
change to this behavior needs a design change.
