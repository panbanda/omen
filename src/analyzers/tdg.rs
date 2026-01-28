//! Technical Debt Gradient (TDG) analyzer.
//!
//! Computes a 0-100 score (higher is better) based on:
//! - Structural complexity (cyclomatic)
//! - Semantic complexity (nesting depth)
//! - Code duplication
//! - Coupling (imports)
//! - Documentation coverage
//! - Consistency (indentation style)
//! - Hotspot (churn x complexity from git history)
//! - Temporal coupling (files that change together)

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::analyzers::{hotspot, temporal};
use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// TDG weight configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weights {
    pub structural_complexity: f32,
    pub semantic_complexity: f32,
    pub duplication: f32,
    pub coupling: f32,
    pub documentation: f32,
    pub consistency: f32,
    pub hotspot: f32,
    pub temporal_coupling: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            structural_complexity: 20.0,
            semantic_complexity: 15.0,
            duplication: 15.0,
            coupling: 15.0,
            documentation: 5.0,
            consistency: 10.0,
            // Hotspot: penalize files that are both highly complex and frequently changed
            hotspot: 10.0,
            // Temporal coupling: penalize files that change together with many others
            temporal_coupling: 10.0,
        }
    }
}

/// TDG threshold configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub max_cyclomatic_complexity: u32,
    pub max_nesting_depth: u32,
    pub max_coupling: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            max_cyclomatic_complexity: 30,
            max_nesting_depth: 4,
            max_coupling: 15,
        }
    }
}

/// TDG analyzer.
pub struct Analyzer {
    weights: Weights,
    thresholds: Thresholds,
    max_file_size: Option<u64>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            weights: Weights::default(),
            thresholds: Thresholds::default(),
            max_file_size: None,
        }
    }

    pub fn with_weights(mut self, weights: Weights) -> Self {
        self.weights = weights;
        self
    }

    pub fn with_thresholds(mut self, thresholds: Thresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    pub fn with_max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = Some(size);
        self
    }

    /// Analyze a single file and return its TDG score.
    pub fn analyze_file(&self, path: &Path) -> Result<Score> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::core::Error::analysis(format!("Failed to read file: {e}")))?;

        if let Some(max_size) = self.max_file_size {
            if content.len() as u64 > max_size {
                return Err(crate::core::Error::analysis(format!(
                    "File size {} exceeds maximum {}",
                    content.len(),
                    max_size
                )));
            }
        }

        let language = Language::from_extension(path);
        self.analyze_source(&content, language, path.to_string_lossy().as_ref())
    }

    /// Analyze source code and return its TDG score.
    pub fn analyze_source(
        &self,
        source: &str,
        language: Language,
        file_path: &str,
    ) -> Result<Score> {
        let mut penalties = Vec::new();

        let mut score = Score::new();
        score.language = language;
        score.confidence = language.confidence();
        score.file_path = file_path.to_string();

        // Analyze each component
        score.structural_complexity = self.analyze_structural_complexity(source, &mut penalties);
        score.semantic_complexity = self.analyze_semantic_complexity(source, &mut penalties);
        score.duplication_ratio = self.analyze_duplication(source, &mut penalties);
        score.coupling_score = self.analyze_coupling(source);
        score.doc_coverage = self.analyze_documentation(source, language);
        score.consistency_score = self.analyze_consistency(source);

        // Check for critical defects
        let (defect_count, has_critical) = self.detect_critical_defects(source, language);
        score.critical_defects_count = defect_count;
        score.has_critical_defects = has_critical;

        // Store penalty attributions
        score.penalties_applied = penalties;

        // Calculate final score
        score.calculate_total();

        Ok(score)
    }

    fn analyze_structural_complexity(
        &self,
        source: &str,
        penalties: &mut Vec<PenaltyAttribution>,
    ) -> f32 {
        let mut points = self.weights.structural_complexity;
        let lines: Vec<&str> = source.lines().collect();
        let cyclomatic = self.estimate_cyclomatic_complexity(&lines);

        if cyclomatic > self.thresholds.max_cyclomatic_complexity {
            let excess = (cyclomatic - self.thresholds.max_cyclomatic_complexity) as f32;
            let penalty = excess.min(15.0) * 0.5;
            penalties.push(PenaltyAttribution {
                source_metric: "structural_complexity".to_string(),
                amount: penalty,
                issue: format!("High cyclomatic complexity: {cyclomatic}"),
            });
            points -= penalty;
        }

        points.max(0.0)
    }

    fn analyze_semantic_complexity(
        &self,
        source: &str,
        penalties: &mut Vec<PenaltyAttribution>,
    ) -> f32 {
        let mut points = self.weights.semantic_complexity;
        let nesting_depth = self.estimate_nesting_depth(source);

        if nesting_depth > self.thresholds.max_nesting_depth as i32 {
            let excess = (nesting_depth - self.thresholds.max_nesting_depth as i32) as f32;
            let penalty = excess.min(10.0);
            penalties.push(PenaltyAttribution {
                source_metric: "semantic_complexity".to_string(),
                amount: penalty,
                issue: format!("Deep nesting: {nesting_depth} levels"),
            });
            points -= penalty;
        }

        points.max(0.0)
    }

    fn analyze_duplication(&self, source: &str, penalties: &mut Vec<PenaltyAttribution>) -> f32 {
        let mut points = self.weights.duplication;
        let ratio = self.estimate_duplication_ratio(source);

        if ratio > 0.1 {
            let penalty = (ratio * 20.0).min(20.0);
            penalties.push(PenaltyAttribution {
                source_metric: "duplication".to_string(),
                amount: penalty,
                issue: format!("Code duplication: {:.1}%", ratio * 100.0),
            });
            points -= penalty;
        }

        points.max(0.0)
    }

    fn analyze_coupling(&self, source: &str) -> f32 {
        let mut import_count = 0;
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("use ")
                || trimmed.starts_with("import ")
                || trimmed.starts_with("from ")
                || trimmed.starts_with("#include ")
            {
                import_count += 1;
            }
        }

        let base_score = self.weights.coupling;
        if import_count > 20 {
            let penalty = ((import_count - 20) as f32 * 0.2).min(10.0);
            (base_score - penalty).max(0.0)
        } else {
            base_score
        }
    }

    fn analyze_documentation(&self, source: &str, language: Language) -> f32 {
        let lines: Vec<&str> = source.lines().collect();
        let total_lines = lines.len();
        if total_lines == 0 {
            return self.weights.documentation;
        }

        let doc_lines = lines
            .iter()
            .filter(|line| self.is_doc_comment(line.trim(), language))
            .count();

        let coverage = doc_lines as f32 / total_lines as f32;
        (coverage * self.weights.documentation).min(self.weights.documentation)
    }

    fn is_doc_comment(&self, line: &str, language: Language) -> bool {
        match language {
            Language::Rust => line.starts_with("///") || line.starts_with("//!"),
            Language::Python => line.starts_with("\"\"\"") || line.starts_with("'''"),
            Language::JavaScript | Language::TypeScript => {
                line.starts_with("/**") || line.starts_with('*')
            }
            Language::Go => line.starts_with("//"),
            _ => line.starts_with("//") || line.starts_with("/*"),
        }
    }

    fn analyze_consistency(&self, source: &str) -> f32 {
        let lines: Vec<&str> = source.lines().collect();
        if lines.is_empty() {
            return self.weights.consistency;
        }

        let mut tab_count = 0;
        let mut space_count = 0;

        for line in &lines {
            if line.starts_with('\t') {
                tab_count += 1;
            } else if line.starts_with("    ") || line.starts_with("  ") {
                space_count += 1;
            }
        }

        let total_indented = tab_count + space_count;
        if total_indented == 0 {
            return self.weights.consistency;
        }

        let consistency = if tab_count > space_count {
            tab_count as f32 / total_indented as f32
        } else {
            space_count as f32 / total_indented as f32
        };

        consistency * self.weights.consistency
    }

    fn estimate_cyclomatic_complexity(&self, lines: &[&str]) -> u32 {
        let mut complexity = 1u32; // Base complexity

        for line in lines {
            let trimmed = line.trim();

            // Control flow statements
            if trimmed.starts_with("if ") || trimmed.contains(" if ") {
                complexity += 1;
            }
            if trimmed.starts_with("for ") || trimmed.contains(" for ") {
                complexity += 1;
            }
            if trimmed.starts_with("while ") || trimmed.contains(" while ") {
                complexity += 1;
            }
            if trimmed.starts_with("match ") || trimmed.contains(" match ") {
                complexity += 1;
            }
            if trimmed.starts_with("switch ") || trimmed.contains(" switch ") {
                complexity += 1;
            }
            if trimmed.starts_with("case ") {
                complexity += 1;
            }
            if trimmed.starts_with("select ") || trimmed.contains(" select ") {
                complexity += 1;
            }

            // Logical operators add to complexity
            complexity += trimmed.matches(" && ").count() as u32;
            complexity += trimmed.matches(" || ").count() as u32;
        }

        complexity
    }

    fn estimate_nesting_depth(&self, source: &str) -> i32 {
        let mut max_depth = 0;
        let mut current_depth = 0;

        for line in source.lines() {
            let trimmed = line.trim();
            current_depth += trimmed.matches('{').count() as i32;
            if current_depth > max_depth {
                max_depth = current_depth;
            }
            current_depth -= trimmed.matches('}').count() as i32;
            if current_depth < 0 {
                current_depth = 0;
            }
        }

        max_depth
    }

    fn estimate_duplication_ratio(&self, source: &str) -> f32 {
        let lines: Vec<&str> = source
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("/*"))
            .collect();

        if lines.len() < 3 {
            return 0.0;
        }

        let mut line_counts: HashMap<&str, usize> = HashMap::new();
        for line in &lines {
            if line.len() > 10 {
                *line_counts.entry(*line).or_insert(0) += 1;
            }
        }

        let duplicates: usize = line_counts
            .values()
            .filter(|&&count| count > 1)
            .map(|&count| count - 1)
            .sum();

        duplicates as f32 / lines.len() as f32
    }

    fn detect_critical_defects(&self, source: &str, language: Language) -> (i32, bool) {
        let test_ranges = if language == Language::Rust {
            cfg_test_module_ranges(source)
        } else {
            Vec::new()
        };

        let mut count = 0;
        let mut byte_offset = 0;

        for line in source.lines() {
            let line_len = line.len();
            let trimmed = line.trim();

            // Skip lines inside #[cfg(test)] modules (Rust)
            if test_ranges
                .iter()
                .any(|r| byte_offset >= r.start && byte_offset < r.end)
            {
                // +1 for the newline character
                byte_offset += line_len + 1;
                continue;
            }

            byte_offset += line_len + 1;

            // Skip comments and string literals
            if trimmed.starts_with("//") || trimmed.starts_with("/*") {
                continue;
            }
            if trimmed.contains("\"panic(") || trimmed.contains("'panic(") {
                continue;
            }
            if trimmed.contains("\".unwrap()") || trimmed.contains("'.unwrap()") {
                continue;
            }

            match language {
                Language::Rust => {
                    if trimmed.contains(".unwrap()") {
                        count += 1;
                    }
                    if trimmed.contains("panic!") {
                        count += 1;
                    }
                }
                Language::Go => {
                    if !source.contains("func Test") && trimmed.contains("panic(") {
                        count += 1;
                    }
                }
                _ => {}
            }
        }

        (count, count > 0)
    }

    /// Compute per-file hotspot scores using git history analysis.
    /// Returns a map of file path -> hotspot score (0.0-1.0, where 1.0 is worst).
    fn compute_hotspot_scores(&self, ctx: &AnalysisContext<'_>) -> HashMap<String, f32> {
        let mut scores = HashMap::new();

        // Run hotspot analysis (silently fails if no git repo)
        let hotspot_analyzer = hotspot::Analyzer::new();
        if let Ok(analysis) = hotspot_analyzer.analyze(ctx) {
            for hs in analysis.hotspots {
                // Hotspot score is 0-1 (normalized churn * complexity percentiles)
                scores.insert(hs.file, hs.score as f32);
            }
        }

        scores
    }

    /// Compute per-file temporal coupling scores using git history.
    /// Returns a map of file path -> coupling score (0.0-1.0, where 1.0 = many couplings).
    fn compute_temporal_scores(&self, ctx: &AnalysisContext<'_>) -> HashMap<String, f32> {
        let mut scores = HashMap::new();

        // Run temporal coupling analysis
        let temporal_analyzer = temporal::Analyzer::new();
        if let Ok(analysis) = temporal_analyzer.analyze(ctx) {
            // Count how many times each file appears in couplings
            let mut coupling_counts: HashMap<String, usize> = HashMap::new();
            for coupling in &analysis.couplings {
                *coupling_counts.entry(coupling.file_a.clone()).or_insert(0) += 1;
                *coupling_counts.entry(coupling.file_b.clone()).or_insert(0) += 1;
            }

            // Normalize to 0-1 scale
            if let Some(&max_count) = coupling_counts.values().max() {
                if max_count > 0 {
                    for (file, count) in coupling_counts {
                        // Higher count = more couplings = higher risk
                        scores.insert(file, count as f32 / max_count as f32);
                    }
                }
            }
        }

        scores
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "tdg"
    }

    fn description(&self) -> &'static str {
        "Calculate Technical Debt Gradient scores (0-100 per file)"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // Run hotspot analysis to get per-file hotspot scores (requires git history)
        let hotspot_scores = self.compute_hotspot_scores(ctx);

        // Run temporal coupling analysis to get per-file coupling counts
        let temporal_scores = self.compute_temporal_scores(ctx);

        let mut scores = Vec::new();

        for path in ctx.files.iter() {
            let content = match ctx.read_file(path) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            if let Some(max_size) = self.max_file_size {
                if content.len() as u64 > max_size {
                    continue;
                }
            }

            let file_path = path
                .strip_prefix(ctx.root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let language = Language::from_extension(path);
            if let Ok(mut score) = self.analyze_source(&content, language, &file_path) {
                // Apply hotspot score (higher hotspot = more risk = lower score)
                // Hotspot score ranges 0-1, where 1 is worst (critical hotspot)
                if let Some(&hs) = hotspot_scores.get(&file_path) {
                    // Invert: hotspot 0 = full points, hotspot 1 = 0 points
                    score.hotspot_score = self.weights.hotspot * (1.0 - hs);
                } else {
                    // No hotspot data means no penalty (file may be new or not in git)
                    score.hotspot_score = self.weights.hotspot;
                }

                // Apply temporal coupling score (more couplings = more risk = lower score)
                if let Some(&tc) = temporal_scores.get(&file_path) {
                    // tc is a 0-1 normalized score where 1 = many couplings
                    score.temporal_coupling_score = self.weights.temporal_coupling * (1.0 - tc);
                } else {
                    // No coupling data means full points
                    score.temporal_coupling_score = self.weights.temporal_coupling;
                }

                // Recalculate total with updated scores
                score.calculate_total();
                scores.push(score);
            }
        }

        Ok(aggregate_project_score(scores))
    }
}

/// Find byte ranges of `#[cfg(test)]` modules using tree-sitter.
///
/// Returns ranges covering each module's body so that line-based analysis
/// can skip test code without fragile brace-counting heuristics.
///
/// Uses a thread-local parser cache to avoid allocating a new parser per file.
fn cfg_test_module_ranges(source: &str) -> Vec<Range<usize>> {
    use std::cell::RefCell;

    thread_local! {
        static RUST_PARSER: RefCell<tree_sitter::Parser> = RefCell::new({
            let ts_lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
            let mut p = tree_sitter::Parser::new();
            p.set_language(&ts_lang).expect("built-in Rust grammar");
            p
        });
    }

    let tree = RUST_PARSER.with(|parser| parser.borrow_mut().parse(source.as_bytes(), None));
    let tree = match tree {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut ranges = Vec::new();
    let mut cursor = tree.root_node().walk();

    // Walk top-level children looking for mod_item nodes preceded by #[cfg(test)]
    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            if node.kind() == "mod_item" {
                if let Some(prev) = node.prev_sibling() {
                    if prev.kind() == "attribute_item" {
                        if let Ok(text) = prev.utf8_text(source.as_bytes()) {
                            if text.contains("cfg") && text.contains("test") {
                                ranges.push(node.start_byte()..node.end_byte());
                            }
                        }
                    }
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    ranges
}

// Types

/// Programming language detected from file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Unknown,
    Rust,
    Go,
    Python,
    JavaScript,
    TypeScript,
    Java,
    C,
    Cpp,
    CSharp,
    Ruby,
    PHP,
    Swift,
    Kotlin,
}

impl Language {
    pub fn from_extension(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Self::Rust,
            Some("go") => Self::Go,
            Some("py") => Self::Python,
            Some("js") => Self::JavaScript,
            Some("ts") | Some("tsx") | Some("jsx") => Self::TypeScript,
            Some("java") => Self::Java,
            Some("c") | Some("h") => Self::C,
            Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") => Self::Cpp,
            Some("cs") => Self::CSharp,
            Some("rb") => Self::Ruby,
            Some("php") => Self::PHP,
            Some("swift") => Self::Swift,
            Some("kt") | Some("kts") => Self::Kotlin,
            _ => Self::Unknown,
        }
    }

    pub fn confidence(&self) -> f32 {
        if *self == Self::Unknown {
            0.5
        } else {
            0.95
        }
    }
}

/// Letter grade from A+ to F.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    #[serde(rename = "A+")]
    APlus,
    A,
    #[serde(rename = "A-")]
    AMinus,
    #[serde(rename = "B+")]
    BPlus,
    B,
    #[serde(rename = "B-")]
    BMinus,
    #[serde(rename = "C+")]
    CPlus,
    C,
    #[serde(rename = "C-")]
    CMinus,
    D,
    F,
}

impl Grade {
    pub fn from_score(score: f32) -> Self {
        match score {
            s if s >= 97.0 => Self::APlus,
            s if s >= 93.0 => Self::A,
            s if s >= 90.0 => Self::AMinus,
            s if s >= 87.0 => Self::BPlus,
            s if s >= 83.0 => Self::B,
            s if s >= 80.0 => Self::BMinus,
            s if s >= 77.0 => Self::CPlus,
            s if s >= 73.0 => Self::C,
            s if s >= 70.0 => Self::CMinus,
            s if s >= 60.0 => Self::D,
            _ => Self::F,
        }
    }

    pub fn to_char(&self) -> char {
        match self {
            Self::APlus | Self::A | Self::AMinus => 'A',
            Self::BPlus | Self::B | Self::BMinus => 'B',
            Self::CPlus | Self::C | Self::CMinus => 'C',
            Self::D => 'D',
            Self::F => 'F',
        }
    }
}

/// Penalty attribution tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PenaltyAttribution {
    pub source_metric: String,
    pub amount: f32,
    pub issue: String,
}

/// TDG score for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Score {
    // Component scores
    pub structural_complexity: f32,
    pub semantic_complexity: f32,
    pub duplication_ratio: f32,
    pub coupling_score: f32,
    pub doc_coverage: f32,
    pub consistency_score: f32,
    pub hotspot_score: f32,
    pub temporal_coupling_score: f32,
    pub entropy_score: f32,

    // Aggregated score and grade
    pub total: f32,
    pub grade: Grade,

    // Metadata
    pub confidence: f32,
    pub language: Language,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file_path: String,
    pub critical_defects_count: i32,
    pub has_critical_defects: bool,

    // Penalty tracking
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub penalties_applied: Vec<PenaltyAttribution>,
}

impl Default for Score {
    fn default() -> Self {
        Self::new()
    }
}

impl Score {
    pub fn new() -> Self {
        Self {
            structural_complexity: 20.0,
            semantic_complexity: 15.0,
            duplication_ratio: 15.0,
            coupling_score: 15.0,
            doc_coverage: 5.0,
            consistency_score: 10.0,
            // Hotspot and temporal scores are set during analyze() from git history
            // Default to max (no penalty) for files without git data
            hotspot_score: 10.0,
            temporal_coupling_score: 10.0,
            entropy_score: 0.0,
            total: 100.0,
            grade: Grade::APlus,
            confidence: 1.0,
            language: Language::Unknown,
            file_path: String::new(),
            critical_defects_count: 0,
            has_critical_defects: false,
            penalties_applied: Vec::new(),
        }
    }

    pub fn calculate_total(&mut self) {
        // Clamp individual components
        self.structural_complexity = self.structural_complexity.clamp(0.0, 20.0);
        self.semantic_complexity = self.semantic_complexity.clamp(0.0, 15.0);
        self.duplication_ratio = self.duplication_ratio.clamp(0.0, 15.0);
        self.coupling_score = self.coupling_score.clamp(0.0, 15.0);
        self.doc_coverage = self.doc_coverage.clamp(0.0, 5.0);
        self.consistency_score = self.consistency_score.clamp(0.0, 10.0);
        self.hotspot_score = self.hotspot_score.clamp(0.0, 10.0);
        self.temporal_coupling_score = self.temporal_coupling_score.clamp(0.0, 10.0);
        self.entropy_score = self.entropy_score.clamp(0.0, 10.0);

        // Sum all components
        let raw_total = self.structural_complexity
            + self.semantic_complexity
            + self.duplication_ratio
            + self.coupling_score
            + self.doc_coverage
            + self.consistency_score
            + self.hotspot_score
            + self.temporal_coupling_score
            + self.entropy_score;

        // Normalize to 0-100 scale
        if raw_total <= 100.0 {
            self.total = raw_total.clamp(0.0, 100.0);
        } else {
            // Scale down when entropy pushes total above 100
            const THEORETICAL_MAX: f32 = 110.0;
            self.total = (raw_total / THEORETICAL_MAX * 100.0).clamp(0.0, 100.0);
        }

        // Auto-fail if critical defects detected
        if self.has_critical_defects {
            self.total = 0.0;
            self.grade = Grade::F;
        } else {
            self.grade = Grade::from_score(self.total);
        }
    }
}

/// Project-level TDG analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub files: Vec<Score>,
    pub average_score: f32,
    pub average_grade: Grade,
    pub total_files: usize,
    pub language_distribution: HashMap<Language, usize>,
    pub grade_distribution: HashMap<String, usize>,
}

fn aggregate_project_score(scores: Vec<Score>) -> Analysis {
    let total_files = scores.len();

    let average_score = if total_files > 0 {
        let sum: f32 = scores.iter().map(|s| s.total).sum();
        sum / total_files as f32
    } else {
        0.0
    };

    let mut lang_dist: HashMap<Language, usize> = HashMap::new();
    let mut grade_dist: HashMap<String, usize> = HashMap::new();

    for score in &scores {
        *lang_dist.entry(score.language).or_insert(0) += 1;
        let grade_key = format!("{:?}", score.grade);
        *grade_dist.entry(grade_key).or_insert(0) += 1;
    }

    Analysis {
        files: scores,
        average_score,
        average_grade: Grade::from_score(average_score),
        total_files,
        language_distribution: lang_dist,
        grade_distribution: grade_dist,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_weights() {
        let weights = Weights::default();
        let total = weights.structural_complexity
            + weights.semantic_complexity
            + weights.duplication
            + weights.coupling
            + weights.documentation
            + weights.consistency
            + weights.hotspot
            + weights.temporal_coupling;
        // All weights sum to 100: 20 + 15 + 15 + 15 + 5 + 10 + 10 + 10
        assert_eq!(total, 100.0);
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new().with_max_file_size(1024 * 1024);
        assert_eq!(analyzer.max_file_size, Some(1024 * 1024));
    }

    #[test]
    fn test_language_detection() {
        assert_eq!(
            Language::from_extension(Path::new("foo.rs")),
            Language::Rust
        );
        assert_eq!(Language::from_extension(Path::new("foo.go")), Language::Go);
        assert_eq!(
            Language::from_extension(Path::new("foo.py")),
            Language::Python
        );
        assert_eq!(
            Language::from_extension(Path::new("foo.js")),
            Language::JavaScript
        );
        assert_eq!(
            Language::from_extension(Path::new("foo.ts")),
            Language::TypeScript
        );
        assert_eq!(
            Language::from_extension(Path::new("foo.txt")),
            Language::Unknown
        );
    }

    #[test]
    fn test_language_confidence() {
        assert_eq!(Language::Rust.confidence(), 0.95);
        assert_eq!(Language::Unknown.confidence(), 0.5);
    }

    #[test]
    fn test_grade_from_score() {
        assert_eq!(Grade::from_score(100.0), Grade::APlus);
        assert_eq!(Grade::from_score(95.0), Grade::A);
        assert_eq!(Grade::from_score(85.0), Grade::B);
        assert_eq!(Grade::from_score(75.0), Grade::C);
        assert_eq!(Grade::from_score(65.0), Grade::D);
        assert_eq!(Grade::from_score(50.0), Grade::F);
    }

    #[test]
    fn test_grade_to_char() {
        assert_eq!(Grade::APlus.to_char(), 'A');
        assert_eq!(Grade::B.to_char(), 'B');
        assert_eq!(Grade::CMinus.to_char(), 'C');
        assert_eq!(Grade::D.to_char(), 'D');
        assert_eq!(Grade::F.to_char(), 'F');
    }

    #[test]
    fn test_score_calculation() {
        let mut score = Score::new();
        score.calculate_total();
        // Score is 100.0 with all components at their default max values
        // (20 + 15 + 15 + 15 + 5 + 10 + 10 + 10 + 0 = 100)
        assert_eq!(score.total, 100.0);
        assert_eq!(score.grade, Grade::APlus);
    }

    #[test]
    fn test_score_with_penalties() {
        let mut score = Score::new();
        score.structural_complexity = 10.0; // -10 penalty
        score.semantic_complexity = 10.0; // -5 penalty
        score.calculate_total();
        assert!(score.total < 100.0);
    }

    #[test]
    fn test_analyze_simple_source() {
        let analyzer = Analyzer::new();
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let result = analyzer
            .analyze_source(source, Language::Rust, "test.rs")
            .unwrap();
        assert!(result.total > 0.0);
        assert!(result.total <= 100.0);
    }

    #[test]
    fn test_analyze_complex_source() {
        let analyzer = Analyzer::new();
        let source = r#"
fn complex() {
    if true {
        if true {
            if true {
                if true {
                    if true {
                        println!("deep");
                    }
                }
            }
        }
    }
}
"#;
        let result = analyzer
            .analyze_source(source, Language::Rust, "test.rs")
            .unwrap();
        // Deep nesting should reduce semantic complexity
        assert!(result.semantic_complexity < 15.0);
    }

    #[test]
    fn test_cyclomatic_estimation() {
        let analyzer = Analyzer::new();
        let lines: Vec<&str> = vec![
            "if condition {",
            "} else if other {",
            "    for item in list {",
            "        if item && other {",
            "        }",
            "    }",
            "}",
        ];
        let complexity = analyzer.estimate_cyclomatic_complexity(&lines);
        assert!(complexity > 1);
    }

    #[test]
    fn test_nesting_depth() {
        let analyzer = Analyzer::new();
        let source = "{ { { } } }";
        let depth = analyzer.estimate_nesting_depth(source);
        assert_eq!(depth, 3);
    }

    #[test]
    fn test_duplication_detection() {
        let analyzer = Analyzer::new();
        // Lines must be exactly identical to count as duplicates
        let source = r#"
calculate_something_long();
calculate_something_long();
calculate_something_long();
calculate_something_long();
"#;
        let ratio = analyzer.estimate_duplication_ratio(source);
        // 4 identical lines, 3 are duplicates: 3/4 = 0.75
        assert!(ratio > 0.0, "ratio should be > 0.0, got {}", ratio);
    }

    #[test]
    fn test_coupling_analysis() {
        let analyzer = Analyzer::new();
        let source = r#"
use std::io;
use std::fs;
import React from 'react';
"#;
        let score = analyzer.analyze_coupling(source);
        assert!(score > 0.0);
    }

    #[test]
    fn test_doc_comment_detection() {
        let analyzer = Analyzer::new();
        assert!(analyzer.is_doc_comment("/// This is a doc comment", Language::Rust));
        assert!(analyzer.is_doc_comment("//! Module docs", Language::Rust));
        assert!(!analyzer.is_doc_comment("// Regular comment", Language::Rust));
    }

    #[test]
    fn test_consistency_analysis() {
        let analyzer = Analyzer::new();
        let source = "    line1\n    line2\n    line3\n";
        let score = analyzer.analyze_consistency(source);
        assert!(score > 0.0);
    }

    #[test]
    fn test_aggregate_project_score() {
        let scores = vec![
            Score {
                total: 90.0,
                grade: Grade::AMinus,
                language: Language::Rust,
                ..Score::new()
            },
            Score {
                total: 80.0,
                grade: Grade::BMinus,
                language: Language::Rust,
                ..Score::new()
            },
        ];
        let analysis = aggregate_project_score(scores);
        assert_eq!(analysis.total_files, 2);
        assert_eq!(analysis.average_score, 85.0);
    }

    #[test]
    fn test_critical_defects_detection() {
        let analyzer = Analyzer::new();
        // Rust code with .unwrap() should be flagged
        let (count, has_critical) =
            analyzer.detect_critical_defects("let x = foo.unwrap();", Language::Rust);
        assert_eq!(count, 1);
        assert!(
            has_critical,
            "has_critical should be true when defects found"
        );

        // Clean code should not be flagged
        let (count, has_critical) =
            analyzer.detect_critical_defects("let x = foo.unwrap_or_default();", Language::Rust);
        assert_eq!(count, 0);
        assert!(
            !has_critical,
            "has_critical should be false when no defects"
        );
    }

    #[test]
    fn test_critical_defects_skips_test_modules() {
        let analyzer = Analyzer::new();
        let source = r#"
fn production() {
    let x = safe_call();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        let x = some_option.unwrap();
        assert!(x > 0);
    }
}
"#;
        let (count, has_critical) = analyzer.detect_critical_defects(source, Language::Rust);
        assert_eq!(count, 0, "unwrap() in #[cfg(test)] should not count");
        assert!(!has_critical);
    }

    #[test]
    fn test_critical_defects_counts_production_unwrap_with_test_module() {
        let analyzer = Analyzer::new();
        let source = r#"
fn production() {
    let x = some_option.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        let x = other.unwrap();
    }
}
"#;
        let (count, has_critical) = analyzer.detect_critical_defects(source, Language::Rust);
        assert_eq!(count, 1, "only production unwrap() should count");
        assert!(has_critical);
    }

    #[test]
    fn test_critical_defects_skips_test_with_unbalanced_braces_in_strings() {
        let analyzer = Analyzer::new();
        // Extra closing braces in string literals fool brace-counting into
        // thinking the test module ended early, causing a false positive
        // on the unwrap() that is still inside test code.
        let source = r#"
fn production() {
    let x = safe_call();
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSE: &str = "}}}}";

    #[test]
    fn test_brace_edge() {
        let result = parse("}}").unwrap();
    }
}
"#;
        let (count, has_critical) = analyzer.detect_critical_defects(source, Language::Rust);
        assert_eq!(
            count, 0,
            "unwrap() inside #[cfg(test)] should not count even with unbalanced braces in strings"
        );
        assert!(!has_critical);
    }

    #[test]
    fn test_critical_defects_auto_fail() {
        let analyzer = Analyzer::new();
        // Source with .unwrap() should trigger critical defect
        let source = r#"
fn risky() {
    let x = some_option.unwrap();
}
"#;
        let result = analyzer
            .analyze_source(source, Language::Rust, "test.rs")
            .unwrap();
        assert!(result.critical_defects_count > 0);
        assert!(result.has_critical_defects);
        assert_eq!(result.total, 0.0);
        assert_eq!(result.grade, Grade::F);
    }
}
