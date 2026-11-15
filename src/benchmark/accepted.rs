//! Reviewed regressions that the base-branch comparison accepts.
//!
//! A pull request that makes a rank or a truth fact worse than the base
//! branch fails `ledger_never_grows`. The pull request can list the
//! regression in an `accepted_regressions.json` file. A reviewer then accepts
//! the regression in the review of that file. An entry must match a regression
//! exactly. A new entry that matches no regression against the base branch is
//! an error. After the merge, the base branch has the regression and the entry.
//! The next pull request has an inherited entry that matches no regression.
//! The check gives a note for this entry and does not fail. A later pull
//! request can delete the entry.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// A rank of a query that is worse than the rank on the base branch. `None`
/// means that the results have no acceptable symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankRegression {
    pub id: String,
    pub base_rank: Option<usize>,
    pub rank: Option<usize>,
}

/// A reviewed rank regression. The id is `<language>/<query id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedRankRegression {
    pub id: String,
    pub base_rank: Option<usize>,
    pub rank: Option<usize>,
    pub reason: String,
}

/// A reviewed truth fact that was correct on the base branch and is now an
/// expected failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedFact {
    pub id: String,
    pub reason: String,
}

/// The file of the accepted regressions in a fixture directory.
pub const ACCEPTED_FILE: &str = "accepted_regressions.json";

/// Read the accepted entries of a fixture directory. A missing file gives no
/// entry. An invalid file is an error.
pub fn load_accepted<T: serde::de::DeserializeOwned>(
    root: &std::path::Path,
) -> Result<Vec<T>, String> {
    let path = root.join(ACCEPTED_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn rank_text(rank: Option<usize>) -> String {
    rank.map_or_else(|| "no rank".to_string(), |rank| format!("rank {rank}"))
}

fn is_worse(base_rank: Option<usize>, rank: Option<usize>) -> bool {
    rank.unwrap_or(usize::MAX) > base_rank.unwrap_or(usize::MAX)
}

fn unmatched_problem(id: &str) -> String {
    format!("{id}: the accepted entry matches no regression against the base branch; remove it")
}

fn inherited_problem(id: &str) -> String {
    format!(
        "{id}: the base branch has this accepted entry, so it cannot accept a new regression; add a new reviewed entry"
    )
}

fn inherited_note(id: &str) -> String {
    format!("{id}: the base branch has this accepted entry and it matches no regression; a later pull request can remove it")
}

fn file_problems<'a>(
    ids: impl Iterator<Item = (&'a str, &'a str)>,
    problems: &mut BTreeSet<String>,
) {
    let mut previous: Option<&str> = None;
    for (id, reason) in ids {
        if previous.is_some_and(|previous| previous >= id) {
            problems.insert(format!(
                "{id}: the accepted entries must have ascending ids"
            ));
        }
        previous = Some(id);
        if reason.trim().is_empty() {
            problems.insert(format!("{id}: the accepted entry must have a reason"));
        }
    }
}

/// The result of a check of the accepted entries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AcceptedReport {
    /// Each problem fails the check.
    pub problems: Vec<String>,
    /// Each note is for the reader. A note does not fail the check.
    pub notes: Vec<String>,
}

/// The problems of the rank regressions that the pull request does not
/// accept, and of the accepted entries that match no regression.
///
/// An empty result means that each regression has an entry and each entry has
/// a regression.
pub fn check_accepted_ranks(
    regressions: &[RankRegression],
    accepted: &[AcceptedRankRegression],
    base_accepted: &[AcceptedRankRegression],
) -> AcceptedReport {
    let mut notes = Vec::new();
    let mut problems = BTreeSet::new();
    file_problems(
        accepted
            .iter()
            .map(|entry| (entry.id.as_str(), entry.reason.as_str())),
        &mut problems,
    );
    let accepted_by_id: BTreeMap<&str, &AcceptedRankRegression> = accepted
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    for regression in regressions {
        let matches = accepted_by_id
            .get(regression.id.as_str())
            .is_some_and(|entry| {
                entry.base_rank == regression.base_rank && entry.rank == regression.rank
            });
        if !matches {
            problems.insert(format!(
                "{}: {}, the base branch has {}",
                regression.id,
                rank_text(regression.rank),
                rank_text(regression.base_rank)
            ));
        } else if accepted_by_id
            .get(regression.id.as_str())
            .is_some_and(|entry| base_accepted.contains(entry))
        {
            problems.insert(inherited_problem(&regression.id));
        }
    }
    for entry in accepted {
        if !is_worse(entry.base_rank, entry.rank) {
            problems.insert(format!(
                "{}: the accepted entry is not a regression; remove it",
                entry.id
            ));
            continue;
        }
        let found = regressions.iter().any(|regression| {
            regression.id == entry.id
                && regression.base_rank == entry.base_rank
                && regression.rank == entry.rank
        });
        if found {
            continue;
        }
        if base_accepted.contains(entry) {
            notes.push(inherited_note(&entry.id));
        } else {
            problems.insert(unmatched_problem(&entry.id));
        }
    }
    AcceptedReport {
        problems: problems.into_iter().collect(),
        notes,
    }
}

/// The ids of the facts that the pull request does not accept, and the
/// problems of the accepted entries that match no fact.
///
/// `grown` has the ids that `ledger_growth` found.
pub fn check_accepted_facts(
    grown: &[String],
    accepted: &[AcceptedFact],
    base_accepted: &[AcceptedFact],
) -> AcceptedReport {
    let mut notes = Vec::new();
    let mut problems = BTreeSet::new();
    file_problems(
        accepted
            .iter()
            .map(|entry| (entry.id.as_str(), entry.reason.as_str())),
        &mut problems,
    );
    let accepted_by_id: BTreeMap<&str, &AcceptedFact> = accepted
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    for id in grown {
        match accepted_by_id.get(id.as_str()) {
            None => {
                problems.insert(id.clone());
            }
            Some(entry) if base_accepted.contains(entry) => {
                problems.insert(inherited_problem(id));
            }
            Some(_) => {}
        }
    }
    let grown_ids: BTreeSet<&str> = grown.iter().map(String::as_str).collect();
    for entry in accepted {
        if grown_ids.contains(entry.id.as_str()) {
            continue;
        }
        if base_accepted.contains(entry) {
            notes.push(inherited_note(&entry.id));
        } else {
            problems.insert(unmatched_problem(&entry.id));
        }
    }
    AcceptedReport {
        problems: problems.into_iter().collect(),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regression(id: &str, base_rank: Option<usize>, rank: Option<usize>) -> RankRegression {
        RankRegression {
            id: id.to_string(),
            base_rank,
            rank,
        }
    }

    fn accepted_rank(
        id: &str,
        base_rank: Option<usize>,
        rank: Option<usize>,
        reason: &str,
    ) -> AcceptedRankRegression {
        AcceptedRankRegression {
            id: id.to_string(),
            base_rank,
            rank,
            reason: reason.to_string(),
        }
    }

    fn accepted_fact(id: &str, reason: &str) -> AcceptedFact {
        AcceptedFact {
            id: id.to_string(),
            reason: reason.to_string(),
        }
    }

    fn rank_problems(
        regressions: &[RankRegression],
        accepted: &[AcceptedRankRegression],
    ) -> Vec<String> {
        check_accepted_ranks(regressions, accepted, &[]).problems
    }

    fn fact_problems(grown: &[String], accepted: &[AcceptedFact]) -> Vec<String> {
        check_accepted_facts(grown, accepted, &[]).problems
    }

    fn write_accepted(dir: &std::path::Path, text: &str) {
        std::fs::write(dir.join(ACCEPTED_FILE), text).unwrap();
    }

    #[test]
    fn a_regression_without_an_entry_fails() {
        let problems = rank_problems(&[regression("go/q", Some(1), Some(3))], &[]);
        assert_eq!(problems, ["go/q: rank 3, the base branch has rank 1"]);
    }

    #[test]
    fn an_exact_entry_accepts_a_regression() {
        let problems = rank_problems(
            &[regression("go/q", Some(1), None)],
            &[accepted_rank("go/q", Some(1), None, "the label was wrong")],
        );
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn an_entry_with_other_ranks_neither_accepts_nor_stays() {
        let problems = rank_problems(
            &[regression("go/q", Some(1), Some(5))],
            &[accepted_rank("go/q", Some(1), Some(3), "reason")],
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
    }

    #[test]
    fn an_entry_without_a_regression_fails() {
        let problems = rank_problems(&[], &[accepted_rank("go/q", Some(1), Some(3), "reason")]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("matches no regression"));
    }

    #[test]
    fn an_entry_that_is_not_worse_fails() {
        let problems = rank_problems(&[], &[accepted_rank("go/q", Some(3), Some(1), "reason")]);
        assert!(problems
            .iter()
            .any(|problem| problem.contains("not a regression")));
    }

    #[test]
    fn a_blank_reason_and_a_wrong_order_fail() {
        let problems = rank_problems(
            &[
                regression("a/q", Some(1), Some(2)),
                regression("b/q", Some(1), Some(2)),
            ],
            &[
                accepted_rank("b/q", Some(1), Some(2), "reason"),
                accepted_rank("a/q", Some(1), Some(2), " "),
            ],
        );
        assert!(problems.iter().any(|problem| problem.contains("ascending")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("must have a reason")));
    }

    #[test]
    fn a_fact_needs_an_entry_and_an_entry_needs_a_fact() {
        let grown = [
            "go/a/symbols/x/found".to_string(),
            "go/a/symbols/y/found".to_string(),
        ];
        let problems = fact_problems(
            &grown,
            &[
                accepted_fact("go/a/symbols/x/found", "reason"),
                accepted_fact("go/a/symbols/z/found", "reason"),
            ],
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.contains(&"go/a/symbols/y/found".to_string()));
        assert!(problems
            .iter()
            .any(|problem| problem.starts_with("go/a/symbols/z/found")));
    }

    #[test]
    fn matching_facts_pass() {
        let grown = ["go/a/symbols/x/found".to_string()];
        let problems = fact_problems(&grown, &[accepted_fact("go/a/symbols/x/found", "reason")]);
        assert!(problems.is_empty());
    }

    #[test]
    fn an_inherited_entry_without_a_regression_is_a_note() {
        let base = tempfile::tempdir().unwrap();
        let head = tempfile::tempdir().unwrap();
        let text = r#"[{"id": "go/q", "base_rank": 1, "rank": 3, "reason": "r"}]"#;
        write_accepted(base.path(), text);
        write_accepted(head.path(), text);
        let base_accepted: Vec<AcceptedRankRegression> = load_accepted(base.path()).unwrap();
        let accepted: Vec<AcceptedRankRegression> = load_accepted(head.path()).unwrap();
        let report = check_accepted_ranks(&[], &accepted, &base_accepted);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.notes.len(), 1);
        assert!(report.notes[0].contains("go/q"));
    }

    #[test]
    fn a_new_entry_without_a_regression_fails() {
        let base = tempfile::tempdir().unwrap();
        let head = tempfile::tempdir().unwrap();
        write_accepted(base.path(), "[]");
        write_accepted(
            head.path(),
            r#"[{"id": "go/q", "base_rank": 1, "rank": 3, "reason": "r"}]"#,
        );
        let base_accepted: Vec<AcceptedRankRegression> = load_accepted(base.path()).unwrap();
        let accepted: Vec<AcceptedRankRegression> = load_accepted(head.path()).unwrap();
        let report = check_accepted_ranks(&[], &accepted, &base_accepted);
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("matches no regression"));
        assert!(report.notes.is_empty());
    }

    #[test]
    fn an_entry_with_changed_ranks_is_new() {
        let base = [accepted_rank("go/q", Some(1), Some(3), "reason")];
        let accepted = [accepted_rank("go/q", Some(1), Some(4), "reason")];
        let report = check_accepted_ranks(&[], &accepted, &base);
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    }

    #[test]
    fn an_inherited_fact_entry_without_a_regression_is_a_note() {
        let base = tempfile::tempdir().unwrap();
        let head = tempfile::tempdir().unwrap();
        let text = r#"[{"id": "go/a/symbols/x/found", "reason": "r"}]"#;
        write_accepted(base.path(), text);
        write_accepted(head.path(), text);
        let base_accepted: Vec<AcceptedFact> = load_accepted(base.path()).unwrap();
        let accepted: Vec<AcceptedFact> = load_accepted(head.path()).unwrap();
        let report = check_accepted_facts(&[], &accepted, &base_accepted);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.notes.len(), 1);
    }

    #[test]
    fn a_new_fact_entry_without_a_regression_fails() {
        let accepted = [accepted_fact("go/a/symbols/x/found", "reason")];
        let report = check_accepted_facts(&[], &accepted, &[]);
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("matches no regression"));
    }

    #[test]
    fn an_inherited_fact_entry_does_not_accept_a_grown_fact() {
        let grown = ["go/a/symbols/x/found".to_string()];
        let entry = [accepted_fact("go/a/symbols/x/found", "old regression")];
        let report = check_accepted_facts(&grown, &entry, &entry);
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("go/a/symbols/x/found"));
        assert!(report.problems[0].contains("new reviewed entry"));
    }

    #[test]
    fn an_inherited_rank_entry_does_not_accept_a_grown_regression() {
        let entry = [accepted_rank("go/q", Some(1), Some(3), "old regression")];
        let report = check_accepted_ranks(&[regression("go/q", Some(1), Some(3))], &entry, &entry);
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("new reviewed entry"));
    }

    #[test]
    fn a_new_rank_pair_accepts_a_grown_regression_over_an_inherited_id() {
        let base = [accepted_rank("go/q", Some(1), Some(3), "old regression")];
        let accepted = [accepted_rank("go/q", Some(1), Some(5), "new reason")];
        let report =
            check_accepted_ranks(&[regression("go/q", Some(1), Some(5))], &accepted, &base);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn a_fact_entry_with_a_new_reason_accepts_a_grown_fact() {
        let grown = ["go/a/symbols/x/found".to_string()];
        let base = [accepted_fact("go/a/symbols/x/found", "old reason")];
        let accepted = [accepted_fact("go/a/symbols/x/found", "new review")];
        let report = check_accepted_facts(&grown, &accepted, &base);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn a_rank_entry_with_a_new_reason_accepts_a_grown_regression() {
        let base = [accepted_rank("go/q", Some(1), Some(3), "old reason")];
        let accepted = [accepted_rank("go/q", Some(1), Some(3), "new review")];
        let report =
            check_accepted_ranks(&[regression("go/q", Some(1), Some(3))], &accepted, &base);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn the_loader_reads_a_missing_and_an_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_accepted::<AcceptedFact>(dir.path()), Ok(Vec::new()));
        std::fs::write(
            dir.path().join(ACCEPTED_FILE),
            r#"[{"id": "a", "reason": "r"}]"#,
        )
        .unwrap();
        assert_eq!(
            load_accepted::<AcceptedFact>(dir.path()),
            Ok(vec![accepted_fact("a", "r")])
        );
        std::fs::write(dir.path().join(ACCEPTED_FILE), "{").unwrap();
        assert!(load_accepted::<AcceptedFact>(dir.path()).is_err());
    }

    #[test]
    fn an_unknown_key_in_the_file_is_an_error() {
        let text = r#"[{"id": "a", "reason": "r", "extra": 1}]"#;
        assert!(serde_json::from_str::<Vec<AcceptedFact>>(text).is_err());
    }
}
