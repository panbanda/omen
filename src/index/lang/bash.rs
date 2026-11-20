//! Bash symbols.

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule};
use crate::index::model::{Exported, SymbolKind};

use std::collections::HashMap;

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

    fn value_kinds(&self) -> Vec<&'static str> {
        vec![super::values::COMMAND]
    }

    fn value_hook_kinds(&self) -> Vec<&'static str> {
        super::values::BASH_HOOK_KINDS.to_vec()
    }

    fn function_values<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
        out: &mut Vec<crate::index::calls::ValueUse<'tree>>,
    ) {
        super::values::bash(node, context, out);
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
                target: None,
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
        out.push(context.import(node, unquote(&text), ImportKind::Load, None, None));
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

/// What a call of an alias name runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alias {
    /// The first word of the alias value.
    Name(String),
    /// The source does not show if bash expands the alias at the call, or
    /// what the first word of the value is.
    Unknown,
}

/// The aliases of a script. Bash expands an alias only while the option
/// `expand_aliases` is on, and only in a line that it reads after it runs
/// the `alias` command. Bash reads a full line, and a full compound command,
/// before it runs it. Thus only an `alias` or `shopt` command at the top
/// level of the script has a known effect. Such a command in a function body
/// or in a compound command can run at a different time or not run.
#[derive(Debug, Default)]
pub struct Aliases {
    /// The byte from which each `shopt` command that sets `expand_aliases`
    /// has an effect, with the new value, in source order. `None` is a value
    /// that the source does not show.
    switches: Vec<(usize, Option<bool>)>,
    /// For each name, the byte from which each `alias` or `unalias` command
    /// that names it has an effect, and the position of the command in
    /// source order. `None` is a removal.
    definitions: HashMap<String, Vec<(usize, usize, Option<Alias>)>>,
    /// The same facts for each `unalias -a`, which removes each alias. One
    /// list in place of an entry for each name keeps the table linear.
    removals: Vec<(usize, usize, Option<Alias>)>,
}

impl Aliases {
    pub fn of(root: Node<'_>, source: &[u8]) -> Self {
        let mut commands = Vec::new();
        let mut pending = vec![(root, false)];
        while let Some((node, nested)) = pending.pop() {
            if node.kind() == COMMAND {
                commands.push((node, nested));
            }
            let nested = nested || node.kind() != "program";
            let mut cursor = node.walk();
            pending.extend(
                node.named_children(&mut cursor)
                    .map(|child| (child, nested)),
            );
        }
        commands.sort_by_key(|(command, _)| command.start_byte());
        let text =
            |node: Node<'_>| String::from_utf8_lossy(&source[node.byte_range()]).into_owned();
        let mut aliases = Self::default();
        for (order, (command, nested)) in commands.into_iter().enumerate() {
            let Some(name) = command.child_by_field_name("name") else {
                continue;
            };
            let mut cursor = command.walk();
            let arguments: Vec<String> = command
                .children_by_field_name("argument", &mut cursor)
                .map(text)
                .collect();
            let effect = source[command.end_byte()..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(source.len(), |offset| command.end_byte() + offset + 1);
            let has = |flag: &str| arguments.iter().any(|argument| argument == flag);
            match text(name).as_str() {
                "shopt" if has("expand_aliases") => {
                    let value = if has("-u") {
                        Some(false)
                    } else if has("-s") {
                        Some(true)
                    } else {
                        continue;
                    };
                    aliases
                        .switches
                        .push((effect, if nested { None } else { value }));
                }
                "alias" => {
                    for argument in &arguments {
                        let Some((name, value)) = unquote(argument).split_once('=') else {
                            continue;
                        };
                        let word = unquote(value)
                            .split_whitespace()
                            .next()
                            .filter(|word| !word.contains(['$', '`', '\\', '\'', '"']));
                        let alias = match word {
                            Some(word) if !nested => Alias::Name(word.to_string()),
                            _ => Alias::Unknown,
                        };
                        aliases
                            .definitions
                            .entry(unquote(name).to_string())
                            .or_default()
                            .push((effect, order, Some(alias)));
                    }
                }
                "unalias" => {
                    let removal = nested.then_some(Alias::Unknown);
                    if has("-a") {
                        aliases.removals.push((effect, order, removal));
                    } else {
                        for argument in &arguments {
                            aliases
                                .definitions
                                .entry(unquote(argument).to_string())
                                .or_default()
                                .push((effect, order, removal.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
        aliases
    }

    /// What a command at `byte` runs when its first word is `name` and bash
    /// can expand that word as an alias. `None` means that the word is not an alias.
    pub fn target(&self, name: &str, byte: usize) -> Option<Alias> {
        fn last(
            entries: &[(usize, usize, Option<Alias>)],
            byte: usize,
        ) -> Option<&(usize, usize, Option<Alias>)> {
            let index = entries
                .partition_point(|(at, _, _)| *at <= byte)
                .checked_sub(1)?;
            Some(&entries[index])
        }
        let (_, defined, alias) = last(self.definitions.get(name)?, byte)?;
        let alias = match last(&self.removals, byte) {
            Some((_, removed, removal)) if removed > defined => removal.clone()?,
            _ => alias.clone()?,
        };
        let switch = self.switches.partition_point(|(at, _)| *at <= byte);
        match switch.checked_sub(1).map(|index| self.switches[index].1) {
            None | Some(Some(false)) => None,
            Some(None) => Some(Alias::Unknown),
            Some(Some(true)) => Some(alias),
        }
    }
}

/// The text without one pair of quotation marks around it.
fn unquote(text: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|quote| text.strip_prefix(*quote)?.strip_suffix(*quote))
        .unwrap_or(text)
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
    fn test_source_without_a_slash_has_no_certain_target() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "bin/main.sh",
                "source lib.sh\n. lib.sh\nsource \"lib.sh\"\nsource ./lib.sh\nsource \"$(dirname \"$0\")/lib.sh\"\n",
            ),
            ("bin/lib.sh", ""),
        ];
        assert_eq!(
            rows(&files, "bin/main.sh"),
            [
                "1 Load lib.sh -->- unresolved",
                "2 Load lib.sh -->- unresolved",
                "3 Load lib.sh -->- unresolved",
                "4 Load ./lib.sh -->- bin/lib.sh",
                "5 Load $(dirname \"$0\")/lib.sh -->- bin/lib.sh",
            ]
        );
    }

    #[test]
    fn test_alias_calls_the_first_word_of_its_value() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "main.sh",
            "shopt -s expand_aliases\nalias plain=greet quoted='greet loud' double=\"greet twice\"\nalias 'whole=greet'\nalias listing='ls -l'\n\ngreet() {\n    echo hi\n}\n\nrun() {\n    plain\n    quoted\n    double\n    whole\n    listing\n}\n",
        )];
        assert_eq!(
            rows(&files, "main.sh"),
            [
                "1 ->shopt Unresolved - []",
                "2 ->alias External - []",
                "3 ->alias External - []",
                "4 ->alias External - []",
                "7 greet>echo External - []",
                "11 run>plain UniqueCandidate SameFile [main.sh#greet]",
                "12 run>quoted UniqueCandidate SameFile [main.sh#greet]",
                "13 run>double UniqueCandidate SameFile [main.sh#greet]",
                "14 run>whole UniqueCandidate SameFile [main.sh#greet]",
                "15 run>listing Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_alias_that_bash_can_defer_gives_no_edge() {
        use crate::index::resolve::testing::rows;
        let greet = "greet() {\n    echo hi\n}\n";
        for (source, expected) in [
            // An alias in a function body is defined only when the function runs.
            (
                format!("shopt -s expand_aliases\n{greet}setup() {{\n    alias plain=greet\n}}\nsetup\nplain\n"),
                "9 ->plain Unresolved - []",
            ),
            // Bash reads a full compound command before it runs it.
            (
                format!("shopt -s expand_aliases\n{greet}if true; then\n    alias plain=greet\n    plain\nfi\n"),
                "7 ->plain Unresolved - []",
            ),
            // The alias in the block may not run, so a later call is also not certain.
            (
                format!("shopt -s expand_aliases\n{greet}if [[ -n \"$X\" ]]; then\n    alias plain=greet\nfi\nplain\n"),
                "8 ->plain Unresolved - []",
            ),
            // A later alias in a function hides an earlier alias of the same name.
            (
                format!("shopt -s expand_aliases\nalias plain=greet\n{greet}setup() {{\n    alias plain=other\n}}\nsetup\nplain\n"),
                "10 ->plain Unresolved - []",
            ),
            // Bash reads a full line before it runs it.
            (
                format!("shopt -s expand_aliases\n{greet}alias plain=greet; plain\n"),
                "5 ->plain Unresolved - []",
            ),
            // A `shopt` in a function can run or not.
            (
                format!("{greet}setup() {{\n    shopt -s expand_aliases\n}}\nsetup\nalias plain=greet\nplain\n"),
                "9 ->plain Unresolved - []",
            ),
        ] {
            let rows = rows(&[("main.sh", source.as_str())], "main.sh");
            assert!(rows.contains(&expected.to_string()), "{source}: {rows:?}");
        }
        let source = format!("shopt -s expand_aliases\n{greet}alias plain=greet\nplain\n");
        let rows = rows(&[("main.sh", source.as_str())], "main.sh");
        assert!(
            rows.contains(&"6 ->plain UniqueCandidate SameFile [main.sh#greet]".to_string()),
            "{rows:?}"
        );
    }

    #[test]
    fn test_unalias_all_and_alias_on_one_line_keep_their_order() {
        use crate::index::resolve::testing::rows;
        let greet = "greet() {\n    echo hi\n}\n";
        let source = format!("shopt -s expand_aliases\n{greet}alias plain=other\nunalias -a; alias plain=greet\nplain\n");
        let found = rows(&[("main.sh", source.as_str())], "main.sh");
        assert!(
            found.contains(&"7 ->plain UniqueCandidate SameFile [main.sh#greet]".to_string()),
            "{found:?}"
        );
        let source =
            format!("shopt -s expand_aliases\n{greet}alias plain=greet; unalias -a\nplain\n");
        let found = rows(&[("main.sh", source.as_str())], "main.sh");
        assert!(
            found.contains(&"6 ->plain Unresolved - []".to_string()),
            "{found:?}"
        );
    }

    #[test]
    fn test_alias_without_expand_aliases_is_not_expanded() {
        use crate::index::resolve::testing::rows;
        for (source, expected) in [
            (
                "alias plain=greet\ngreet() {\n    echo hi\n}\nrun() {\n    plain\n}\n",
                "6 run>plain Unresolved - []",
            ),
            (
                "alias plain=greet\ngreet() {\n    echo hi\n}\nrun() {\n    plain\n}\nshopt -s expand_aliases\n",
                "6 run>plain Unresolved - []",
            ),
            (
                "shopt -s expand_aliases\nshopt -u expand_aliases\nalias plain=greet\ngreet() {\n    echo hi\n}\nrun() {\n    plain\n}\n",
                "8 run>plain Unresolved - []",
            ),
            (
                "shopt -s expand_aliases\nalias plain=greet\nunalias plain\ngreet() {\n    echo hi\n}\nrun() {\n    plain\n}\n",
                "8 run>plain Unresolved - []",
            ),
            (
                "shopt -s expand_aliases\ngreet() {\n    echo hi\n}\nrun() {\n    plain\n}\nalias plain=greet\n",
                "6 run>plain Unresolved - []",
            ),
        ] {
            let rows = rows(&[("main.sh", source)], "main.sh");
            assert!(rows.contains(&expected.to_string()), "{source}: {rows:?}");
        }
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
                "9 base>echo External - []",
                "18 outer.inner>echo External - []",
                "20 outer>inner UniqueCandidate LexicalScope [main.sh#outer.inner]",
                "24 greet>echo External - []",
                "28 run>base UniqueCandidate SameFile [main.sh#base]",
                "28 run>cat Unresolved - []",
                "30 run>twice UniqueCandidate ImportedModule [lib/util.sh#twice]",
                "31 run>apply UniqueCandidate SameFile [main.sh#apply]",
                "32 run>greet UniqueCandidate SameFile [main.sh#greet~2]",
                "33 run>pick Ambiguous ImportedModule [lib/fancy.sh#pick, lib/plain.sh#pick]",
                "34 run>git Unresolved - []",
                "38 greet~2>echo External - []",
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

#[cfg(test)]
mod alias_timing_tests {
    use std::path::Path;

    use crate::core::Language;
    use crate::index::extract::extract_file;
    use crate::index::timing::{assert_scales_linearly, assert_scales_linearly_above, Stopwatch};
    use crate::parser::Parser;

    /// `size` aliases and `size` functions, then one call of each alias.
    fn aliases_at(size: usize) -> std::time::Duration {
        let mut source = String::from("shopt -s expand_aliases\n");
        for position in 0..size {
            source.push_str(&format!(
                "alias a{position}='f{position} x'\nf{position}() {{ :; }}\n"
            ));
        }
        for position in 0..size {
            source.push_str(&format!("a{position}\n"));
        }
        let started = Stopwatch::start();
        let parsed = Parser::new()
            .parse(source.as_bytes(), Language::Bash, Path::new("main.sh"))
            .unwrap();
        let facts = extract_file(&parsed, "main.sh");
        let elapsed = started.elapsed();
        let aliased = facts
            .reference_facts
            .iter()
            .filter(|site| matches!(site.alias, Some(super::Alias::Name(_))))
            .count();
        assert_eq!(aliased, size);
        elapsed
    }

    #[test]
    fn test_each_alias_call_has_its_target() {
        aliases_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_aliases_are_read_in_linear_time() {
        assert_scales_linearly("aliases", 5_000, aliases_at);
    }

    /// `size` aliases, each followed by `unalias -a`, then a call of each
    /// alias and a last alias with its call.
    fn removals_at(size: usize) -> std::time::Duration {
        let mut source = String::from("shopt -s expand_aliases\n");
        for position in 0..size {
            source.push_str(&format!("alias a{position}=f{position}\nunalias -a\n"));
        }
        let calls = source.len();
        for position in 0..size {
            source.push_str(&format!("a{position}\n"));
        }
        let last = source.len();
        source.push_str("alias a0=g\na0\n");
        let parsed = Parser::new()
            .parse(source.as_bytes(), Language::Bash, Path::new("main.sh"))
            .unwrap();
        let started = Stopwatch::start();
        let aliases = super::Aliases::of(parsed.root_node(), source.as_bytes());
        let elapsed = started.elapsed();
        for position in 0..size {
            assert_eq!(aliases.target(&format!("a{position}"), calls), None);
        }
        assert_eq!(
            aliases.target("a0", last + "alias a0=g\n".len()),
            Some(super::Alias::Name("g".to_string()))
        );
        elapsed
    }

    #[test]
    fn test_unalias_all_removes_each_earlier_alias() {
        removals_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_unalias_all_is_read_in_linear_time() {
        // The linear work is small, thus a lower floor shows a quadratic
        // growth before the growth uses much memory.
        assert_scales_linearly_above(
            "unalias -a",
            3_000,
            std::time::Duration::from_millis(5),
            removals_at,
        );
    }
}
