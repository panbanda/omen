# Rebuilt call-resolution evaluation

This is a fresh reconstruction, not a recovery of the deleted checkout or its
raw results. Baseline: `22b57e8d1bbdc39ad3222a8f3495dcf7362a8caa`.
Candidate source: `b34fb4d1f73aae6f9c3b5568f211c318d3a837c0`.
Both binaries were built with Rust 1.98.1, `--locked`, the dev profile,
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_OPT_LEVEL=0 CARGO_INCREMENTAL=0`.
Host: Linux x86_64; harness: Python 3.12.14.

## Development corpus: semantic gate passed

28 authored cases: seven each for Rust, TypeScript, Ruby and Go. Labels are
source-reviewed, closed-world direct call targets. Fixture programs were not
executed in this reconstruction. Raw paired observations, binary hashes and
corpus hash: [synthetic.json](synthetic.json).

| Metric | Original | Rebuilt |
| --- | ---: | ---: |
| Correct edges | 8 | 17 |
| Incorrect edges | 17 | 4 |
| Missed true edges | 16 | 7 |
| Precision | 32.0% | 81.0% |
| Recall | 33.3% | 70.8% |
| Response bytes, once per case | 9,710 | 15,669 |
| Cases regressing versus original | — | 0 |

Import-path evidence restores the four correct edges that the old alphabetical
guess happened to select, and resolves three additional late-import cases.
Explicit Ruby receiver evidence resolves the fourth. **The combined change passes
semantic non-regression with strict gains.** Response bytes still increase by
about 61.4%; that resource loss remains separate from semantic correctness.

Go has five correct, one incorrect and one missed edge. Each other language has
four correct, one incorrect and two missed edges. Remaining false edges are the
parameter-shadow cases. Missing targets are three aliases and four shadowed
function parameters. Candidate lists can help an agent investigate but are not
scored as correct asserted edges.

## Real-repository checks

Four real projects are pinned in [real.json](../../tests/fixtures/quality/real.json).
Omen indexes the full discovered file set for every query, with its ordinary
file filtering. No files were sampled away to accelerate analysis. Six queries
have explicitly partial source-reviewed labels; unreviewed relationships are
not counted as true or false. These are development checks, not held-out tests.

| Repository | Revision | Reviewed behavior |
| --- | --- | --- |
| rack/rack | a9833c8f3bd6b6d1e0ab35de00a1f1a16b5095f5 | URI receiver mismatch; preserve escape inside a Ruby iteration block |
| colinhacks/zod | 59bbc03e10c636b9eb3c393dfeb552819774ec21 | Base64 conversion and property helper calls |
| BurntSushi/ripgrep | 3fce3b5bb0236da2df6d99672afb8a719642eca7 | Explicit self call and rejection of an unrelated as_ref target |
| spf13/cobra | adbc8813901bba65827259daa8e22ff94ec1f30e | Calls to ExactArgs/MatchAll; OnlyValidArgs is passed, not called |

Raw observations: [real-repositories.json](real-repositories.json).
Of six reviewed cases, four satisfy their labels before and all six after. Rack
and ripgrep improve; four cases are unchanged and none regress. All observations
were error-free and deterministic, and the partial-label non-regression gate passes.

The ripgrep query exposed an unrecognized `self.method()` AST form. A failing
regression drove support for that form. An intermediate attempt to extract every
Rust field call introduced an incorrect `path.as_ref()` edge to a Glob method;
that attempt was rejected and replaced with explicit-self handling. Its failure
is preserved as a regression test and a negative label in the real corpus.

This does not resolve Rust receiver types or impl scopes. Same-file names are
still candidates; traits and deref can invalidate them. Project test suites and coding-agent task success
were not measured. Merely returning JSON is not a successful correctness check.
The initially considered TypeFest repository was excluded before measurement
because this experiment targets runtime call relationships, not type utilities.

## Validation and interpretation

All 2,281 Rust tests passed, with six ignored; the six Python scorer tests and
format check passed. Tests cover direct calls across all 14 language variants,
nested scopes across 13 applicable variants, duplicates, order invariance,
same-line evidence ownership, transitive nested calls, and impact uncertainty.

Strict all-target/all-feature Clippy fails on existing code: 11 library
diagnostics, plus 13 test diagnostics, in churn, cohesion, ownership, smells,
parser, report rendering, score trend, and config. Those files are byte-unchanged
from the baseline. No unrestricted Clippy pass or coverage percentage is claimed.

Each final benchmark uses one validated warmup and 30 paired measured runs,
alternating baseline/candidate order. Compilation completed before measurements;
the two benchmark runs ran serially. Warm process/filesystem cache effects and
shared-host noise remain. Times are descriptive dev-binary measurements, not
release-performance or statistical non-inferiority evidence. Thirty repetitions
do not make 28 cases into 840 independent correctness examples.

Actual model edits, tokens/cost, peak memory, retrieval quality, complete impact,
compiler binding, and independent held-out generalization remain unmeasured.
Therefore `broad_improvement_proven` is always false. The semantic and partial
real-repository gates pass, while response budgeting remains a measured regression.

## Reproduction

Build the baseline in a separate clean worktree, copy its binary, then build the
candidate with the same environment:

```sh
export PATH="/root/.cargo/bin:$PATH"
export RUSTUP_TOOLCHAIN=stable
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_OPT_LEVEL=0 CARGO_INCREMENTAL=0
cargo build --locked -j 8
```

The paired harness that consumed those two binaries was removed along with the
repository's Python tooling, so these results cannot currently be regenerated; a
Rust replacement is needed. It compared a baseline binary against the candidate
over 30 repetitions and exited 2 for the four synthetic regressions after
writing its report. For real checks it read the pinned manifest at
`tests/fixtures/quality/real.json`, whose `directory` names had to be cloned
under a common parent at each exact revision; it refused dirty or wrong-revision
checkouts. It made no network requests, executed no fixture or project code, and
called no models.
