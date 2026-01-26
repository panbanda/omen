//! Mutant generator - walks AST and collects mutants from operators.

use std::collections::HashSet;
use std::path::Path;

use crate::core::{Language, Result};
use crate::parser::{ParseResult, Parser};

use super::operator::MutationOperator;
use super::Mutant;

/// Generator that creates mutants from source code.
pub struct MutantGenerator {
    parser: Parser,
    operators: Vec<Box<dyn MutationOperator>>,
}

impl MutantGenerator {
    /// Create a new mutant generator with the given operators.
    pub fn new(operators: Vec<Box<dyn MutationOperator>>) -> Self {
        Self {
            parser: Parser::new(),
            operators,
        }
    }

    /// Generate mutants for a file.
    pub fn generate_for_file(&self, path: &Path) -> Result<Vec<Mutant>> {
        let result = self.parser.parse_file(path)?;
        Ok(self.generate_from_parse_result(&result))
    }

    /// Generate mutants for source content.
    pub fn generate_for_content(
        &self,
        content: &[u8],
        language: Language,
        path: &Path,
    ) -> Result<Vec<Mutant>> {
        let result = self.parser.parse(content, language, path)?;
        Ok(self.generate_from_parse_result(&result))
    }

    /// Generate mutants from an already-parsed source.
    pub fn generate_from_parse_result(&self, result: &ParseResult) -> Vec<Mutant> {
        let prefix = generate_mutant_prefix(result);
        let mut all_mutants = Vec::new();

        for operator in &self.operators {
            if operator.supports_language(result.language) {
                let mutants = operator.generate_mutants(result, &prefix);
                all_mutants.extend(mutants);
            }
        }

        deduplicate_mutants(all_mutants)
    }

    /// Get the operators used by this generator.
    pub fn operators(&self) -> &[Box<dyn MutationOperator>] {
        &self.operators
    }
}

/// Generate a prefix for mutant IDs based on the file path.
fn generate_mutant_prefix(result: &ParseResult) -> String {
    let filename = result
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Use a hash of the full path for uniqueness
    let path_str = result.path.to_string_lossy();
    let hash = xxhash_rust::xxh3::xxh3_64(path_str.as_bytes());

    format!("{}-{:x}", filename, hash & 0xFFFF)
}

/// Remove duplicate mutants (same location and replacement).
fn deduplicate_mutants(mutants: Vec<Mutant>) -> Vec<Mutant> {
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(mutants.len());

    for mutant in mutants {
        // Key is (byte_range, replacement)
        let key = (mutant.byte_range, mutant.replacement.clone());
        if seen.insert(key) {
            result.push(mutant);
        }
    }

    result
}

/// Filter mutants based on line range.
pub fn filter_by_lines(mutants: Vec<Mutant>, start_line: u32, end_line: u32) -> Vec<Mutant> {
    mutants
        .into_iter()
        .filter(|m| m.line >= start_line && m.line <= end_line)
        .collect()
}

/// Filter mutants based on operator names.
pub fn filter_by_operators(mutants: Vec<Mutant>, operators: &[&str]) -> Vec<Mutant> {
    mutants
        .into_iter()
        .filter(|m| operators.contains(&m.operator.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzers::mutation::operators::{
        ArithmeticOperator, LiteralOperator, RelationalOperator,
    };
    use std::fs;
    use tempfile::TempDir;

    fn create_generator() -> MutantGenerator {
        MutantGenerator::new(vec![
            Box::new(LiteralOperator),
            Box::new(RelationalOperator),
            Box::new(ArithmeticOperator),
        ])
    }

    #[test]
    fn test_generator_new() {
        let gen = create_generator();
        assert_eq!(gen.operators().len(), 3);
    }

    #[test]
    fn test_generate_for_content_rust() {
        let gen = create_generator();
        let code = b"fn main() { let x = 42; }";

        let mutants = gen
            .generate_for_content(code, Language::Rust, Path::new("test.rs"))
            .unwrap();

        assert!(!mutants.is_empty());

        // Should have mutations for the literal 42
        let literal_mutants: Vec<_> = mutants.iter().filter(|m| m.operator == "CRR").collect();
        assert!(!literal_mutants.is_empty());
    }

    #[test]
    fn test_generate_for_content_with_operators() {
        let gen = create_generator();
        let code = b"fn check(a: i32, b: i32) -> bool { a + b < 10 }";

        let mutants = gen
            .generate_for_content(code, Language::Rust, Path::new("test.rs"))
            .unwrap();

        // Should have CRR mutations (for 10)
        assert!(mutants.iter().any(|m| m.operator == "CRR"));

        // Should have AOR mutations (for +)
        assert!(mutants.iter().any(|m| m.operator == "AOR"));

        // Should have ROR mutations (for <)
        assert!(mutants.iter().any(|m| m.operator == "ROR"));
    }

    #[test]
    fn test_generate_for_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"fn main() { let x = 1; }").unwrap();

        let gen = create_generator();
        let mutants = gen.generate_for_file(&file_path).unwrap();

        assert!(!mutants.is_empty());
    }

    #[test]
    fn test_generate_for_nonexistent_file() {
        let gen = create_generator();
        let result = gen.generate_for_file(Path::new("/nonexistent/file.rs"));
        assert!(result.is_err());
    }

    #[test]
    fn test_deduplicate_mutants() {
        let mutants = vec![
            Mutant::new("1", "test.rs", "CRR", 1, 1, "42", "0", "desc", (0, 2)),
            Mutant::new("2", "test.rs", "CRR", 1, 1, "42", "0", "desc", (0, 2)), // Duplicate
            Mutant::new("3", "test.rs", "CRR", 1, 1, "42", "1", "desc", (0, 2)), // Different replacement
        ];

        let deduped = deduplicate_mutants(mutants);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn test_filter_by_lines() {
        let mutants = vec![
            Mutant::new("1", "test.rs", "CRR", 1, 1, "42", "0", "desc", (0, 2)),
            Mutant::new("2", "test.rs", "CRR", 5, 1, "10", "0", "desc", (50, 52)),
            Mutant::new("3", "test.rs", "CRR", 10, 1, "20", "0", "desc", (100, 102)),
        ];

        let filtered = filter_by_lines(mutants, 3, 7);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].line, 5);
    }

    #[test]
    fn test_filter_by_operators() {
        let mutants = vec![
            Mutant::new("1", "test.rs", "CRR", 1, 1, "42", "0", "desc", (0, 2)),
            Mutant::new("2", "test.rs", "ROR", 1, 5, "<", ">", "desc", (5, 6)),
            Mutant::new("3", "test.rs", "AOR", 1, 10, "+", "-", "desc", (10, 11)),
        ];

        let filtered = filter_by_operators(mutants, &["CRR", "AOR"]);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|m| m.operator != "ROR"));
    }

    #[test]
    fn test_generate_mutant_prefix() {
        let parser = Parser::new();
        let result = parser
            .parse(b"fn main() {}", Language::Rust, Path::new("test.rs"))
            .unwrap();

        let prefix = generate_mutant_prefix(&result);
        assert!(prefix.starts_with("test.rs-"));
    }

    #[test]
    fn test_mutant_ids_unique_within_file() {
        let gen = create_generator();
        let code = b"fn main() { let x = 1; let y = 2; let z = 3; }";

        let mutants = gen
            .generate_for_content(code, Language::Rust, Path::new("test.rs"))
            .unwrap();

        // All IDs should be unique
        let ids: HashSet<_> = mutants.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), mutants.len());
    }

    #[test]
    fn test_generator_respects_language_support() {
        // Create a generator with only LiteralOperator
        let gen = MutantGenerator::new(vec![Box::new(LiteralOperator)]);

        let code = b"def main():\n    x = 42";
        let mutants = gen
            .generate_for_content(code, Language::Python, Path::new("test.py"))
            .unwrap();

        // Should only have CRR mutations
        assert!(mutants.iter().all(|m| m.operator == "CRR"));
    }
}
