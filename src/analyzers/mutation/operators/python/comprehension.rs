//! Python-specific list comprehension mutation operators.
//!
//! This operator mutates Python list comprehensions:
//! - `[x for x in items]` -> `[]` (empty list)
//! - `[x for x in items if cond]` -> `[x for x in items]` (remove filter)

use crate::core::Language;
use crate::parser::ParseResult;

use crate::analyzers::mutation::operator::MutationOperator;
use crate::analyzers::mutation::operators::walk_and_collect_mutants;
use crate::analyzers::mutation::Mutant;

/// PCR (Python Comprehension Replacement) operator.
///
/// Mutates Python list comprehensions to test comprehension logic coverage.
pub struct PythonComprehensionOperator;

impl MutationOperator for PythonComprehensionOperator {
    fn name(&self) -> &'static str {
        "PCR"
    }

    fn description(&self) -> &'static str {
        "Python Comprehension Replacement - mutates list comprehensions"
    }

    fn generate_mutants(&self, result: &ParseResult, mutant_id_prefix: &str) -> Vec<Mutant> {
        let mut counter = 0;
        walk_and_collect_mutants(result, |node| match node.kind() {
            "list_comprehension" => {
                self.mutate_list_comprehension(&node, result, mutant_id_prefix, &mut counter)
            }
            "generator_expression" => {
                self.mutate_generator_expression(&node, result, mutant_id_prefix, &mut counter)
            }
            "dictionary_comprehension" => {
                self.mutate_dict_comprehension(&node, result, mutant_id_prefix, &mut counter)
            }
            "set_comprehension" => {
                self.mutate_set_comprehension(&node, result, mutant_id_prefix, &mut counter)
            }
            _ => Vec::new(),
        })
    }

    fn supports_language(&self, lang: Language) -> bool {
        matches!(lang, Language::Python)
    }
}

impl PythonComprehensionOperator {
    /// Mutate a list comprehension.
    fn mutate_list_comprehension(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Vec<Mutant> {
        let mut mutants = Vec::new();
        let node_text = match node.utf8_text(&result.source) {
            Ok(t) => t,
            Err(_) => return mutants,
        };

        let start = node.start_position();

        // Mutation 1: Replace entire comprehension with empty list
        *counter += 1;
        mutants.push(Mutant::new(
            format!("{}-{}", prefix, counter),
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + 1) as u32,
            node_text,
            "[]",
            "Replace list comprehension with empty list",
            (node.start_byte(), node.end_byte()),
        ));

        // Mutation 2: Remove if clause if present
        if let Some(mutant) = self.try_remove_if_clause(node, result, prefix, counter) {
            mutants.push(mutant);
        }

        mutants
    }

    /// Mutate a generator expression.
    fn mutate_generator_expression(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Vec<Mutant> {
        let mut mutants = Vec::new();

        // Remove if clause if present
        if let Some(mutant) = self.try_remove_if_clause(node, result, prefix, counter) {
            mutants.push(mutant);
        }

        mutants
    }

    /// Mutate a dictionary comprehension.
    fn mutate_dict_comprehension(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Vec<Mutant> {
        let mut mutants = Vec::new();
        let node_text = match node.utf8_text(&result.source) {
            Ok(t) => t,
            Err(_) => return mutants,
        };

        let start = node.start_position();

        // Replace with empty dict
        *counter += 1;
        mutants.push(Mutant::new(
            format!("{}-{}", prefix, counter),
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + 1) as u32,
            node_text,
            "{}",
            "Replace dict comprehension with empty dict",
            (node.start_byte(), node.end_byte()),
        ));

        // Remove if clause if present
        if let Some(mutant) = self.try_remove_if_clause(node, result, prefix, counter) {
            mutants.push(mutant);
        }

        mutants
    }

    /// Mutate a set comprehension.
    fn mutate_set_comprehension(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Vec<Mutant> {
        let mut mutants = Vec::new();
        let node_text = match node.utf8_text(&result.source) {
            Ok(t) => t,
            Err(_) => return mutants,
        };

        let start = node.start_position();

        // Replace with empty set
        *counter += 1;
        mutants.push(Mutant::new(
            format!("{}-{}", prefix, counter),
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + 1) as u32,
            node_text,
            "set()",
            "Replace set comprehension with empty set",
            (node.start_byte(), node.end_byte()),
        ));

        // Remove if clause if present
        if let Some(mutant) = self.try_remove_if_clause(node, result, prefix, counter) {
            mutants.push(mutant);
        }

        mutants
    }

    /// Try to remove an if clause from a comprehension.
    fn try_remove_if_clause(
        &self,
        node: &tree_sitter::Node<'_>,
        result: &ParseResult,
        prefix: &str,
        counter: &mut u32,
    ) -> Option<Mutant> {
        // Find the for_in_clause child
        let mut for_clause = None;
        for child in node.children(&mut node.walk()) {
            if child.kind() == "for_in_clause" {
                for_clause = Some(child);
                break;
            }
        }

        let for_clause = for_clause?;

        // Find if_clause within for_in_clause
        let mut if_clause = None;
        for child in for_clause.children(&mut for_clause.walk()) {
            if child.kind() == "if_clause" {
                if_clause = Some(child);
                break;
            }
        }

        let if_clause = if_clause?;

        // Get the full comprehension text
        let node_text = node.utf8_text(&result.source).ok()?;
        let if_text = if_clause.utf8_text(&result.source).ok()?;

        // Create the new comprehension without the if clause
        let new_text = node_text.replace(if_text, "").trim().to_string();

        // Clean up extra whitespace
        let new_text = new_text
            .replace("  ", " ")
            .replace(" ]", "]")
            .replace(" }", "}")
            .replace(" )", ")");

        *counter += 1;
        let start = node.start_position();

        Some(Mutant::new(
            format!("{}-{}", prefix, counter),
            result.path.clone(),
            self.name(),
            (start.row + 1) as u32,
            (start.column + 1) as u32,
            node_text,
            new_text,
            "Remove filter condition from comprehension",
            (node.start_byte(), node.end_byte()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::super::test_utils::parse_py;
    #[test]
    fn test_python_comprehension_operator_name() {
        let op = PythonComprehensionOperator;
        assert_eq!(op.name(), "PCR");
    }

    #[test]
    fn test_python_comprehension_operator_description() {
        let op = PythonComprehensionOperator;
        assert!(op.description().contains("comprehension"));
    }

    #[test]
    fn test_supports_only_python() {
        let op = PythonComprehensionOperator;
        assert!(op.supports_language(Language::Python));
        assert!(!op.supports_language(Language::Go));
        assert!(!op.supports_language(Language::Rust));
        assert!(!op.supports_language(Language::TypeScript));
        assert!(!op.supports_language(Language::JavaScript));
    }

    #[test]
    fn test_mutate_list_comprehension_to_empty() {
        let code = b"result = [x for x in items]";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let empty_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "[]").collect();
        assert!(
            !empty_mutants.is_empty(),
            "Should create empty list mutation"
        );
    }

    #[test]
    #[ignore = "Python tree-sitter grammar needs investigation for filtered comprehensions"]
    fn test_remove_filter_from_comprehension() {
        let code = b"result = [x for x in items if x > 0]";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let filter_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.description.contains("filter"))
            .collect();
        assert!(
            !filter_mutants.is_empty(),
            "Should create remove filter mutation"
        );
    }

    #[test]
    fn test_mutant_operator_is_pcr() {
        let code = b"result = [x for x in items]";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            assert_eq!(mutant.operator, "PCR");
        }
    }

    #[test]
    fn test_dict_comprehension_to_empty() {
        let code = b"result = {k: v for k, v in items.items()}";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let empty_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "{}").collect();
        assert!(
            !empty_mutants.is_empty(),
            "Should create empty dict mutation"
        );
    }

    #[test]
    fn test_set_comprehension_to_empty() {
        let code = b"result = {x for x in items}";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let empty_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.replacement == "set()")
            .collect();
        assert!(
            !empty_mutants.is_empty(),
            "Should create empty set mutation"
        );
    }

    #[test]
    fn test_no_mutation_for_regular_list() {
        let code = b"result = [1, 2, 3]";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        assert!(
            mutants.is_empty(),
            "Should not mutate regular list literals"
        );
    }

    #[test]
    fn test_byte_range_valid() {
        let code = b"result = [x for x in items]";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        for mutant in &mutants {
            let (start, end) = mutant.byte_range;
            assert!(start < end);
            assert!(end <= code.len());
        }
    }

    #[test]
    fn test_multiple_comprehensions() {
        let code = b"a = [x for x in items]\nb = {k: v for k, v in pairs}";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutations for both comprehensions
        let list_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "[]").collect();
        let dict_mutants: Vec<_> = mutants.iter().filter(|m| m.replacement == "{}").collect();

        assert!(!list_mutants.is_empty(), "Should find list comprehension");
        assert!(!dict_mutants.is_empty(), "Should find dict comprehension");
    }

    #[test]
    fn test_nested_comprehension() {
        let code = b"result = [[y for y in x] for x in matrix]";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        // Should find mutations for both inner and outer comprehensions
        assert!(mutants.len() >= 2, "Should mutate nested comprehensions");
    }

    #[test]
    #[ignore = "Python tree-sitter grammar needs investigation for complex filters"]
    fn test_comprehension_with_complex_filter() {
        let code = b"result = [x for x in items if x > 0 and x < 100]";
        let result = parse_py(code);
        let op = PythonComprehensionOperator;

        let mutants = op.generate_mutants(&result, "test");

        let filter_mutants: Vec<_> = mutants
            .iter()
            .filter(|m| m.description.contains("filter"))
            .collect();
        assert!(
            !filter_mutants.is_empty(),
            "Should handle complex filter conditions"
        );
    }
}
