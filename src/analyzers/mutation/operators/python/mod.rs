//! Python-specific mutation operators.
//!
//! These operators target Python language idioms and patterns:
//! - PIR (Python Identity Replacement): Identity and membership operator mutations
//! - PCR (Python Comprehension Replacement): List/dict/set comprehension mutations

mod comprehension;
mod identity;

pub use comprehension::PythonComprehensionOperator;
pub use identity::PythonIdentityOperator;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzers::mutation::operator::MutationOperator;
    use crate::core::Language;

    #[test]
    fn test_python_operators_support_python() {
        let identity_op = PythonIdentityOperator;
        let comp_op = PythonComprehensionOperator;

        assert!(identity_op.supports_language(Language::Python));
        assert!(comp_op.supports_language(Language::Python));
    }

    #[test]
    fn test_python_operators_do_not_support_other_languages() {
        let identity_op = PythonIdentityOperator;
        let comp_op = PythonComprehensionOperator;

        let other_langs = [
            Language::Go,
            Language::Rust,
            Language::TypeScript,
            Language::JavaScript,
            Language::Java,
        ];

        for lang in other_langs {
            assert!(!identity_op.supports_language(lang));
            assert!(!comp_op.supports_language(lang));
        }
    }

    #[test]
    fn test_python_operators_have_unique_names() {
        let identity_op = PythonIdentityOperator;
        let comp_op = PythonComprehensionOperator;

        assert_ne!(identity_op.name(), comp_op.name());
        assert_eq!(identity_op.name(), "PIR");
        assert_eq!(comp_op.name(), "PCR");
    }
}
