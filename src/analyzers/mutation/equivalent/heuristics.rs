//! Heuristic detection of likely-equivalent mutants.
//!
//! Uses pattern-based heuristics to identify mutants that are likely
//! equivalent to the original code (i.e., cannot be killed by any test).

use super::super::Mutant;
use super::features::MutantFeatures;

/// Reason for flagging a mutant as likely equivalent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquivalenceReason {
    /// Mutation is in logging/debug code that doesn't affect behavior.
    InLoggingStatement,
    /// Mutation is in dead/unreachable code.
    InDeadCode,
    /// Mutation doesn't affect observable return value.
    NoObservableBehavior,
    /// Mutation is in boilerplate/generated code.
    InBoilerplate,
    /// Mutation produces semantically equivalent code.
    SemanticallyEquivalent,
}

impl std::fmt::Display for EquivalenceReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InLoggingStatement => write!(f, "Mutation is in logging/debug statement"),
            Self::InDeadCode => write!(f, "Mutation is in dead/unreachable code"),
            Self::NoObservableBehavior => write!(f, "Mutation doesn't affect observable behavior"),
            Self::InBoilerplate => write!(f, "Mutation is in boilerplate/generated code"),
            Self::SemanticallyEquivalent => {
                write!(f, "Mutation produces semantically equivalent code")
            }
        }
    }
}

/// Heuristic-based detector for equivalent mutants.
#[derive(Debug, Default)]
pub struct HeuristicDetector {
    /// Patterns that indicate boilerplate code.
    boilerplate_patterns: Vec<String>,
}

impl HeuristicDetector {
    /// Create a new heuristic detector.
    pub fn new() -> Self {
        Self {
            boilerplate_patterns: default_boilerplate_patterns(),
        }
    }

    /// Create a detector with custom boilerplate patterns.
    pub fn with_boilerplate_patterns(patterns: Vec<String>) -> Self {
        Self {
            boilerplate_patterns: patterns,
        }
    }

    /// Check if a mutant is likely equivalent based on heuristics.
    pub fn is_likely_equivalent(&self, mutant: &Mutant, features: &MutantFeatures) -> bool {
        self.equivalence_reason(mutant, features).is_some()
    }

    /// Get the reason why a mutant is likely equivalent, if any.
    pub fn equivalence_reason(
        &self,
        mutant: &Mutant,
        features: &MutantFeatures,
    ) -> Option<EquivalenceReason> {
        // Check for mutations in logging statements (highest priority)
        if features.in_logging {
            return Some(EquivalenceReason::InLoggingStatement);
        }

        // Check for mutations in dead code
        if features.in_dead_code {
            return Some(EquivalenceReason::InDeadCode);
        }

        // Check for semantically equivalent mutations
        if is_semantically_equivalent(mutant) {
            return Some(EquivalenceReason::SemanticallyEquivalent);
        }

        // Check for mutations in boilerplate code
        if self.is_in_boilerplate(mutant) {
            return Some(EquivalenceReason::InBoilerplate);
        }

        // Check for mutations that don't affect observable behavior
        if !features.affects_return && is_likely_side_effect_free(mutant, features) {
            return Some(EquivalenceReason::NoObservableBehavior);
        }

        None
    }

    /// Check if mutation is in boilerplate code based on patterns.
    fn is_in_boilerplate(&self, mutant: &Mutant) -> bool {
        let file_path = mutant.file_path.to_string_lossy().to_lowercase();
        let description = mutant.description.to_lowercase();

        for pattern in &self.boilerplate_patterns {
            let lower_pattern = pattern.to_lowercase();
            if file_path.contains(&lower_pattern) || description.contains(&lower_pattern) {
                return true;
            }
        }

        // Check for common boilerplate indicators in the mutation itself
        let original_lower = mutant.original.to_lowercase();
        is_boilerplate_value(&original_lower)
    }
}

/// Default patterns that indicate boilerplate code.
fn default_boilerplate_patterns() -> Vec<String> {
    vec![
        "generated".to_string(),
        "auto-generated".to_string(),
        "do not edit".to_string(),
        ".pb.".to_string(), // Protobuf generated
        "_generated".to_string(),
        "mock".to_string(),
        "stub".to_string(),
    ]
}

/// Check if a value is typically boilerplate.
fn is_boilerplate_value(value: &str) -> bool {
    // Common default values that are often semantically equivalent when mutated
    matches!(
        value,
        "0" | "1" | "true" | "false" | "\"\"" | "''" | "none" | "null" | "nil"
    )
}

/// Check if mutation produces semantically equivalent code.
fn is_semantically_equivalent(mutant: &Mutant) -> bool {
    let original = mutant.original.trim();
    let replacement = mutant.replacement.trim();

    // x == x is always true (identity comparison)
    // Replacing true with (x == x) is equivalent
    if (original == "true" && replacement.contains("=="))
        || (replacement == "true" && original.contains("=="))
    {
        // Only if comparing same values
        if let Some((left, right)) = original
            .split_once("==")
            .or_else(|| replacement.split_once("=="))
        {
            if left.trim() == right.trim() {
                return true;
            }
        }
    }

    // x + 0 == x, x * 1 == x, x - 0 == x, x / 1 == x
    if is_identity_operation(original, replacement) {
        return true;
    }

    // String format equivalences
    if is_format_equivalent(original, replacement) {
        return true;
    }

    // Boolean double negation
    if is_double_negation(original, replacement) {
        return true;
    }

    false
}

/// Check if mutation is an identity operation (no behavioral change).
fn is_identity_operation(original: &str, replacement: &str) -> bool {
    // Adding/subtracting 0
    if (original == "0" && (replacement.contains("+") || replacement.contains("-")))
        || (replacement == "0" && (original.contains("+") || original.contains("-")))
    {
        return false; // This could actually change behavior
    }

    // Multiplying/dividing by 1
    if original == "1" && replacement.contains("*") {
        return true;
    }
    if replacement == "1" && original.contains("/") {
        return false; // Division by original value, not 1
    }

    // x | 0 == x, x & MAX == x, x ^ 0 == x
    if (original == "0" || replacement == "0")
        && (original.contains("|")
            || original.contains("^")
            || replacement.contains("|")
            || replacement.contains("^"))
    {
        return true;
    }

    false
}

/// Check if strings are format-equivalent.
fn is_format_equivalent(original: &str, replacement: &str) -> bool {
    // Empty string variants
    let empty_variants = ["\"\"", "''", "String::new()", "String::from(\"\")", "str()"];

    let orig_is_empty = empty_variants.contains(&original);
    let repl_is_empty = empty_variants.contains(&replacement);

    orig_is_empty && repl_is_empty
}

/// Check for boolean double negation equivalence.
fn is_double_negation(original: &str, replacement: &str) -> bool {
    // !!x == x
    if original.starts_with("!!") && replacement == &original[2..] {
        return true;
    }
    if replacement.starts_with("!!") && original == &replacement[2..] {
        return true;
    }

    // not not x == x
    if original.starts_with("not not ") && replacement == &original[8..] {
        return true;
    }

    false
}

/// Check if mutation is likely side-effect free.
fn is_likely_side_effect_free(mutant: &Mutant, features: &MutantFeatures) -> bool {
    // Deep in the AST and not affecting return suggests local variable
    // that may not be observable
    if features.ast_depth > 8 && !features.affects_return {
        return true;
    }

    // CRR mutations on string literals in non-observable contexts
    if features.operator_type == "CRR"
        && is_string_literal(&mutant.original)
        && !features.affects_return
    {
        // String constants not in return/assert are often just labels/messages
        return true;
    }

    false
}

/// Check if a value is a string literal.
fn is_string_literal(value: &str) -> bool {
    (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_mutant(operator: &str, original: &str, replacement: &str) -> Mutant {
        Mutant::new(
            "test-1",
            "test.rs",
            operator,
            1,
            1,
            original,
            replacement,
            "test mutation",
            (0, original.len()),
        )
    }

    fn create_features() -> MutantFeatures {
        MutantFeatures::default()
    }

    #[test]
    fn test_detector_new() {
        let detector = HeuristicDetector::new();
        assert!(!detector.boilerplate_patterns.is_empty());
    }

    #[test]
    fn test_detector_with_custom_patterns() {
        let patterns = vec!["custom".to_string()];
        let detector = HeuristicDetector::with_boilerplate_patterns(patterns);
        assert_eq!(detector.boilerplate_patterns.len(), 1);
    }

    #[test]
    fn test_is_likely_equivalent_in_logging() {
        let detector = HeuristicDetector::new();
        let mutant = create_mutant("CRR", "42", "0");
        let mut features = create_features();
        features.in_logging = true;

        assert!(detector.is_likely_equivalent(&mutant, &features));
        assert_eq!(
            detector.equivalence_reason(&mutant, &features),
            Some(EquivalenceReason::InLoggingStatement)
        );
    }

    #[test]
    fn test_is_likely_equivalent_in_dead_code() {
        let detector = HeuristicDetector::new();
        let mutant = create_mutant("CRR", "42", "0");
        let mut features = create_features();
        features.in_dead_code = true;

        assert!(detector.is_likely_equivalent(&mutant, &features));
        assert_eq!(
            detector.equivalence_reason(&mutant, &features),
            Some(EquivalenceReason::InDeadCode)
        );
    }

    #[test]
    fn test_not_equivalent_normal_code() {
        let detector = HeuristicDetector::new();
        let mutant = create_mutant("CRR", "42", "0");
        let mut features = create_features();
        features.affects_return = true;

        assert!(!detector.is_likely_equivalent(&mutant, &features));
        assert!(detector.equivalence_reason(&mutant, &features).is_none());
    }

    #[test]
    fn test_semantically_equivalent_identity_operations() {
        // These are patterns that result in no behavioral change
        assert!(is_identity_operation("1", "x * 1"));
        assert!(is_identity_operation("0", "x | 0"));
        assert!(is_identity_operation("0", "x ^ 0"));
    }

    #[test]
    fn test_format_equivalent_empty_strings() {
        assert!(is_format_equivalent("\"\"", "''"));
        assert!(is_format_equivalent("String::new()", "\"\""));
        assert!(!is_format_equivalent("\"hello\"", "\"world\""));
    }

    #[test]
    fn test_double_negation() {
        assert!(is_double_negation("!!x", "x"));
        assert!(is_double_negation("x", "!!x"));
        assert!(is_double_negation("not not x", "x"));
        assert!(!is_double_negation("!x", "x"));
    }

    #[test]
    fn test_is_string_literal() {
        assert!(is_string_literal("\"hello\""));
        assert!(is_string_literal("'hello'"));
        assert!(!is_string_literal("hello"));
        assert!(!is_string_literal("42"));
    }

    #[test]
    fn test_boilerplate_detection() {
        let detector = HeuristicDetector::new();

        let mutant = Mutant::new(
            "test-1",
            "foo.pb.go",
            "CRR",
            1,
            1,
            "42",
            "0",
            "test",
            (0, 2),
        );

        assert!(detector.is_in_boilerplate(&mutant));
    }

    #[test]
    fn test_no_observable_behavior() {
        let detector = HeuristicDetector::new();
        let mutant = create_mutant("CRR", "\"label\"", "\"other\"");
        let mut features = create_features();
        features.affects_return = false;
        features.ast_depth = 10;

        assert!(detector.is_likely_equivalent(&mutant, &features));
        assert_eq!(
            detector.equivalence_reason(&mutant, &features),
            Some(EquivalenceReason::NoObservableBehavior)
        );
    }

    #[test]
    fn test_equivalence_reason_display() {
        assert_eq!(
            EquivalenceReason::InLoggingStatement.to_string(),
            "Mutation is in logging/debug statement"
        );
        assert_eq!(
            EquivalenceReason::InDeadCode.to_string(),
            "Mutation is in dead/unreachable code"
        );
        assert_eq!(
            EquivalenceReason::NoObservableBehavior.to_string(),
            "Mutation doesn't affect observable behavior"
        );
        assert_eq!(
            EquivalenceReason::InBoilerplate.to_string(),
            "Mutation is in boilerplate/generated code"
        );
        assert_eq!(
            EquivalenceReason::SemanticallyEquivalent.to_string(),
            "Mutation produces semantically equivalent code"
        );
    }

    #[test]
    fn test_logging_priority_over_dead_code() {
        // When both logging and dead code are true, logging should be returned
        let detector = HeuristicDetector::new();
        let mutant = create_mutant("CRR", "42", "0");
        let mut features = create_features();
        features.in_logging = true;
        features.in_dead_code = true;

        assert_eq!(
            detector.equivalence_reason(&mutant, &features),
            Some(EquivalenceReason::InLoggingStatement)
        );
    }

    #[test]
    fn test_generated_file_patterns() {
        let detector = HeuristicDetector::new();

        // Protobuf generated file
        let mutant1 = Mutant::new("1", "user.pb.go", "CRR", 1, 1, "42", "0", "test", (0, 2));
        assert!(detector.is_in_boilerplate(&mutant1));

        // Regular file
        let mutant2 = Mutant::new("1", "user.go", "CRR", 1, 1, "42", "0", "test", (0, 2));
        assert!(!detector.is_in_boilerplate(&mutant2));
    }
}
