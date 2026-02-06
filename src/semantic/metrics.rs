//! Metric annotation for indexed symbols.
//!
//! Runs complexity and TDG analyzers during sync to annotate each symbol
//! with quality metrics for quality-weighted search ranking.

use std::collections::HashMap;
use std::path::Path;

use crate::analyzers::complexity;
use crate::analyzers::tdg;
use crate::analyzers::tdg::Grade;
use crate::core::Language;

/// Quality metrics for a single symbol.
#[derive(Debug, Clone, Default)]
pub struct SymbolMetrics {
    pub cyclomatic_complexity: u32,
    pub cognitive_complexity: u32,
    pub tdg_score: f32,
    pub tdg_grade: String,
}

/// Compute metrics for all symbols in a file.
///
/// Returns a map from (symbol_name, start_line) to SymbolMetrics.
/// Uses the complexity analyzer for per-function metrics and TDG for file-level grade.
pub fn compute_file_metrics(
    path: &Path,
    content: &[u8],
    _language: Language,
) -> HashMap<(String, u32), SymbolMetrics> {
    let mut metrics_map = HashMap::new();

    // Run complexity analysis
    let complexity_analyzer = complexity::Analyzer::new();
    let complexity_result = complexity_analyzer.analyze_content(path, content.to_vec());

    // Run TDG analysis (uses its own Language enum derived from file extension)
    let tdg_analyzer = tdg::Analyzer::new();
    let source_str = String::from_utf8_lossy(content);
    let tdg_lang = tdg::Language::from_extension(path);
    let tdg_result = tdg_analyzer.analyze_source(&source_str, tdg_lang, &path.to_string_lossy());

    let (tdg_score, tdg_grade) = match tdg_result {
        Ok(score) => (score.total, grade_to_string(score.grade)),
        Err(_) => (0.0, String::new()),
    };

    if let Ok(file_result) = complexity_result {
        for func in &file_result.functions {
            let key = (func.name.clone(), func.start_line);
            metrics_map.insert(
                key,
                SymbolMetrics {
                    cyclomatic_complexity: func.metrics.cyclomatic,
                    cognitive_complexity: func.metrics.cognitive,
                    tdg_score,
                    tdg_grade: tdg_grade.clone(),
                },
            );
        }
    }

    // For symbols not matched by complexity (e.g. impl blocks), provide file-level TDG
    if !tdg_grade.is_empty() && metrics_map.is_empty() {
        // Store a sentinel entry that the caller can use as fallback
        metrics_map.insert(
            (String::new(), 0),
            SymbolMetrics {
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score,
                tdg_grade,
            },
        );
    }

    metrics_map
}

/// Look up metrics for a specific symbol, falling back to file-level TDG if no exact match.
pub fn lookup_metrics(
    metrics_map: &HashMap<(String, u32), SymbolMetrics>,
    symbol_name: &str,
    start_line: u32,
) -> SymbolMetrics {
    // Exact match by name + line
    if let Some(m) = metrics_map.get(&(symbol_name.to_string(), start_line)) {
        return m.clone();
    }

    // Try matching by name only (line numbers may differ slightly between extractors)
    for ((name, _), m) in metrics_map {
        if name == symbol_name {
            return m.clone();
        }
    }

    // Fall back to file-level sentinel
    if let Some(m) = metrics_map.get(&(String::new(), 0)) {
        return SymbolMetrics {
            cyclomatic_complexity: 0,
            cognitive_complexity: 0,
            tdg_score: m.tdg_score,
            tdg_grade: m.tdg_grade.clone(),
        };
    }

    SymbolMetrics::default()
}

fn grade_to_string(grade: Grade) -> String {
    match grade {
        Grade::APlus => "A+",
        Grade::A => "A",
        Grade::AMinus => "A-",
        Grade::BPlus => "B+",
        Grade::B => "B",
        Grade::BMinus => "B-",
        Grade::CPlus => "C+",
        Grade::C => "C",
        Grade::CMinus => "C-",
        Grade::D => "D",
        Grade::F => "F",
    }
    .to_string()
}

/// Convert a TDG grade string to a quality weight for ranking.
pub fn grade_to_weight(grade: &str) -> f32 {
    match grade {
        "A+" | "A" | "A-" => 1.0,
        "B+" | "B" | "B-" => 0.85,
        "C+" | "C" | "C-" => 0.60,
        "D" => 0.40,
        "F" => 0.20,
        _ => 1.0, // No penalty if grade unknown
    }
}

/// Compute a quality-adjusted score from semantic similarity and metrics.
pub fn quality_adjusted_score(semantic_score: f32, tdg_grade: &str) -> f32 {
    semantic_score * grade_to_weight(tdg_grade)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grade_to_weight() {
        assert_eq!(grade_to_weight("A+"), 1.0);
        assert_eq!(grade_to_weight("A"), 1.0);
        assert_eq!(grade_to_weight("B"), 0.85);
        assert_eq!(grade_to_weight("C"), 0.60);
        assert_eq!(grade_to_weight("D"), 0.40);
        assert_eq!(grade_to_weight("F"), 0.20);
        assert_eq!(grade_to_weight(""), 1.0);
    }

    #[test]
    fn test_quality_adjusted_score() {
        let score = quality_adjusted_score(0.9, "A");
        assert!((score - 0.9).abs() < 1e-6);

        let score = quality_adjusted_score(0.9, "F");
        assert!((score - 0.18).abs() < 1e-6);

        let score = quality_adjusted_score(0.9, "B");
        assert!((score - 0.765).abs() < 1e-6);
    }

    #[test]
    fn test_compute_file_metrics_rust() {
        let source = b"fn simple() {\n    let x = 1;\n}\n\nfn complex(x: i32) -> i32 {\n    if x > 0 {\n        if x > 10 {\n            x * 2\n        } else {\n            x + 1\n        }\n    } else {\n        0\n    }\n}\n";
        let path = Path::new("test.rs");

        let metrics = compute_file_metrics(path, source, Language::Rust);
        assert!(!metrics.is_empty());

        // "simple" should have low complexity
        let simple = lookup_metrics(&metrics, "simple", 1);
        assert_eq!(simple.cyclomatic_complexity, 1);

        // "complex" should have higher complexity
        let complex = lookup_metrics(&metrics, "complex", 5);
        assert!(complex.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_lookup_metrics_fallback() {
        let mut map = HashMap::new();
        map.insert(
            (String::new(), 0),
            SymbolMetrics {
                cyclomatic_complexity: 0,
                cognitive_complexity: 0,
                tdg_score: 42.0,
                tdg_grade: "B".to_string(),
            },
        );

        let result = lookup_metrics(&map, "unknown_func", 99);
        assert_eq!(result.tdg_score, 42.0);
        assert_eq!(result.tdg_grade, "B");
    }

    #[test]
    fn test_lookup_metrics_exact_match() {
        let mut map = HashMap::new();
        map.insert(
            ("my_func".to_string(), 10),
            SymbolMetrics {
                cyclomatic_complexity: 5,
                cognitive_complexity: 8,
                tdg_score: 30.0,
                tdg_grade: "C".to_string(),
            },
        );

        let result = lookup_metrics(&map, "my_func", 10);
        assert_eq!(result.cyclomatic_complexity, 5);
        assert_eq!(result.cognitive_complexity, 8);
    }

    #[test]
    fn test_lookup_metrics_name_match_different_line() {
        let mut map = HashMap::new();
        map.insert(
            ("my_func".to_string(), 10),
            SymbolMetrics {
                cyclomatic_complexity: 5,
                cognitive_complexity: 8,
                tdg_score: 30.0,
                tdg_grade: "C".to_string(),
            },
        );

        // Different start line but same name
        let result = lookup_metrics(&map, "my_func", 11);
        assert_eq!(result.cyclomatic_complexity, 5);
    }

    #[test]
    fn test_symbol_metrics_default() {
        let m = SymbolMetrics::default();
        assert_eq!(m.cyclomatic_complexity, 0);
        assert_eq!(m.cognitive_complexity, 0);
        assert_eq!(m.tdg_score, 0.0);
        assert!(m.tdg_grade.is_empty());
    }
}
