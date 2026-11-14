//! Reviewed regressions that the base-branch comparison accepts.
//!
//! A pull request that makes a rank or a truth fact worse than the base
//! branch fails `ledger_never_grows`. The pull request can list the
//! regression in an `accepted_regressions.json` file. A reviewer then accepts
//! the regression in the review of that file. An entry must match a regression
//! exactly. An entry that matches no regression against the base branch is an
//! error. After the merge, the base branch has the regression. Thus the next
//! pull request must delete the entry.

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

/// The problems of the rank regressions that the pull request does not
/// accept, and of the accepted entries that match no regression.
///
/// An empty result means that each regression has an entry and each entry has
/// a regression.
pub fn check_accepted_ranks(
    regressions: &[RankRegression],
    accepted: &[AcceptedRankRegression],
) -> Vec<String> {
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
        if !found {
            problems.insert(format!(
                "{}: the accepted entry matches no regression against the base branch; remove it",
                entry.id
            ));
        }
    }
    problems.into_iter().collect()
}

/// The ids of the facts that the pull request does not accept, and the
/// problems of the accepted entries that match no fact.
///
/// `grown` has the ids that `ledger_growth` found.
pub fn check_accepted_facts(grown: &[String], accepted: &[AcceptedFact]) -> Vec<String> {
    let mut problems = BTreeSet::new();
    file_problems(
        accepted
            .iter()
            .map(|entry| (entry.id.as_str(), entry.reason.as_str())),
        &mut problems,
    );
    let accepted_ids: BTreeSet<&str> = accepted.iter().map(|entry| entry.id.as_str()).collect();
    for id in grown {
        if !accepted_ids.contains(id.as_str()) {
            problems.insert(id.clone());
        }
    }
    let grown_ids: BTreeSet<&str> = grown.iter().map(String::as_str).collect();
    for entry in accepted {
        if !grown_ids.contains(entry.id.as_str()) {
            problems.insert(format!(
                "{}: the accepted entry matches no regression against the base branch; remove it",
                entry.id
            ));
        }
    }
    problems.into_iter().collect()
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

    #[test]
    fn a_regression_without_an_entry_fails() {
        let problems = check_accepted_ranks(&[regression("go/q", Some(1), Some(3))], &[]);
        assert_eq!(problems, ["go/q: rank 3, the base branch has rank 1"]);
    }

    #[test]
    fn an_exact_entry_accepts_a_regression() {
        let problems = check_accepted_ranks(
            &[regression("go/q", Some(1), None)],
            &[accepted_rank("go/q", Some(1), None, "the label was wrong")],
        );
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn an_entry_with_other_ranks_neither_accepts_nor_stays() {
        let problems = check_accepted_ranks(
            &[regression("go/q", Some(1), Some(5))],
            &[accepted_rank("go/q", Some(1), Some(3), "reason")],
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
    }

    #[test]
    fn an_entry_without_a_regression_fails() {
        let problems =
            check_accepted_ranks(&[], &[accepted_rank("go/q", Some(1), Some(3), "reason")]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("matches no regression"));
    }

    #[test]
    fn an_entry_that_is_not_worse_fails() {
        let problems =
            check_accepted_ranks(&[], &[accepted_rank("go/q", Some(3), Some(1), "reason")]);
        assert!(problems
            .iter()
            .any(|problem| problem.contains("not a regression")));
    }

    #[test]
    fn a_blank_reason_and_a_wrong_order_fail() {
        let problems = check_accepted_ranks(
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
        let problems = check_accepted_facts(
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
        let problems =
            check_accepted_facts(&grown, &[accepted_fact("go/a/symbols/x/found", "reason")]);
        assert!(problems.is_empty());
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
