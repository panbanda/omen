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
omen -f json benchmark --baseline /path/to/baseline \
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

## Rust workspaces, `cfg` twins, and a wrapped `self`

The index reads each `Cargo.toml` file as TOML data. It does not run `cargo`,
and it does not read the lockfile.

### Crates

The manifest gives the root file of each crate of a package:

- The library is the file of `[lib] path`, or `src/lib.rs`.
- Each `path` of `[[bin]]`, `[[example]]`, `[[test]]`, and `[[bench]]` is a root.
- `src/main.rs`, the build script, and each file in `src/bin`, `examples`,
  `tests`, and `benches` is a root. A key such as `autobins = false` stops this
  rule for its directory.

A root file has its child modules in its own directory, for each name of the
root file. Thus `mod command;` in `src/util.rs` is `src/command.rs` when
`[lib] path` is `src/util.rs`. `crate::` in a module names that root file.

The first name of a path can be the name of a crate. The manifest nearest to
the file tells which crate it is:

| The manifest has | The name refers to |
| --- | --- |
| The name as its own library | The library of the package, also from its binary |
| A dependency with `path` | The library of the package in that directory |
| A dependency with `workspace = true` | The entry of `[workspace.dependencies]` in the workspace manifest |
| A dependency that a `[patch]` table gives a `path` | The library of the package in that directory |
| A dependency without a directory | A crate outside the repository: the status is `external` |
| A dependency whose package has a different `[lib] name` | The library of that package, by the name of the library |
| No dependency with the name | No crate: the call has no edge from this rule |

The key of a dependency is the name in the source, also when `package` gives a
different package. A name has `_` for `-`. A crate of the repository that the
manifest does not make a dependency is not a target, also when it has the name.
A dependency whose directory is not in the index gives no edge and is not
`external`.

One case uses the name of a package as evidence. A dependency has
`workspace = true`, and the index does not have the `[workspace.dependencies]`
table. Then the one package of the repository with that library name is the
target. Two packages with the name give no target.

### Paths, types, and modules

- A path that names a module file or a crate comes before a type that has the
  last name of the path. A module in a file of a different crate can have the
  name of a crate, and it is not the target.
- `kit::Type::method` resolves only when the module file of `kit` defines
  `Type`, or exports it again with `pub use`, also with a wildcard.
- A type can come from a wildcard import. The module of the import defines the
  type, or exports it again, as a prelude does.
- The first name of a path can be a module in the file. Thus
  `use perf as backend;` and `use perf::reset;` resolve to `mod perf { ... }`
  of the file. A crate or a function with the same name is not the target.
- Two imports of one type in one file give one type.

### `cfg` twins

Rust permits two definitions of one function when `cfg` attributes make sure
that no build has the two. The index keeps the two symbols: `extract_zip` and
`extract_zip~2`. A call that finds only such definitions has an edge to each of
them. The status is `unique_candidate`, and the basis is the basis of the rule
that found them.

The definitions are twins only when all these conditions are true:

- They are in one file, with one name, one kind, and one container path.
- Each definition has a `#[cfg(...)]` attribute, on itself or on an item that
  contains it.
- No two definitions have the same `cfg` text.
- The `impl` blocks that contain them have the same text before the body.

The last condition keeps `impl A for T` and `impl B for T` apart, because one
build can have the two. A call that finds a twin and a different definition
stays `ambiguous`. A candidate that has only the name gets no edge from this rule.

An entry point `main` with twins is one entry point. Its symbol is the first
twin in the file, and a path starts only at that twin. A sink that only a
later twin reaches has no path from this entry point.

### A local name that holds `self` in a wrapper

A method call on a local name resolves as a call on `self` when the first
value of the name is one of these texts:

- `Arc::new(self)` and `Rc::new(self)`
- `Arc::clone(self)`, `Arc::clone(&self)`, and the same forms of `Rc`
- `self.clone()`

The basis is `enclosing_type`. The rule reads the text of the `let` statement.
It does not infer a type. The rule does not apply in these two cases:

- The file can see a type of the repository that has the name of the wrapper.
- The enclosing type has no member with the name of the call.

### Measured results

The baseline is `main` at `0d77c1d`. The two binaries are release builds. The
numbers are the `resolution_summary` of `omen -f json impact`.

| Repository | Call sites | Edges before | Edges after |
| --- | ---: | ---: | ---: |
| zed-industries/zed at `df5cf7b` | 706,551 | 108,911 | 209,543 |
| omen at `0d77c1d` | 47,233 | 12,784 | 12,878 |
| BurntSushi/ripgrep at `3fce3b5` | 14,836 | 6,362 | 6,421 |

These repositories are development data, not a held-out evaluation.

- A comparison of the edges of each call site on zed gave 111,283 new edges
  and 10,165 edges that the baseline had and that are not there now.
- 9,753 of these were edges to a test fixture of a lint tool that defines a
  second `gpui` crate. The baseline selected its types by name.
- 401 went to a method of a trait implementation in a different crate. The
  call now has an edge to the method of the type itself.
- 8 had a receiver of a different type, and the call has no edge now.
- 3 were correct edges, and the call has no edge now. Two calls of
  `TextSummary::from` find two `From` implementations. One call finds a
  function that has a twin in a different file.
- On omen and on ripgrep, no edge of the baseline is absent.
- An author read 40 of the 80,408 new edges between crates of zed: each
  2,010th edge in sorted order. The target was correct for each of the 40.
  This is a sample, and it is not a measurement of all edges.
- The entry points of zed go from 162 to 156, because six `main` functions
  have a twin. One path from a twin of `main` to a sink in that twin is absent.
- The paired benchmark gave no change on `corpus.json` (28 cases),
  `scoped.json` (8 cases), and `real.json` (42 cases). These corpora have no
  workspace case, thus they show no gain.
- The processor time of `omen impact` on zed was 21.3 s before and 22.5 s
  after: the median of seven runs on a host with other work.

### Limits

- A path through a module in the file after the name of a crate, such as
  `util::inner::run` with `mod inner { ... }` in the root file, does not resolve.
- A wildcard import of such a module, as `use kit::prelude::*` with
  `mod prelude { ... }` in the root file, gives each name that the root file
  exports. The index does not limit the names to the names of that module.
- An import and a crate can have one name in the two namespaces of Rust, as
  `use languages::language;` and the crate `language` have. A path that starts
  with that name then reads the import, and it can stay without a target.
- `#[path = "..."]` on a `mod` item, `extern crate ... as`, and a re-export of
  a crate with a new name are not read.
- A twin in a different file, as `#[cfg(unix)] mod imp;` with two files gives,
  is not a twin for the index.
- A trait method call and a function value are not a part of this change.
