# Coding-agent evaluation: separate outcomes from infrastructure

`scripts/agent_evaluation.py` validates and scores complete paired run records.
It **does not** call a model, execute patches, verify log contents, or establish
that a model's code understanding improved. No real model trials were run in
this pass; no model API credential was configured. Provider/model selection,
authorized spending, a trusted execution runner, and independent held-out tasks
are required before that claim can be made.

## Register before collecting outcomes

Commit a JSON registration containing:

```json
{
  "version": 1,
  "split": "held_out",
  "development_repositories": ["rack/rack", "colinhacks/zod", "BurntSushi/ripgrep", "spf13/cobra"],
  "model_snapshot": "REPLACE_WITH_IMMUTABLE_MODEL_SNAPSHOT",
  "prompt_sha256": "REPLACE_WITH_64_LOWERCASE_HEX_CHARACTERS",
  "token_budget": 20000,
  "cases": [
    {"id": "task-001", "repository": "owner/held-out-project", "revision": "REPLACE_WITH_40_HEX_COMMIT", "language": "rust"}
  ]
}
```

The example is intentionally incomplete, not a claimed evaluation. Require at
least 30 paired tasks spanning at least five repository clusters. Split by
repository; declared development repositories cannot also be held out. Humans
must verify independence, representativeness, registration timing, immutable
model identifiers, and trusted project/test setup. JSON alone cannot prove them.

For every registered task, collect exactly one `baseline` and one `candidate`
run, including errors and timeouts. A run records:

- `case_id`, `repository`, `revision`, `language`, `variant`, and paired `seed`.
- Registered `model_snapshot`, `prompt_sha256`, `token_budget`, and canonical
  `registration_sha256` (computed by `agent_evaluation.digest`).
- SHA-256 hashes of `tool_snapshot_sha256`, `patch_sha256`, `test_log_sha256`.
- `status`: `completed`, `error`, or `timeout`; booleans `tests_pass` and
  `patch_applies`; independently reviewed integer `unrelated_edits`.
- Nonnegative integer `input_tokens`/`output_tokens`, positive `elapsed_ms`, and
  finite nonnegative `cost_usd`. Record all model and tool-loop tokens.

Artifacts must be retained and checked independently. A hash-shaped string is
not evidence that a patch passed its tests. The scorer always reports
`outcomes_independently_verified: false` because that verification is external.
Apply/execute only trusted, sandboxed fixtures with no ambient credentials.
Use identical runner limits, model settings and prompts, and alternate run order.

## Gate

Success requires completed execution, patch application, passing independent
tests, and no unrelated edits. The scorer rejects missing/duplicate/unregistered
runs, changed model/prompt/budget/revision/seed, missing hashes, invalid resources,
and held-out/development overlap. Errors/timeouts remain failures.

It computes 2000 deterministic repository-cluster bootstrap draws. Repositories
are equally weighted; tasks within each repository are equally weighted. Four
predeclared comparisons use Bonferroni-adjusted two-sided intervals:

1. Success-rate delta lower bound must be positive.
2. Token-ratio upper bound must be at most 1.05.
3. Elapsed-time-ratio upper bound must be at most 1.05.
4. Cost-ratio upper bound must be at most 1.05.

No language may have a net success drop, and no case may gain unrelated edits.
Zero-token connection failures remain in the sample. A positive token/cost
increase from a zero baseline makes the ratio unmeasurable and fails the combined
gate (reported interval is null). Development or insufficient
samples never pass. Do not tune margins after observing results. These intervals
do not remove dataset bias, benchmark contamination, or uncertain success labels.

```sh
python3 -m unittest discover -s scripts -p 'test_agent_evaluation.py'
python3 scripts/agent_evaluation.py --registration registration.json \
  --runs complete-runs.json --output agent-results.json
```

## Proof supplied by this PR

Eleven deterministic unit tests validate scorer behavior, including rejection of
missing observations, confounded pairs, invalid costs/tokens, leaking held-out
repositories, resource regressions, unrelated edits and false success on timeouts.
Synthetic outcomes in tests are explicitly fixture data, not model results.
A CI job runs these tests without model access, network requests or API keys.

The resulting flag is named `reported_outcome_gate`, not “understanding proven.”
Only independently collected and verified runs can support a coding-agent claim.
