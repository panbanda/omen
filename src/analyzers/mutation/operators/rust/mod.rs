//! Rust-specific mutation operators.
//!
//! This module provides mutation operators that target Rust-specific language
//! features like Option, Result, and borrowing.

mod borrow;
mod option;
mod result;

pub use borrow::BorrowOperator;
pub use option::OptionOperator;
pub use result::ResultOperator;

use super::super::operator::OperatorRegistry;

/// Register all Rust-specific operators with a registry.
pub fn register_rust_operators(registry: &mut OperatorRegistry) {
    registry.register(Box::new(OptionOperator));
    registry.register(Box::new(ResultOperator));
    registry.register(Box::new(BorrowOperator));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Language;

    #[test]
    fn test_register_rust_operators() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        assert_eq!(registry.operators().len(), 3);
    }

    #[test]
    fn test_all_operators_support_rust() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        let rust_ops = registry.for_language(Language::Rust);
        assert_eq!(rust_ops.len(), 3);
    }

    #[test]
    fn test_operators_do_not_support_other_languages() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        let go_ops = registry.for_language(Language::Go);
        assert!(go_ops.is_empty());

        let py_ops = registry.for_language(Language::Python);
        assert!(py_ops.is_empty());

        let js_ops = registry.for_language(Language::JavaScript);
        assert!(js_ops.is_empty());
    }

    #[test]
    fn test_operator_names() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        let names: Vec<_> = registry.operators().iter().map(|op| op.name()).collect();
        assert!(names.contains(&"RustOption"));
        assert!(names.contains(&"RustResult"));
        assert!(names.contains(&"RustBorrow"));
    }

    #[test]
    fn test_get_by_names() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        let ops = registry.get_by_names(&["RustOption", "RustResult"]);
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn test_get_by_names_single() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        let ops = registry.get_by_names(&["RustBorrow"]);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].name(), "RustBorrow");
    }

    #[test]
    fn test_get_by_names_nonexistent() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        let ops = registry.get_by_names(&["NonExistent"]);
        assert!(ops.is_empty());
    }

    #[test]
    fn test_operator_descriptions_are_nonempty() {
        let mut registry = OperatorRegistry::new();
        register_rust_operators(&mut registry);

        for op in registry.operators() {
            assert!(!op.description().is_empty());
        }
    }
}
