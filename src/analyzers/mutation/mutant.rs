//! Mutant types for mutation testing.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A single mutant representing a code mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mutant {
    /// Unique identifier for the mutant.
    pub id: String,
    /// Path to the file containing the mutant.
    pub file_path: PathBuf,
    /// Name of the mutation operator that generated this mutant.
    pub operator: String,
    /// Line number (1-indexed).
    pub line: u32,
    /// Column number (1-indexed).
    pub column: u32,
    /// Original code that will be replaced.
    pub original: String,
    /// Replacement code.
    pub replacement: String,
    /// Human-readable description of the mutation.
    pub description: String,
    /// Byte range in the original source (start, end).
    pub byte_range: (usize, usize),
}

impl Mutant {
    /// Create a new mutant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        file_path: impl Into<PathBuf>,
        operator: impl Into<String>,
        line: u32,
        column: u32,
        original: impl Into<String>,
        replacement: impl Into<String>,
        description: impl Into<String>,
        byte_range: (usize, usize),
    ) -> Self {
        Self {
            id: id.into(),
            file_path: file_path.into(),
            operator: operator.into(),
            line,
            column,
            original: original.into(),
            replacement: replacement.into(),
            description: description.into(),
            byte_range,
        }
    }

    /// Apply this mutant to the given source content.
    ///
    /// Returns the mutated source code.
    pub fn apply(&self, source: &[u8]) -> Vec<u8> {
        let (start, end) = self.byte_range;
        let mut result = Vec::with_capacity(source.len() - (end - start) + self.replacement.len());
        result.extend_from_slice(&source[..start]);
        result.extend_from_slice(self.replacement.as_bytes());
        result.extend_from_slice(&source[end..]);
        result
    }
}

/// Status of a mutant after test execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutantStatus {
    /// Test failed - the mutant was detected (good).
    Killed,
    /// Test passed - the mutant was not detected (bad).
    Survived,
    /// Test execution timed out.
    Timeout,
    /// Build failed when applying the mutant.
    BuildError,
    /// Mutant is equivalent to original (can't be killed).
    Equivalent,
    /// Mutant was not executed (e.g., dry run).
    Pending,
    /// Mutant was skipped by ML prediction (predicted to be killed).
    Skipped,
}

impl MutantStatus {
    /// Returns true if this status represents a detected mutant.
    pub fn is_killed(&self) -> bool {
        matches!(self, Self::Killed)
    }

    /// Returns true if this status represents an undetected mutant.
    pub fn is_survived(&self) -> bool {
        matches!(self, Self::Survived)
    }

    /// Returns true if this status should be counted in the mutation score.
    pub fn counts_for_score(&self) -> bool {
        matches!(self, Self::Killed | Self::Survived)
    }
}

/// Result of running tests against a mutant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationResult {
    /// The mutant that was tested.
    pub mutant: Mutant,
    /// Status after test execution.
    pub status: MutantStatus,
    /// Duration of test execution in milliseconds.
    pub duration_ms: u64,
    /// Optional output from the test command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl MutationResult {
    /// Create a new mutation result.
    pub fn new(mutant: Mutant, status: MutantStatus, duration_ms: u64) -> Self {
        Self {
            mutant,
            status,
            duration_ms,
            output: None,
        }
    }

    /// Create a mutation result with output.
    pub fn with_output(mut self, output: impl Into<String>) -> Self {
        self.output = Some(output.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutant_new() {
        let mutant = Mutant::new(
            "mut-1",
            "src/main.rs",
            "CRR",
            10,
            5,
            "42",
            "0",
            "Replace constant 42 with 0",
            (100, 102),
        );

        assert_eq!(mutant.id, "mut-1");
        assert_eq!(mutant.file_path, PathBuf::from("src/main.rs"));
        assert_eq!(mutant.operator, "CRR");
        assert_eq!(mutant.line, 10);
        assert_eq!(mutant.column, 5);
        assert_eq!(mutant.original, "42");
        assert_eq!(mutant.replacement, "0");
        assert_eq!(mutant.byte_range, (100, 102));
    }

    #[test]
    fn test_mutant_apply() {
        let mutant = Mutant::new(
            "mut-1",
            "test.rs",
            "CRR",
            1,
            1,
            "42",
            "0",
            "Replace 42 with 0",
            (8, 10),
        );

        let source = b"let x = 42;";
        let result = mutant.apply(source);
        assert_eq!(result, b"let x = 0;");
    }

    #[test]
    fn test_mutant_apply_longer_replacement() {
        let mutant = Mutant::new(
            "mut-1",
            "test.rs",
            "CRR",
            1,
            1,
            "1",
            "100",
            "Replace 1 with 100",
            (8, 9),
        );

        let source = b"let x = 1;";
        let result = mutant.apply(source);
        assert_eq!(result, b"let x = 100;");
    }

    #[test]
    fn test_mutant_apply_shorter_replacement() {
        let mutant = Mutant::new(
            "mut-1",
            "test.rs",
            "CRR",
            1,
            1,
            "100",
            "0",
            "Replace 100 with 0",
            (8, 11),
        );

        let source = b"let x = 100;";
        let result = mutant.apply(source);
        assert_eq!(result, b"let x = 0;");
    }

    #[test]
    fn test_mutant_status_is_killed() {
        assert!(MutantStatus::Killed.is_killed());
        assert!(!MutantStatus::Survived.is_killed());
        assert!(!MutantStatus::Timeout.is_killed());
    }

    #[test]
    fn test_mutant_status_is_survived() {
        assert!(!MutantStatus::Killed.is_survived());
        assert!(MutantStatus::Survived.is_survived());
        assert!(!MutantStatus::Timeout.is_survived());
    }

    #[test]
    fn test_mutant_status_counts_for_score() {
        assert!(MutantStatus::Killed.counts_for_score());
        assert!(MutantStatus::Survived.counts_for_score());
        assert!(!MutantStatus::Timeout.counts_for_score());
        assert!(!MutantStatus::BuildError.counts_for_score());
        assert!(!MutantStatus::Equivalent.counts_for_score());
        assert!(!MutantStatus::Pending.counts_for_score());
    }

    #[test]
    fn test_mutation_result_new() {
        let mutant = Mutant::new("mut-1", "test.rs", "CRR", 1, 1, "42", "0", "desc", (0, 2));

        let result = MutationResult::new(mutant, MutantStatus::Killed, 150);

        assert_eq!(result.status, MutantStatus::Killed);
        assert_eq!(result.duration_ms, 150);
        assert!(result.output.is_none());
    }

    #[test]
    fn test_mutation_result_with_output() {
        let mutant = Mutant::new("mut-1", "test.rs", "CRR", 1, 1, "42", "0", "desc", (0, 2));

        let result =
            MutationResult::new(mutant, MutantStatus::Killed, 150).with_output("test output");

        assert_eq!(result.output, Some("test output".to_string()));
    }

    #[test]
    fn test_mutant_serialization() {
        let mutant = Mutant::new("mut-1", "test.rs", "CRR", 1, 1, "42", "0", "desc", (0, 2));

        let json = serde_json::to_string(&mutant).unwrap();
        assert!(json.contains("\"id\":\"mut-1\""));
        assert!(json.contains("\"operator\":\"CRR\""));
    }

    #[test]
    fn test_mutant_status_serialization() {
        assert_eq!(
            serde_json::to_string(&MutantStatus::Killed).unwrap(),
            "\"killed\""
        );
        assert_eq!(
            serde_json::to_string(&MutantStatus::Survived).unwrap(),
            "\"survived\""
        );
        assert_eq!(
            serde_json::to_string(&MutantStatus::BuildError).unwrap(),
            "\"build_error\""
        );
    }
}
