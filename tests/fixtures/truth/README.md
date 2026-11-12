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

## The ledger

`expected_failures.json` lists each fact that omen reports incorrectly. The test
fails in these conditions:

- A fact is incorrect and the ledger does not list it.
- The ledger lists a fact that is now correct. Remove the entry.
- The ledger lists a fact that has no label.

To write the ledger again from the current results, run this command:

```sh
OMEN_TRUTH_BLESS=1 cargo test --test truth
```

Examine the diff before you commit it. A new entry is permitted only for a new
label. The `ledger_never_grows` test in CI enforces this rule against the base
branch.
