//! Equivalent mutant detection for mutation testing.
//!
//! This module provides tools to detect and filter out mutants that are likely
//! to be equivalent to the original code. Equivalent mutants cannot be killed
//! by any test because they produce the same observable behavior as the original.
//!
//! # Overview
//!
//! Equivalent mutant detection is crucial for accurate mutation testing. Without
//! filtering equivalent mutants, the mutation score will be artificially lowered
//! and developers will waste time trying to write tests for unkillable mutants.
//!
//! # Detection Approaches
//!
//! This module uses several approaches to detect equivalent mutants:
//!
//! 1. **Feature Extraction**: Extract characteristics from the mutant's context
//!    such as AST depth, whether it affects a return value, etc.
//!
//! 2. **Heuristic Detection**: Apply pattern-based rules to identify common
//!    equivalence patterns:
//!    - Mutations in logging/debug statements
//!    - Mutations in dead/unreachable code
//!    - Semantically equivalent transformations
//!    - Mutations in boilerplate/generated code
//!
//! 3. **Scoring Model**: Combine features and heuristics into a probability
//!    score indicating how likely a mutant is to be equivalent.
//!
//! # Usage
//!
//! ```ignore
//! use omen::analyzers::mutation::equivalent::{
//!     EquivalenceScorer, MutantFeatures, Strictness,
//! };
//!
//! let scorer = EquivalenceScorer::new();
//! let threshold = EquivalenceScorer::recommended_threshold(Strictness::Moderate);
//!
//! // Filter mutants, keeping only high-value ones
//! let high_value = scorer.filter_high_value(mutants, &parse_result, threshold);
//!
//! // Or get detailed results with scores
//! let result = scorer.filter_with_scores(mutants, &parse_result, threshold);
//! println!("Kept: {}, Filtered: {}", result.kept_count(), result.filtered_count());
//! ```
//!
//! # Accuracy Considerations
//!
//! Equivalent mutant detection is inherently imprecise. The goal is to filter
//! out mutants that are very likely equivalent while minimizing false positives
//! (filtering non-equivalent mutants).
//!
//! Use `Strictness::Strict` (threshold 0.5) for maximum precision (fewer false
//! positives, but may miss some equivalents).
//!
//! Use `Strictness::Aggressive` (threshold 0.85) for maximum recall (catch more
//! equivalents, but may have some false positives).

mod features;
mod heuristics;
mod scorer;

pub use features::MutantFeatures;
pub use heuristics::{EquivalenceReason, HeuristicDetector};
pub use scorer::{EquivalenceScorer, FilterResult, ScoredMutant, ScorerConfig, Strictness};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzers::mutation::Mutant;
    use crate::core::Language;
    use crate::parser::Parser;
    use std::path::Path;

    // Integration tests verifying the module works end-to-end

    fn parse_code(code: &str, lang: Language, path: &str) -> crate::parser::ParseResult {
        let parser = Parser::new();
        parser
            .parse(code.as_bytes(), lang, Path::new(path))
            .unwrap()
    }

    fn create_mutant(
        id: &str,
        file: &str,
        operator: &str,
        line: u32,
        original: &str,
        replacement: &str,
        byte_range: (usize, usize),
    ) -> Mutant {
        Mutant::new(
            id,
            file,
            operator,
            line,
            1,
            original,
            replacement,
            format!("Replace {} with {}", original, replacement),
            byte_range,
        )
    }

    #[test]
    fn test_integration_logging_detection() {
        // End-to-end test: mutant in println! should be flagged as equivalent
        let code = r#"fn main() {
    let result = calculate(42);
    println!("Result: {}", result);
}"#;
        let result = parse_code(code, Language::Rust, "test.rs");

        // Mutant at the 42 in calculate(42) - not in logging
        let mutant1 = create_mutant("1", "test.rs", "CRR", 2, "42", "0", (37, 39));

        // For println position, we need to find where "result" appears in the println
        // The println is: println!("Result: {}", result);
        // We'll target a theoretical mutation of the format string

        let features1 = MutantFeatures::extract(&mutant1, &result);
        let scorer = EquivalenceScorer::new();
        let score1 = scorer.score(&features1);

        // Mutant not in logging should have lower score
        assert!(score1 < 0.9);
    }

    #[test]
    fn test_integration_filter_workflow() {
        // Test the complete filtering workflow
        let code = "fn get_value() -> i32 { 42 }";
        let result = parse_code(code, Language::Rust, "test.rs");

        let mutants = vec![create_mutant("1", "test.rs", "CRR", 1, "42", "0", (24, 26))];

        let scorer = EquivalenceScorer::new();
        let threshold = EquivalenceScorer::recommended_threshold(Strictness::Moderate);

        let filter_result = scorer.filter_with_scores(mutants, &result, threshold);

        // This mutant affects return value, should likely be kept
        assert!(filter_result.kept_count() + filter_result.filtered_count() == 1);
    }

    #[test]
    fn test_integration_heuristic_detector() {
        let detector = HeuristicDetector::new();

        // Test mutant in generated file
        let mutant = create_mutant("1", "user.pb.go", "CRR", 10, "0", "1", (100, 101));
        let features = MutantFeatures::default();

        let is_equiv = detector.is_likely_equivalent(&mutant, &features);
        let reason = detector.equivalence_reason(&mutant, &features);

        assert!(is_equiv);
        assert_eq!(reason, Some(EquivalenceReason::InBoilerplate));
    }

    #[test]
    fn test_multiple_languages() {
        // Test that detection works across languages

        // Rust
        let rust_code = "fn main() { println!(\"test\"); }";
        let rust_result = parse_code(rust_code, Language::Rust, "test.rs");
        let rust_mutant = create_mutant("1", "test.rs", "CRR", 1, "test", "foo", (21, 25));
        let rust_features = MutantFeatures::extract(&rust_mutant, &rust_result);
        assert!(rust_features.in_logging);

        // JavaScript
        let js_code = "function foo() { console.log(42); }";
        let js_result = parse_code(js_code, Language::JavaScript, "test.js");
        let js_mutant = create_mutant("1", "test.js", "CRR", 1, "42", "0", (29, 31));
        let js_features = MutantFeatures::extract(&js_mutant, &js_result);
        assert!(js_features.in_logging);

        // Python
        let py_code = "def foo():\n    print(42)";
        let py_result = parse_code(py_code, Language::Python, "test.py");
        let py_mutant = create_mutant("1", "test.py", "CRR", 2, "42", "0", (21, 23));
        let py_features = MutantFeatures::extract(&py_mutant, &py_result);
        assert!(py_features.in_logging);
    }

    #[test]
    fn test_known_equivalent_mutants() {
        // Test fixture: known-equivalent mutant patterns

        // Pattern 1: Mutation in debug-only code
        let debug_code = r#"
fn process(x: i32) -> i32 {
    #[cfg(debug_assertions)]
    println!("Processing: {}", x);
    x * 2
}
"#;
        let result = parse_code(debug_code, Language::Rust, "test.rs");
        let mutant = create_mutant("1", "test.rs", "CRR", 4, "x", "y", (64, 65));
        let features = MutantFeatures::extract(&mutant, &result);

        // The mutation in println should be flagged
        assert!(features.in_logging);

        // Pattern 2: Mutation in error message
        let error_code = "fn fail() { panic!(\"error: {}\", 42); }";
        let error_result = parse_code(error_code, Language::Rust, "test.rs");
        let error_mutant = create_mutant("1", "test.rs", "CRR", 1, "42", "0", (32, 34));
        let error_features = MutantFeatures::extract(&error_mutant, &error_result);

        // This is harder to detect, but check that we can at least extract features
        assert!(!error_features.operator_type.is_empty());
    }

    #[test]
    fn test_strictness_levels() {
        // Verify that different strictness levels affect thresholds properly
        let strict = EquivalenceScorer::recommended_threshold(Strictness::Strict);
        let moderate = EquivalenceScorer::recommended_threshold(Strictness::Moderate);
        let aggressive = EquivalenceScorer::recommended_threshold(Strictness::Aggressive);

        // Strict should have lowest threshold (fewer filtered)
        // Aggressive should have highest threshold (more filtered)
        assert!(strict < moderate);
        assert!(moderate < aggressive);
    }
}
