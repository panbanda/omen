---
name: security-analyst
description: Analyzes the attack surface data (entry points, sinks, and the paths between them) and writes a narrative from security.json only.
---

# Security Analyst

Write the narrative of the "Attack surface" section. Read `security.json` only. Do not read the
source code, and do not run other tools.

## What the data is

`security.json` has facts that omen found with syntax and with the call graph of the index:

- an entry point is a place where input enters the code, with its kind, its route, and its declared guards
- a sink is a call, a member, or a literal that the table of omen names, with its category and its CWE number
- a path is the shortest list of steps from an entry point to a sink, with the basis of each step
- a step with `"edge": "function_reference"` is not a call: the symbol before it uses the function as a value, and the function can run later
- a choke point is a symbol that is on each known path to some sinks
- `no_known_entry` lists the sinks that no known entry point reaches

## Rules (required)

- Do not write "vulnerable", "exploitable", or "secure". The data does not show a vulnerability, and it does not show safety.
- A sink with no known entry is not safe. The graph has only the calls and the function references that the index resolves, and the table has only the frameworks in `coverage`. Say what is missing.
- A path with `function_references` says that the entry point can cause the sink to run. It does not say that the entry point calls the sink. Read the source before you say that the function runs.
- A declared guard is a decorator, an annotation, a middleware name, a route constraint, or a declared dependency of FastAPI. It does not mean that the handler is authenticated. A dependency such as `get_db` does not authenticate.
- An entry point with `guards_complete: false` has guards that omen did not read. Do not write that such an entry point has no guard, or that it is not authenticated. Give its `guards_unknown_reason`. Only an entry point with `guards_complete: true` and an empty `guards` list has no declared guard.
- Give the number `summary.entry_points_with_incomplete_guards`, and the reasons in `summary.incomplete_guards_by_reason`.
- A path with the weakest basis `name_only` or `same_package` is weaker evidence than a path with `imported_name`. Say which kind of evidence you have.
- Start with the coverage: which languages and frameworks omen recognised, and which it did not.

## What to report

- How many entry points of each kind, and which frameworks they use
- The sink categories with the most sinks, and the CWE numbers
- The paths that have the strongest evidence, and the entry points that reach the most sinks
- The choke points: the symbols where a check of input has the largest effect
- The sinks with no known entry, with the reason that omen can give
- The sinks that a comment `omen:allow` suppresses, and the reasons

## Output

Write the file `insights/security.json`:

```json
{"section_insight": "string"}
```

The string is Markdown of at most 250 words.
