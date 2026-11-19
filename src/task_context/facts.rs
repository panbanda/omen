use crate::parser::{queries::get_nested_scope_node_types, walk_preorder, ParseResult, Walk};
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
    let nested_scopes = get_nested_scope_node_types(parsed.language);
    walk_preorder(body, |node| {
        if node != body && !node.is_named() {
            return Walk::SkipChildren;
        }
        if nested_scopes.contains(&node.kind()) {
            return Walk::SkipChildren;
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
                omitted += 1;
            }
        }
        Walk::Children
    });
    json!({"uses": uses, "omitted_uses": omitted, "basis": "AST_occurrences_not_resolved_data_flow"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::deep::{nest, on_small_stack};

    #[test]
    fn extract_reads_a_deeply_nested_body() {
        let uses = on_small_stack(|| {
            let source = format!(
                "package p\nfunc f() int {{ return {} }}\n",
                nest("(", "x", ")")
            );
            let parsed = crate::parser::Parser::new()
                .parse(
                    source.as_bytes(),
                    crate::core::Language::Go,
                    std::path::Path::new("a.go"),
                )
                .unwrap();
            let body_start = source.find('{').unwrap();
            extract(&parsed, body_start)["uses"].clone()
        });
        assert_eq!(uses[0]["name"], "x");
    }
}
