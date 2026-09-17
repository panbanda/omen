# Understanding improvements: delivered evidence and remaining work

This is a first implementation batch across the ten proposed areas, **not
completion of all ten**. No aggregate “best” score or coding-agent improvement
claim is justified by these results. Every gain below has a narrower scope.

| Area | Current advance | Evidence | Still required |
| --- | --- | --- | --- |
| Scope/type resolution | Visible nested definitions and enclosing type-name hints | [PR #509](https://github.com/panbanda/omen/pull/509): location tests and 1→7 correct edges on eight development cases | Real receiver types, namespace identity, variables/parameters, overloads/traits |
| Imports/aliases | Named TS/JS and top-level Rust aliases; relative source paths | #509: alias/path collision regressions | Go aliases, barrels/re-exports, wildcard/default imports, Ruby constants |
| Change-focused context | Exact-symbol bounded neighborhood with omissions | [PR #510](https://github.com/panbanda/omen/pull/510): budget and one-call retrieval tests | Free-text/diff task planning, independent relevance labels |
| Test relationships | Transitive graph test candidates with convention labels | #510: 3/4→4/4 definitions across four language fixtures | Actual execution coverage and missed-failing-test evaluation |
| Data flow | Bounded AST use/write/call occurrences | #510: nested-scope exclusion and syntactic write assertions | Binding-resolved def-use, value propagation, error/async paths, taint |
| Framework ontology | Directory-convention role hints | #510: label/provenance tests only | Routes/handlers, DI, schemas, jobs, migrations, component relationships |
| Incremental indexing | Content-validated syntax reuse and unchanged graph reuse | #510: 768 cold parses; zero parses/rebuilds over 30 warm requests per real repo; edit/delete fresh-rebuild equality | Dependency-local graph updates, release latency and peak RSS evaluation |
| History awareness | Optional co-change hints separated from edges | #510: controlled Git history fixture, missing-history handling | Held-out historical bug-fix retrieval/ranking gains |
| Confidence/provenance | New resolution bases, parse diagnostics, explicit uncertainty/omissions | #509/#510: behavior and evidence retention tests | Independently measured calibration and reliable typed bindings |
| Coding-agent outcomes | Registered paired-outcome scorer with fail-closed checks | `omen eval`: 20 unit tests and 3 CLI tests | Configured model runner, authorized budget, independent held-out tasks and logs |

## Tradeoffs that must stay visible

- [PR #508](https://github.com/panbanda/omen/pull/508) reduces response tokens with
  lossless tables; that result does not imply better graph or model understanding.
- #510's richer bundle uses 528–532 tokens versus 201–202 for `get_symbol` on the
  small fixtures. Retrieval depth improves, but token efficiency worsens there.
- Syntax reuse is measured work avoidance, not a proven end-to-end speedup.
- Current real repositories have been used during development. They are not
  held out, and their six reviewed graph checks are partial labels.
- Source-role/test-name conventions must not be promoted to factual framework
  or execution-coverage edges.

## Next evidence-gated work

1. Add independent receiver/shadowing/namespace fixtures before widening symbol
   inference. Prefer abstention to untested guesses, while measuring lost recall.
2. Add language/framework adapters only alongside reviewed relationship labels.
3. Add dynamic test-coverage evidence in trusted project-specific test runners.
4. Freeze new repositories and tasks before model trials; keep development repos
   excluded. Configure a provider/model snapshot and spending cap without putting
   credentials in this repository.
5. Run paired coding tasks and independently verify patches/test logs. Apply
   `AGENT_EVALUATION.md`; never present synthetic scorer fixtures as model wins.

Implementation PR stack: #508 → #509 → #510, all merged to main. This
evaluation PR is independent against main.
