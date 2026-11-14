# Coding-agent evaluation: separate outcomes from infrastructure

The command `omen eval` (`src/eval/`) validates and scores complete paired run records.
It does not call a model. It does not execute patches. It does not verify the contents of logs.
It does not show that the code understanding of a model improved.

This pass did not run real model trials. No model API credential was configured.
Before you can make that claim, you need these items:

- A selected provider and model.
- Authorized spending.
- A trusted execution runner.
- Independent held-out tasks.

## Register before you collect outcomes

Commit a JSON registration that contains these fields:

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

The example is intentionally incomplete. It is not a claimed evaluation.
Use at least 30 paired tasks. The tasks must span at least five repository clusters.
Split the tasks by repository. A declared development repository cannot also be a held-out repository.

Humans must verify these items:

- Independence.
- Representativeness.
- Registration timing.
- Immutable model identifiers.
- Trusted project and test setup.

JSON alone cannot prove these items.

For each registered task, collect exactly one `baseline` run and one `candidate` run.
Include the runs that have errors and the runs that time out. A run records these fields:

- `case_id`, `repository`, `revision`, `language`, `variant`, and the paired `seed`.
- The registered `model_snapshot`, `prompt_sha256`, and `token_budget`.
  Also the canonical `registration_sha256`, which `omen::eval::digest` calculates.
- The SHA-256 hashes `tool_snapshot_sha256`, `patch_sha256`, and `test_log_sha256`.
- `status`: `completed`, `error`, or `timeout`.
- The booleans `tests_pass` and `patch_applies`.
- The integer `unrelated_edits`, which a reviewer sets independently.
- The non-negative integers `input_tokens` and `output_tokens`.
- A positive `elapsed_ms`.
- A finite, non-negative `cost_usd`.

Record all the tokens of the model and of the tool loop.

Keep the artifacts and check them independently. A string that has the form of a hash is not evidence that a patch passed its tests.
The scorer always reports `outcomes_independently_verified: false`, because the verification is external.
Apply and execute only trusted, sandboxed fixtures that have no ambient credentials.
Use identical runner limits, model settings, and prompts. Alternate the run order.

## Gate

A run is a success when all these conditions are true:

- The execution completed.
- The patch applied.
- The independent tests pass.
- The run has no unrelated edits.

The scorer rejects these cases:

- A missing, duplicate, or unregistered run.
- A changed model, prompt, budget, revision, or seed.
- A missing hash.
- An invalid resource value.
- An overlap of held-out and development repositories.

An error or a timeout stays a failure.

The scorer calculates 2000 repository-cluster bootstrap draws.
Each repository has the same weight. Each task in a repository has the same weight.
The scorer visits the clusters in sorted repository order.
It draws with a SplitMix64 generator that starts at zero.
The generator is written in `src/eval/`. It does not come from a crate.
Thus the draws are the same across toolchains and releases.

Four predeclared comparisons use Bonferroni-adjusted two-sided intervals:

1. The lower bound of the success-rate delta must be positive.
2. The upper bound of the token ratio must be at most 1.05.
3. The upper bound of the elapsed-time ratio must be at most 1.05.
4. The upper bound of the cost ratio must be at most 1.05.

No language can have a net success drop. No candidate run can make an unrelated edit.
The unrelated-edit count of the baseline is not an allowance. Omen rejects a candidate on its own count.

A connection failure with zero tokens stays in the sample.
If the token count or the cost increases from a zero baseline, the ratio cannot be measured.
Then the combined gate fails, and the reported interval is null.
A development sample or a sample that is too small never passes.
Do not tune the margins after you observe the results.
These intervals do not remove dataset bias, benchmark contamination, or uncertain success labels.

The command writes the report to stdout in the selected `--format`.
A failed gate is a threshold violation. Thus the command exits with status 2, as every other omen gate does.
Invalid evidence is an error, and the command exits with status 1.

```sh
cargo test eval::
omen -f json eval --registration registration.json \
  --runs complete-runs.json --output agent-results.json
```

## Proof that this implementation supplies

Twenty unit tests in `src/eval/mod.rs` validate the behavior of the scorer. They cover these cases:

- A missing observation.
- A confounded pair.
- A malformed case.
- An invalid cost or token count.
- A token count whose sum overflows.
- A leak of a held-out repository.
- A regression of tokens, time, or cost.
- A patch that was not applied.
- A candidate with unrelated edits.
- A false success on a timeout.
- The canonical digest and the determinism.

Three CLI tests in `tests/integration_tests.rs` cover both exit paths and a malformed runs file.
The synthetic outcomes in the tests are fixture data. They are not model results.
The existing `Test` CI job runs these tests. They do not run a model, and they do not need a model credential.
CI is not isolated from the network, because `actions/checkout` reaches GitHub.
The isolation covers only what the scorer does. It does not cover the runner.

The resulting flag has the name `reported_outcome_gate`. It does not have the name "understanding proven".
Only runs that are collected and verified independently can support a coding-agent claim.
