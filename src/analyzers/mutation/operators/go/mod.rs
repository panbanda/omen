//! Go-specific mutation operators.
//!
//! These operators target Go language idioms and patterns:
//! - GER (Go Error Replacement): Error handling mutations
//! - GNR (Go Nil Replacement): Nil value and comparison mutations

mod error;
mod nil;

pub use error::GoErrorOperator;
pub use nil::GoNilOperator;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzers::mutation::operator::MutationOperator;
    use crate::core::Language;

    #[test]
    fn test_go_operators_support_go() {
        let error_op = GoErrorOperator;
        let nil_op = GoNilOperator;

        assert!(error_op.supports_language(Language::Go));
        assert!(nil_op.supports_language(Language::Go));
    }

    #[test]
    fn test_go_operators_do_not_support_other_languages() {
        let error_op = GoErrorOperator;
        let nil_op = GoNilOperator;

        let other_langs = [
            Language::Rust,
            Language::Python,
            Language::TypeScript,
            Language::JavaScript,
            Language::Java,
        ];

        for lang in other_langs {
            assert!(!error_op.supports_language(lang));
            assert!(!nil_op.supports_language(lang));
        }
    }

    #[test]
    fn test_go_operators_have_unique_names() {
        let error_op = GoErrorOperator;
        let nil_op = GoNilOperator;

        assert_ne!(error_op.name(), nil_op.name());
        assert_eq!(error_op.name(), "GER");
        assert_eq!(nil_op.name(), "GNR");
    }
}
