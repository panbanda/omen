//! Ruby symbols.

use tree_sitter::Node;

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule, ScopeRole};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind, ImportTarget};

const STRING: &str = "string";
const STRING_CONTENT: &str = "string_content";

pub struct Ruby;

const METHOD: &str = "method";
const SINGLETON_METHOD: &str = "singleton_method";
const CLASS: &str = "class";
const MODULE: &str = "module";
const LAMBDA: &str = "lambda";
const CALL: &str = "call";
const ASSIGNMENT: &str = "assignment";
const IDENTIFIER: &str = "identifier";
const SCOPE_RESOLUTION: &str = "scope_resolution";
const ARGUMENT_LIST: &str = "argument_list";
const SIMPLE_SYMBOL: &str = "simple_symbol";

const DEFINITIONS: &[DefinitionRule] = &[
    DefinitionRule::function(METHOD, NameRule::Field("name")),
    DefinitionRule::function(SINGLETON_METHOD, NameRule::Field("name")),
    DefinitionRule::new(CLASS, SymbolKind::Class, NameRule::Field("name")),
    DefinitionRule::new(MODULE, SymbolKind::Module, NameRule::Field("name")),
    DefinitionRule::new(LAMBDA, SymbolKind::Function, NameRule::Hook),
    DefinitionRule::new(CALL, SymbolKind::Function, NameRule::Hook),
];

/// The methods that change the visibility of instance methods.
const INSTANCE_MODIFIERS: &[(&str, bool)] = &[
    ("private", false),
    ("protected", false),
    ("public", true),
    ("module_function", true),
];

/// The methods that change the visibility of singleton methods.
const SINGLETON_MODIFIERS: &[(&str, bool)] = &[
    ("private_class_method", false),
    ("public_class_method", true),
];

fn modifier(name: &str, singleton: bool) -> Option<bool> {
    let table = if singleton {
        SINGLETON_MODIFIERS
    } else {
        INSTANCE_MODIFIERS
    };
    table
        .iter()
        .find(|(modifier, _)| *modifier == name)
        .map(|(_, public)| *public)
}

/// The name of the method that a call without a receiver calls.
fn bare_call_name(call: Node<'_>, context: &Context<'_, '_>) -> Option<String> {
    if call.kind() != CALL || call.child_by_field_name("receiver").is_some() {
        return None;
    }
    call.child_by_field_name("method")
        .map(|method| context.text(method))
}

/// Tells if a method in the body of a class or a module is public.
///
/// A modifier without arguments, such as `private`, changes the methods after
/// it. A modifier with a symbol, such as `private :name`, changes that method
/// from each position in the body. `private def name` changes that definition.
fn is_public_method(definition: Node<'_>, name: &str, context: &Context<'_, '_>) -> bool {
    let singleton = definition.kind() == SINGLETON_METHOD;
    let Some(parent) = definition.parent() else {
        return true;
    };
    if parent.kind() == ARGUMENT_LIST {
        return parent
            .parent()
            .and_then(|call| bare_call_name(call, context))
            .and_then(|modifier_name| modifier(&modifier_name, singleton))
            .unwrap_or(true);
    }
    let mut section = true;
    let mut named = None;
    let mut cursor = parent.walk();
    for statement in parent.named_children(&mut cursor) {
        match statement.kind() {
            IDENTIFIER if statement.start_byte() < definition.start_byte() => {
                if let Some(public) = modifier(&context.text(statement), singleton) {
                    section = public;
                }
            }
            CALL => {
                let Some(public) = bare_call_name(statement, context)
                    .and_then(|modifier_name| modifier(&modifier_name, singleton))
                else {
                    continue;
                };
                let Some(arguments) = statement.child_by_field_name("arguments") else {
                    continue;
                };
                let mut cursor = arguments.walk();
                let names_this_method = arguments.named_children(&mut cursor).any(|argument| {
                    argument.kind() == SIMPLE_SYMBOL
                        && context.text(argument).trim_start_matches(':') == name
                });
                if names_this_method {
                    named = Some(public);
                }
            }
            _ => {}
        }
    }
    named.unwrap_or(section)
}

impl LanguageSpec for Ruby {
    fn import_kinds(&self) -> Vec<&'static str> {
        vec![CALL]
    }

    fn import_hook_kinds(&self) -> Vec<&'static str> {
        vec![STRING, STRING_CONTENT]
    }

    /// Only `require` and `require_relative` with a literal path are imports.
    /// `include` and a superclass are not imports of a file. The directory of
    /// `require` is not in the syntax, thus that binding is external.
    fn imports(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<ImportBinding>) {
        let Some(method) = bare_call_name(node, context) else {
            return;
        };
        if !["require", "require_relative"].contains(&method.as_str()) {
            return;
        }
        let path = node
            .child_by_field_name("arguments")
            .filter(|arguments| arguments.named_child_count() == 1)
            .and_then(|arguments| arguments.named_child(0))
            .filter(|string| string.kind() == STRING && string.named_child_count() == 1)
            .and_then(|string| string.named_child(0))
            .filter(|content| content.kind() == STRING_CONTENT);
        let Some(path) = path else {
            return;
        };
        let mut binding = context.import(node, context.text(path), ImportKind::Load, None, None);
        if method == "require" {
            binding.target_file = ImportTarget::External;
        }
        out.push(binding);
    }

    fn definitions(&self) -> Vec<DefinitionRule> {
        DEFINITIONS.to_vec()
    }

    fn hook_kinds(&self) -> Vec<&'static str> {
        vec![
            ASSIGNMENT,
            IDENTIFIER,
            SCOPE_RESOLUTION,
            ARGUMENT_LIST,
            SIMPLE_SYMBOL,
        ]
    }

    fn comment_kinds(&self) -> &'static [&'static str] {
        &["comment"]
    }

    /// A lambda has a name only when an assignment binds it to one variable.
    /// `lambda { }` and `proc { }` are calls that make a lambda.
    fn name_node<'tree>(
        &self,
        node: Node<'tree>,
        context: &Context<'_, 'tree>,
    ) -> Option<Node<'tree>> {
        if node.kind() == CALL {
            let name = bare_call_name(node, context)?;
            if !["lambda", "proc"].contains(&name.as_str()) {
                return None;
            }
            node.child_by_field_name("block")?;
        }
        let assignment = node.parent().filter(|parent| parent.kind() == ASSIGNMENT)?;
        if assignment.child_by_field_name("right") != Some(node) {
            return None;
        }
        assignment
            .child_by_field_name("left")
            .filter(|left| left.kind() == IDENTIFIER)
    }

    fn refine<'tree>(&self, draft: &mut Draft<'tree>, context: &Context<'_, 'tree>) -> bool {
        let node = draft.node;
        match node.kind() {
            LAMBDA | CALL => draft.span = Span::between(&draft.name_node, &node),
            METHOD => {
                if draft.kind == SymbolKind::Method && draft.name == "initialize" {
                    draft.kind = SymbolKind::Constructor;
                }
            }
            CLASS | MODULE => {
                // The methods of a module are methods, as the methods of a class are.
                draft.role = ScopeRole::Type;
                // `class Outer::Inner` defines `Inner` in the scope `Outer`.
                while draft.name_node.kind() == SCOPE_RESOLUTION {
                    let Some(name) = draft.name_node.child_by_field_name("name") else {
                        break;
                    };
                    if let Some(scope) = draft.name_node.child_by_field_name("scope") {
                        let scope = context.text(scope);
                        draft
                            .container_path
                            .extend(scope.split("::").map(str::to_string));
                    }
                    draft.name_node = name;
                    draft.name = context.text(name);
                }
                if let Some(superclass) = node.child_by_field_name("superclass") {
                    if let Some(name) = superclass.named_child(0) {
                        draft.supertypes.push(context.text(name));
                    }
                }
            }
            _ => {}
        }
        true
    }

    /// A method is public unless a visibility modifier changes it. Ruby makes a
    /// method that a method defines a method of the same object, thus a nested
    /// definition has the visibility of a definition at that level.
    fn exported(&self, draft: &Draft<'_>, context: &Context<'_, '_>) -> Exported {
        let node = draft.node;
        if [LAMBDA, CALL].contains(&node.kind()) {
            return Exported::NotExported;
        }
        let Some(owner) = context
            .scopes
            .iter()
            .rev()
            .find(|scope| scope.role == ScopeRole::Type)
        else {
            return Exported::Exported;
        };
        let direct_member = context
            .scopes
            .last()
            .is_some_and(|scope| scope.role == ScopeRole::Type);
        let is_method = [METHOD, SINGLETON_METHOD].contains(&node.kind());
        if is_method && direct_member && !is_public_method(node, &draft.name, context) {
            return Exported::NotExported;
        }
        owner.exported.clone()
    }
}

#[cfg(test)]
mod tests {
    use crate::core::Language;
    use crate::index::extract::testing::{keys, rows, symbols};

    #[test]
    fn test_top_level_methods_and_lambdas() {
        let source = "def outer\n  def inner\n    5\n  end\n  inner()\nend\n\ndef closure\n  add = ->(n) { n + 1 }\n  sub = lambda { |n| n - 1 }\n  made = proc { 1 }\n  [1].map { |n| n }\n  other = build { 2 }\n  add.call(2)\nend\n\nhandler = -> { 9 }\n";
        assert_eq!(
            rows(Language::Ruby, source),
            [
                "Function outer - 1-6 exported",
                "Function inner outer 2-4 exported",
                "Function closure - 8-15 exported",
                "Function add closure 9-9 not_exported",
                "Function sub closure 10-10 not_exported",
                "Function made closure 11-11 not_exported",
                "Function handler - 17-17 not_exported",
            ]
        );
    }

    #[test]
    fn test_classes_and_modules() {
        let source = "module Describable\n  def describe\n    'shape'\n  end\nend\n\nclass Circle < Shape\n  def initialize(radius)\n    @radius = radius\n  end\n\n  def self.unit\n    new(1.0)\n  end\n\n  def area\n    3.14\n  end\n\n  class Inner\n    def go\n    end\n  end\nend\n\nclass Shop::Square\n  def side\n  end\nend\n";
        assert_eq!(
            rows(Language::Ruby, source),
            [
                "Module Describable - 1-5 exported",
                "Method describe Describable 2-4 exported",
                "Class Circle - 7-24 exported",
                "Constructor initialize Circle 8-10 exported",
                "Method unit Circle 12-14 exported",
                "Method area Circle 16-18 exported",
                "Class Inner Circle 20-23 exported",
                "Method go Inner 21-22 exported",
                "Class Square - 26-29 exported",
                "Method side Square 27-28 exported",
            ]
        );
        assert_eq!(symbols(Language::Ruby, source)[2].supertypes, ["Shape"]);
        let keys = keys(Language::Ruby, source);
        assert_eq!(keys[7], "file#Circle.Inner.go");
        assert_eq!(keys[9], "file#Shop.Square.side");
    }

    #[test]
    fn test_visibility_sections() {
        let source = "class Circle\n  def area\n  end\n\n  protected\n\n  def radius\n  end\n\n  private\n\n  def scaled\n  end\n\n  def self.unit\n  end\n\n  public\n\n  def diameter\n  end\nend\n";
        assert_eq!(
            rows(Language::Ruby, source),
            [
                "Class Circle - 1-22 exported",
                "Method area Circle 2-3 exported",
                "Method radius Circle 7-8 not_exported",
                "Method scaled Circle 12-13 not_exported",
                "Method unit Circle 15-16 exported",
                "Method diameter Circle 20-21 exported",
            ]
        );
    }

    #[test]
    fn test_visibility_of_named_methods() {
        let source = "class Circle\n  def self.build\n  end\n  private_class_method :build\n\n  def early\n  end\n\n  private\n\n  def late\n  end\n\n  def shown\n  end\n  public :shown\n  private :early\n\n  private def inline\n  end\n\n  class << self\n    def open\n    end\n\n    private\n\n    def closed\n    end\n  end\nend\n";
        assert_eq!(
            rows(Language::Ruby, source),
            [
                "Class Circle - 1-31 exported",
                "Method build Circle 2-3 not_exported",
                "Method early Circle 6-7 not_exported",
                "Method late Circle 11-12 not_exported",
                "Method shown Circle 14-15 exported",
                "Method inline Circle 19-20 not_exported",
                "Method open Circle 23-24 exported",
                "Method closed Circle 28-29 not_exported",
            ]
        );
    }

    #[test]
    fn test_module_function_keeps_methods_public() {
        let source = "module Util\n  def self.helper\n    1\n  end\n\n  module_function\n\n  def twice(value)\n    value * 2\n  end\nend\n";
        assert_eq!(
            rows(Language::Ruby, source),
            [
                "Module Util - 1-11 exported",
                "Method helper Util 2-4 exported",
                "Method twice Util 8-10 exported",
            ]
        );
    }

    #[test]
    fn test_imports() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "app/main.rb",
                "require 'set'\nrequire_relative 'shapes'\nrequire('json')\nrequire_relative 'lib/x.rb'\nrequire_relative '../top'\nrequire_relative '../../outside'\nrequire_relative \"a/#{name}\"\nloader.require 'x'\nrequire_relative 'missing'\nrequire 'shapes'\n\nclass Circle < Shape\n  include Describable\nend\n",
            ),
            ("app/shapes.rb", ""),
            ("app/lib/x.rb", ""),
            ("top.rb", ""),
        ];
        assert_eq!(
            rows(&files, "app/main.rb"),
            [
                "1 Load set -->- external",
                "2 Load shapes -->- app/shapes.rb",
                "3 Load json -->- external",
                "4 Load lib/x.rb -->- app/lib/x.rb",
                "5 Load ../top -->- top.rb",
                "6 Load ../../outside -->- unresolved",
                "9 Load missing -->- unresolved",
                "10 Load shapes -->- external",
            ]
        );
    }
}
