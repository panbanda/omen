# Security precision on real repositories

This document gives the precision of the sinks that `omen security` finds on real
code. It also gives the patterns that caused false positives, the fixes for them,
and the categories that are experimental.

## Method

Omen ran on five repositories and on itself. Omen only read the files. Nobody built
or ran code of these repositories.

| Repository | Languages that gave findings |
|---|---|
| rack | Ruby |
| zod | TypeScript |
| ripgrep | Rust |
| cobra | Go |
| litellm | Python, Rust, JavaScript, Bash, Ruby |
| omen | Rust (the branch `feat/omen-5-b`) |

The copies of the five repositories have no version metadata, so this document
cannot give their commits. A new review must record the commits.

A person read each finding in its source file and gave it one label.

- **True**: the code does what the category says. A sink in test code is true when
  the construct is real. A sink with a constant argument is true.
- **False**: the construct is not what the category says, the code declares that no
  security property rests on it, or the value is made up.

The category `hardcoded_credential` has a stricter rule. A token is true only if it
can be a real secret. A token in a test fixture with a made-up value is false.

The category `sql_string_construction` is true when the argument of `execute` is a
string that code builds from parts. It is false when the driver composes the string
and quotes each part.

Two repositories gave 200 findings or more in one category. Only litellm did this,
for `sql_string_construction` (228) and for `hardcoded_credential` (126, fewer than
200 but reviewed in full). Each one of the 228 SQL findings was read and labeled.
For all other categories, each finding was read and labeled too. No sample was used.

The entry points were checked in a different way. A person cannot label 1395 entry
points by hand, so a script checked each entry point against its own source text:
the route decorator for FastAPI, the `__main__` guard for Python, the registration
call for Express, and the `fn main` line for Rust and Go. Each of the 1395 entry
points passed. This check finds wrong entry points. It does not find missing entry
points, so the recall of entry points is not known.

## Result before the fixes

Precision is the number of true findings divided by the number of findings.

| Category | Findings | True | Precision |
|---|---|---|---|
| command_execution | 152 | 152 | 100% |
| memory_unsafe | 29 | 29 | 100% |
| unsafe_deserialization | 29 | 29 | 100% |
| code_evaluation | 22 | 21 | 95% |
| weak_hash | 31 | 12 | 38% |
| sql_string_construction | 228 | 39 | 17% |
| tls_verification_disabled | 1 | 0 | 0% |
| hardcoded_credential | 126 | 0 | 0% |

By language:

| Category | Language | Findings | True | Precision |
|---|---|---|---|---|
| code_evaluation | Bash | 2 | 2 | 100% |
| code_evaluation | JavaScript | 9 | 9 | 100% |
| code_evaluation | Python | 6 | 6 | 100% |
| code_evaluation | Ruby | 2 | 1 | 50% |
| code_evaluation | TypeScript | 3 | 3 | 100% |
| command_execution | Go | 4 | 4 | 100% |
| command_execution | Ruby | 1 | 1 | 100% |
| command_execution | Rust | 147 | 147 | 100% |
| hardcoded_credential | Python | 125 | 0 | 0% |
| hardcoded_credential | Rust | 1 | 0 | 0% |
| memory_unsafe | Rust | 29 | 29 | 100% |
| sql_string_construction | Python | 228 | 39 | 17% |
| tls_verification_disabled | Bash | 1 | 0 | 0% |
| unsafe_deserialization | Python | 7 | 7 | 100% |
| unsafe_deserialization | Ruby | 22 | 22 | 100% |
| weak_hash | Python | 30 | 11 | 37% |
| weak_hash | Rust | 1 | 1 | 100% |

By repository:

| Repository | Category | Findings | True |
|---|---|---|---|
| cobra | command_execution | 4 | 4 |
| litellm | code_evaluation | 17 | 17 |
| litellm | command_execution | 7 | 7 |
| litellm | hardcoded_credential | 126 | 0 |
| litellm | memory_unsafe | 15 | 15 |
| litellm | sql_string_construction | 228 | 39 |
| litellm | tls_verification_disabled | 1 | 0 |
| litellm | unsafe_deserialization | 7 | 7 |
| litellm | weak_hash | 31 | 12 |
| omen | command_execution | 136 | 136 |
| omen | memory_unsafe | 10 | 10 |
| rack | code_evaluation | 2 | 1 |
| rack | unsafe_deserialization | 22 | 22 |
| ripgrep | command_execution | 5 | 5 |
| ripgrep | memory_unsafe | 4 | 4 |
| zod | code_evaluation | 3 | 3 |

## False-positive patterns and fixes

Each fix has a test that failed before the fix. Each fix also has a case in the
truth fixtures, so a later change cannot bring the false positive back.

| Pattern | Findings | Cause | Fix |
|---|---|---|---|
| The SQL composition of `psycopg`: `sql.SQL("DROP {}").format(sql.Identifier(name))` | 190 | The rule counted each `format` call as a built string. | A `format` call on `SQL(...)`, or with arguments that are all `Identifier`, `Literal`, `Placeholder`, `SQL`, or `Composed` calls, is not a built string. A `+` that joins such an object is not a built string. |
| `hashlib.md5(data, usedforsecurity=False)` | 19 | The rule did not read the keyword that declares the non-security use. | `hashlib.md5`, `hashlib.sha1`, and `hashlib.new` with a weak name are not sinks when `usedforsecurity=False`. |
| `curl -K -` in a shell script | 1 | The matcher made the option lower case, so `-K` (a configuration file) matched `-k` (no certificate check). | The Bash `curl` row compares the options with the case of the source. |
| `instance_eval(&block)` and `instance_eval { ... }` in Ruby | 1 | The rule counted a block as code to evaluate. | `instance_eval`, `class_eval`, and `module_eval` are sinks only when the first argument is not a block. `eval` is unchanged. |
| The example token of the provider documentation, `AKIAIOSFODNN7EXAMPLE` | 62 | The token has the format of a key. | A literal that contains `example` is not a credential. |

## Result after the fixes

The fixes ran on the same repositories that the review used. The numbers below are
therefore not a held-out test. A change of the rules must keep the truth fixtures
green, and a new review on other repositories must confirm the numbers.

| Category | Findings | False positives that remain | Findings in test code | Findings with a constant argument |
|---|---|---|---|---|
| command_execution | 152 | 0 | 128 | 138 |
| memory_unsafe | 29 | 0 | 13 | 0 |
| unsafe_deserialization | 29 | 0 | 29 | 0 |
| code_evaluation | 21 | 0 | 5 | 13 |
| weak_hash | 12 | 0 | 11 | 0 |
| sql_string_construction | 39 | 0 | 39 | 0 |
| tls_verification_disabled | 0 | 0 | 0 | 0 |
| hardcoded_credential | 64 | 64 | 64 | 0 |

The 64 tokens of `hardcoded_credential` in litellm are made-up values in test
fixtures, for example `AKIASCRIPTEDPROVIDER`. Nothing in the syntax tells a made-up
token from a real one, so no further rule can remove them.

Observations about the true findings:

- 128 of 152 `command_execution` sinks are in test code. 138 of 152 have a constant
  program name, such as `Command::new("git")`. The command is real, but the
  program does not come from an input. An argument of the command can still come
  from an input.
- All 9 JavaScript `code_evaluation` findings are `Function("return this")()` in
  bundled files of the browser. The argument is constant.
- All 39 SQL findings are in test code. 36 are an f-string with a table name from a
  constant. Nothing in the shipped code of litellm builds SQL from parts.
- 11 of 12 `weak_hash` findings are SHA-1 digests that give a short name to a test
  fixture. The other one is the SHA-1 object id of Git in Rust.

## Experimental categories

A category is experimental when its precision on real code stays below 50 percent
after the fixes. Omen does not look for the sinks of an experimental category,
unless the caller asks for them.

| Category | Precision after the fixes | State |
|---|---|---|
| hardcoded_credential | 0% (0 of 64) | experimental |

Turn it on with one of these:

- the flag `omen security --experimental`;
- the parameter `experimental` of the MCP tool `security`;
- the key `experimental = true` in the table `[security]` of the configuration.

When the category is off, the field `coverage.experimental_off` lists it, and the
coverage statement says that omen did not look for it. The SARIF output carries the
same statement in `runs[0].properties`.

The truth fixtures still label `hardcoded_credential`, because the matcher always
finds the sinks. The analysis hides them.

## What the numbers do not say

- The review covers six repositories and seven languages. Java, C, C++, C#, PHP,
  and TSX gave no findings in these repositories. The precision of those languages
  is only the precision of the truth fixtures.
- A true finding is a construct that exists. It is not a vulnerability. Omen does
  not follow data from an input to the sink.
- Recall is not known. A sink that the table does not name does not appear.
- Entry points: the check finds wrong entries only. See "Method".
- Each result of `omen security` carries the coverage statement and the count
  `sinks_with_no_known_entry`. A sink with no known entry point is not safe. It
  means that no entry point that omen knows reaches the sink through a call that
  the index resolves.
