# Security analysis

The command `omen security` finds three things: the sinks (the calls and the
constructs that can cause harm), the entry points (the places where outside input
enters the code), and the paths from an entry point to a sink.

Omen does not follow data. A path says that a call chain exists. It does not say
that input reaches the sink. A sink with no known entry point is not safe: it means
that no entry point that omen knows reaches it through a call that the index
resolves. Each result carries a coverage statement that says what omen recognises.
An empty result does not mean that the code is safe.

## Commands

```bash
omen security                       # all sections
omen security sinks                 # the sinks
omen security surface               # the entry points
omen security paths                 # the paths from entry points to sinks
omen security paths --entry KEY     # the paths of one entry point
omen security paths --sink KEY      # the paths to one sink
omen security --category weak_hash  # the sinks of one category
omen security --min-basis same_file # use only the strongest call edges
omen security --experimental        # include the experimental categories
omen -f sarif security              # SARIF 2.1.0 for a code scanning service
omen -f mermaid security paths      # a Mermaid diagram of the paths
```

The MCP tool `security` has the same sections and filters. The tools `symbol` and
`impact` have a field `security` for the symbol that they describe.

## Sink categories

| Category | CWE | Example |
|---|---|---|
| command_execution | 78 | `subprocess.run(command, shell=True)` |
| code_evaluation | 95 | `eval(text)` |
| unsafe_deserialization | 502 | `pickle.loads(data)` |
| sql_string_construction | 89 | `cursor.execute("..." + name)` |
| weak_hash | 328 | `hashlib.md5(data)` |
| weak_cipher | 327 | `DES.new(key)` |
| tls_verification_disabled | 295 | `requests.get(url, verify=False)` |
| hardcoded_credential (experimental) | 798 | a token with a provider prefix |
| memory_unsafe | 119 | a Rust `unsafe` block |

A sink has a basis: `imported_symbol` (an import binds the callee), `builtin` (the
language gives the name), or `name_only` (only the name is evidence). A sink in test
code has the flag `in_test`. A call with only literal arguments has the flag
`constant_argument`.

The precision of each category on real repositories is in
`docs/benchmarks/security-precision.md`. A category with a precision below 50
percent is experimental: omen does not look for it unless you use `--experimental`.

## Suppression

A comment on the line of the sink, or on the line before it, suppresses a sink:

```python
os.system(command)  # omen:allow command_execution: the value comes from our own build
```

The reason is mandatory. A suppression with no reason, or with an unknown category,
does not suppress anything, and the output lists it in `invalid_suppressions`. The
output counts the suppressed sinks and lists them in `suppressed`.

## Configuration

```toml
[security]
gate = "off"                   # off, warn, or error
min_basis = "same_package"     # the weakest call edge of a path
gate_categories = []           # empty means each category
gate_basis = "builtin"         # the gate counts this basis or a stronger one
experimental = false           # include the experimental categories
```

The gate (`--gate`) compares the branch with its base and counts the sinks that the
change adds. You can add sinks and entry points for your own wrappers and
frameworks with `[[security.sinks]]` and `[[security.entry_points]]`. See
`omen.example.toml`.

## Pull requests and reports

`omen diff -f markdown` adds a Security section to the comment: the counts, the new
sinks, the new entry points, and one diagram for each new path. `omen report
generate` writes `security.json`, and the report shows the section "Attack
surface" with the coverage statement, a matrix, a Sankey diagram, the entry
points, and the choke points. A choke point is a symbol that each path to a
sink goes through.
