//! Mutation operator trait and registry.

use crate::core::Language;
use crate::parser::ParseResult;

use super::Mutant;

/// Trait for mutation operators.
///
/// A mutation operator defines how to generate mutations for a specific
/// type of code construct (e.g., literals, operators, statements).
pub trait MutationOperator: Send + Sync {
    /// Short name for the operator (e.g., "CRR", "ROR", "AOR").
    fn name(&self) -> &'static str;

    /// Human-readable description of what the operator does.
    fn description(&self) -> &'static str;

    /// Generate mutants from a parsed source file.
    ///
    /// Returns a list of all possible mutations this operator can generate
    /// for the given source code.
    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant>;

    /// Check if this operator supports the given language.
    fn supports_language(&self, lang: Language) -> bool;
}

/// Registry of all available mutation operators.
pub struct OperatorRegistry {
    operators: Vec<Box<dyn MutationOperator>>,
}

impl Default for OperatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OperatorRegistry {
    /// Create a new registry with all built-in operators.
    pub fn new() -> Self {
        Self {
            operators: Vec::new(),
        }
    }

    /// Register an operator.
    pub fn register(&mut self, operator: Box<dyn MutationOperator>) {
        self.operators.push(operator);
    }

    /// Get all registered operators.
    pub fn operators(&self) -> &[Box<dyn MutationOperator>] {
        &self.operators
    }

    /// Get operators filtered by name.
    pub fn get_by_names(&self, names: &[&str]) -> Vec<&dyn MutationOperator> {
        self.operators
            .iter()
            .filter(|op| names.contains(&op.name()))
            .map(|op| op.as_ref())
            .collect()
    }

    /// Get operators that support the given language.
    pub fn for_language(&self, lang: Language) -> Vec<&dyn MutationOperator> {
        self.operators
            .iter()
            .filter(|op| op.supports_language(lang))
            .map(|op| op.as_ref())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestOperator;

    impl MutationOperator for TestOperator {
        fn name(&self) -> &'static str {
            "TEST"
        }

        fn description(&self) -> &'static str {
            "Test operator"
        }

        fn generate_mutants(&self, _result: &ParseResult, _prefix: &str) -> Vec<Mutant> {
            Vec::new()
        }

        fn supports_language(&self, _lang: Language) -> bool {
            true
        }
    }

    struct RustOnlyOperator;

    impl MutationOperator for RustOnlyOperator {
        fn name(&self) -> &'static str {
            "RUST"
        }

        fn description(&self) -> &'static str {
            "Rust-only operator"
        }

        fn generate_mutants(&self, _result: &ParseResult, _prefix: &str) -> Vec<Mutant> {
            Vec::new()
        }

        fn supports_language(&self, lang: Language) -> bool {
            matches!(lang, Language::Rust)
        }
    }

    #[test]
    fn test_operator_registry_new() {
        let registry = OperatorRegistry::new();
        assert!(registry.operators().is_empty());
    }

    #[test]
    fn test_operator_registry_register() {
        let mut registry = OperatorRegistry::new();
        registry.register(Box::new(TestOperator));

        assert_eq!(registry.operators().len(), 1);
        assert_eq!(registry.operators()[0].name(), "TEST");
    }

    #[test]
    fn test_operator_registry_get_by_names() {
        let mut registry = OperatorRegistry::new();
        registry.register(Box::new(TestOperator));
        registry.register(Box::new(RustOnlyOperator));

        let ops = registry.get_by_names(&["TEST"]);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].name(), "TEST");

        let ops = registry.get_by_names(&["TEST", "RUST"]);
        assert_eq!(ops.len(), 2);

        let ops = registry.get_by_names(&["NONEXISTENT"]);
        assert!(ops.is_empty());
    }

    #[test]
    fn test_operator_registry_for_language() {
        let mut registry = OperatorRegistry::new();
        registry.register(Box::new(TestOperator));
        registry.register(Box::new(RustOnlyOperator));

        let ops = registry.for_language(Language::Rust);
        assert_eq!(ops.len(), 2);

        let ops = registry.for_language(Language::Python);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].name(), "TEST");
    }
}
