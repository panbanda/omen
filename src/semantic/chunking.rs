//! Chunking for semantic search.
//!
//! Splits long functions at statement boundaries so each chunk has focused
//! vocabulary for better TF-IDF relevance. Short functions remain as single
//! chunks. Each chunk carries its parent type (class/struct/impl) when applicable.

use crate::index::extract::FileSymbols;
use crate::index::model::Symbol;

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
    /// The first line of the definition. A sub-chunk has its own `start_line`.
    pub definition_line: u32,
    /// The position of the definition among the definitions of one name on one line.
    pub definition_ordinal: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub chunk_index: u32,
    pub total_chunks: u32,
}

/// The definitions of a file that the search indexes: the functions, the
/// methods, and the constructors, in source order. The index holds only
/// definitions that have a name, because a search result needs a name.
pub fn searchable_symbols(facts: &FileSymbols) -> Vec<&Symbol> {
    facts
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.is_callable()
                && (symbol.body_range.is_some() || facts.language == crate::core::Language::Ruby)
        })
        .collect()
}

/// Extract chunks from the facts of a file.
///
/// Each function becomes one or more chunks. Long functions are split at
/// statement boundaries. The enclosing class/struct/impl name is attached
/// as `parent_name` when the function is nested inside one.
pub fn extract_chunks(source: &[u8], facts: &FileSymbols, rel_path: &str) -> Vec<Chunk> {
    let source = String::from_utf8_lossy(source);
    let lines: Vec<&str> = source.lines().collect();
    let functions = searchable_symbols(facts);

    let mut all_chunks = Vec::new();

    for (func, definition_ordinal) in functions.iter().zip(definition_ordinals(&functions)) {
        let parent_name = parent_name(facts, func);
        let start_line = func.range.start_line;
        let end_line = func.range.end_line;

        let start = (start_line as usize).saturating_sub(1);
        let end = (end_line as usize).min(lines.len());

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
                definition_line: start_line,
                definition_ordinal,
                start_line,
                end_line,
                chunk_index: 0,
                total_chunks: 1,
            });
        } else {
            let sub_chunks = split_at_statement_boundaries(&body, start_line);
            let total = sub_chunks.len() as u32;
            for (i, (chunk_body, chunk_start, chunk_end)) in sub_chunks.into_iter().enumerate() {
                all_chunks.push(Chunk {
                    file_path: rel_path.to_string(),
                    symbol_name: func.name.clone(),
                    symbol_type: "function".to_string(),
                    parent_name: parent_name.clone(),
                    signature: func.signature.clone(),
                    content: chunk_body,
                    definition_line: start_line,
                    definition_ordinal,
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

/// The position of each function among the functions that have its name and
/// its first line. A line can have more than one definition of one name.
pub fn definition_ordinals(functions: &[&Symbol]) -> Vec<u32> {
    let mut seen: std::collections::HashMap<(&str, u32), u32> = std::collections::HashMap::new();
    functions
        .iter()
        .map(|func| {
            let count = seen
                .entry((func.name.as_str(), func.range.start_line))
                .or_insert(0);
            let ordinal = *count;
            *count += 1;
            ordinal
        })
        .collect()
}

/// The name of the type to which a function belongs: the receiver type of a
/// Rust `impl` block or of a Go receiver, or the innermost class-like
/// definition that contains the function.
fn parent_name(facts: &FileSymbols, function: &Symbol) -> Option<String> {
    if let Some(receiver) = &function.receiver_type {
        return Some(receiver.clone());
    }
    facts
        .symbols
        .iter()
        .filter(|symbol| symbol.is_class_like(facts.language))
        .filter(|symbol| {
            symbol.range.start_byte <= function.range.start_byte
                && function.range.end_byte <= symbol.range.end_byte
                && symbol.key != function.key
        })
        .min_by_key(|symbol| symbol.range.end_byte - symbol.range.start_byte)
        .map(|symbol| symbol.name.clone())
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
    use crate::core::Language;
    use std::path::Path;

    fn facts_of(source: &[u8], language: Language, name: &str) -> FileSymbols {
        let parsed = crate::index::headers::parse(Path::new(name), language, source).unwrap();
        crate::index::extract::extract_file(&parsed, name)
    }

    fn chunks_of(source: &[u8], language: Language, name: &str) -> Vec<Chunk> {
        extract_chunks(source, &facts_of(source, language, name), name)
    }

    fn parent_of(source: &[u8], language: Language, name: &str) -> Option<String> {
        let chunks = chunks_of(source, language, name);
        assert_eq!(chunks.len(), 1, "{name}: {chunks:?}");
        chunks[0].parent_name.clone()
    }

    #[test]
    fn test_short_function_single_chunk() {
        let chunks = chunks_of(b"fn foo() {\n    x + 1\n}", Language::Rust, "src/lib.rs");

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].total_chunks, 1);
        assert_eq!(chunks[0].symbol_name, "foo");
        assert_eq!(chunks[0].file_path, "src/lib.rs");
    }

    #[test]
    fn test_long_function_splits() {
        let mut body = String::from("fn long_func() {\n");
        for i in 0..60 {
            body.push_str(&format!("    let x{i} = {i};\n"));
        }
        body.push('}');

        let chunks = chunks_of(body.as_bytes(), Language::Rust, "src/lib.rs");

        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );
        assert!(chunks.iter().all(|c| c.total_chunks == chunks.len() as u32));
        assert!(chunks.iter().all(|c| c.symbol_name == "long_func"));
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i as u32);
        }
    }

    #[test]
    fn test_parent_name_from_impl() {
        let source = b"struct Foo {}\nimpl Foo {\n    fn bar() {\n        1\n    }\n}";
        assert_eq!(
            parent_of(source, Language::Rust, "src/lib.rs").as_deref(),
            Some("Foo")
        );
    }

    #[test]
    fn test_no_parent_for_free_function() {
        assert!(parent_of(b"fn free() { 1 }", Language::Rust, "src/lib.rs").is_none());
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
            definition_line: 1,
            definition_ordinal: 0,
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
            definition_line: 5,
            definition_ordinal: 0,
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
    fn test_file_without_functions_has_no_chunks() {
        assert!(chunks_of(b"const LIMIT: u32 = 1;\n", Language::Rust, "test.rs").is_empty());
    }

    #[test]
    fn test_parent_detection_go() {
        let source = b"package main\ntype Server struct{}\nfunc (s *Server) Start() {\n\treturn\n}";
        assert_eq!(
            parent_of(source, Language::Go, "main.go").as_deref(),
            Some("Server")
        );
    }

    #[test]
    fn test_parent_detection_python() {
        let source = b"class Foo:\n    def bar(self):\n        pass\n";
        assert_eq!(
            parent_of(source, Language::Python, "test.py").as_deref(),
            Some("Foo")
        );
    }

    #[test]
    fn test_parent_detection_typescript() {
        let source = b"class Widget {\n    render() {\n        return 1;\n    }\n}";
        assert_eq!(
            parent_of(source, Language::TypeScript, "test.ts").as_deref(),
            Some("Widget")
        );
    }

    #[test]
    fn test_parent_detection_tsx_and_jsx() {
        let source = b"class Widget {\n    render() {\n        return 1;\n    }\n}";
        for (language, name) in [(Language::Tsx, "test.tsx"), (Language::Jsx, "test.jsx")] {
            assert_eq!(
                parent_of(source, language, name).as_deref(),
                Some("Widget"),
                "{name}"
            );
        }
    }

    #[test]
    fn test_parent_detection_javascript() {
        let source = b"class Widget {\n    render() {\n        return 1;\n    }\n}";
        assert_eq!(
            parent_of(source, Language::JavaScript, "test.js").as_deref(),
            Some("Widget")
        );
    }

    #[test]
    fn test_parent_detection_java() {
        let source = b"class Service {\n    void run() {\n        return;\n    }\n}";
        assert_eq!(
            parent_of(source, Language::Java, "Test.java").as_deref(),
            Some("Service")
        );
    }

    #[test]
    fn test_parent_detection_cpp() {
        let source = b"class Engine {\npublic:\n    void start() {\n        return;\n    }\n};";
        assert_eq!(
            parent_of(source, Language::Cpp, "test.cpp").as_deref(),
            Some("Engine")
        );
    }

    #[test]
    fn test_parent_detection_c_struct() {
        let source = b"struct Point { int x; int y; };\nvoid draw() { return; }";
        assert!(parent_of(source, Language::C, "test.c").is_none());
    }

    #[test]
    fn test_parent_detection_ruby() {
        let source = b"class Dog\n  def bark\n    puts 'woof'\n  end\nend";
        assert_eq!(
            parent_of(source, Language::Ruby, "test.rb").as_deref(),
            Some("Dog")
        );
    }

    #[test]
    fn test_parent_detection_php() {
        let source =
            b"<?php\nclass Controller {\n    function index() {\n        return 1;\n    }\n}";
        assert_eq!(
            parent_of(source, Language::Php, "test.php").as_deref(),
            Some("Controller")
        );
    }

    #[test]
    fn test_parent_detection_csharp() {
        let source = b"class Handler {\n    void Process() {\n        return;\n    }\n}";
        assert_eq!(
            parent_of(source, Language::CSharp, "test.cs").as_deref(),
            Some("Handler")
        );
    }

    #[test]
    fn test_parent_detection_bash_none() {
        let source = b"#!/bin/bash\nmy_func() {\n    echo hello\n}";
        assert!(parent_of(source, Language::Bash, "test.sh").is_none());
    }

    #[test]
    fn test_impl_trait_for_type_uses_type_not_trait() {
        let source =
            b"trait Display {}\nimpl Display for Foo {\n    fn fmt() {\n        return;\n    }\n}";
        assert_eq!(
            parent_of(source, Language::Rust, "src/lib.rs").as_deref(),
            Some("Foo")
        );
    }

    #[test]
    fn test_two_definitions_of_one_name_on_one_line_have_two_ordinals() {
        let source = b"function same() {} function same() {}\n";
        let facts = facts_of(source, Language::JavaScript, "test.js");
        let symbols = searchable_symbols(&facts);

        assert_eq!(definition_ordinals(&symbols), [0, 1]);
    }
}
