//! Bash symbols.

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule};
use crate::index::model::{Exported, SymbolKind};

use tree_sitter::Node;

use crate::index::model::{ImportBinding, ImportKind};

const COMMAND: &str = "command";

use crate::index::calls::{Argument, Call, Callee};
use crate::index::model::ReferenceKind;

const WORD: &str = "word";

pub struct Bash;

const DEFINITIONS: &[DefinitionRule] = &[DefinitionRule::new(
    "function_definition",
    SymbolKind::Function,
    NameRule::Field("name"),
)];

impl LanguageSpec for Bash {
    fn call_kinds(&self) -> Vec<&'static str> {
        vec![COMMAND]
    }

    fn call_hook_kinds(&self) -> Vec<&'static str> {
        vec![WORD]
    }

    /// A command whose name is one word is a call of that name. A name that
    /// an expansion gives, as in `"$1" argument`, is not a call of a known name.
    fn call<'tree>(&self, node: Node<'tree>, _context: &Context<'_, 'tree>) -> Option<Call<'tree>> {
        let name = node.child_by_field_name("name")?;
        let word = name
            .named_child(0)
            .filter(|word| word.kind() == WORD && name.named_child_count() == 1)?;
        let mut cursor = node.walk();
        let arguments = node.children_by_field_name("argument", &mut cursor).count();
        Some(Call {
            callee: Callee {
                name_node: word,
                receiver: None,
                qualifier: None,
            },
            kind: ReferenceKind::Call,
            arguments: vec![Argument::Other; arguments],
        })
    }

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
    fn test_a_dotted_name_and_a_nested_name_get_different_keys() {
        let source = "outer.inner() {\n    echo dotted\n}\n\nouter() {\n    inner() {\n        echo nested\n    }\n}\n";
        let keys = keys(Language::Bash, source);
        assert_eq!(keys.len(), 3, "{keys:?}");
        let unique: std::collections::BTreeSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), 3, "{keys:?}");
        assert_eq!(keys[0], "file#outer.inner");
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

    #[test]
    fn test_calls() {
        use crate::index::resolve::testing::rows;
        let files = [
            (
                "main.sh",
                "source ./lib/util.sh\nif [[ -n \"$FANCY\" ]]; then\n    source ./lib/fancy.sh\nelse\n    source ./lib/plain.sh\nfi\n\nbase() {\n    echo 4\n}\n\napply() {\n    \"$1\" \"$2\"\n}\n\nouter() {\n    inner() {\n        echo 5\n    }\n    inner\n}\n\ngreet() {\n    echo first\n}\n\nrun() {\n    base | cat\n    local value\n    value=$(twice 2)\n    apply base 3\n    greet\n    pick\n    git status\n}\n\ngreet() {\n    echo second\n}\n",
            ),
            ("lib/util.sh", "twice() {\n    echo 2\n}\n\nstatus() {\n    echo ok\n}\n"),
            ("lib/fancy.sh", "pick() {\n    echo fancy\n}\n"),
            ("lib/plain.sh", "pick() {\n    echo plain\n}\n"),
        ];
        assert_eq!(
            rows(&files, "main.sh"),
            [
                "1 ->source Unresolved - []",
                "3 ->source Unresolved - []",
                "5 ->source Unresolved - []",
                "9 base>echo Unresolved - []",
                "18 outer.inner>echo Unresolved - []",
                "20 outer>inner UniqueCandidate LexicalScope [main.sh#outer.inner]",
                "24 greet>echo Unresolved - []",
                "28 run>base UniqueCandidate SameFile [main.sh#base]",
                "28 run>cat Unresolved - []",
                "30 run>twice UniqueCandidate ImportedModule [lib/util.sh#twice]",
                "31 run>apply UniqueCandidate SameFile [main.sh#apply]",
                "32 run>greet UniqueCandidate SameFile [main.sh#greet~2]",
                "33 run>pick Ambiguous ImportedModule [lib/fancy.sh#pick, lib/plain.sh#pick]",
                "34 run>git Unresolved - []",
                "38 greet~2>echo Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_later_definition_replaces_the_earlier_one() {
        use crate::index::imports::testing::index;
        use crate::index::resolve::resolve_references;
        let files = [(
            "main.sh",
            "greet() {\n    echo first\n}\n\nrun() {\n    greet\n}\n\ngreet() {\n    echo second\n}\n",
        )];
        let mut facts = index(&files);
        resolve_references(&mut facts);
        let call = facts[0]
            .references
            .iter()
            .find(|reference| reference.name == "greet")
            .unwrap();
        assert_eq!(call.resolution.candidates, ["main.sh#greet~2"]);
        assert_eq!(call.resolution.superseded, ["main.sh#greet"]);
    }
}
