//! Text formatting for semantic search indexing.

use crate::parser::FunctionNode;

/// Format a symbol into enriched text for TF-IDF indexing.
///
/// Format: `[{file_path}] {symbol_name}\n{body}`
/// This format benchmarked at +15% MRR over bare code.
pub fn format_enriched_text(file_path: &str, func: &FunctionNode, source: &str) -> String {
    let body = get_function_body(func, source);
    format!("[{}] {}\n{}", file_path, func.name, body)
}

/// Extract the function body from source by line numbers, with truncation.
fn get_function_body(func: &FunctionNode, source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = (func.start_line as usize).saturating_sub(1);
    let end = (func.end_line as usize).min(lines.len());

    let body = if start < end && start < lines.len() {
        lines[start..end].join("\n")
    } else {
        func.signature.clone()
    };

    let max_chars = 1500;
    if body.len() > max_chars {
        let boundary = body.floor_char_boundary(max_chars);
        body[..boundary].to_string()
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_enriched_text() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 3,
            body_byte_range: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        let source = "fn test_func() {\n    println!(\"hello\");\n}";
        let text = format_enriched_text("src/main.rs", &func, source);
        assert!(text.starts_with("[src/main.rs] test_func\n"));
        assert!(text.contains("println!"));
    }

    #[test]
    fn test_format_enriched_text_truncation() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 1,
            body_byte_range: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        let source = "x".repeat(3000);
        let text = format_enriched_text("test.rs", &func, &source);
        // header + truncated body
        assert!(text.len() <= 1600);
    }

    #[test]
    fn test_format_enriched_text_multibyte() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 1,
            body_byte_range: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        // CJK characters are 3 bytes each; 600 chars = 1800 bytes > 1500
        let source = "\u{4e16}".repeat(600);
        let text = format_enriched_text("test.rs", &func, &source);
        assert!(text.is_char_boundary(text.len()));
    }
}
