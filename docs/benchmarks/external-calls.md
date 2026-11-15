# External calls

This document gives the result of the rules that mark a call as `External`. A call is
`External` when the source shows that its definition is outside the repository. The
rules need evidence. They never use a name alone.

## Rules

A rule runs only for a call that has no candidate. The status of a call that has a
candidate does not change. The rules give one of three kinds of evidence.

- **Import**: an import from outside the repository binds the first name of the
  qualifier or of the receiver. In Rust, a path that starts with `std`, `core`, `alloc`,
  `proc_macro`, or `test`, with a prelude type, or with the name of a dependency in
  the nearest `Cargo.toml`, also counts. A dependency with a `path` key is a crate of
  the repository and does not count. A value that only contains the word `path`, such
  as `package = "pathfinding"`, has no such key.
- **Builtin**: the call has no receiver and no qualifier. Its name is in the list of
  builtins of the language, or a header or a library that the file imports provides
  it. No symbol of the repository, no local name, and no import has the name.
- **Declared type**: the receiver is a local name with a declared type. The type is a
  builtin type, or an import from outside the repository binds it. No repository type
  has the name.

A rule does not apply in these cases:

- The receiver has no recorded binding and no import binds it, and a function or a
  method of the repository has the name of the call.
- A file has a wildcard import of a repository module. The module can bind any name.
- The repository binds the name again: a C or C++ `#define`, a Bash `alias`, an
  assignment to `globalThis`, `window`, `global`, or `self`, or a Ruby `class << Name`.
  A parameter or a variable with one of these names is not the global object.
- A PHP function, method, or class name matches a repository name when the case is
  ignored. A property or a constant has a case-sensitive name and does not hide a function.

- A repository symbol has the name of a builtin, a class, or a namespace.
- A local name that the call sees has the name.
- A member of the repository can extend the external type. Examples are a trait
  implementation for a type from outside the repository and a member of an interface.
- A member of the repository has the name of the call, and the rule uses a declared
  type. A pattern can hide the local name, and the index does not model this.
- A Rust path has a `.`. Such a path is a chain of values, not a path of a crate.
- A Rust path starts with a lower case name that no manifest declares. A macro can
  bind the name.

The lists are in `src/index/resolve/external.rs`. `LIST_VERSION` is 1. A test checks that
each list is sorted, has no duplicate, and is small. A test for each language checks
a builtin, a call that a repository symbol hides, and a local name that hides the call.

The `External` status has no basis. The JSON shape does not change.

## Where each rule applies

| Language | Import | Builtin | Declared type |
|---|---|---|---|
| Bash | not applicable: no import binds a name | builtin commands | not applicable: no declared types |
| C | not applicable: no namespace | functions of `stdio.h`, `stdlib.h`, `string.h`, `ctype.h`, `math.h` | not applicable: no methods |
| C++ | `std` qualifier | the same headers, and the `c` headers | `std` types |
| C# | `using` of `System` namespaces | not applicable: no free function | `string`, and types of the `System` namespaces |
| Go | package import | predeclared functions | type of an imported package |
| Java | import, and `java.lang` classes | not applicable: a call without a receiver names a member | imported types, and `java.lang` types |
| JavaScript | import and `require` | global functions | not applicable: no declared types |
| JSX | import and `require` | global functions | not applicable: no declared types |
| PHP | `use` | internal functions | internal classes |
| Python | import | builtin functions and exceptions | builtin types |
| Ruby | `require` of a library | Kernel methods | not applicable: no declared types |
| Rust | `std`, prelude types, crates | `Some`, `Ok`, `Err`, `drop` | prelude types and imported types |
| TSX | import and `require` | global functions | global types |
| TypeScript | import and `require` | global functions | global types |

## Method

The base is `origin/main` at 1bb8d2d. The measurement ran the shared index on six
repositories before and after the change. Omen only read the files. Nobody built or
ran code of the repositories other than omen. The pre-registration was written before
the change. It is in the scratchpad of the work, not in the repository.

The omen repository is the copy at 108be1a that the SCIP spike used, so that
positions match the SCIP index.

A check on the pairs of references gave these results:

- The `UniqueCandidate` references and their candidates are the same before and
  after, for all six repositories.
- Almost all changes of status are `Unresolved` to `External`. The section "Review
  changes" gives the other changes.

## Counts by status

The columns `Unique` and `Ambiguous` give the counts before the change.

| Repository | Language | Unique | Ambiguous | Unresolved before | Unresolved after | External before | External after | New External |
|---|---|---|---|---|---|---|---|---|
| omen | Bash | 24 | 12 | 41 | 16 | 0 | 25 | 25 |
| omen | C | 28 | 17 | 16 | 10 | 0 | 6 | 6 |
| omen | CSharp | 72 | 2 | 20 | 16 | 4 | 8 | 4 |
| omen | Cpp | 56 | 15 | 20 | 11 | 0 | 9 | 9 |
| omen | Go | 34 | 2 | 29 | 24 | 27 | 32 | 5 |
| omen | Java | 67 | 3 | 33 | 21 | 12 | 24 | 12 |
| omen | JavaScript | 58 | 4 | 34 | 22 | 25 | 37 | 12 |
| omen | Jsx | 48 | 4 | 15 | 7 | 25 | 33 | 8 |
| omen | Php | 64 | 24 | 24 | 14 | 5 | 15 | 10 |
| omen | Python | 49 | 12 | 33 | 30 | 38 | 41 | 3 |
| omen | Ruby | 55 | 10 | 145 | 127 | 0 | 18 | 18 |
| omen | Rust | 12006 | 5104 | 25437 | 21982 | 1532 | 4987 | 3455 |
| omen | Tsx | 49 | 5 | 15 | 7 | 25 | 33 | 8 |
| omen | TypeScript | 61 | 5 | 26 | 16 | 25 | 35 | 10 |
| ripgrep | Bash | 6 | 0 | 29 | 8 | 0 | 21 | 21 |
| ripgrep | Ruby | 0 | 0 | 22 | 22 | 0 | 0 | 0 |
| ripgrep | Rust | 6356 | 2414 | 5541 | 4822 | 468 | 1187 | 719 |
| cobra | Go | 2157 | 117 | 1487 | 529 | 647 | 1605 | 958 |
| zod | Bash | 0 | 0 | 5 | 0 | 0 | 5 | 5 |
| zod | JavaScript | 10 | 0 | 7 | 4 | 12 | 15 | 3 |
| zod | Tsx | 7 | 7 | 100 | 81 | 43 | 62 | 19 |
| zod | TypeScript | 14460 | 11996 | 16045 | 13934 | 11406 | 13512 | 2106 |
| rack | Ruby | 3369 | 3303 | 14049 | 12855 | 0 | 1194 | 1194 |
| litellm | Bash | 365 | 0 | 1334 | 1124 | 0 | 210 | 210 |
| litellm | Go | 1559 | 14 | 4617 | 1832 | 1817 | 4602 | 2785 |
| litellm | JavaScript | 21 | 11 | 57 | 54 | 18 | 21 | 3 |
| litellm | Python | 417836 | 77090 | 361667 | 264371 | 172062 | 269358 | 97296 |
| litellm | Ruby | 0 | 0 | 19 | 19 | 0 | 0 | 0 |
| litellm | Rust | 16347 | 10874 | 43790 | 39482 | 5502 | 9810 | 4308 |
| litellm | Tsx | 18011 | 2087 | 72267 | 68587 | 78066 | 81681 | 3615 |
| litellm | TypeScript | 12093 | 1346 | 23465 | 20940 | 19789 | 22156 | 2367 |

## Precision

The precision is the share of the new `External` references that SCIP also calls
external. A reference is external for SCIP when its occurrence names a symbol with no
definition in the index. The join of a reference to a SCIP occurrence uses the exact file
and range. A reference with no occurrence is a join failure. It is not counted as a
correct reference.

| Repository | Language | New External | Joined | SCIP external | SCIP in repository | Join failures | Precision |
|---|---|---|---|---|---|---|---|
| omen | Rust | 3455 | 3441 | 3441 | 0 | 14 | 100% |
| ripgrep | Rust | 719 | 685 | 685 | 0 | 34 | 100% |
| cobra | Go | 958 | 675 | 675 | 0 | 283 | 100% |
| zod | TypeScript | 2106 | 2076 | 2076 | 0 | 30 | 100% |

Every precision is above the limit of 98%. No rule is withdrawn.

The first version of the rules had these errors. A SCIP check found each error, and a
test now covers each one.

- A trait name as the start of a path, such as `Default::default()`, can name an
  implementation in the repository. The rule no longer uses trait names.
- A path of a repository trait that is implemented for a type from outside the
  repository, such as `io::Error::from(x)`, can name the implementation.
- A pattern can hide a local name that has a declared type, such as
  `if let Some(prev) = prev`.
- A chain of values, such as `m.status.is_ready()`, is not a path of a crate.

## Languages that SCIP did not measure

The spike has no SCIP index for Bash, C, C++, C#, Java, JavaScript, JSX, PHP, Python,
Ruby, and TSX. Their precision is not measured. The rules for these languages are not
withdrawn, because no measurement shows a precision below 98%. This is not a proof of
precision.

A person read random samples of the new `External` references in the repositories. The
samples have 25 references of rack (Ruby), 25 of litellm (Python), 20 of litellm (Go),
12 of litellm (Bash), and 12 of each other language of omen. The reader found no
reference that is wrong. A sample is not a measurement of precision.

## Limits

- The rules read the dependencies of `Cargo.toml` only. They do not read `package.json`
  or `go.mod`. A JavaScript import that no file binds is not external.
- Most `Unresolved` references of omen and ripgrep are calls on an expression or on a
  local name with no declared type, such as `x.unwrap()`. No rule has evidence for
  them. They stay `Unresolved`.
- The deadcode analyzer keeps a definition alive when an `Unresolved` call or an
  `External` call has its name. A wrong `External` status cannot make a definition dead.
- A repository symbol with the name of a builtin stops the rule in the whole
  repository, also in a file that cannot see the symbol. This choice loses some
  references and gives no wrong reference.
- The truth labels for the new cases are development data. The `status` field of a call
  label checks the status. The truth, quality, and retrieval ledgers stay empty.

## Review changes

A review found that the first version gave `External` in cases that a repository
definition can receive. The version in this document has these changes:

- The deadcode analyzer treats an `External` call as it treats an `Unresolved` call.
- The index records the parameters of ECMAScript patterns, of Java lambdas without a
  type, and of C# lambdas without a type, as local names. The name of the single
  parameter of an arrow function or a lambda is visible in the body of that function only.
- The rules abstain in the cases that the section "Rules" lists. The count of
  `External` references fell, mostly for Rust and TypeScript. The precision did not fall.
- Five zod references and 230 litellm references changed from `Unresolved` to
  `Ambiguous`, and seven litellm references changed from `Ambiguous` to `Unresolved`.
  The new local names cause these changes. No edge changed.
