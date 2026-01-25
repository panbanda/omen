//! Built-in mutation operators.
//!
//! Operators are named using standard mutation testing conventions:
//! - CRR: Constant Replacement (LiteralOperator)
//! - ROR: Relational Operator Replacement
//! - AOR: Arithmetic Operator Replacement
//! - COR: Conditional Operator Replacement
//! - UOR: Unary Operator Replacement
//! - SDL: Statement Deletion
//! - RVR: Return Value Replacement
//! - BVO: Boundary Value Operator
//! - BOR: Bitwise Operator Replacement
//! - ASR: Assignment Operator Replacement

mod arithmetic;
mod assignment;
mod bitwise;
mod boundary;
mod conditional;
pub mod go;
mod literal;
pub mod python;
mod relational;
mod return_value;
pub mod ruby;
pub mod rust;
mod statement;
pub mod typescript;
mod unary;

pub use arithmetic::ArithmeticOperator;
pub use assignment::AssignmentOperator;
pub use bitwise::BitwiseOperator;
pub use boundary::BoundaryOperator;
pub use conditional::ConditionalOperator;
pub use go::{GoErrorOperator, GoNilOperator};
pub use literal::LiteralOperator;
pub use python::{PythonComprehensionOperator, PythonIdentityOperator};
pub use relational::RelationalOperator;
pub use return_value::ReturnValueOperator;
pub use ruby::{RubyNilOperator, RubySymbolOperator};
pub use rust::{BorrowOperator, OptionOperator, ResultOperator};
pub use statement::StatementOperator;
pub use typescript::{TypeScriptEqualityOperator, TypeScriptOptionalOperator};
pub use unary::UnaryOperator;

use super::operator::OperatorRegistry;

/// Create a registry with default operators (CRR, ROR, AOR).
///
/// These are the most commonly used operators that work across all languages
/// and provide good mutation coverage with reasonable execution time.
pub fn default_registry() -> OperatorRegistry {
    let mut registry = OperatorRegistry::new();
    registry.register(Box::new(LiteralOperator));
    registry.register(Box::new(RelationalOperator));
    registry.register(Box::new(ArithmeticOperator));
    registry
}

/// Create a registry with all available operators.
///
/// Includes language-specific operators and all mutation categories.
/// Use this for thorough mutation testing when execution time is not a concern.
pub fn full_registry() -> OperatorRegistry {
    let mut registry = default_registry();
    registry.register(Box::new(ConditionalOperator));
    registry.register(Box::new(UnaryOperator));
    registry.register(Box::new(BoundaryOperator));
    registry.register(Box::new(BitwiseOperator));
    registry.register(Box::new(AssignmentOperator));
    registry.register(Box::new(StatementOperator));
    registry.register(Box::new(ReturnValueOperator));
    // Language-specific operators
    rust::register_rust_operators(&mut registry);
    register_go_operators(&mut registry);
    register_typescript_operators(&mut registry);
    register_python_operators(&mut registry);
    register_ruby_operators(&mut registry);
    registry
}

/// Register Go-specific operators.
pub fn register_go_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(GoErrorOperator));
    registry.register(Box::new(GoNilOperator));
}

/// Register TypeScript/JavaScript-specific operators.
pub fn register_typescript_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(TypeScriptEqualityOperator));
    registry.register(Box::new(TypeScriptOptionalOperator));
}

/// Register Python-specific operators.
pub fn register_python_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(PythonIdentityOperator));
    registry.register(Box::new(PythonComprehensionOperator));
}

/// Register Ruby-specific operators.
pub fn register_ruby_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(RubyNilOperator));
    registry.register(Box::new(RubySymbolOperator));
}

/// Create a registry optimized for fast execution.
///
/// Uses only the most effective operators and excludes those that tend
/// to produce many equivalent mutants.
pub fn fast_registry() -> OperatorRegistry {
    let mut registry = OperatorRegistry::new();
    registry.register(Box::new(RelationalOperator));
    registry.register(Box::new(ArithmeticOperator));
    // Literal operator excluded in fast mode as it tends to produce
    // more equivalent mutants
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_has_three_operators() {
        let registry = default_registry();
        assert_eq!(registry.operators().len(), 3);
    }

    #[test]
    fn test_default_registry_operator_names() {
        let registry = default_registry();
        let names: Vec<&str> = registry.operators().iter().map(|op| op.name()).collect();
        assert!(names.contains(&"CRR"));
        assert!(names.contains(&"ROR"));
        assert!(names.contains(&"AOR"));
    }

    #[test]
    fn test_full_registry_includes_default() {
        let full = full_registry();
        let default = default_registry();
        assert!(full.operators().len() >= default.operators().len());
    }

    #[test]
    fn test_fast_registry_is_subset() {
        let fast = fast_registry();
        let default = default_registry();
        assert!(fast.operators().len() <= default.operators().len());
    }

    #[test]
    fn test_fast_registry_has_ror_and_aor() {
        let registry = fast_registry();
        let names: Vec<&str> = registry.operators().iter().map(|op| op.name()).collect();
        assert!(names.contains(&"ROR"));
        assert!(names.contains(&"AOR"));
    }
}
