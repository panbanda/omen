//! Debt/SATD component scoring, folding SATD comments and stub findings
//! into a single density-based score.
//!
//! Kept as its own module (mirroring `score::trend`) purely to keep
//! `score/mod.rs` under its line-count budget; the functions here are only
//! ever called from `score::mod`.

use std::collections::HashSet;

/// Debt/SATD component score, combining SATD items with stub findings.
///
/// Either input may be absent (e.g. `report generate` found a `stubs.json`
/// but not a `satd.json`, or vice versa); the component is still computed
/// from whichever is present. Stubs count toward the same "debt units" as
/// SATD items, weighted by severity (High=2.0, Medium=1.0, Low=0.5,
/// mirroring how SATD's own category weights work in
/// `parser::queries::satd`), so unresolved stubs make the debt component
/// score worse just like unresolved SATD comments do.
///
/// A single unfinished site can independently trip both analyzers -- e.g.
/// `// TODO: implement` is both a stub `elision` finding and a generic SATD
/// `TODO` marker -- so SATD items whose (file, line) coincides with any line
/// of a stub finding are excluded from the SATD side of the density; the
/// stub side already counts that site once. With no stubs (or `stubs:
/// None`), no SATD items are excluded and the density reduces to plain SATD
/// item count, so a repo with no stubs scores exactly as it did before this
/// analyzer existed.
pub(super) fn calculate_debt_score(
    satd: Option<&crate::analyzers::satd::Analysis>,
    stubs: Option<&crate::analyzers::stubs::Analysis>,
    file_count: usize,
) -> f64 {
    if file_count == 0 {
        return 100.0;
    }

    let stub_sites: HashSet<(&str, u32)> = stubs
        .map(|s| {
            s.stubs
                .iter()
                .flat_map(|stub| {
                    stub.lines
                        .iter()
                        .map(move |&line| (stub.file.as_str(), line))
                })
                .collect()
        })
        .unwrap_or_default();

    let satd_units = satd
        .map(|s| {
            s.items
                .iter()
                .filter(|item| !stub_sites.contains(&(item.file.as_str(), item.line)))
                .count()
        })
        .unwrap_or(0);

    let stub_units: f64 = stubs
        .map(|s| {
            s.stubs
                .iter()
                .map(|stub| stub_debt_weight(stub.severity))
                .sum()
        })
        .unwrap_or(0.0);

    let density = (satd_units as f64 + stub_units) / file_count as f64;
    score_from_density(density)
}

/// Resolve the full "satd" debt component (score + human-readable details)
/// from SATD and/or stub results. Returns `None` only when both inputs are
/// unavailable, mirroring `ScoreAccumulator::skip`'s semantics for a
/// component that couldn't be computed at all; the caller is expected to
/// fall back to `acc.skip("satd", ...)` in that case using its own error
/// context. Shared between `score::Analyzer::analyze` (live) and
/// `compute_from_components` (from pre-generated JSON) so both apply the
/// same "either input present" and message-formatting rules.
pub(super) fn debt_component(
    satd: Option<&crate::analyzers::satd::Analysis>,
    satd_error: Option<&str>,
    stubs: Option<&crate::analyzers::stubs::Analysis>,
    file_count: usize,
) -> Option<(f64, String)> {
    if satd.is_none() && stubs.is_none() {
        return None;
    }
    let score = calculate_debt_score(satd, stubs, file_count);
    let high_priority = satd
        .map(|s| {
            s.items
                .iter()
                .filter(|i| {
                    matches!(
                        i.severity,
                        crate::analyzers::satd::Severity::Critical
                            | crate::analyzers::satd::Severity::High
                    )
                })
                .count()
        })
        .unwrap_or(0);
    let debt_items = satd.map(|s| s.items.len()).unwrap_or(0);
    let stub_count = stubs.map(|s| s.summary.total_stubs).unwrap_or(0);
    let details = match (satd.is_some(), stub_count) {
        (true, 0) => format!("Found {debt_items} debt items ({high_priority} high priority)"),
        (true, _) => format!(
            "Found {debt_items} debt items ({high_priority} high priority), {stub_count} stub(s)"
        ),
        (false, _) => format!(
            "SATD unavailable ({}); {stub_count} stub(s) found",
            satd_error.unwrap_or("unknown error")
        ),
    };
    Some((score, details))
}

fn stub_debt_weight(severity: crate::analyzers::stubs::Severity) -> f64 {
    match severity {
        crate::analyzers::stubs::Severity::High => 2.0,
        crate::analyzers::stubs::Severity::Medium => 1.0,
        crate::analyzers::stubs::Severity::Low => 0.5,
    }
}

/// Debt-density-to-score banding used by `calculate_debt_score`.
fn score_from_density(density: f64) -> f64 {
    // 0 debt: 100, 0.1 per file: 90, 0.5 per file: 70, 1+ per file: 50 or less
    if density <= 0.0 {
        100.0
    } else if density <= 0.1 {
        90.0 + (0.1 - density) * 100.0
    } else if density <= 0.5 {
        70.0 + (0.5 - density) * 50.0
    } else if density <= 1.0 {
        50.0 + (1.0 - density) * 40.0
    } else {
        (50.0 - (density - 1.0) * 10.0).max(0.0)
    }
}

#[cfg(test)]
pub(super) fn empty_satd() -> crate::analyzers::satd::Analysis {
    crate::analyzers::satd::Analysis::default()
}

#[cfg(test)]
pub(super) fn empty_stubs() -> crate::analyzers::stubs::Analysis {
    crate::analyzers::stubs::Analysis {
        stubs: vec![],
        by_category: std::collections::BTreeMap::new(),
        summary: crate::analyzers::stubs::StubSummary::default(),
    }
}

#[cfg(test)]
pub(super) fn stub_with_severity(
    severity: crate::analyzers::stubs::Severity,
) -> crate::analyzers::stubs::Stub {
    stub_at_line(severity, 1)
}

#[cfg(test)]
pub(super) fn stub_at_line(
    severity: crate::analyzers::stubs::Severity,
    line: u32,
) -> crate::analyzers::stubs::Stub {
    crate::analyzers::stubs::Stub {
        file: "test.rs".to_string(),
        line,
        lines: vec![line],
        category: crate::analyzers::stubs::Category::NotImplemented,
        categories: vec![crate::analyzers::stubs::Category::NotImplemented],
        severity,
        snippet: "todo!()".to_string(),
        language: crate::core::Language::Rust,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_debt_score_none() {
        assert_eq!(calculate_debt_score(None, None, 10), 100.0);
    }

    #[test]
    fn test_calculate_debt_score_zero_files() {
        assert_eq!(calculate_debt_score(Some(&empty_satd()), None, 0), 100.0);
    }

    #[test]
    fn test_calculate_debt_score_low_density() {
        // 1 item in 100 files = 0.01 density, score should be high (> 90)
        let result = crate::analyzers::satd::Analysis {
            items: vec![crate::analyzers::satd::SatdItem {
                file: "test.rs".to_string(),
                line: 1,
                marker: "TODO".to_string(),
                text: "test".to_string(),
                category: "design".to_string(),
                severity: crate::analyzers::satd::Severity::Low,
                weight: 1.0,
            }],
            by_category: std::collections::HashMap::new(),
            density: 0.01,
            summary: crate::analyzers::satd::AnalysisSummary {
                total_items: 1,
                weighted_count: 1.0,
                density: 0.01,
            },
        };
        let score = calculate_debt_score(Some(&result), None, 100);
        assert!(score > 90.0);
    }

    #[test]
    fn test_calculate_debt_score_high_density() {
        // Many items per file
        let items: Vec<_> = (0..20)
            .map(|i| crate::analyzers::satd::SatdItem {
                file: "test.rs".to_string(),
                line: i,
                marker: "TODO".to_string(),
                text: "test".to_string(),
                category: "design".to_string(),
                severity: crate::analyzers::satd::Severity::Low,
                weight: 1.0,
            })
            .collect();
        let result = crate::analyzers::satd::Analysis {
            items,
            by_category: std::collections::HashMap::new(),
            density: 4.0,
            summary: crate::analyzers::satd::AnalysisSummary {
                total_items: 20,
                weighted_count: 20.0,
                density: 4.0,
            },
        };
        let score = calculate_debt_score(Some(&result), None, 5);
        assert!(score < 50.0);
    }

    #[test]
    fn test_calculate_debt_score_zero_stubs_matches_satd_only_score() {
        let satd = empty_satd();
        let with_none = calculate_debt_score(Some(&satd), None, 10);
        let with_empty = calculate_debt_score(Some(&satd), Some(&empty_stubs()), 10);
        assert_eq!(with_none, with_empty);
        assert_eq!(with_none, 100.0);
    }

    #[test]
    fn test_calculate_debt_score_stubs_lower_the_score() {
        let satd = empty_satd();
        let baseline = calculate_debt_score(Some(&satd), None, 10);

        let mut stubs = empty_stubs();
        stubs.stubs = vec![
            stub_with_severity(crate::analyzers::stubs::Severity::High),
            stub_at_line(crate::analyzers::stubs::Severity::High, 2),
        ];
        stubs.summary.total_stubs = 2;
        stubs.summary.high_severity = 2;
        let with_stubs = calculate_debt_score(Some(&satd), Some(&stubs), 10);

        assert!(
            with_stubs < baseline,
            "expected stubs to lower the debt score: baseline={baseline}, with_stubs={with_stubs}"
        );
    }

    #[test]
    fn test_calculate_debt_score_missing_satd_still_uses_stubs() {
        // A report with stubs.json but no satd.json must not silently drop
        // the stub findings from the debt component.
        let mut stubs = empty_stubs();
        stubs.stubs = vec![
            stub_with_severity(crate::analyzers::stubs::Severity::High),
            stub_at_line(crate::analyzers::stubs::Severity::High, 2),
            stub_at_line(crate::analyzers::stubs::Severity::High, 3),
        ];
        stubs.summary.total_stubs = 3;
        stubs.summary.high_severity = 3;

        let with_only_stubs = calculate_debt_score(None, Some(&stubs), 10);
        let clean = calculate_debt_score(None, None, 10);
        assert!(
            with_only_stubs < clean,
            "with_only_stubs={with_only_stubs}, clean={clean}"
        );
    }

    #[test]
    fn test_calculate_debt_score_deduplicates_satd_and_stub_at_same_site() {
        // A comment whose text is both a SATD "TODO" marker and a stub
        // elision finding at the same (file, line) must only count once
        // toward the debt density, not twice.
        let satd = crate::analyzers::satd::Analysis {
            items: vec![crate::analyzers::satd::SatdItem {
                file: "test.rs".to_string(),
                line: 1,
                marker: "TODO".to_string(),
                text: "TODO: implement".to_string(),
                category: "requirement".to_string(),
                severity: crate::analyzers::satd::Severity::Low,
                weight: 0.25,
            }],
            by_category: std::collections::HashMap::new(),
            density: 0.0,
            summary: crate::analyzers::satd::AnalysisSummary::default(),
        };
        let mut stubs = empty_stubs();
        stubs.stubs = vec![stub_with_severity(
            crate::analyzers::stubs::Severity::Medium,
        )]; // line 1
        stubs.summary.total_stubs = 1;
        stubs.summary.medium_severity = 1;

        let combined = calculate_debt_score(Some(&satd), Some(&stubs), 10);
        // Same score as if the SATD item weren't there at all: it's fully
        // shadowed by the co-located stub finding.
        let stubs_only = calculate_debt_score(None, Some(&stubs), 10);
        assert_eq!(combined, stubs_only);
    }

    #[test]
    fn test_calculate_debt_score_keeps_satd_item_on_a_different_line() {
        // Dedup must be precise: a SATD item that merely lives in the same
        // FILE as a stub finding, but on a different line, is a genuinely
        // separate debt site and must still count -- only truly co-located
        // (same file, same line) overlaps are excluded.
        let satd = crate::analyzers::satd::Analysis {
            items: vec![crate::analyzers::satd::SatdItem {
                file: "test.rs".to_string(),
                line: 42,
                marker: "TODO".to_string(),
                text: "TODO: implement".to_string(),
                category: "requirement".to_string(),
                severity: crate::analyzers::satd::Severity::Low,
                weight: 0.25,
            }],
            by_category: std::collections::HashMap::new(),
            density: 0.0,
            summary: crate::analyzers::satd::AnalysisSummary::default(),
        };
        let mut stubs = empty_stubs();
        stubs.stubs = vec![stub_with_severity(
            crate::analyzers::stubs::Severity::Medium,
        )]; // line 1, distinct from the SATD item's line 42
        stubs.summary.total_stubs = 1;
        stubs.summary.medium_severity = 1;

        let combined = calculate_debt_score(Some(&satd), Some(&stubs), 10);
        // The SATD item's debt unit must still apply: this must score
        // strictly worse than the stub-only baseline (which excludes it).
        let stubs_only = calculate_debt_score(None, Some(&stubs), 10);
        assert!(
            combined < stubs_only,
            "expected the different-line SATD item to still lower the score: combined={combined}, stubs_only={stubs_only}"
        );

        // And it must match a hand-computed density that includes both the
        // (undeduped) SATD item and the stub's severity weight.
        let satd_only = calculate_debt_score(Some(&satd), None, 10);
        let expected = score_from_density(1.0 / 10.0 + 1.0 / 10.0); // 1 SATD item + 1 Medium stub (weight 1.0)
        assert_eq!(combined, expected);
        assert!(satd_only > combined);
    }
}
