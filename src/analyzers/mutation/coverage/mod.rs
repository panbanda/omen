//! Coverage integration for mutation testing.
//!
//! This module parses coverage data from various formats and uses it to
//! guide mutation testing by filtering mutants to only covered lines.
//!
//! Supported coverage formats:
//! - LLVM coverage JSON (`cargo llvm-cov --json`)
//! - Istanbul/NYC JSON (JavaScript/TypeScript)
//! - coverage.py JSON (Python)

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::{Error, Result};

use super::{Mutant, MutationResult};

/// Coverage data parsed from a coverage report.
///
/// Maps file paths to the set of line numbers that are covered by tests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageData {
    /// Map of file path to covered line numbers (1-indexed).
    pub files: HashMap<PathBuf, HashSet<u32>>,
}

impl CoverageData {
    /// Create new empty coverage data.
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Check if a specific location is covered by tests.
    ///
    /// Returns `true` if the line is covered, `false` if not covered or file not found.
    pub fn is_covered(&self, path: &Path, line: u32) -> bool {
        self.files
            .get(path)
            .map(|lines| lines.contains(&line))
            .unwrap_or(false)
    }

    /// Filter mutants to only those on covered lines.
    ///
    /// Returns a tuple of (covered_mutants, uncovered_mutants).
    pub fn filter_covered(&self, mutants: Vec<Mutant>) -> (Vec<Mutant>, Vec<Mutant>) {
        let mut covered = Vec::new();
        let mut uncovered = Vec::new();

        for mutant in mutants {
            if self.is_covered(&mutant.file_path, mutant.line) {
                covered.push(mutant);
            } else {
                uncovered.push(mutant);
            }
        }

        (covered, uncovered)
    }

    /// Calculate coverage-weighted mutation score.
    ///
    /// This weights each mutant by whether it's on a covered line.
    /// Mutants on covered lines that survive are worse than mutants on
    /// uncovered lines that survive (since uncovered lines aren't tested anyway).
    ///
    /// Formula: weighted_score = covered_killed / covered_total
    /// This only counts mutants on covered lines, giving a more accurate
    /// picture of test effectiveness.
    pub fn weighted_score(&self, results: &[MutationResult]) -> f64 {
        let mut covered_killed = 0;
        let mut covered_total = 0;

        for result in results {
            if self.is_covered(&result.mutant.file_path, result.mutant.line)
                && result.status.counts_for_score()
            {
                covered_total += 1;
                if result.status.is_killed() {
                    covered_killed += 1;
                }
            }
        }

        if covered_total == 0 {
            return 0.0;
        }

        covered_killed as f64 / covered_total as f64
    }

    /// Get total number of covered files.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Get total number of covered lines across all files.
    pub fn total_covered_lines(&self) -> usize {
        self.files.values().map(|lines| lines.len()).sum()
    }

    /// Merge coverage data from another source.
    pub fn merge(&mut self, other: CoverageData) {
        for (path, lines) in other.files {
            self.files.entry(path).or_default().extend(lines);
        }
    }
}

/// Mutation-coverage matrix mapping mutants to tests and vice versa.
///
/// This allows efficient lookup of which tests are relevant for a given mutant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageMatrix {
    /// Map from mutant ID to test names that cover the mutant's location.
    pub mutant_to_tests: HashMap<String, Vec<String>>,
    /// Map from test name to mutant IDs that the test covers.
    pub test_to_mutants: HashMap<String, Vec<String>>,
}

impl CoverageMatrix {
    /// Build a coverage matrix from coverage data and mutants.
    ///
    /// Note: This is a simplified implementation that maps mutants to tests
    /// based on file coverage. For full accuracy, you'd need per-test coverage.
    pub fn build(_coverage: &CoverageData, mutants: &[Mutant]) -> Self {
        let mut mutant_to_tests: HashMap<String, Vec<String>> = HashMap::new();
        let mut test_to_mutants: HashMap<String, Vec<String>> = HashMap::new();

        // For each mutant, we create a placeholder test entry
        // In practice, this would be populated from per-test coverage data
        for mutant in mutants {
            let test_name = format!("test_{}", mutant.file_path.display());
            mutant_to_tests
                .entry(mutant.id.clone())
                .or_default()
                .push(test_name.clone());
            test_to_mutants
                .entry(test_name)
                .or_default()
                .push(mutant.id.clone());
        }

        Self {
            mutant_to_tests,
            test_to_mutants,
        }
    }

    /// Get tests relevant to a specific mutant.
    pub fn get_relevant_tests(&self, mutant_id: &str) -> &[String] {
        self.mutant_to_tests
            .get(mutant_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get mutants covered by a specific test.
    pub fn get_covered_mutants(&self, test_name: &str) -> &[String] {
        self.test_to_mutants
            .get(test_name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

// ============================================================================
// LLVM Coverage Format
// ============================================================================

#[derive(Debug, Deserialize)]
struct LlvmCoverageReport {
    data: Vec<LlvmCoverageData>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    report_type: Option<String>,
    #[allow(dead_code)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlvmCoverageData {
    files: Vec<LlvmFileCoverage>,
}

#[derive(Debug, Deserialize)]
struct LlvmFileCoverage {
    filename: String,
    segments: Vec<LlvmSegment>,
}

// Segment format: [line, column, count, has_count, is_region_entry, is_gap]
type LlvmSegment = (u32, u32, u64, bool, bool, bool);

/// Parse LLVM coverage JSON output (from `cargo llvm-cov --json`).
pub fn parse_llvm_cov(path: &Path) -> Result<CoverageData> {
    let content = fs::read_to_string(path)?;
    parse_llvm_cov_str(&content)
}

fn parse_llvm_cov_str(content: &str) -> Result<CoverageData> {
    let report: LlvmCoverageReport = serde_json::from_str(content)
        .map_err(|e| Error::analysis(format!("Failed to parse LLVM coverage JSON: {e}")))?;

    let mut coverage = CoverageData::new();

    for data in report.data {
        for file in data.files {
            let path = PathBuf::from(&file.filename);
            let mut covered_lines = HashSet::new();

            for segment in file.segments {
                let (line, _column, count, has_count, _is_entry, _is_gap) = segment;
                // A line is covered if has_count is true and count > 0
                if has_count && count > 0 {
                    covered_lines.insert(line);
                }
            }

            if !covered_lines.is_empty() {
                coverage.files.insert(path, covered_lines);
            }
        }
    }

    Ok(coverage)
}

// ============================================================================
// Istanbul/NYC Coverage Format
// ============================================================================

#[derive(Debug, Deserialize)]
struct IstanbulFileCoverage {
    path: String,
    #[serde(rename = "statementMap")]
    statement_map: HashMap<String, IstanbulLocation>,
    s: HashMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct IstanbulLocation {
    start: IstanbulPosition,
    #[allow(dead_code)]
    end: IstanbulPosition,
}

#[derive(Debug, Deserialize)]
struct IstanbulPosition {
    line: u32,
    #[allow(dead_code)]
    column: u32,
}

/// Parse Istanbul/NYC coverage JSON output.
pub fn parse_istanbul(path: &Path) -> Result<CoverageData> {
    let content = fs::read_to_string(path)?;
    parse_istanbul_str(&content)
}

fn parse_istanbul_str(content: &str) -> Result<CoverageData> {
    let report: HashMap<String, IstanbulFileCoverage> = serde_json::from_str(content)
        .map_err(|e| Error::analysis(format!("Failed to parse Istanbul coverage JSON: {e}")))?;

    let mut coverage = CoverageData::new();

    for file_coverage in report.values() {
        let path = PathBuf::from(&file_coverage.path);
        let mut covered_lines = HashSet::new();

        for (stmt_id, location) in &file_coverage.statement_map {
            if let Some(&count) = file_coverage.s.get(stmt_id) {
                if count > 0 {
                    covered_lines.insert(location.start.line);
                }
            }
        }

        if !covered_lines.is_empty() {
            coverage.files.insert(path, covered_lines);
        }
    }

    Ok(coverage)
}

// ============================================================================
// coverage.py Format
// ============================================================================

#[derive(Debug, Deserialize)]
struct CoveragePyReport {
    files: HashMap<String, CoveragePyFile>,
}

#[derive(Debug, Deserialize)]
struct CoveragePyFile {
    executed_lines: Vec<u32>,
}

/// Parse coverage.py JSON output.
pub fn parse_coverage_py(path: &Path) -> Result<CoverageData> {
    let content = fs::read_to_string(path)?;
    parse_coverage_py_str(&content)
}

fn parse_coverage_py_str(content: &str) -> Result<CoverageData> {
    let report: CoveragePyReport = serde_json::from_str(content)
        .map_err(|e| Error::analysis(format!("Failed to parse coverage.py JSON: {e}")))?;

    let mut coverage = CoverageData::new();

    for (file_path, file_data) in report.files {
        let path = PathBuf::from(file_path);
        let covered_lines: HashSet<u32> = file_data.executed_lines.into_iter().collect();

        if !covered_lines.is_empty() {
            coverage.files.insert(path, covered_lines);
        }
    }

    Ok(coverage)
}

/// Automatically detect coverage format and parse.
pub fn parse_auto(path: &Path) -> Result<CoverageData> {
    let content = fs::read_to_string(path)?;

    // Try LLVM format first (has "type" field)
    if content.contains("\"type\":") && content.contains("llvm.coverage") {
        return parse_llvm_cov_str(&content);
    }

    // Try coverage.py format (has "files" with "executed_lines")
    if content.contains("\"executed_lines\"") {
        return parse_coverage_py_str(&content);
    }

    // Try Istanbul format (has "statementMap")
    if content.contains("\"statementMap\"") {
        return parse_istanbul_str(&content);
    }

    Err(Error::analysis(
        "Could not detect coverage format. Supported: llvm-cov, istanbul, coverage.py",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzers::mutation::MutantStatus;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("coverage")
            .join(name)
    }

    fn make_mutant(id: &str, path: &str, line: u32) -> Mutant {
        Mutant::new(id, path, "CRR", line, 1, "42", "0", "test mutation", (0, 2))
    }

    fn make_result(mutant: Mutant, status: MutantStatus) -> MutationResult {
        MutationResult::new(mutant, status, 100)
    }

    // ========================================================================
    // CoverageData tests
    // ========================================================================

    #[test]
    fn test_coverage_data_new_is_empty() {
        let coverage = CoverageData::new();
        assert!(coverage.files.is_empty());
        assert_eq!(coverage.file_count(), 0);
        assert_eq!(coverage.total_covered_lines(), 0);
    }

    #[test]
    fn test_coverage_data_is_covered_returns_true_for_covered_line() {
        let mut coverage = CoverageData::new();
        let path = PathBuf::from("src/main.rs");
        let mut lines = HashSet::new();
        lines.insert(10);
        lines.insert(20);
        coverage.files.insert(path.clone(), lines);

        assert!(coverage.is_covered(&path, 10));
        assert!(coverage.is_covered(&path, 20));
    }

    #[test]
    fn test_coverage_data_is_covered_returns_false_for_uncovered_line() {
        let mut coverage = CoverageData::new();
        let path = PathBuf::from("src/main.rs");
        let mut lines = HashSet::new();
        lines.insert(10);
        coverage.files.insert(path.clone(), lines);

        assert!(!coverage.is_covered(&path, 15));
        assert!(!coverage.is_covered(&path, 1));
    }

    #[test]
    fn test_coverage_data_is_covered_returns_false_for_unknown_file() {
        let coverage = CoverageData::new();
        let path = PathBuf::from("nonexistent.rs");

        assert!(!coverage.is_covered(&path, 10));
    }

    #[test]
    fn test_coverage_data_filter_covered_separates_mutants() {
        let mut coverage = CoverageData::new();
        let path = PathBuf::from("src/main.rs");
        let mut lines = HashSet::new();
        lines.insert(10);
        lines.insert(20);
        coverage.files.insert(path, lines);

        let mutants = vec![
            make_mutant("1", "src/main.rs", 10),  // covered
            make_mutant("2", "src/main.rs", 15),  // not covered
            make_mutant("3", "src/main.rs", 20),  // covered
            make_mutant("4", "src/other.rs", 10), // different file
        ];

        let (covered, uncovered) = coverage.filter_covered(mutants);

        assert_eq!(covered.len(), 2);
        assert_eq!(uncovered.len(), 2);
        assert_eq!(covered[0].id, "1");
        assert_eq!(covered[1].id, "3");
        assert_eq!(uncovered[0].id, "2");
        assert_eq!(uncovered[1].id, "4");
    }

    #[test]
    fn test_coverage_data_filter_covered_empty_mutants() {
        let coverage = CoverageData::new();
        let (covered, uncovered) = coverage.filter_covered(vec![]);

        assert!(covered.is_empty());
        assert!(uncovered.is_empty());
    }

    #[test]
    fn test_coverage_data_filter_covered_all_covered() {
        let mut coverage = CoverageData::new();
        let path = PathBuf::from("src/main.rs");
        let mut lines = HashSet::new();
        lines.insert(1);
        lines.insert(2);
        lines.insert(3);
        coverage.files.insert(path, lines);

        let mutants = vec![
            make_mutant("1", "src/main.rs", 1),
            make_mutant("2", "src/main.rs", 2),
            make_mutant("3", "src/main.rs", 3),
        ];

        let (covered, uncovered) = coverage.filter_covered(mutants);

        assert_eq!(covered.len(), 3);
        assert!(uncovered.is_empty());
    }

    #[test]
    fn test_coverage_data_filter_covered_none_covered() {
        let coverage = CoverageData::new();

        let mutants = vec![
            make_mutant("1", "src/main.rs", 1),
            make_mutant("2", "src/main.rs", 2),
        ];

        let (covered, uncovered) = coverage.filter_covered(mutants);

        assert!(covered.is_empty());
        assert_eq!(uncovered.len(), 2);
    }

    #[test]
    fn test_coverage_data_weighted_score_all_killed() {
        let mut coverage = CoverageData::new();
        let path = PathBuf::from("src/main.rs");
        let mut lines = HashSet::new();
        lines.insert(10);
        lines.insert(20);
        coverage.files.insert(path, lines);

        let results = vec![
            make_result(make_mutant("1", "src/main.rs", 10), MutantStatus::Killed),
            make_result(make_mutant("2", "src/main.rs", 20), MutantStatus::Killed),
        ];

        let score = coverage.weighted_score(&results);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coverage_data_weighted_score_mixed() {
        let mut coverage = CoverageData::new();
        let path = PathBuf::from("src/main.rs");
        let mut lines = HashSet::new();
        lines.insert(10);
        lines.insert(20);
        coverage.files.insert(path, lines);

        let results = vec![
            make_result(make_mutant("1", "src/main.rs", 10), MutantStatus::Killed),
            make_result(make_mutant("2", "src/main.rs", 20), MutantStatus::Survived),
        ];

        let score = coverage.weighted_score(&results);
        assert!((score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coverage_data_weighted_score_ignores_uncovered() {
        let mut coverage = CoverageData::new();
        let path = PathBuf::from("src/main.rs");
        let mut lines = HashSet::new();
        lines.insert(10);
        coverage.files.insert(path, lines);

        let results = vec![
            make_result(make_mutant("1", "src/main.rs", 10), MutantStatus::Killed),
            make_result(make_mutant("2", "src/main.rs", 99), MutantStatus::Survived), // uncovered
        ];

        // Only the covered mutant (killed) should count
        let score = coverage.weighted_score(&results);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coverage_data_weighted_score_empty() {
        let coverage = CoverageData::new();
        let results: Vec<MutationResult> = vec![];

        let score = coverage.weighted_score(&results);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coverage_data_weighted_score_no_covered_mutants() {
        let coverage = CoverageData::new();
        let results = vec![make_result(
            make_mutant("1", "src/main.rs", 10),
            MutantStatus::Survived,
        )];

        let score = coverage.weighted_score(&results);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_coverage_data_merge() {
        let mut coverage1 = CoverageData::new();
        let path1 = PathBuf::from("src/a.rs");
        let mut lines1 = HashSet::new();
        lines1.insert(1);
        lines1.insert(2);
        coverage1.files.insert(path1, lines1);

        let mut coverage2 = CoverageData::new();
        let path2 = PathBuf::from("src/b.rs");
        let mut lines2 = HashSet::new();
        lines2.insert(10);
        coverage2.files.insert(path2, lines2);

        // Also add overlapping file
        let path1_again = PathBuf::from("src/a.rs");
        let mut more_lines = HashSet::new();
        more_lines.insert(3);
        coverage2.files.insert(path1_again, more_lines);

        coverage1.merge(coverage2);

        assert_eq!(coverage1.file_count(), 2);
        assert!(coverage1.is_covered(&PathBuf::from("src/a.rs"), 1));
        assert!(coverage1.is_covered(&PathBuf::from("src/a.rs"), 2));
        assert!(coverage1.is_covered(&PathBuf::from("src/a.rs"), 3));
        assert!(coverage1.is_covered(&PathBuf::from("src/b.rs"), 10));
    }

    #[test]
    fn test_coverage_data_file_count() {
        let mut coverage = CoverageData::new();
        assert_eq!(coverage.file_count(), 0);

        coverage.files.insert(PathBuf::from("a.rs"), HashSet::new());
        assert_eq!(coverage.file_count(), 1);

        coverage.files.insert(PathBuf::from("b.rs"), HashSet::new());
        assert_eq!(coverage.file_count(), 2);
    }

    #[test]
    fn test_coverage_data_total_covered_lines() {
        let mut coverage = CoverageData::new();
        assert_eq!(coverage.total_covered_lines(), 0);

        let mut lines1 = HashSet::new();
        lines1.insert(1);
        lines1.insert(2);
        coverage.files.insert(PathBuf::from("a.rs"), lines1);

        let mut lines2 = HashSet::new();
        lines2.insert(10);
        lines2.insert(20);
        lines2.insert(30);
        coverage.files.insert(PathBuf::from("b.rs"), lines2);

        assert_eq!(coverage.total_covered_lines(), 5);
    }

    // ========================================================================
    // CoverageMatrix tests
    // ========================================================================

    #[test]
    fn test_coverage_matrix_build() {
        let coverage = CoverageData::new();
        let mutants = vec![
            make_mutant("1", "src/main.rs", 10),
            make_mutant("2", "src/lib.rs", 20),
        ];

        let matrix = CoverageMatrix::build(&coverage, &mutants);

        assert_eq!(matrix.mutant_to_tests.len(), 2);
        assert!(matrix.mutant_to_tests.contains_key("1"));
        assert!(matrix.mutant_to_tests.contains_key("2"));
    }

    #[test]
    fn test_coverage_matrix_get_relevant_tests() {
        let coverage = CoverageData::new();
        let mutants = vec![make_mutant("1", "src/main.rs", 10)];

        let matrix = CoverageMatrix::build(&coverage, &mutants);

        let tests = matrix.get_relevant_tests("1");
        assert!(!tests.is_empty());
        assert!(tests[0].contains("src/main.rs"));
    }

    #[test]
    fn test_coverage_matrix_get_relevant_tests_unknown_mutant() {
        let matrix = CoverageMatrix::default();
        let tests = matrix.get_relevant_tests("nonexistent");
        assert!(tests.is_empty());
    }

    #[test]
    fn test_coverage_matrix_get_covered_mutants() {
        let coverage = CoverageData::new();
        let mutants = vec![make_mutant("1", "src/main.rs", 10)];

        let matrix = CoverageMatrix::build(&coverage, &mutants);

        let test_name = format!("test_{}", "src/main.rs");
        let covered = matrix.get_covered_mutants(&test_name);
        assert_eq!(covered.len(), 1);
        assert_eq!(covered[0], "1");
    }

    #[test]
    fn test_coverage_matrix_get_covered_mutants_unknown_test() {
        let matrix = CoverageMatrix::default();
        let covered = matrix.get_covered_mutants("nonexistent_test");
        assert!(covered.is_empty());
    }

    // ========================================================================
    // LLVM coverage parser tests
    // ========================================================================

    #[test]
    fn test_parse_llvm_cov_valid() {
        let path = fixture_path("llvm-cov.json");
        let coverage = parse_llvm_cov(&path).expect("Failed to parse");

        assert_eq!(coverage.file_count(), 2);
        assert!(coverage.is_covered(&PathBuf::from("src/main.rs"), 1));
        assert!(coverage.is_covered(&PathBuf::from("src/main.rs"), 2));
        assert!(!coverage.is_covered(&PathBuf::from("src/main.rs"), 3)); // count=0
        assert!(coverage.is_covered(&PathBuf::from("src/lib.rs"), 1));
    }

    #[test]
    fn test_parse_llvm_cov_uncovered_lines() {
        let path = fixture_path("llvm-cov.json");
        let coverage = parse_llvm_cov(&path).expect("Failed to parse");

        // Lines with count=0 should not be covered
        assert!(!coverage.is_covered(&PathBuf::from("src/main.rs"), 3));
        assert!(!coverage.is_covered(&PathBuf::from("src/main.rs"), 11));
        assert!(!coverage.is_covered(&PathBuf::from("src/main.rs"), 12));
    }

    #[test]
    fn test_parse_llvm_cov_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/coverage.json");
        let result = parse_llvm_cov(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llvm_cov_malformed() {
        let path = fixture_path("malformed.json");
        let result = parse_llvm_cov(&path);
        assert!(result.is_err());
    }

    // ========================================================================
    // Istanbul parser tests
    // ========================================================================

    #[test]
    fn test_parse_istanbul_valid() {
        let path = fixture_path("istanbul.json");
        let coverage = parse_istanbul(&path).expect("Failed to parse");

        assert_eq!(coverage.file_count(), 2);
        assert!(coverage.is_covered(&PathBuf::from("src/index.ts"), 1));
        assert!(coverage.is_covered(&PathBuf::from("src/index.ts"), 2));
        assert!(coverage.is_covered(&PathBuf::from("src/index.ts"), 10));
        assert!(!coverage.is_covered(&PathBuf::from("src/index.ts"), 5)); // count=0
    }

    #[test]
    fn test_parse_istanbul_uncovered_statements() {
        let path = fixture_path("istanbul.json");
        let coverage = parse_istanbul(&path).expect("Failed to parse");

        // Statements with count=0 should not be covered
        assert!(!coverage.is_covered(&PathBuf::from("src/index.ts"), 5));
        assert!(!coverage.is_covered(&PathBuf::from("src/utils.ts"), 3));
    }

    #[test]
    fn test_parse_istanbul_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/coverage.json");
        let result = parse_istanbul(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_istanbul_malformed() {
        let path = fixture_path("malformed.json");
        let result = parse_istanbul(&path);
        assert!(result.is_err());
    }

    // ========================================================================
    // coverage.py parser tests
    // ========================================================================

    #[test]
    fn test_parse_coverage_py_valid() {
        let path = fixture_path("coverage-py.json");
        let coverage = parse_coverage_py(&path).expect("Failed to parse");

        assert_eq!(coverage.file_count(), 2);
        assert!(coverage.is_covered(&PathBuf::from("src/main.py"), 1));
        assert!(coverage.is_covered(&PathBuf::from("src/main.py"), 2));
        assert!(coverage.is_covered(&PathBuf::from("src/main.py"), 15));
        assert!(!coverage.is_covered(&PathBuf::from("src/main.py"), 4)); // missing
    }

    #[test]
    fn test_parse_coverage_py_executed_lines() {
        let path = fixture_path("coverage-py.json");
        let coverage = parse_coverage_py(&path).expect("Failed to parse");

        // Check specific executed lines from fixture
        let main_py = PathBuf::from("src/main.py");
        for line in [1, 2, 3, 5, 8, 10, 15] {
            assert!(
                coverage.is_covered(&main_py, line),
                "Line {line} should be covered"
            );
        }
    }

    #[test]
    fn test_parse_coverage_py_missing_lines() {
        let path = fixture_path("coverage-py.json");
        let coverage = parse_coverage_py(&path).expect("Failed to parse");

        // Check missing lines from fixture
        let main_py = PathBuf::from("src/main.py");
        for line in [4, 6, 7, 11, 12] {
            assert!(
                !coverage.is_covered(&main_py, line),
                "Line {line} should NOT be covered"
            );
        }
    }

    #[test]
    fn test_parse_coverage_py_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/coverage.json");
        let result = parse_coverage_py(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_coverage_py_malformed() {
        let path = fixture_path("malformed.json");
        let result = parse_coverage_py(&path);
        assert!(result.is_err());
    }

    // ========================================================================
    // Auto-detection tests
    // ========================================================================

    #[test]
    fn test_parse_auto_llvm() {
        let path = fixture_path("llvm-cov.json");
        let coverage = parse_auto(&path).expect("Failed to parse");

        assert!(coverage.file_count() > 0);
        assert!(coverage.is_covered(&PathBuf::from("src/main.rs"), 1));
    }

    #[test]
    fn test_parse_auto_istanbul() {
        let path = fixture_path("istanbul.json");
        let coverage = parse_auto(&path).expect("Failed to parse");

        assert!(coverage.file_count() > 0);
        assert!(coverage.is_covered(&PathBuf::from("src/index.ts"), 1));
    }

    #[test]
    fn test_parse_auto_coverage_py() {
        let path = fixture_path("coverage-py.json");
        let coverage = parse_auto(&path).expect("Failed to parse");

        assert!(coverage.file_count() > 0);
        assert!(coverage.is_covered(&PathBuf::from("src/main.py"), 1));
    }

    #[test]
    fn test_parse_auto_unknown_format() {
        let path = fixture_path("malformed.json");
        let result = parse_auto(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_auto_empty_file() {
        let path = fixture_path("empty.json");
        let result = parse_auto(&path);
        assert!(result.is_err());
    }

    // ========================================================================
    // Serialization tests
    // ========================================================================

    #[test]
    fn test_coverage_data_serialization() {
        let mut coverage = CoverageData::new();
        let mut lines = HashSet::new();
        lines.insert(1);
        lines.insert(2);
        coverage.files.insert(PathBuf::from("test.rs"), lines);

        let json = serde_json::to_string(&coverage).expect("serialize");
        let deserialized: CoverageData = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.file_count(), 1);
        assert!(deserialized.is_covered(&PathBuf::from("test.rs"), 1));
        assert!(deserialized.is_covered(&PathBuf::from("test.rs"), 2));
    }

    #[test]
    fn test_coverage_matrix_serialization() {
        let coverage = CoverageData::new();
        let mutants = vec![make_mutant("1", "test.rs", 10)];
        let matrix = CoverageMatrix::build(&coverage, &mutants);

        let json = serde_json::to_string(&matrix).expect("serialize");
        let deserialized: CoverageMatrix = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.mutant_to_tests.len(), 1);
    }
}
