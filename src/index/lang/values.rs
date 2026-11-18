//! Where each language uses a function as a value, without a call of it.
//!
//! A table gives the positions of a value: an argument, the right side of an
//! assignment, a returned value, and an element of a collection. A function
//! in this file reads the syntax that a table cannot describe. The walk
//! records a name, and the resolver tells if the name is a function. A form
//! that the syntax does not show to be a function, as a name in a string is,
//! gives no record.

use tree_sitter::Node;

use crate::index::calls::{CallSyntax, Callee, ValueSyntax, ValueUse};
use crate::index::extract::Context;
use crate::index::model::Receiver;

/// A class in a list of parent classes is also a value. It is not a function,
/// thus the resolver gives it no candidate.
pub const PYTHON: ValueSyntax = ValueSyntax {
    positions: &[
        ("argument_list", ""),
        ("keyword_argument", "value"),
        ("assignment", "right"),
        ("return_statement", ""),
        ("default_parameter", "value"),
        ("typed_default_parameter", "value"),
        ("list", ""),
        ("tuple", ""),
        ("set", ""),
        ("pair", "value"),
    ],
    lists: &["expression_list"],
    forms: &["identifier", "attribute"],
    variables: false,
};

const ECMA_FORMS: &[&str] = &["identifier", "member_expression"];
const OBJECT: &str = "object";
const SHORTHAND_PROPERTY_IDENTIFIER: &str = "shorthand_property_identifier";
const JSX_ATTRIBUTE: &str = "jsx_attribute";
const JSX_EXPRESSION: &str = "jsx_expression";

pub const JAVASCRIPT: ValueSyntax = ValueSyntax {
    positions: &[
        ("arguments", ""),
        ("variable_declarator", "value"),
        ("assignment_expression", "right"),
        ("return_statement", ""),
        ("pair", "value"),
        ("array", ""),
        ("field_definition", "value"),
    ],
    lists: &[],
    forms: ECMA_FORMS,
    variables: false,
};

/// A value in `value as Type` or `value!` gives no record.
pub const TYPESCRIPT: ValueSyntax = ValueSyntax {
    positions: &[
        ("arguments", ""),
        ("variable_declarator", "value"),
        ("assignment_expression", "right"),
        ("return_statement", ""),
        ("pair", "value"),
        ("array", ""),
        ("public_field_definition", "value"),
    ],
    lists: &[],
    forms: ECMA_FORMS,
    variables: false,
};

/// The node kinds of ECMAScript that `ecma` reads and that are not in a table.
pub const ECMA_KINDS: &[&str] = &[OBJECT, JSX_ATTRIBUTE];
pub const ECMA_HOOK_KINDS: &[&str] = &[
    OBJECT,
    SHORTHAND_PROPERTY_IDENTIFIER,
    JSX_ATTRIBUTE,
    JSX_EXPRESSION,
];

fn bare(name: Node<'_>) -> ValueUse<'_> {
    ValueUse {
        callee: Callee {
            target: None,
            name_node: name,
            receiver: None,
            qualifier: None,
        },
        node: name,
        receiver: None,
        name: None,
        variable: false,
    }
}

/// `{ name }` gives the value of `name` to a property. `onClick={name}`
/// gives a function to an attribute of a JSX element. A JSX expression that
/// is a child of an element is not a value of an attribute.
pub fn ecma<'tree>(
    table: &ValueSyntax,
    calls: &CallSyntax,
    node: Node<'tree>,
    context: &Context<'_, 'tree>,
    out: &mut Vec<ValueUse<'tree>>,
) {
    match node.kind() {
        OBJECT => {
            let mut cursor = node.walk();
            let names = node
                .named_children(&mut cursor)
                .filter(|child| child.kind() == SHORTHAND_PROPERTY_IDENTIFIER);
            out.extend(names.map(bare));
        }
        JSX_ATTRIBUTE => {
            let mut cursor = node.walk();
            let value = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == JSX_EXPRESSION)
                .and_then(|expression| expression.named_child(0))
                .filter(|value| table.forms.contains(&value.kind()));
            let used = value.and_then(|value| {
                Some(ValueUse {
                    callee: calls.callee(value, context)?,
                    receiver: calls.member_receiver_range(value),
                    node: value,
                    name: None,
                    variable: false,
                })
            });
            out.extend(used);
        }
        _ => table.values(calls, node, context, out),
    }
}

/// The key of a struct literal, as `Field` in `T{Field: value}` is, is not a
/// value: the table reads only the value of a keyed element.
pub const GO: ValueSyntax = ValueSyntax {
    positions: &[
        ("argument_list", ""),
        ("short_var_declaration", "right"),
        ("var_spec", "value"),
        ("assignment_statement", "right"),
        ("return_statement", ""),
        ("keyed_element", "value"),
        ("literal_value", ""),
    ],
    lists: &["expression_list", "literal_element"],
    forms: &["identifier", "selector_expression"],
    variables: false,
};

/// A field `value.name` is not a method in Rust, thus a member is not a form.
/// The last expression of a block is the value of the block.
pub const RUST: ValueSyntax = ValueSyntax {
    positions: &[
        ("arguments", ""),
        ("let_declaration", "value"),
        ("assignment_expression", "right"),
        ("return_expression", ""),
        ("field_initializer", "value"),
        ("shorthand_field_initializer", ""),
        ("array_expression", ""),
        ("tuple_expression", ""),
        ("block", ""),
    ],
    lists: &[],
    forms: &["identifier", "scoped_identifier", "generic_function"],
    variables: false,
};

/// A method group: the name of a method without arguments.
pub const CSHARP: ValueSyntax = ValueSyntax {
    positions: &[
        ("argument", ""),
        ("assignment_expression", "right"),
        ("variable_declarator", ""),
        ("return_statement", ""),
        ("initializer_expression", ""),
        ("arrow_expression_clause", ""),
    ],
    lists: &[],
    forms: &["identifier", "member_access_expression"],
    variables: false,
};

/// The name of a function is a pointer to the function. `&name` is the same
/// pointer. A member `value.name` is a field, thus a member is not a form.
pub const C: ValueSyntax = ValueSyntax {
    positions: &[
        ("argument_list", ""),
        ("init_declarator", "value"),
        ("assignment_expression", "right"),
        ("return_statement", ""),
        ("initializer_list", ""),
        ("initializer_pair", "value"),
        ("pointer_expression", "argument"),
    ],
    lists: &[],
    forms: &["identifier"],
    variables: false,
};

/// `std::thread worker(run);` is the declaration of a function in the
/// grammar, thus `run` gives no record there.
pub const CPP: ValueSyntax = ValueSyntax {
    positions: C.positions,
    lists: &[],
    forms: &["identifier", "qualified_identifier"],
    variables: false,
};

pub const METHOD_REFERENCE: &str = "method_reference";
pub const TYPE_ARGUMENTS: &str = "type_arguments";

/// A name in Java is a variable, and it is never a method. A variable can
/// hold a lambda.
pub const JAVA: ValueSyntax = ValueSyntax {
    positions: &[
        ("argument_list", ""),
        ("variable_declarator", "value"),
        ("assignment_expression", "right"),
        ("return_statement", ""),
    ],
    lists: &[],
    forms: &["identifier"],
    variables: true,
};

/// `Type::name`, `this::name`, `super::name`, and `value::name`. Type
/// arguments can be before the name, as in `Type::<T>name`. `Type::new` is a
/// constructor, and it gives no record: its last named child is not a name.
pub fn java<'tree>(
    calls: &CallSyntax,
    node: Node<'tree>,
    context: &Context<'_, 'tree>,
    out: &mut Vec<ValueUse<'tree>>,
) {
    if node.kind() != METHOD_REFERENCE {
        JAVA.values(calls, node, context, out);
        return;
    }
    let count = node.named_child_count() as u32;
    let (Some(target), Some(name)) = (node.named_child(0), node.named_child(count.wrapping_sub(1)))
    else {
        return;
    };
    let between_is_type_arguments = match count {
        2 => true,
        3 => node
            .named_child(1)
            .is_some_and(|between| between.kind() == TYPE_ARGUMENTS),
        _ => false,
    };
    if !between_is_type_arguments || name.kind() != "identifier" {
        return;
    }
    let (receiver, qualifier) = calls.before_name(target, context);
    out.push(ValueUse {
        callee: Callee {
            target: None,
            name_node: name,
            receiver,
            qualifier,
        },
        node,
        receiver: Some((target.start_byte(), target.end_byte())),
        name: None,
        variable: false,
    });
}

pub const RUBY_CALL: &str = "call";
pub const SIMPLE_SYMBOL: &str = "simple_symbol";

fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_alphanumeric() || "_?!".contains(character))
}

/// A name without arguments in Ruby is a variable or a call of a method. It
/// is never a method as a value. A variable can hold a lambda or a proc.
pub const RUBY: ValueSyntax = ValueSyntax {
    positions: &[
        ("argument_list", ""),
        ("assignment", "right"),
        ("pair", "value"),
        ("array", ""),
    ],
    lists: &[],
    forms: &["identifier"],
    variables: true,
};

/// `method(:name)` and `value.method(:name)` give the method `name` as an
/// object. `&:name` calls `name` on each element, and the type of an element
/// is not known, thus it gives no record.
pub fn ruby<'tree>(
    calls: &CallSyntax,
    node: Node<'tree>,
    context: &Context<'_, 'tree>,
    out: &mut Vec<ValueUse<'tree>>,
) {
    if node.kind() != RUBY_CALL {
        RUBY.values(calls, node, context, out);
        return;
    }
    let is_method = node
        .child_by_field_name("method")
        .is_some_and(|method| context.text(method) == "method");
    let symbol = node
        .child_by_field_name("arguments")
        .filter(|arguments| arguments.named_child_count() == 1)
        .and_then(|arguments| arguments.named_child(0))
        .filter(|symbol| symbol.kind() == SIMPLE_SYMBOL);
    let (true, Some(symbol)) = (is_method, symbol) else {
        return;
    };
    let text = context.text(symbol);
    let Some(name) = text.strip_prefix(':').filter(|name| is_name(name)) else {
        return;
    };
    let target = node.child_by_field_name("receiver");
    let (receiver, qualifier) = match target {
        Some(target) => calls.before_name(target, context),
        None => (None, None),
    };
    out.push(ValueUse {
        callee: Callee {
            target: None,
            name_node: symbol,
            receiver,
            qualifier,
        },
        node,
        receiver: target.map(|target| (target.start_byte(), target.end_byte())),
        name: Some(name.to_string()),
        variable: false,
    });
}

pub const ARRAY_CREATION_EXPRESSION: &str = "array_creation_expression";
pub const PHP_HOOK_KINDS: &[&str] = &[
    ARRAY_CREATION_EXPRESSION,
    "array_element_initializer",
    "variable_name",
    "class_constant_access_expression",
    "string",
    "encapsed_string",
    "string_content",
];

/// A PHP variable and a PHP function do not share names. A variable can hold
/// a function value.
pub const PHP: ValueSyntax = ValueSyntax {
    positions: &[
        ("argument", ""),
        ("assignment_expression", "right"),
        ("return_statement", ""),
        ("array_element_initializer", ""),
    ],
    lists: &[],
    forms: &["variable_name"],
    variables: true,
};

/// `[$this, 'name']` and `[Type::class, 'name']` are the callable arrays of
/// PHP: the first element is the object or the class, and the second element
/// is the name of the method. A string alone, as `'name'` is, can be a text
/// and not a function, thus it gives no record.
pub fn php<'tree>(
    calls: &CallSyntax,
    node: Node<'tree>,
    context: &Context<'_, 'tree>,
    out: &mut Vec<ValueUse<'tree>>,
) {
    if node.kind() != ARRAY_CREATION_EXPRESSION {
        PHP.values(calls, node, context, out);
        return;
    }
    let element = |position: u32| {
        node.named_child(position)
            .filter(|element| {
                element.kind() == "array_element_initializer" && element.child_count() == 1
            })
            .and_then(|element| element.named_child(0))
    };
    let (Some(target), Some(text), 2) = (element(0), element(1), node.named_child_count()) else {
        return;
    };
    let name = (["string", "encapsed_string"].contains(&text.kind())
        && text.named_child_count() == 1)
        .then(|| text.named_child(0))
        .flatten()
        .filter(|content| content.kind() == "string_content");
    let Some(name) = name.filter(|name| is_name(&context.text(*name))) else {
        return;
    };
    let (receiver, qualifier) = match target.kind() {
        "variable_name" if context.text(target) == "$this" => (Some(Receiver::SelfValue), None),
        "class_constant_access_expression" => {
            let class = target.named_child(0).map(|class| context.text(class));
            let constant = target.named_child(1).map(|constant| context.text(constant));
            match (class, constant.as_deref()) {
                (Some(class), Some("class")) if ["self", "static"].contains(&class.as_str()) => {
                    (Some(Receiver::SelfValue), None)
                }
                (Some(class), Some("class")) => (None, Some(class)),
                _ => return,
            }
        }
        _ => return,
    };
    out.push(ValueUse {
        callee: Callee {
            target: None,
            name_node: name,
            receiver,
            qualifier,
        },
        node,
        receiver: Some((target.start_byte(), target.end_byte())),
        name: None,
        variable: false,
    });
}

pub const COMMAND: &str = "command";
pub const BASH_HOOK_KINDS: &[&str] = &[COMMAND, "word", "raw_string", "string", "string_content"];

/// `trap name SIGNAL` runs the function `name` when the signal comes. The
/// first operand of `trap` is always a command, also in quotation marks. Only
/// an operand that is one name gives a record. A function name that a
/// different command gets as an argument, as `xargs` does, gives no record:
/// the syntax does not tell that the argument is a command.
pub fn bash<'tree>(
    node: Node<'tree>,
    context: &Context<'_, 'tree>,
    out: &mut Vec<ValueUse<'tree>>,
) {
    let is_trap = node
        .child_by_field_name("name")
        .is_some_and(|name| context.text(name) == "trap");
    if !is_trap {
        return;
    }
    let mut cursor = node.walk();
    let mut operands = node
        .children_by_field_name("argument", &mut cursor)
        .peekable();
    if operands
        .peek()
        .is_some_and(|operand| context.text(*operand) == "--")
    {
        operands.next();
    }
    let (Some(handler), Some(_)) = (operands.next(), operands.next()) else {
        return;
    };
    let text = context.text(handler);
    let name = match handler.kind() {
        "word" => Some(text.as_str()),
        "raw_string" => text
            .strip_prefix('\'')
            .and_then(|text| text.strip_suffix('\'')),
        "string" => text
            .strip_prefix('"')
            .and_then(|text| text.strip_suffix('"')),
        _ => None,
    };
    let is_function_name = |name: &&str| {
        !name.starts_with('-')
            && name
                .chars()
                .all(|character| character.is_alphanumeric() || "_-.:".contains(character))
            && name.chars().any(char::is_alphabetic)
    };
    let Some(name) = name.filter(is_function_name) else {
        return;
    };
    out.push(ValueUse {
        callee: Callee {
            target: None,
            name_node: handler,
            receiver: None,
            qualifier: None,
        },
        node: handler,
        receiver: None,
        name: Some(name.to_string()),
        variable: false,
    });
}
