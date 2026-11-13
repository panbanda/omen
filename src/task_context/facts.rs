use crate::parser::{queries::get_nested_scope_node_types, ParseResult};
use serde_json::{json, Value};

pub(super) fn extract(parsed: &ParseResult, body_start: usize) -> Value {
    let root = parsed.tree.root_node();
    let Some(mut body) = root.descendant_for_byte_range(body_start, body_start.saturating_add(1))
    else {
        return json!({"uses": [], "omitted_uses": 0});
    };
    while let Some(parent) = body.parent() {
        if parent.start_byte() != body_start {
            break;
        }
        body = parent;
    }
    let mut uses = Vec::new();
    let mut omitted = 0usize;
    fn visit(
        node: tree_sitter::Node<'_>,
        parsed: &ParseResult,
        uses: &mut Vec<Value>,
        omitted: &mut usize,
    ) {
        if get_nested_scope_node_types(parsed.language).contains(&node.kind()) {
            return;
        }
        if matches!(
            node.kind(),
            "identifier" | "field_identifier" | "property_identifier"
        ) {
            let access = node
                .parent()
                .map(|parent| {
                    if parent.child_by_field_name("left") == Some(node)
                        || parent.child_by_field_name("name") == Some(node)
                            && parent.kind().contains("declarator")
                    {
                        "syntactic_write"
                    } else if parent.child_by_field_name("function") == Some(node) {
                        "call_target"
                    } else {
                        "syntactic_use"
                    }
                })
                .unwrap_or("syntactic_use");
            if uses.len() < 64 {
                uses.push(json!({"name": parsed.node_text(&node), "line": node.start_position().row + 1, "access": access}));
            } else {
                *omitted += 1;
            }
        }
        for child in node.named_children(&mut node.walk()) {
            visit(child, parsed, uses, omitted);
        }
    }
    visit(body, parsed, &mut uses, &mut omitted);
    json!({"uses": uses, "omitted_uses": omitted, "basis": "AST_occurrences_not_resolved_data_flow"})
}
