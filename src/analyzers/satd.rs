//! SATD (Self-Admitted Technical Debt) analyzer.
//!
//! Finds TODO, FIXME, HACK, and other debt markers in comments. omen:ignore

use std::time::Instant;

use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result, SourceFile};
use crate::parser::queries::satd;

/// SATD analyzer.
pub struct Analyzer {
    /// Compiled regex patterns for each category.
    patterns: Vec<(String, Regex, f64)>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Create a new SATD analyzer.
    pub fn new() -> Self {
        let patterns = satd::all_categories()
            .iter()
            .map(|(category, markers, weight)| {
                let pattern = format!(r"(?i)\b({})\b", markers.join("|"));
                let regex = Regex::new(&pattern).expect("Invalid SATD pattern");
                (category.to_string(), regex, *weight)
            })
            .collect();

        Self { patterns }
    }

    /// Analyze a single file for SATD.
    pub fn analyze_file(&self, file: &SourceFile) -> Vec<SatdItem> {
        let content = file.content_str();
        let mut items = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            // Check if line is a comment
            if !is_comment_line(line) {
                continue;
            }

            // Skip lines with omen:ignore directive
            if has_ignore_directive(line) {
                continue;
            }

            for (category, regex, weight) in &self.patterns {
                if let Some(mat) = regex.find(line) {
                    let marker = mat.as_str().to_uppercase();

                    // Check if this is a valid SATD marker (not a false positive)
                    if !is_valid_satd_marker(line, mat.start(), &marker) {
                        continue;
                    }

                    items.push(SatdItem {
                        file: file.path.to_string_lossy().to_string(),
                        line: line_num as u32 + 1,
                        category: category.clone(),
                        severity: severity_from_weight(*weight),
                        marker,
                        text: line.trim().chars().take(200).collect(),
                        weight: *weight,
                    });
                    break; // One category per line
                }
            }
        }

        items
    }
}

/// Markers that are commonly false positives when not at the start of a comment.
const AMBIGUOUS_MARKERS: &[&str] = &[
    "ERROR",
    "NEED",
    "SKIP",
    "FAILS",
    "IMPLEMENT",
    "IGNORE",
    "PENDING",
    "SLOW",
    "UNSAFE",
    "DOC",
    "DOCUMENT",
];

/// Check if a line contains an omen:ignore directive.
///
/// Supports: `omen:ignore`, `omen:ignore-line`, `omen:ignore-satd`.
/// Case-insensitive matching, matching the Go v3 behavior.
fn has_ignore_directive(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("omen:ignore")
}

/// Check if a SATD marker is valid (not a false positive).
///
/// For ambiguous markers like ERROR, NEED, SKIP, we require them to be
/// followed by a colon (e.g., "ERROR:", "SKIP:") to distinguish SATD from omen:ignore
/// normal explanatory comments like "// Skip this step if authenticated".
///
/// Clear markers like TODO, FIXME, HACK, BUG are accepted anywhere. omen:ignore
fn is_valid_satd_marker(line: &str, match_start: usize, marker: &str) -> bool {
    // Clear markers (TODO, FIXME, HACK, BUG, etc.) are always valid omen:ignore
    if !AMBIGUOUS_MARKERS.contains(&marker) {
        return true;
    }

    // For ambiguous markers, require a colon or similar punctuation after the marker
    // This distinguishes "// SKIP: test disabled" from "// Skip this step" omen:ignore
    let match_end = match_start + marker.len();
    if match_end < line.len() {
        let next_char = line[match_end..].chars().next();
        // Accept markers followed by : or - (common SATD patterns)
        if matches!(next_char, Some(':') | Some('-')) {
            return true;
        }
    }

    // Ambiguous markers without punctuation are likely false positives
    false
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "satd"
    }

    fn description(&self) -> &'static str {
        "Find self-admitted technical debt (TODO/FIXME/HACK comments)"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        let start = Instant::now();

        // Single pass: collect SATD items and LOC simultaneously to avoid double file loading
        // Collect into Vec first for efficient parallel iteration
        let files: Vec<_> = ctx.files.iter().collect();
        let (items, total_loc): (Vec<SatdItem>, usize) = files
            .par_iter()
            .filter_map(|path| SourceFile::load(path).ok())
            .map(|file| {
                let loc = file.lines_of_code();
                let file_items = self.analyze_file(&file);
                (file_items, loc)
            })
            .reduce(
                || (Vec::new(), 0),
                |(mut items1, loc1), (items2, loc2)| {
                    items1.extend(items2);
                    (items1, loc1 + loc2)
                },
            );

        // Group by category
        let mut by_category = std::collections::HashMap::new();
        for item in &items {
            *by_category.entry(item.category.clone()).or_insert(0usize) += 1;
        }

        // Calculate weighted density
        let total_weight: f64 = items.iter().map(|i| i.weight).sum();

        let density = if total_loc > 0 {
            total_weight / (total_loc as f64 / 1000.0)
        } else {
            0.0
        };

        let total_items: usize = by_category.values().sum();
        let analysis = Analysis {
            items,
            by_category,
            density,
            summary: AnalysisSummary {
                total_items,
                weighted_count: total_weight,
                density,
            },
        };

        tracing::info!(
            "SATD analysis completed in {:?}: {} items, density {:.2}",
            start.elapsed(),
            analysis.summary.total_items,
            analysis.summary.density
        );

        Ok(analysis)
    }
}

/// Full SATD analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// All SATD items found.
    pub items: Vec<SatdItem>,
    /// Count by category.
    pub by_category: std::collections::HashMap<String, usize>,
    /// Weighted density per 1K LOC.
    pub density: f64,
    /// Summary statistics.
    pub summary: AnalysisSummary,
}

/// A single SATD item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatdItem {
    /// File path.
    pub file: String,
    /// Line number (1-indexed).
    pub line: u32,
    /// Category (design, defect, requirement, etc.).
    pub category: String,
    /// Severity level.
    pub severity: Severity,
    /// Matched marker (TODO, FIXME, etc.). omen:ignore
    pub marker: String,
    /// Comment text (truncated).
    pub text: String,
    /// Severity weight.
    #[serde(skip)]
    pub weight: f64,
}

/// SATD severity level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

/// Analysis summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    /// Total SATD items.
    pub total_items: usize,
    /// Weighted count.
    pub weighted_count: f64,
    /// Density per 1K LOC.
    pub density: f64,
}

/// Check if a line is a comment.
fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("'''")
        || trimmed.starts_with("\"\"\"")
        || trimmed.starts_with("--")
        || trimmed.starts_with(';')
}

/// Convert weight to severity level.
fn severity_from_weight(weight: f64) -> Severity {
    if weight >= 4.0 {
        Severity::Critical
    } else if weight >= 2.0 {
        Severity::High
    } else if weight >= 1.0 {
        Severity::Medium
    } else {
        Severity::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Language;

    #[test]
    fn test_satd_detection() {
        let analyzer = Analyzer::new();
        let content = b"// TODO: implement this\n// FIXME: broken\nfn main() {}\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, "TODO");
        assert_eq!(items[1].marker, "FIXME");
    }

    #[test]
    fn test_satd_categories() {
        let analyzer = Analyzer::new();
        let content = b"// HACK: workaround\n// SECURITY: vulnerable\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].category, "design");
        assert_eq!(items[1].category, "security");
    }

    #[test]
    fn test_severity_from_weight() {
        assert_eq!(severity_from_weight(4.0), Severity::Critical);
        assert_eq!(severity_from_weight(2.0), Severity::High);
        assert_eq!(severity_from_weight(1.0), Severity::Medium);
        assert_eq!(severity_from_weight(0.25), Severity::Low);
    }

    #[test]
    fn test_satd_false_positives_are_filtered() {
        let analyzer = Analyzer::new();

        // These should NOT trigger SATD detection - they're explanatory comments
        let false_positive_content = b"\
// Handles all other unknown errors\n\
# We need to validate input before processing\n\
// Skip this step if already authenticated\n\
fn main() {}\n\
"
        .to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, false_positive_content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(
            items.len(),
            0,
            "Should not detect SATD in explanatory comments, found: {:?}",
            items
                .iter()
                .map(|i| (&i.text, &i.marker))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_satd_true_positives_with_markers_at_start() {
        let analyzer = Analyzer::new();

        // These SHOULD trigger SATD detection - markers at start of comment
        let true_positive_content = [
            b"// TODO: implement this feature\n" as &[u8],
            b"// FIXME: broken authentication\n",
            b"// ERROR: this needs fixing\n",
            b"// NEED: add validation\n",
            b"// SKIP: until API is ready\n",
            b"fn main() {}\n",
        ]
        .concat();
        let file = SourceFile::from_content("test.rs", Language::Rust, true_positive_content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(
            items.len(),
            5,
            "Should detect all 5 SATD markers at start of comment, found: {:?}",
            items
                .iter()
                .map(|i| (&i.text, &i.marker))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_documentation_debt_detection() {
        let analyzer = Analyzer::new();
        let content = b"// DOC: needs API documentation\nfn main() {}\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].category, "documentation");
        assert_eq!(items[0].marker, "DOC");
    }

    #[test]
    fn test_undocumented_marker() {
        let analyzer = Analyzer::new();
        let content = b"// UNDOCUMENTED: public interface\nfn main() {}\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].category, "documentation");
        assert_eq!(items[0].marker, "UNDOCUMENTED");
    }

    #[test]
    fn test_documentation_markers_require_punctuation() {
        let analyzer = Analyzer::new();

        // "DOC" and "DOCUMENT" without colon should not trigger (ambiguous)
        let false_positive_content =
            b"// See the documentation for details\n// Document this later\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, false_positive_content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(
            items.len(),
            0,
            "Should not detect documentation markers in prose, found: {:?}",
            items
                .iter()
                .map(|i| (&i.text, &i.marker))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_all_documentation_markers() {
        let analyzer = Analyzer::new();
        let content = [
            b"// DOC: add API docs\n" as &[u8],
            b"// UNDOCUMENTED public function\n",
            b"// DOCUMENT: this module\n",
            b"// NODOC for internal use\n",
            b"// UNDOC: missing docs\n",
            b"fn main() {}\n",
        ]
        .concat();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        // DOC and DOCUMENT require colon (ambiguous markers)
        // UNDOCUMENTED, NODOC, UNDOC are unambiguous omen:ignore
        assert_eq!(
            items.len(),
            5,
            "Should detect all documentation markers, found: {:?}",
            items
                .iter()
                .map(|i| (&i.text, &i.marker))
                .collect::<Vec<_>>()
        );
        for item in &items {
            assert_eq!(item.category, "documentation");
        }
    }

    #[test]
    fn test_ignore_directive_suppresses_detection() {
        let analyzer = Analyzer::new();
        let content = [
            b"// TODO: real debt\n" as &[u8],
            b"// TODO: false positive omen:ignore\n",
            b"// FIXME: also ignored omen:ignore-satd\n",
            b"// HACK: case insensitive OMEN:IGNORE\n",
            b"fn main() {}\n",
        ]
        .concat();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        let items = analyzer.analyze_file(&file);
        assert_eq!(
            items.len(),
            1,
            "Only the line without omen:ignore should be detected"
        );
        assert_eq!(items[0].marker, "TODO");
        assert!(items[0].text.contains("real debt"));
    }

    #[test]
    fn test_has_ignore_directive() {
        assert!(has_ignore_directive("// TODO: false positive omen:ignore"));
        assert!(has_ignore_directive("// FIXME: not debt omen:ignore-satd"));
        assert!(has_ignore_directive("// BUG fix detection OMEN:IGNORE"));
        assert!(has_ignore_directive("// omen:ignore-line"));
        assert!(!has_ignore_directive("// TODO: real debt"));
        assert!(!has_ignore_directive("// normal comment"));
    }
}
