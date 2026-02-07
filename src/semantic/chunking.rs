//! AST-aware chunking for semantic search.
//!
//! Splits long functions at statement boundaries so each chunk has focused
//! vocabulary for better TF-IDF relevance. Short functions remain as single
//! chunks. Each chunk carries its parent type (class/struct/impl) when applicable.

use crate::core::Language;
use crate::parser::{FunctionNode, ParseResult};

/// Maximum characters per chunk. Functions longer than this get split at
/// statement boundaries.
const MAX_CHUNK_CHARS: usize = 500;

/// A chunk of code ready for indexing.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub file_path: String,
    pub symbol_name: String,
    pub symbol_type: String,
    pub parent_name: Option<String>,
    pub signature: String,
    pub content: String,
    pub start_line: u32,
    pub end_line: u32,
    pub chunk_index: u32,
    pub total_chunks: u32,
}

/// Extract chunks from a parsed file.
///
/// Each function becomes one or more chunks. Long functions are split at
/// statement boundaries. The enclosing class/struct/impl name is attached
/// as `parent_name` when the function is nested inside one.
pub fn extract_chunks(
    parse_result: &ParseResult,
    functions: &[FunctionNode],
    rel_path: &str,
) -> Vec<Chunk> {
    let source = String::from_utf8_lossy(&parse_result.source);
    let lines: Vec<&str> = source.lines().collect();

    let parent_map = build_parent_map(parse_result);

    let mut all_chunks = Vec::new();

    for func in functions {
        let parent_name = find_parent(&parent_map, func.start_line, func.end_line);

        let start = (func.start_line as usize).saturating_sub(1);
        let end = (func.end_line as usize).min(lines.len());

        let body = if start < end && start < lines.len() {
            lines[start..end].join("\n")
        } else {
            func.signature.clone()
        };

        if body.len() <= MAX_CHUNK_CHARS {
            all_chunks.push(Chunk {
                file_path: rel_path.to_string(),
                symbol_name: func.name.clone(),
                symbol_type: "function".to_string(),
                parent_name: parent_name.clone(),
                signature: func.signature.clone(),
                content: body,
                start_line: func.start_line,
                end_line: func.end_line,
                chunk_index: 0,
                total_chunks: 1,
            });
        } else {
            let sub_chunks = split_at_statement_boundaries(&body, func.start_line);
            let total = sub_chunks.len() as u32;
            for (i, (chunk_body, chunk_start, chunk_end)) in sub_chunks.into_iter().enumerate() {
                all_chunks.push(Chunk {
                    file_path: rel_path.to_string(),
                    symbol_name: func.name.clone(),
                    symbol_type: "function".to_string(),
                    parent_name: parent_name.clone(),
                    signature: func.signature.clone(),
                    content: chunk_body,
                    start_line: chunk_start,
                    end_line: chunk_end,
                    chunk_index: i as u32,
                    total_chunks: total,
                });
            }
        }
    }

    all_chunks
}

/// A parent type span (class/struct/impl).
struct ParentSpan {
    name: String,
    start_line: u32,
    end_line: u32,
}

/// Build a map of parent type spans from the AST.
fn build_parent_map(parse_result: &ParseResult) -> Vec<ParentSpan> {
    let mut parents = Vec::new();
    let root = parse_result.root_node();
    let type_kinds = get_type_node_kinds(parse_result.language);

    fn visit(
        node: tree_sitter::Node<'_>,
        source: &[u8],
        type_kinds: &[&str],
        parents: &mut Vec<ParentSpan>,
    ) {
        if type_kinds.contains(&node.kind()) {
            if let Some(name) = extract_type_name(&node, source) {
                parents.push(ParentSpan {
                    name,
                    start_line: node.start_position().row as u32 + 1,
                    end_line: node.end_position().row as u32 + 1,
                });
            }
        }
        for child in node.children(&mut node.walk()) {
            visit(child, source, type_kinds, parents);
        }
    }

    visit(root, &parse_result.source, &type_kinds, &mut parents);
    parents
}

/// Find the most specific (innermost) parent that encloses the given line range.
fn find_parent(parents: &[ParentSpan], start_line: u32, end_line: u32) -> Option<String> {
    let mut best: Option<&ParentSpan> = None;
    for p in parents {
        if p.start_line <= start_line && p.end_line >= end_line {
            match best {
                Some(current) => {
                    // Prefer the tighter (inner) span
                    if (p.end_line - p.start_line) < (current.end_line - current.start_line) {
                        best = Some(p);
                    }
                }
                None => best = Some(p),
            }
        }
    }
    best.map(|p| p.name.clone())
}

fn get_type_node_kinds(lang: Language) -> Vec<&'static str> {
    match lang {
        Language::Rust => vec!["struct_item", "enum_item", "impl_item", "trait_item"],
        Language::Go => vec!["type_declaration"],
        Language::Python => vec!["class_definition"],
        Language::TypeScript | Language::JavaScript | Language::Tsx | Language::Jsx => {
            vec!["class_declaration", "class"]
        }
        Language::Java | Language::CSharp => {
            vec!["class_declaration", "interface_declaration"]
        }
        Language::Cpp => vec!["class_specifier", "struct_specifier"],
        Language::C => vec!["struct_specifier"],
        Language::Ruby => vec!["class", "module"],
        Language::Php => vec!["class_declaration", "interface_declaration"],
        Language::Bash => vec![],
    }
}

fn extract_type_name(node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
    // Try "name" field first, then look for an identifier child
    let name_node = node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        let found = node
            .children(&mut cursor)
            .find(|c| c.kind() == "identifier" || c.kind() == "type_identifier");
        found
    });
    name_node
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Split a function body into chunks at statement boundaries.
///
/// Returns (chunk_text, start_line, end_line) tuples.
fn split_at_statement_boundaries(body: &str, base_line: u32) -> Vec<(String, u32, u32)> {
    let lines: Vec<&str> = body.lines().collect();
    if lines.is_empty() {
        return vec![(body.to_string(), base_line, base_line)];
    }

    // Find split points: lines that look like statement boundaries.
    // A line is a boundary candidate if it starts at the same or lower
    // indentation as the second line (first line of the body after the
    // signature) and is non-empty.
    let body_indent = lines
        .iter()
        .skip(1) // skip signature line
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut chunks: Vec<(String, u32, u32)> = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut chunk_start_idx: usize = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        // At a statement boundary if we're at body indent level and
        // the current chunk is already large enough.
        let at_boundary =
            i > 0 && !trimmed.is_empty() && indent <= body_indent && !current_lines.is_empty();

        let current_len: usize = current_lines.iter().map(|l| l.len() + 1).sum();

        if at_boundary && current_len >= MAX_CHUNK_CHARS {
            let text = current_lines.join("\n");
            let start = base_line + chunk_start_idx as u32;
            let end = base_line + (i - 1) as u32;
            chunks.push((text, start, end));
            current_lines.clear();
            chunk_start_idx = i;
        }

        current_lines.push(line);
    }

    // Remaining lines form the last chunk
    if !current_lines.is_empty() {
        let text = current_lines.join("\n");
        let start = base_line + chunk_start_idx as u32;
        let end = base_line + (lines.len() - 1) as u32;
        chunks.push((text, start, end));
    }

    // If we ended up with a single chunk, just return it
    if chunks.is_empty() {
        chunks.push((
            body.to_string(),
            base_line,
            base_line + lines.len() as u32 - 1,
        ));
    }

    chunks
}

/// Format a chunk into enriched text for TF-IDF indexing.
pub fn format_chunk_text(chunk: &Chunk) -> String {
    let parent_prefix = match &chunk.parent_name {
        Some(parent) => format!("{}::", parent),
        None => String::new(),
    };

    let chunk_suffix = if chunk.total_chunks > 1 {
        format!(" ({}/{})", chunk.chunk_index + 1, chunk.total_chunks)
    } else {
        String::new()
    };

    format!(
        "[{}] {}{}{}\n{}",
        chunk.file_path, parent_prefix, chunk.symbol_name, chunk_suffix, chunk.content
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_function_single_chunk() {
        let func = FunctionNode {
            name: "foo".to_string(),
            start_line: 1,
            end_line: 3,
            body_byte_range: None,
            is_exported: true,
            signature: "fn foo()".to_string(),
        };

        let source = b"fn foo() {\n    x + 1\n}";
        let parse_result = parse_rust(source);
        let chunks = extract_chunks(&parse_result, &[func], "src/lib.rs");

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].total_chunks, 1);
        assert_eq!(chunks[0].symbol_name, "foo");
    }

    #[test]
    fn test_long_function_splits() {
        // Build a function body longer than MAX_CHUNK_CHARS with statement boundaries
        let mut body = String::from("fn long_func() {\n");
        for i in 0..60 {
            body.push_str(&format!("    let x{i} = {i};\n"));
        }
        body.push('}');

        let line_count = body.lines().count() as u32;
        let func = FunctionNode {
            name: "long_func".to_string(),
            start_line: 1,
            end_line: line_count,
            body_byte_range: None,
            is_exported: true,
            signature: "fn long_func()".to_string(),
        };

        let parse_result = parse_rust(body.as_bytes());
        let chunks = extract_chunks(&parse_result, &[func], "src/lib.rs");

        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );
        assert!(chunks.iter().all(|c| c.total_chunks == chunks.len() as u32));
        assert!(chunks.iter().all(|c| c.symbol_name == "long_func"));

        // Verify chunk indices are sequential
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i as u32);
        }
    }

    #[test]
    fn test_parent_name_from_impl() {
        let source = b"struct Foo {}\nimpl Foo {\n    fn bar() {\n        1\n    }\n}";
        let parse_result = parse_rust(source);

        let func = FunctionNode {
            name: "bar".to_string(),
            start_line: 3,
            end_line: 5,
            body_byte_range: None,
            is_exported: false,
            signature: "fn bar()".to_string(),
        };

        let chunks = extract_chunks(&parse_result, &[func], "src/lib.rs");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].parent_name.as_deref(), Some("Foo"));
    }

    #[test]
    fn test_no_parent_for_free_function() {
        let source = b"fn free() { 1 }";
        let parse_result = parse_rust(source);

        let func = FunctionNode {
            name: "free".to_string(),
            start_line: 1,
            end_line: 1,
            body_byte_range: None,
            is_exported: false,
            signature: "fn free()".to_string(),
        };

        let chunks = extract_chunks(&parse_result, &[func], "src/lib.rs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].parent_name.is_none());
    }

    #[test]
    fn test_format_chunk_text_simple() {
        let chunk = Chunk {
            file_path: "src/lib.rs".to_string(),
            symbol_name: "foo".to_string(),
            symbol_type: "function".to_string(),
            parent_name: None,
            signature: "fn foo()".to_string(),
            content: "fn foo() { 1 }".to_string(),
            start_line: 1,
            end_line: 1,
            chunk_index: 0,
            total_chunks: 1,
        };

        let text = format_chunk_text(&chunk);
        assert_eq!(text, "[src/lib.rs] foo\nfn foo() { 1 }");
    }

    #[test]
    fn test_format_chunk_text_with_parent_and_index() {
        let chunk = Chunk {
            file_path: "src/lib.rs".to_string(),
            symbol_name: "bar".to_string(),
            symbol_type: "function".to_string(),
            parent_name: Some("Foo".to_string()),
            signature: "fn bar()".to_string(),
            content: "fn bar() { code }".to_string(),
            start_line: 5,
            end_line: 10,
            chunk_index: 1,
            total_chunks: 3,
        };

        let text = format_chunk_text(&chunk);
        assert!(text.starts_with("[src/lib.rs] Foo::bar (2/3)\n"));
    }

    #[test]
    fn test_split_at_boundaries_short() {
        let body = "fn foo() {\n    x + 1\n}";
        let result = split_at_statement_boundaries(body, 1);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_empty_functions_list() {
        let source = b"fn foo() {}";
        let parse_result = parse_rust(source);
        let chunks = extract_chunks(&parse_result, &[], "test.rs");
        assert!(chunks.is_empty());
    }

    fn parse_rust(source: &[u8]) -> ParseResult {
        let parser = crate::parser::Parser::new();
        parser
            .parse(source, Language::Rust, std::path::Path::new("test.rs"))
            .unwrap()
    }
}
