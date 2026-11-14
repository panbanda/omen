# Ground-truth fixtures

Each case is a directory: `<language>/<case>/`. It contains source files and a
`labels.json` file. `cargo test --test truth` compares the facts that omen
reports with the labels.

These labels are development data. An author wrote them and examined them. They
are not evidence of accuracy on code that the author did not see.

## Rules for a case

- Write source that the compiler or interpreter of the language accepts. Record
  in `oracle` how you made sure.
- Omen does not run the source.
- Use the directory name that `Language` has in JSON, for example `csharp`.
- Label each category in `src/benchmark/truth.rs`, or add a `not_applicable`
  entry with a reason.
- Omen reports one result for each callee name in a caller. Thus do not label
  two calls with the same callee name in one caller.

## Rules for a label

- A symbol has a `file`, a `name`, and a `start_line`. A call refers to a symbol
  by its `label`. Thus a label stays correct when the symbol ID format changes.
- `container` is the nearest type or function that contains the definition.
- `exported` is `true` only when the syntax makes the definition a part of the
  public interface of its package.
  A `protected` member is not exported. A nested function is exported only
  when the language makes it global, as Ruby, PHP, and Bash do.
- A declaration without a body, such as a C prototype, is a symbol. A call
  binds to the definition, not to the declaration.
- `expected` lists each definition to which the call binds. An empty list means
  that omen must not report an edge.
- A call through a local variable binds to a closure when the function assigns
  that closure to the variable one time. A call through a parameter binds to
  no definition.
- `candidates_required` lists definitions that omen must offer as candidates.
  The harness measures candidates and edges separately.
- An import `path` is the path in the source, without quotation marks. For a
  wildcard import, it is the path of the module.
- `names` lists each name that the import binds. `alias` is the local name when
  the local name is different.
- `required_file` is the file to which the import resolves. Use `null` for a
  dependency that is not in the case.
- With `closed_world`, a symbol or import that omen reports without a label is
  a failure.

## Sink labels

A `sinks` label states that omen reports a sink on a line, or that omen does
not report one. The security cases are in the directories `security` and
`security_decoys`.

- `category` is one of the nine sink categories. `scenario` is one of seven
  rows: `real_call`, `aliased_import`, `safe_variant`, `name_in_string`,
  `name_in_comment`, `same_name_symbol`, and `in_test`.
- `flagged: false` means that omen must not report a sink of this category on
  this line. A label that is not flagged has no `basis` and no flags.
- `basis` is the evidence that omen gives: `imported_symbol`, `builtin`, or
  `name_only`. A syntax pattern that has no import, such as a string literal or
  an `unsafe` block, has the basis `builtin`.
- `in_test` is `true` for a sink in test code. `constant_argument` is checked
  only when the label gives it.
- With `sinks_closed_world`, a sink that omen reports without a label is a
  failure. A label takes one reported sink. A second label on the same line
  needs a second sink. Without the setting, `closed_world` applies.
- A `not_applicable` entry means that the language cannot have the construct.
  A gap in the detector is not a reason. Label the fact, and list the failure
  in the ledger.
- Each language has each category and scenario. Use a `not_applicable` entry
  with the dimension `sinks` when a row does not apply. The category of the
  entry is `<category>/<scenario>`. A category without a scenario covers each
  scenario of that category.
- A repository symbol can hide a real call in the same case, because the
  resolver of the index can select it. For this reason, Ruby, PHP, C, C++, and
  Bash keep the `same_name_symbol` rows in the case `security_decoys`.

## Path labels

A `paths` label states that omen reports the shortest path from an entry point
to a sink, or that omen reports none. The cases are in the directories `paths`.

- An entry point and a sink are given by the file and the line on which they
  start: `entry_file`, `entry_line`, `sink_file`, and `sink_line`.
- `flagged: false` means that omen must not report a path between the two.
- `steps` lists the keys of the symbols on the path, from the handler to the
  symbol that holds the sink. `weakest_basis` is the weakest basis of a call on
  the path. `path_count` is the number of shortest paths.
- `guards` lists the guards of the entry point of the path.
- `choke_points` lists symbols that each path to a sink goes through.
- Omen starts a path only at an entry point that has a symbol. An entry point
  that is a block of the file, such as a Python `__main__` block or a Bash
  script, starts no path. The cases for these do not exist, so the labels do
  not claim that the paths exist.

## The ledger

`expected_failures.json` lists each fact that omen reports incorrectly. The test
fails in these conditions:

- A fact is incorrect and the ledger does not list it.
- The ledger lists a fact that is now correct. Remove the entry.
- The ledger lists a fact that the test does not check. An entry for a fact
  without a label is correct only when `closed_world` makes that fact a failure.

To write the ledger again from the current results, run this command:

```sh
OMEN_TRUTH_BLESS=1 cargo test --test truth
```

This command writes each current failure. Thus it can add entries, not only
remove them: a change that makes an old label fail gets a new entry, and the
test then passes. Examine the diff before you commit it. A new entry is
permitted only for a new label. The command does not enforce this rule. The
`ledger_never_grows` test in CI enforces it against the base branch.
