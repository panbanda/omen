# Omen improvement standard v1

An improvement claim must name its baseline, dataset, metric, and limitations.
This standard separates development regressions, real-repository checks, and
held-out coding-agent outcomes. Passing one does not imply passing the others.

## Evidence and ontology

Represent symbols with source locations and calls with candidate locations,
resolution status, and the basis of that status. A unique name candidate is not
a compiler-proven binding. Ambiguous and unresolved calls must remain visible;
silently choosing the first name match manufactures knowledge.

The graph currently represents name candidates, not complete language semantics.
Imports, aliases, receiver types, lexical shadowing, macros, dynamic dispatch,
reflection, generated code, and stale indexes need their own tests and evidence.
Missing edges must never be interpreted as proof that a change is safe.

## Required comparisons

| Dimension | Measurement | Gate |
| --- | --- | --- |
| Direct call accuracy | True, false, and missed edges per case and language | No case gains a false or missed edge; at least one strict gain |
| Candidate coverage | True targets retained among candidates | Separate from asserted graph edges |
| Reliability | Errors, timeouts, malformed output, determinism | Zero failures; never drop failed observations |
| Real repositories | Pinned revisions, reviewed expected relationships | Report partial labels as partial, never whole-repository accuracy |
| Code editing | Held-out fixes, independent project tests, compile success, wrong edits | Paired runs with identical model snapshot and tool/token budgets |
| Retrieval | Recall@k and MRR on independently labeled questions | Separate from call accuracy |
| Resources | Cold/warm p50/p95, peak RSS, output bytes, actual tokens and cost | Pre-register margins; bytes are not tokens |
| Impact and identity | Overloads, scopes, transitive dependencies, revision changes | Separate labeled tests |

An incorrect target is both a false positive and a missed true target. Empty
predictions have undefined precision, not 100% precision. Empty gold sets have
undefined recall. Report null for undefined metrics. Never pool away a failing
language or case. Unknown or unmeasured dimensions are not passes.

## Reproducible experiments

Record baseline/candidate commit and binary hashes, corpus hash, exact commands,
toolchains, repository revisions, and raw observations. Use the same dataset and
configuration for both versions; alternate run order, validate warmups, and
require at least two observations for deterministic checks. For resource claims,
use at least 30 paired runs with separate cold and warm measurements. Shared-host
debug timings are descriptive, not evidence of release performance.

Before stochastic experiments, freeze repositories, task selection, primary
metrics, model snapshot, prompt, budgets, sample size and analysis. Split by
repository into development and held-out evaluation sets. Use repository-cluster
paired confidence intervals (95%) and adjust for multiple primary comparisons.
The default resource non-inferiority margin is 5%, assessed with the confidence
interval's upper bound; changing it after seeing results invalidates that gate.

## Improvement loop

1. Record a real failure and independently review the expected behavior.
2. Write a failing regression test before changing production code.
3. Make the smallest evidence-supported change and run all affected languages.
4. Run the fixed paired corpus. Disclose every regression, including lost correct
   guesses caused by rejecting ambiguity. Keep the change experimental if a gate fails.
5. Evaluate new repositories and actual coding tasks before claiming generality.

There is no weighted overall score. “Better in every aspect” requires all declared
dimensions to be measured and non-inferior, with a strict improvement in at least
one. The included development harness always reports broad_improvement_proven=false.

Run `python3 scripts/quality_benchmark.py --help` for the paired CLI harness.
Its corpus is freshly reconstructed, not a recovery of previous raw measurements.
Real-repository source is analyzed read-only; executing project code is a separate
trusted-code experiment. No model calls are made by this harness.
