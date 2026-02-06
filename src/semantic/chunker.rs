//! AST-aware chunking for semantic search.
//!
//! Splits large functions at statement boundaries using the tree-sitter AST,
//! ensuring each chunk fits within the embedding model's token limit while
//! preserving semantic coherence.

use crate::parser::{FunctionNode, ParseResult};

/// Maximum characters per chunk. At ~4 chars/token (BPE estimate), this targets
/// ~500 tokens, fitting well within BGE-small's 512 token limit.
const MAX_CHUNK_CHARS: usize = 2000;

/// A chunk of source code ready for embedding.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub symbol_name: String,
    pub symbol_type: String,
    pub signature: String,
    pub content: String,
    pub start_line: u32,
    pub end_line: u32,
    pub chunk_index: u32,
    pub total_chunks: u32,
}

/// Chunk all functions from a parse result.
///
/// Small functions become a single chunk. Large functions are split at statement
/// boundaries identified by the tree-sitter AST, with the function signature
/// prepended to each chunk for context.
pub fn chunk_functions(functions: &[FunctionNode], parse_result: &ParseResult) -> Vec<Chunk> {
    let source_str = String::from_utf8_lossy(&parse_result.source);
    let mut all_chunks = Vec::new();

    for func in functions {
        let func_text = get_function_text(func, &source_str);

        if func_text.len() <= MAX_CHUNK_CHARS {
            all_chunks.push(Chunk {
                symbol_name: func.name.clone(),
                symbol_type: "function".to_string(),
                signature: func.signature.clone(),
                content: func_text,
                start_line: func.start_line,
                end_line: func.end_line,
                chunk_index: 0,
                total_chunks: 1,
            });
        } else {
            let sub_chunks = split_function(func, parse_result, &source_str);
            let total = sub_chunks.len().max(1) as u32;
            for (i, sub) in sub_chunks.into_iter().enumerate() {
                all_chunks.push(Chunk {
                    symbol_name: func.name.clone(),
                    symbol_type: "function".to_string(),
                    signature: func.signature.clone(),
                    content: format!("{}\n{}", func.signature, sub.text),
                    start_line: sub.start_line,
                    end_line: sub.end_line,
                    chunk_index: i as u32,
                    total_chunks: total,
                });
            }
        }
    }

    all_chunks
}

/// Estimate token count from character count (conservative BPE estimate).
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

struct SubChunk {
    text: String,
    start_line: u32,
    end_line: u32,
}

fn get_function_text(func: &FunctionNode, source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = (func.start_line as usize).saturating_sub(1);
    let end = (func.end_line as usize).min(lines.len());

    if start < end && start < lines.len() {
        lines[start..end].join("\n")
    } else {
        func.signature.clone()
    }
}

fn split_function(func: &FunctionNode, parse_result: &ParseResult, source: &str) -> Vec<SubChunk> {
    // Try tree-sitter-based splitting using body byte range
    if let Some((body_start, body_end)) = func.body_byte_range {
        let root = parse_result.root_node();
        if let Some(body_node) = root.descendant_for_byte_range(body_start, body_end) {
            let ranges = collect_statement_ranges(&body_node);
            if !ranges.is_empty() {
                return group_into_chunks(&ranges, source);
            }
        }
    }

    // Fallback: line-based splitting
    split_by_lines(func, source)
}

struct StatementRange {
    start_line: u32,
    end_line: u32,
}

fn collect_statement_ranges(body_node: &tree_sitter::Node<'_>) -> Vec<StatementRange> {
    let mut ranges = Vec::new();
    let mut cursor = body_node.walk();

    for child in body_node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        ranges.push(StatementRange {
            start_line: child.start_position().row as u32 + 1,
            end_line: child.end_position().row as u32 + 1,
        });
    }

    ranges
}

/// Group statement ranges into chunks that fit within MAX_CHUNK_CHARS.
/// Reserves space for the function signature that will be prepended.
fn group_into_chunks(ranges: &[StatementRange], source: &str) -> Vec<SubChunk> {
    let lines: Vec<&str> = source.lines().collect();
    // Reserve space for "signature\n" prefix
    let max_body_chars = MAX_CHUNK_CHARS.saturating_sub(200);

    let mut chunks = Vec::new();
    let mut chunk_start: Option<u32> = None;
    let mut chunk_end: u32 = 0;
    let mut chunk_chars: usize = 0;

    for range in ranges {
        let text_len = text_len_for_range(range.start_line, range.end_line, &lines);

        // Statement alone exceeds limit: flush current chunk, emit this one solo
        if text_len > max_body_chars {
            if let Some(start) = chunk_start {
                chunks.push(build_sub_chunk(start, chunk_end, &lines));
            }
            chunks.push(build_sub_chunk(range.start_line, range.end_line, &lines));
            chunk_start = None;
            chunk_chars = 0;
            continue;
        }

        // Adding this statement would exceed limit: flush
        if chunk_chars + text_len > max_body_chars && chunk_start.is_some() {
            if let Some(start) = chunk_start {
                chunks.push(build_sub_chunk(start, chunk_end, &lines));
            }
            chunk_start = Some(range.start_line);
            chunk_end = range.end_line;
            chunk_chars = text_len;
        } else {
            if chunk_start.is_none() {
                chunk_start = Some(range.start_line);
            }
            chunk_end = range.end_line;
            chunk_chars += text_len;
        }
    }

    if let Some(start) = chunk_start {
        chunks.push(build_sub_chunk(start, chunk_end, &lines));
    }

    chunks
}

fn text_len_for_range(start_line: u32, end_line: u32, lines: &[&str]) -> usize {
    let start = (start_line as usize).saturating_sub(1);
    let end = (end_line as usize).min(lines.len());
    if start >= end || start >= lines.len() {
        return 0;
    }
    lines[start..end].iter().map(|l| l.len() + 1).sum()
}

fn build_sub_chunk(start_line: u32, end_line: u32, lines: &[&str]) -> SubChunk {
    let start = (start_line as usize).saturating_sub(1);
    let end = (end_line as usize).min(lines.len());
    let text = if start < end && start < lines.len() {
        lines[start..end].join("\n")
    } else {
        String::new()
    };
    SubChunk {
        text,
        start_line,
        end_line,
    }
}

/// Fallback: split function body by lines when tree-sitter body node is unavailable.
fn split_by_lines(func: &FunctionNode, source: &str) -> Vec<SubChunk> {
    let lines: Vec<&str> = source.lines().collect();
    let start = (func.start_line as usize).saturating_sub(1);
    let end = (func.end_line as usize).min(lines.len());

    if start >= end || start >= lines.len() {
        return vec![SubChunk {
            text: func.signature.clone(),
            start_line: func.start_line,
            end_line: func.end_line,
        }];
    }

    let max_body_chars = MAX_CHUNK_CHARS.saturating_sub(200);
    let mut chunks = Vec::new();
    let mut chunk_start = start;
    let mut chunk_chars: usize = 0;

    for i in start..end {
        let line_len = lines[i].len() + 1;

        if chunk_chars + line_len > max_body_chars && chunk_chars > 0 {
            chunks.push(SubChunk {
                text: lines[chunk_start..i].join("\n"),
                start_line: chunk_start as u32 + 1,
                end_line: i as u32,
            });
            chunk_start = i;
            chunk_chars = 0;
        }

        chunk_chars += line_len;
    }

    if chunk_start < end {
        chunks.push(SubChunk {
            text: lines[chunk_start..end].join("\n"),
            start_line: chunk_start as u32 + 1,
            end_line: end as u32,
        });
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        // 2000 chars -> 500 tokens
        let text = "a".repeat(2000);
        assert_eq!(estimate_tokens(&text), 500);
    }

    #[test]
    fn test_small_function_single_chunk() {
        let source = "fn hello() {\n    println!(\"hello\");\n}\n";
        let func = FunctionNode {
            name: "hello".to_string(),
            start_line: 1,
            end_line: 3,
            body_byte_range: None,
            is_exported: true,
            signature: "fn hello()".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Rust,
                std::path::Path::new("test.rs"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].total_chunks, 1);
        assert!(chunks[0].content.contains("println!"));
    }

    #[test]
    fn test_large_function_splits() {
        // Generate a function with many statements that exceeds MAX_CHUNK_CHARS
        let mut body_lines = Vec::new();
        for i in 0..200 {
            body_lines.push(format!("    let x{} = {};", i, i));
        }
        let source = format!("fn big_func() {{\n{}\n}}", body_lines.join("\n"));

        let func = FunctionNode {
            name: "big_func".to_string(),
            start_line: 1,
            end_line: body_lines.len() as u32 + 2,
            body_byte_range: Some((16, source.len())),
            is_exported: true,
            signature: "fn big_func()".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Rust,
                std::path::Path::new("test.rs"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        // All chunks should reference the same symbol
        for chunk in &chunks {
            assert_eq!(chunk.symbol_name, "big_func");
            assert_eq!(chunk.total_chunks, chunks.len() as u32);
            // Each chunk should include the signature for context
            assert!(chunk.content.contains("fn big_func()"));
        }

        // Chunk indices should be sequential
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i as u32);
        }
    }

    #[test]
    fn test_chunking_preserves_all_lines() {
        let mut body_lines = Vec::new();
        for i in 0..100 {
            body_lines.push(format!("    let var{} = {};", i, i));
        }
        let source = format!("fn covered() {{\n{}\n}}", body_lines.join("\n"));

        let func = FunctionNode {
            name: "covered".to_string(),
            start_line: 1,
            end_line: body_lines.len() as u32 + 2,
            body_byte_range: Some((15, source.len())),
            is_exported: true,
            signature: "fn covered()".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Rust,
                std::path::Path::new("test.rs"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);

        // Every `let varN` should appear in at least one chunk
        for i in 0..100 {
            let needle = format!("var{}", i);
            let found = chunks.iter().any(|c| c.content.contains(&needle));
            assert!(found, "var{} not found in any chunk", i);
        }
    }

    #[test]
    fn test_line_based_fallback() {
        // Each line ~50 chars; 100 lines = ~5000 chars, well over MAX_CHUNK_CHARS
        let mut lines = Vec::new();
        for i in 0..100 {
            lines.push(format!(
                "    let variable_with_long_name_{} = some_value_{};",
                i, i
            ));
        }
        let source = format!("fn no_body() {{\n{}\n}}", lines.join("\n"));

        // No body_byte_range -> forces line-based fallback
        let func = FunctionNode {
            name: "no_body".to_string(),
            start_line: 1,
            end_line: lines.len() as u32 + 2,
            body_byte_range: None,
            is_exported: false,
            signature: "fn no_body()".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Rust,
                std::path::Path::new("test.rs"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        for chunk in &chunks {
            assert_eq!(chunk.symbol_name, "no_body");
        }
    }

    #[test]
    fn test_multiple_functions_chunked() {
        let source = concat!(
            "fn small() {\n    1\n}\n\n",
            "fn also_small() {\n    2\n}\n"
        );

        let funcs = vec![
            FunctionNode {
                name: "small".to_string(),
                start_line: 1,
                end_line: 3,
                body_byte_range: None,
                is_exported: true,
                signature: "fn small()".to_string(),
            },
            FunctionNode {
                name: "also_small".to_string(),
                start_line: 5,
                end_line: 7,
                body_byte_range: None,
                is_exported: true,
                signature: "fn also_small()".to_string(),
            },
        ];

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Rust,
                std::path::Path::new("test.rs"),
            )
            .unwrap();

        let chunks = chunk_functions(&funcs, &parse_result);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].symbol_name, "small");
        assert_eq!(chunks[1].symbol_name, "also_small");
    }

    #[test]
    fn test_chunk_fields_populated() {
        let source = "fn test_fn() {\n    let x = 1;\n}\n";
        let func = FunctionNode {
            name: "test_fn".to_string(),
            start_line: 1,
            end_line: 3,
            body_byte_range: None,
            is_exported: true,
            signature: "fn test_fn()".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Rust,
                std::path::Path::new("test.rs"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);
        assert_eq!(chunks.len(), 1);

        let c = &chunks[0];
        assert_eq!(c.symbol_name, "test_fn");
        assert_eq!(c.symbol_type, "function");
        assert_eq!(c.signature, "fn test_fn()");
        assert_eq!(c.start_line, 1);
        assert_eq!(c.end_line, 3);
        assert_eq!(c.chunk_index, 0);
        assert_eq!(c.total_chunks, 1);
    }

    #[test]
    fn test_empty_functions_list() {
        let source = "// no functions";
        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Rust,
                std::path::Path::new("test.rs"),
            )
            .unwrap();

        let chunks = chunk_functions(&[], &parse_result);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_max_chunk_chars_constant() {
        assert_eq!(MAX_CHUNK_CHARS, 2000);
    }

    #[test]
    fn test_typescript_chunking() {
        let source = "function greet(name: string): void {\n    console.log(name);\n}\n";
        let func = FunctionNode {
            name: "greet".to_string(),
            start_line: 1,
            end_line: 3,
            body_byte_range: None,
            is_exported: false,
            signature: "function greet(name: string): void".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::TypeScript,
                std::path::Path::new("test.ts"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("console.log"));
    }

    #[test]
    fn test_python_chunking() {
        let source = "def hello():\n    print(\"hello\")\n";
        let func = FunctionNode {
            name: "hello".to_string(),
            start_line: 1,
            end_line: 2,
            body_byte_range: None,
            is_exported: false,
            signature: "def hello()".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Python,
                std::path::Path::new("test.py"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("print"));
    }

    #[test]
    fn test_go_chunking() {
        let source = "func Hello() {\n\tfmt.Println(\"hello\")\n}\n";
        let func = FunctionNode {
            name: "Hello".to_string(),
            start_line: 1,
            end_line: 3,
            body_byte_range: None,
            is_exported: true,
            signature: "func Hello()".to_string(),
        };

        let parser = crate::parser::Parser::new();
        let parse_result = parser
            .parse(
                source.as_bytes(),
                crate::core::Language::Go,
                std::path::Path::new("test.go"),
            )
            .unwrap();

        let chunks = chunk_functions(&[func], &parse_result);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("Println"));
    }
}
