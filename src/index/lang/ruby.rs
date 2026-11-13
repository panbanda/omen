//! Ruby symbols.

use tree_sitter::Node;

use crate::index::extract::{Context, DefinitionRule, Draft, LanguageSpec, NameRule, ScopeRole};
use crate::index::model::{Exported, Span, SymbolKind};

use crate::index::model::{ImportBinding, ImportKind, ImportTarget};

const STRING: &str = "string";
const STRING_CONTENT: &str = "string_content";

use crate::index::calls::{Call, CallSyntax, Callee, LocalName};
use crate::index::model::{Receiver, ReferenceKind};

const SUPER: &str = "super";
const OPERATOR_ASSIGNMENT: &str = "operator_assignment";
const CONSTANT: &str = "constant";
const PARAMETER_LISTS: &[&str] = &["method_parameters", "block_parameters", "lambda_parameters"];
const NAMED_PARAMETERS: &[&str] = &[
    "optional_parameter",
    "keyword_parameter",
    "splat_parameter",
    "hash_splat_parameter",
    "block_parameter",
];

/// A call has its receiver and its method name in the call node.
const CALL_SYNTAX: CallSyntax = CallSyntax {
    calls: &[(CALL, "", "arguments")],
    constructions: &[],
    members: &[(CALL, "receiver", "method")],
    paths: &[(SCOPE_RESOLUTION, "scope", "name")],
    names: &[IDENTIFIER, CONSTANT],
    self_words: &["self"],
    super_words: &[SUPER],
    locals: &[(ASSIGNMENT, "left", "", "right")],
    parameters: &[],
    return_type_field: "",
    literals: &[],
};

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

/// The modules that `include` and `prepend` in the body of a class or a
/// module add. The object gets the methods of these modules.
fn mixins(definition: Node<'_>, context: &Context<'_, '_>) -> Vec<String> {
    let body = definition.child_by_field_name("body").unwrap_or(definition);
    let mut cursor = body.walk();
    let mut found = Vec::new();
    for statement in body.named_children(&mut cursor) {
        let is_mixin = bare_call_name(statement, context)
            .is_some_and(|name| ["include", "prepend"].contains(&name.as_str()));
        let Some(arguments) = statement
            .child_by_field_name("arguments")
            .filter(|_| is_mixin)
        else {
            continue;
        };
        let mut inner = arguments.walk();
        for argument in arguments.named_children(&mut inner) {
            if [CONSTANT, SCOPE_RESOLUTION].contains(&argument.kind()) {
                found.push(context.text(argument));
            }
        }
    }
    found
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
    fn call_syntax(&self) -> CallSyntax {
        CALL_SYNTAX
    }

    fn call_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().call_kinds();
        kinds.extend([IDENTIFIER, SUPER]);
        kinds
    }

    fn local_kinds(&self) -> Vec<&'static str> {
        let mut kinds = self.call_syntax().local_kinds();
        kinds.extend(PARAMETER_LISTS);
        kinds
    }

    fn call_hook_kinds(&self) -> Vec<&'static str> {
        [
            PARAMETER_LISTS,
            NAMED_PARAMETERS,
            &[SUPER, OPERATOR_ASSIGNMENT],
        ]
        .concat()
    }

    /// A name without parentheses and without arguments can be a call. The
    /// walk gives each such name as a call, and the resolver gives no edge
    /// when the method binds the name. `super` calls the method of the parent
    /// class with the same name.
    fn call<'tree>(&self, node: Node<'tree>, context: &Context<'_, 'tree>) -> Option<Call<'tree>> {
        let parent_call = |word: Node<'tree>| Call {
            callee: Callee {
                name_node: word,
                receiver: Some(Receiver::Super),
                qualifier: None,
            },
            kind: ReferenceKind::Call,
            arguments: Vec::new(),
        };
        let parent = node.parent();
        let field_of_parent =
            |field: &str| parent.and_then(|parent| parent.child_by_field_name(field)) == Some(node);
        match node.kind() {
            CALL => {
                let method = node.child_by_field_name("method")?;
                if method.kind() != SUPER {
                    return self.call_syntax().call(node, context);
                }
                let mut call = parent_call(method);
                call.arguments = self.call_syntax().call(node, context)?.arguments;
                Some(call)
            }
            SUPER => (!field_of_parent("method")).then(|| parent_call(node)),
            _ => {
                let parent_kind = parent.map_or("", |parent| parent.kind());
                let binds = match parent_kind {
                    CALL => field_of_parent("method"),
                    ASSIGNMENT | OPERATOR_ASSIGNMENT => field_of_parent("left"),
                    _ => field_of_parent("name"),
                };
                let parameter = parent.is_some_and(|parent| {
                    PARAMETER_LISTS.contains(&parent.kind())
                        || NAMED_PARAMETERS.contains(&parent.kind())
                });
                (!binds && !parameter).then_some(Call {
                    callee: Callee {
                        name_node: node,
                        receiver: None,
                        qualifier: None,
                    },
                    kind: ReferenceKind::Call,
                    arguments: Vec::new(),
                })
            }
        }
    }

    /// The parameters of a method, a block, or a lambda are local names.
    fn locals(&self, node: Node<'_>, context: &Context<'_, '_>, out: &mut Vec<LocalName>) {
        if !PARAMETER_LISTS.contains(&node.kind()) {
            let before = out.len();
            self.call_syntax().locals(node, context, out);
            // `Alias = Name` gives a second name to a class or a module.
            let named = node
                .child_by_field_name("right")
                .filter(|right| right.kind() == CONSTANT);
            if let (Some(named), Some(local)) = (named, out.get_mut(before)) {
                local.type_name = Some(context.text(named));
                local.value_start = None;
            }
            return;
        }
        let mut cursor = node.walk();
        for parameter in node.named_children(&mut cursor) {
            let name = match parameter.kind() {
                IDENTIFIER => Some(parameter),
                kind if NAMED_PARAMETERS.contains(&kind) => parameter.child_by_field_name("name"),
                _ => None,
            };
            if let Some(name) = name {
                out.push(LocalName {
                    name: context.text(name),
                    type_name: None,
                    value_start: None,
                    is_parameter: true,
                });
            }
        }
    }

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
                draft.supertypes.extend(mixins(node, context));
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
                "10 Load shapes -->- unresolved",
            ]
        );
    }

    #[test]
    fn test_require_looks_in_the_load_roots() {
        use crate::index::imports::testing::rows;
        let files = [
            (
                "bin/run.rb",
                "require 'shop'\nrequire 'shop/cart'\nrequire 'twice'\nrequire 'hidden'\nrequire 'set'\nrequire 'gem_part.rb'\nrequire 'ext_part'\n",
            ),
            ("lib/shop.rb", ""),
            ("lib/shop/cart.rb", ""),
            ("twice.rb", ""),
            ("lib/twice.rb", ""),
            ("deep/dir/hidden.rb", ""),
            ("gems/tool/tool.gemspec", "Gem::Specification.new do |spec|\n  spec.name = 'tool'\nend\n"),
            ("gems/tool/lib/gem_part.rb", ""),
            (
                "gems/ext/ext.gemspec",
                "Gem::Specification.new do |spec|\n  spec.require_paths = [\"src\", 'more']\nend\n",
            ),
            ("gems/ext/src/ext_part.rb", ""),
        ];
        assert_eq!(
            rows(&files, "bin/run.rb"),
            [
                "1 Load shop -->- lib/shop.rb",
                "2 Load shop/cart -->- lib/shop/cart.rb",
                "3 Load twice -->- ambiguous(lib/twice.rb,twice.rb)",
                "4 Load hidden -->- unresolved",
                "5 Load set -->- external",
                "6 Load gem_part.rb -->- gems/tool/lib/gem_part.rb",
                "7 Load ext_part -->- gems/ext/src/ext_part.rb",
            ]
        );
    }

    #[test]
    fn test_require_paths_that_are_not_literals_give_no_load_root() {
        use crate::index::imports::ruby_require_paths;
        assert_eq!(ruby_require_paths("spec.name = 'a'\n"), ["lib"]);
        assert_eq!(
            ruby_require_paths("spec.require_paths = ['lib', \"ext\"]\n"),
            ["lib", "ext"]
        );
        assert!(ruby_require_paths("spec.require_paths = [base, 'lib']\n").is_empty());
        assert!(ruby_require_paths("spec.require_paths = paths\n").is_empty());
        assert!(ruby_require_paths("spec.require_paths = [\"#{root}/lib\"]\n").is_empty());
    }

    const CALL_FILES: &[(&str, &str)] = &[
        (
            "main.rb",
            "require_relative 'shapes'\nrequire_relative 'other'\n\ndef local\n  4\nend\n\ndef bare\n  local() + local + helper\nend\n\ndef outer\n  def inner\n    5\n  end\n  inner()\nend\n\ndef closure(shape)\n  add = ->(n) { n + local }\n  circle = Circle.new(1.0)\n  list = []\n  list.push(1)\n  add.call(2) + circle.area + shape.area + add + Util.twice(2) + Circle.unit + Set.new\nend\n",
        ),
        ("other.rb", "def helper\n  2\nend\n\ndef push(value)\n  value\nend\n"),
        (
            "shapes.rb",
            "module Util\n  def self.twice(value)\n    value * 2\n  end\nend\n\nmodule Describable\n  def describe\n    \"shape #{area}\"\n  end\nend\n\nclass Shape\n  include Describable\n\n  def initialize(name)\n    @name = name\n  end\n\n  def area\n    0.0\n  end\n\n  def label\n    @name\n  end\nend\n\nclass Circle < Shape\n  def self.unit\n    build(1.0)\n  end\n\n  def self.build(radius)\n    new(radius)\n  end\n\n  def initialize(radius)\n    super('circle')\n    @radius = radius\n  end\n\n  def area\n    self.scaled(3.14) + radius\n  end\n\n  def label\n    super.upcase\n  end\n\n  def radius\n    @radius\n  end\n\n  def scaled(factor)\n    factor\n  end\nend\n",
        ),
    ];

    #[test]
    fn test_calls_with_and_without_parentheses() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "main.rb"),
            [
                "1 ->require_relative Unresolved - []",
                "2 ->require_relative Unresolved - []",
                "9 bare>local UniqueCandidate SameFile [main.rb#local]",
                "9 bare>local UniqueCandidate SameFile [main.rb#local]",
                "9 bare>helper UniqueCandidate ImportedModule [other.rb#helper]",
                "16 outer>inner UniqueCandidate LexicalScope [main.rb#outer.inner]",
                "20 closure.add>n Unresolved - []",
                "20 closure.add>local UniqueCandidate SameFile [main.rb#local]",
                "21 closure>Circle.new UniqueCandidate QualifiedPath [shapes.rb#Circle.initialize]",
                "23 closure>list.push Unresolved - []",
                "23 closure>list Unresolved - []",
                "24 closure>add.call UniqueCandidate LexicalScope [main.rb#closure.add]",
                "24 closure>add Unresolved - []",
                "24 closure>circle.area UniqueCandidate ReceiverDeclaredType [shapes.rb#Circle.area]",
                "24 closure>circle Unresolved - []",
                "24 closure>shape.area Ambiguous NameOnly [shapes.rb#Circle.area, shapes.rb#Shape.area]",
                "24 closure>shape Unresolved - []",
                "24 closure>add Unresolved - []",
                "24 closure>Util.twice UniqueCandidate QualifiedPath [shapes.rb#Util.twice]",
                "24 closure>Circle.unit UniqueCandidate QualifiedPath [shapes.rb#Circle.unit]",
                "24 closure>Set.new Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_calls_in_classes_and_modules() {
        use crate::index::resolve::testing::rows;
        assert_eq!(
            rows(CALL_FILES, "shapes.rb"),
            [
                "3 Util.twice>value Unresolved - []",
                "9 Describable.describe>area Ambiguous NameOnly [shapes.rb#Circle.area, shapes.rb#Shape.area]",
                "14 Shape>include Unresolved - []",
                "17 Shape.initialize>name Unresolved - []",
                "31 Circle.unit>build UniqueCandidate EnclosingType [shapes.rb#Circle.build]",
                "35 Circle.build>new UniqueCandidate EnclosingType [shapes.rb#Circle.initialize]",
                "35 Circle.build>radius Unresolved - []",
                "39 Circle.initialize>super.super UniqueCandidate EnclosingType [shapes.rb#Shape.initialize]",
                "40 Circle.initialize>radius Unresolved - []",
                "44 Circle.area>self.scaled UniqueCandidate EnclosingType [shapes.rb#Circle.scaled]",
                "44 Circle.area>radius UniqueCandidate EnclosingType [shapes.rb#Circle.radius]",
                "48 Circle.label>super.upcase Unresolved - []",
                "48 Circle.label>super.super UniqueCandidate EnclosingType [shapes.rb#Shape.label]",
                "56 Circle.scaled>factor Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_receiver_without_parentheses_can_be_a_call() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "main.rb",
            "def callback\n  [1]\nend\n\ndef main(list)\n  callback.sum + list.size\nend\n",
        )];
        assert_eq!(
            rows(&files, "main.rb"),
            [
                "6 main>callback.sum Unresolved - []",
                "6 main>callback UniqueCandidate SameFile [main.rb#callback]",
                "6 main>list.size Unresolved - []",
                "6 main>list Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_method_of_the_object_that_a_call_gives() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "builder.rb",
            "class Builder\n  def self.app\n    self.new.to_app\n  end\n\n  def self.other\n    Builder.new.to_app.size\n  end\n\n  def to_app\n    1\n  end\nend\n",
        )];
        assert_eq!(
            rows(&files, "builder.rb"),
            [
                "3 Builder.app>(..).to_app UniqueCandidate ReceiverDeclaredType [builder.rb#Builder.to_app]",
                "3 Builder.app>self.new UniqueCandidate EnclosingType [builder.rb#Builder]",
                "7 Builder.other>(..).size Unresolved - []",
                "7 Builder.other>(..).to_app UniqueCandidate ReceiverDeclaredType [builder.rb#Builder.to_app]",
                "7 Builder.other>Builder.new UniqueCandidate QualifiedPath [builder.rb#Builder]",
            ]
        );
    }

    #[test]
    fn test_method_of_a_file_that_the_caller_does_not_load_is_not_an_edge() {
        use crate::index::resolve::testing::rows;
        let files = [
            (
                "lib/handler.rb",
                "require_relative 'tools'\n\nclass Handler\n  attr_reader :realm\n\n  def challenge\n    realm() + loaded()\n  end\nend\n",
            ),
            ("lib/tools.rb", "def loaded\n  1\nend\n"),
            ("test/spec_handler.rb", "def realm\n  'test'\nend\n"),
        ];
        assert_eq!(
            rows(&files, "lib/handler.rb"),
            [
                "1 ->require_relative Unresolved - []",
                "4 Handler>attr_reader Unresolved - []",
                "7 Handler.challenge>realm Ambiguous NameOnly [test/spec_handler.rb#realm]",
                "7 Handler.challenge>loaded UniqueCandidate ImportedModule [lib/tools.rb#loaded]",
            ]
        );
    }

    #[test]
    fn test_module_method_calls_a_method_of_the_including_class() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "request.rb",
            "class Request\n  module Env\n    def get_header(name)\n    end\n  end\n\n  module Helpers\n    def body\n      get_header('input')\n      missing('input')\n    end\n  end\n\n  include Env\n  include Helpers\nend\n\nmodule Shared\n  def label\n    title()\n  end\nend\n\nclass Book\n  include Shared\n  def title\n  end\nend\n\nclass Film\n  include Shared\n  def title\n  end\nend\n",
        )];
        assert_eq!(
            rows(&files, "request.rb"),
            [
                "9 Request.Helpers.body>get_header UniqueCandidate EnclosingType [request.rb#Request.Env.get_header]",
                "10 Request.Helpers.body>missing Unresolved - []",
                "14 Request>include Unresolved - []",
                "15 Request>include Unresolved - []",
                "20 Shared.label>title Ambiguous NameOnly [request.rb#Book.title, request.rb#Film.title]",
                "25 Book>include Unresolved - []",
                "31 Film>include Unresolved - []",
            ]
        );
    }

    #[test]
    fn test_mixin_class_of_the_object_and_qualified_class() {
        use crate::index::resolve::testing::rows;
        let files = [(
            "rack.rb",
            "module Rack\n  module Helpers\n    def get_header(name)\n      name\n    end\n  end\n\n  class Request\n    include Helpers\n  end\n\n  module Auth\n    class Request\n      def provided?\n        true\n      end\n    end\n  end\n\n  class Builder\n    def generate\n      self.class.new.to_app\n    end\n\n    def to_app\n      request = Rack::Request.new\n      request.get_header('a')\n      auth = Auth::Request.new\n      auth.provided?\n    end\n  end\nend\n",
        )];
        assert_eq!(
            rows(&files, "rack.rb"),
            [
                "4 Rack.Helpers.get_header>name Unresolved - []",
                "9 Rack.Request>include Unresolved - []",
                "22 Rack.Builder.generate>(..).to_app UniqueCandidate ReceiverDeclaredType [rack.rb#Rack.Builder.to_app]",
                "22 Rack.Builder.generate>(..).new UniqueCandidate ReceiverDeclaredType [rack.rb#Rack.Builder]",
                "22 Rack.Builder.generate>self.class Unresolved - []",
                "26 Rack.Builder.to_app>Rack::Request::new UniqueCandidate QualifiedPath [rack.rb#Rack.Request]",
                "27 Rack.Builder.to_app>request.get_header UniqueCandidate ReceiverDeclaredType [rack.rb#Rack.Helpers.get_header]",
                "27 Rack.Builder.to_app>request Unresolved - []",
                "28 Rack.Builder.to_app>Auth::Request::new UniqueCandidate QualifiedPath [rack.rb#Rack.Auth.Request]",
                "29 Rack.Builder.to_app>auth.provided? UniqueCandidate ReceiverDeclaredType [rack.rb#Rack.Auth.Request.provided?]",
                "29 Rack.Builder.to_app>auth Unresolved - []",
            ]
        );
        assert_eq!(
            symbols(Language::Ruby, files[0].1)[3].supertypes,
            ["Helpers"]
        );
    }
}
