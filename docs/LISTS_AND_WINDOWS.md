# Lists and history windows

This document gives the rules for each list that a command or a tool pages,
and for each analyzer that reads the git history.

## The order of a list

Each list that `--top`, `--offset`, `limit`, or `offset` cuts is in rank order.
The row with the most risk is first. Thus the first page has the rows that are
most important. A second key breaks each tie, so two runs give the same order.

| Command | List | Order |
|---|---|---|
| `changes` | `commits` | `risk_score`, highest first. Then the newest commit, then the hash. |
| `churn` | `files` | `churn_score`, highest first. Then the path. |
| `clones` | `clones` | `similarity`, highest first. Then the number of lines, then the place. |
| `cohesion` | `classes` | `lcom`, highest first. |
| `complexity` | the functions of `files` | Cyclomatic complexity, highest first. Then the cognitive complexity, the path, and the line. |
| `deadcode` | `items` | `confidence`, highest first. Then the file, the line, and the name. |
| `defect` | `files` | `probability`, highest first. Then the path. |
| `flags` | `flags` | The priority score, highest first. Then the stale flags, then the age. |
| `graph` | `nodes` | `pagerank`, highest first. Then the path. |
| `hotspot` | `hotspots` | `score`, highest first. Then the path. |
| `ownership` | `files` | `concentration`, highest first. Then the number of lines, then the path. |
| `repomap` | `symbols` | `pagerank`, highest first. Then the file and the position. |
| `satd` | `items` | Severity, from `critical` to `low`. Then the file and the line. |
| `smells` | `smells` | Severity, highest first. Then the type and the components. |
| `stubs` | `stubs` | Severity, from `high` to `low`. Then the file and the line. |
| `tdg` | `files` | `total`, lowest first: a low score is much debt. Then the path. |
| `temporal` | `couplings` | `coupling_strength`, highest first. Then the number of co-changes, then the paths. |

`security` gives its sinks in the order of the file and the position. A sink
has no severity.

### The functions of `complexity`

`complexity` groups its functions by file. A page applies to the functions of
all the files as one list in rank order. A file is in the result only when one
of its functions is on the page. The files are in the order of their first
function on the page. The values of a file, such as `avg_cyclomatic`, are for
the whole file.

### The omitted rows

A command states each cut that it makes:

- The CLI adds `<list>_omitted` for each list that `--top` or `--offset` cuts.
  `complexity` adds `functions_omitted` and `files_omitted`.
- `churn` and `hotspot` have a default `--top` of 20. The other commands give
  each row when `--top` is absent.
- An MCP tool gives `total_items`, `returned`, `has_more`, `next_offset`, and
  one `truncated` entry for each list that it cut. `limit` has the default 50.

## The rows that are test code

Each row of a list of files or of symbols has `is_test`. `docs/TEST_CODE.md`
gives the rule. A row of a file uses the path of the file. A row with a line,
such as a function of `complexity` or an item of `satd`, uses the syntax too:
a function in a Rust `#[cfg(test)]` module has `is_test: true`. A row of two
files, as a coupling of `temporal` has, is test code when one of its files is.

`--exclude-tests` (CLI) and `exclude_tests` (MCP) remove the rows that are
test code. The report then has `tests_excluded`, the number of removed rows.
For `complexity`, the number is for the functions. The default keeps each row.

These commands and tools have the option: `churn`, `clones`, `cohesion`,
`complexity`, `deadcode`, `defect`, `graph`, `hotspot`, `ownership`, `repomap`,
`satd`, `security`, `smells`, `stubs`, `tdg`, and `temporal`. The command `all`
has it too. For `security`, the option removes the sinks and the entry points
in test code, and `tests_excluded` is the number of removed sinks. A command
without a list of files, such as `changes`, gives an error for the flag.

The key `exclude_tests` of the configuration is a different control. It
removes the test files from the input of `clones` and `temporal`.

Two limits apply:

- The summary of a report does not change. It is for all the code that the
  analyzer read, and a percentile is a rank in all the files.
- An analyzer that does not read the shared index does not follow a Rust
  `#[cfg(test)] mod name;` declaration to the file of the module. Such a file
  has `is_test: true` only when its path follows a test convention.
  `deadcode`, `security`, `repomap`, `cohesion`, `smells`, and `graph` read
  the index and follow the declaration.

Omen does not read a vendored, minified, or generated file. The file set
ignores the directories `node_modules`, `vendor`, `target`, `dist`, `build`,
`coverage`, `.next`, `_next`, `.turbo`, and `.cache`, and a script with `.min`,
`.bundle`, or `.chunk` before its extension. `include_generated = true` in the
configuration keeps these files. Use `exclude_patterns` for a vendored file
that has a different name.

## History windows

A history analyzer reads the commits of one window. The window ends at the
date of the analyzed revision, not at the current time.

| Command | Default window | Configuration key | `--since all` |
|---|---|---|---|
| `churn` | `6m` (180 days) | `churn.since` | Valid |
| `hotspot` | `90d` | `hotspot.since` | Valid |
| `defect` | `30d` | `defect.since` | Valid |
| `temporal` | `30d` | none | Valid |
| `changes` | `30d` | `changes.since` | Not valid |
| `report generate` | `1y` | none | Valid |
| `score trend` | `all` | none | Valid |

Each command has `--since`, and each MCP tool has `since`. A duration is a
number with `d`, `w`, `m`, or `y`.

Start with the default window. If `commits_matched` is less than 20, run the
command again with a wider window. Use the value of `next_since` when the
report has it. A window with fewer than 20 commits gives weak percentiles, and
`changes` then uses fixed thresholds.

### The window block

`churn`, `hotspot`, `defect`, `temporal`, and `changes` give the same block:

```json
"churn_window": {
  "days": 30,
  "anchor_date": "2024-01-22T12:00:00+00:00",
  "commits_matched": 4,
  "history_complete": true
}
```

| Key | Value |
|---|---|
| `days` | The width of the window. `null` is all the history. |
| `anchor_date` | The date of the analyzed revision. The window ends at this date. |
| `commits_matched` | The number of commits in the window. |
| `history_complete` | `false` for a shallow clone. |
| `history_note` | Present only when `history_complete` is `false`. |
| `next_since` | Present only when the list is empty and a wider window exists. |

`period_days` of `churn` and `temporal` is `null` for all the history.

### An empty result

A window with few commits can give an empty list. An empty list then does not
tell that the repository has nothing to report. `next_since` is the next
window to try: `90d`, then `6m`, then `1y`, then `all`. `changes` does not
accept `all`, thus its last value is `1y`. An MCP tool also gives a hint: the
same call with `since` set to that value.

### A shallow clone

A shallow clone has only a part of the history. `history_complete` is then
`false`, and `history_note` tells that each count is a lower limit. Run
`git fetch --unshallow` to get the full history.

`ownership` reads `git blame`. In a shallow clone, git gives each line that is
older than the boundary commit to that commit. The author of that commit is
not the author of those lines. Thus `ownership` does not give those lines to a
contributor:

- The shares of a file use only the lines that changed after the boundary.
- `unattributed_lines` of a file is the number of the other lines.
- A file in which each line is older than the boundary is not in `files`.
  `summary.files_without_history` counts these files.
- `history_complete` is `false`, and `history_note` gives the cause.

The bus factor of a shallow clone uses only the lines with a known author. Do
not use it as the bus factor of the repository.

## The risk level of `changes`

`changes` gives each commit a `risk_score` from 0 to 1 and a `risk_level`.

- A window with 20 or more commits has percentile thresholds: `high` is the
  95th percentile of the scores of the window, and `medium` is the 80th.
  `risk_thresholds.basis` is then `percentile`.
- A window with fewer commits has the fixed thresholds 0.6 and 0.3, and
  `risk_thresholds.basis` is `absolute`. A percentile of a few scores is one of
  those scores: with one commit, each threshold is the score of that commit,
  and the commit would be a high risk whatever its score is.

## The values of a hotspot row

The score of a hotspot is `churn_percentile / 100 * complexity_percentile /
100`. A row gives the two values that the percentiles rank:

| Key | Value |
|---|---|
| `churn_score` | One for each commit of the window, and one more for each 100 lines that the commit adds or removes. `churn_percentile` is its rank in the files that changed. |
| `total_complexity` | The sum of the cyclomatic complexity of the functions of the file. `complexity_percentile` is its rank in the files of the repository. |
| `avg_complexity` | The mean cyclomatic complexity of a function. The score does not use it. |

A large file with many small functions has a high `total_complexity` and a
low `avg_complexity`.

## Numbers

A weight or a score that the code holds as a 32-bit float is in the JSON as
its shortest decimal: `0.3`, not `0.30000001192092896`. The `confidence` of
`deadcode` has two decimal places. The `churn_score` of a hotspot is the exact
decimal of its commits and lines: `220.57`, not `220.56999999999996`.
