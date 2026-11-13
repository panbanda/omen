//! Bash symbols.

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule};
use crate::index::model::{Exported, SymbolKind};

use tree_sitter::Node;

use crate::index::model::{ImportBinding, ImportKind};

const COMMAND: &str = "command";

pub struct Bash;

const DEFINITIONS: &[DefinitionRule] = &[DefinitionRule::new(
    "function_definition",
    SymbolKind::Function,
    NameRule::Field("name"),
)];

impl LanguageSpec for Bash {
    fn import_kinds(&self) -> Vec<&'static str> {
        vec![COMMAND]
    }

    /// `source path` and `. path` load a file. The path is the text of the
    /// first argument without its quotation marks.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        let name = node
            .child_by_field_name("name")
            .map(|name| context.text(name));
        if !matches!(name.as_deref(), Some("source" | ".")) {
            return;
        }
        let Some(argument) = node.child_by_field_name("argument") else {
            return;
        };
        let text = context.text(argument);
        let path = ['"', '\'']
            .iter()
            .find_map(|quote| text.strip_prefix(*quote)?.strip_suffix(*quote))
            .unwrap_or(&text);
        out.push(context.import(node, path, ImportKind::Load, None, None));
    }

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

    #[test]
    fn test_imports() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "bin/main.sh",
                "source ./lib/util.sh\n. \"$(dirname \"$0\")/other.sh\"\nsource /dev/null\n. \"${DIR}/x.sh\"\nsource ../top.sh\nsource missing.sh\necho source x\n. 'lib/util.sh'\nsource \"$(dirname \"${BASH_SOURCE[0]}\")/lib/util.sh\"\nsource ../../outside.sh\n",
            ),
            ("bin/lib/util.sh", ""),
            ("bin/other.sh", ""),
            ("top.sh", ""),
        ];
        assert_eq!(
            rows(&files, "bin/main.sh"),
            [
                "1 Load ./lib/util.sh -->- bin/lib/util.sh",
                "2 Load $(dirname \"$0\")/other.sh -->- bin/other.sh",
                "3 Load /dev/null -->- external",
                "4 Load ${DIR}/x.sh -->- unresolved",
                "5 Load ../top.sh -->- top.sh",
                "6 Load missing.sh -->- unresolved",
                "8 Load lib/util.sh -->- bin/lib/util.sh",
                "9 Load $(dirname \"${BASH_SOURCE[0]}\")/lib/util.sh -->- bin/lib/util.sh",
                "10 Load ../../outside.sh -->- unresolved",
            ]
        );
    }
}
