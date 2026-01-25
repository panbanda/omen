//! TypeScript-specific mutation operators.
//!
//! These operators target TypeScript/JavaScript language idioms:
//! - TER (TypeScript Equality Replacement): Strict vs loose equality mutations
//! - TOR (TypeScript Optional Replacement): Optional chaining and nullish coalescing mutations

mod equality;
mod optional;

pub use equality::TypeScriptEqualityOperator;
pub use optional::TypeScriptOptionalOperator;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzers::mutation::operator::MutationOperator;
    use crate::core::Language;

    #[test]
    fn test_ts_operators_support_typescript() {
        let equality_op = TypeScriptEqualityOperator;
        let optional_op = TypeScriptOptionalOperator;

        assert!(equality_op.supports_language(Language::TypeScript));
        assert!(optional_op.supports_language(Language::TypeScript));
    }

    #[test]
    fn test_ts_operators_support_javascript() {
        let equality_op = TypeScriptEqualityOperator;
        let optional_op = TypeScriptOptionalOperator;

        assert!(equality_op.supports_language(Language::JavaScript));
        assert!(optional_op.supports_language(Language::JavaScript));
    }

    #[test]
    fn test_ts_operators_support_tsx_jsx() {
        let equality_op = TypeScriptEqualityOperator;
        let optional_op = TypeScriptOptionalOperator;

        assert!(equality_op.supports_language(Language::Tsx));
        assert!(optional_op.supports_language(Language::Tsx));
        assert!(equality_op.supports_language(Language::Jsx));
        assert!(optional_op.supports_language(Language::Jsx));
    }

    #[test]
    fn test_ts_operators_do_not_support_other_languages() {
        let equality_op = TypeScriptEqualityOperator;
        let optional_op = TypeScriptOptionalOperator;

        let other_langs = [
            Language::Go,
            Language::Rust,
            Language::Python,
            Language::Java,
        ];

        for lang in other_langs {
            assert!(!equality_op.supports_language(lang));
            assert!(!optional_op.supports_language(lang));
        }
    }

    #[test]
    fn test_ts_operators_have_unique_names() {
        let equality_op = TypeScriptEqualityOperator;
        let optional_op = TypeScriptOptionalOperator;

        assert_ne!(equality_op.name(), optional_op.name());
        assert_eq!(equality_op.name(), "TER");
        assert_eq!(optional_op.name(), "TOR");
    }
}
