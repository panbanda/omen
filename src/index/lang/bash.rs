//! Bash symbols.

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule};
use crate::index::model::{Exported, SymbolKind};

pub struct Bash;

const DEFINITIONS: &[DefinitionRule] = &[DefinitionRule::new(
    "function_definition",
    SymbolKind::Function,
    NameRule::Field("name"),
)];

impl LanguageSpec for Bash {
    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    /// Bash has one global table of functions. A script that sources a file
    /// can call each function in it, and also a function that a function defines.
    fn exported(&self, _draft: &Draft<'_>, _context: &Context<'_, '_>) -> Exported {
        Exported::Exported
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows};

    #[test]
    fn test_both_function_forms() {
        let source = "base() {\n    echo 4\n}\n\nfunction twice_local {\n    echo 2\n}\n\nfunction both() {\n    echo 3\n}\n";
        assert_eq!(
            rows(Language::Bash, source),
            [
                "Function base - 1-3 exported",
                "Function twice_local - 5-7 exported",
                "Function both - 9-11 exported",
            ]
        );
    }

    #[test]
    fn test_nested_function_has_the_outer_function_as_container() {
        let source = "outer() {\n    inner() {\n        echo 5\n    }\n    inner\n}\n";
        assert_eq!(
            rows(Language::Bash, source),
            [
                "Function outer - 1-6 exported",
                "Function inner outer 2-4 exported",
            ]
        );
    }

    #[test]
    fn test_second_definition_of_a_name_gets_an_ordinal() {
        let source = "greet() {\n    echo first\n}\n\ngreet() {\n    echo second\n}\n";
        assert_eq!(keys(Language::Bash, source), ["file#greet", "file#greet~2"]);
    }
}
