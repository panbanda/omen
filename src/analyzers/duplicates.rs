//! Code clone/duplicate detection using MinHash with LSH.
//!
//! Uses Locality-Sensitive Hashing for O(n) average-case candidate filtering,
//! then verifies with actual Jaccard similarity calculation.
//!
//! # References
//!
//! - Broder, A.Z. (1997) "On the Resemblance and Containment of Documents"
//!   SEQUENCES '97 (MinHash algorithm)
//! - Indyk, P., Motwani, R. (1998) "Approximate Nearest Neighbors: Towards
//!   Removing the Curse of Dimensionality" (LSH theory)
//!
//! # Configuration
//!
//! Default: 200 hashes, 20 bands x 10 rows, 0.70 similarity threshold.
//! These parameters provide good precision/recall balance for code clones.

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{AnalysisContext, Analyzer as AnalyzerTrait, Result};

/// Clone type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneType {
    /// Exact clones (whitespace only differs)
    Type1,
    /// Parametric clones (identifiers/literals differ)
    Type2,
    /// Structural clones (statements added/removed)
    Type3,
}

impl CloneType {
    fn from_similarity(similarity: f64) -> Self {
        if similarity >= 0.95 {
            CloneType::Type1
        } else if similarity >= 0.85 {
            CloneType::Type2
        } else {
            CloneType::Type3
        }
    }
}

/// Configuration for duplicate detection.
#[derive(Debug, Clone)]
pub struct Config {
    pub min_tokens: usize,
    pub similarity_threshold: f64,
    pub shingle_size: usize,
    pub num_hash_functions: usize,
    pub num_bands: usize,
    pub rows_per_band: usize,
    pub normalize_identifiers: bool,
    pub normalize_literals: bool,
    pub ignore_comments: bool,
    pub min_group_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_tokens: 50,
            similarity_threshold: 0.70,
            shingle_size: 5,
            num_hash_functions: 200,
            num_bands: 20,
            rows_per_band: 10,
            normalize_identifiers: true,
            normalize_literals: true,
            ignore_comments: true,
            min_group_size: 2,
        }
    }
}

/// Duplicates analyzer using MinHash with LSH.
pub struct Analyzer {
    config: Config,
    max_file_size: usize,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            max_file_size: 0, // No limit
        }
    }

    pub fn with_config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn with_min_tokens(mut self, min_tokens: usize) -> Self {
        self.config.min_tokens = min_tokens;
        self
    }

    pub fn with_similarity_threshold(mut self, threshold: f64) -> Self {
        self.config.similarity_threshold = threshold;
        self
    }

    pub fn with_max_file_size(mut self, max_size: usize) -> Self {
        self.max_file_size = max_size;
        self
    }

    /// Extract code fragments from file content.
    fn extract_fragments(&self, path: &str, content: &[u8]) -> Vec<CodeFragment> {
        let content_str = match std::str::from_utf8(content) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let lines: Vec<&str> = content_str.lines().collect();
        let mut fragments = Vec::new();

        // Try function-level extraction first
        let func_fragments = self.extract_function_fragments(path, &lines);
        if !func_fragments.is_empty() {
            fragments.extend(func_fragments);
        }

        // Fall back to whole file as single fragment if no functions found
        if fragments.is_empty() {
            if let Some(frag) = self.create_fragment(path, 0, lines.len().saturating_sub(1), &lines)
            {
                fragments.push(frag);
            }
        }

        fragments
    }

    /// Extract function-level code fragments.
    fn extract_function_fragments(&self, path: &str, lines: &[&str]) -> Vec<CodeFragment> {
        let mut fragments = Vec::new();
        let lang = detect_language(path);

        let mut in_function = false;
        let mut func_start_line = 0;
        let mut func_lines: Vec<&str> = Vec::new();
        let mut brace_depth = 0;
        let mut end_depth = 0; // For Ruby's end keyword

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            if !in_function {
                if is_function_start(trimmed, lang) {
                    in_function = true;
                    func_start_line = i;
                    func_lines = vec![line];
                    brace_depth =
                        line.matches('{').count() as i32 - line.matches('}').count() as i32;
                    if lang == "python" {
                        brace_depth = 1; // Python uses indentation
                    } else if lang == "ruby" {
                        end_depth = 1; // Ruby uses end keyword
                    }
                }
            } else {
                func_lines.push(line);

                if lang == "python" {
                    // Python function ends at dedent or new def/class
                    let is_dedent =
                        !trimmed.is_empty() && !line.starts_with(' ') && !line.starts_with('\t');
                    let is_new_block = trimmed.starts_with("def ")
                        || trimmed.starts_with("class ")
                        || i == lines.len() - 1;

                    if is_dedent && is_new_block {
                        // End of function
                        let end = if func_lines.len() > 1 { i - 1 } else { i };
                        if let Some(frag) = self.create_fragment(
                            path,
                            func_start_line,
                            end,
                            &func_lines[..func_lines.len().saturating_sub(1)],
                        ) {
                            fragments.push(frag);
                        }
                        // Check if this line starts a new function
                        if is_function_start(trimmed, lang) {
                            func_start_line = i;
                            func_lines = vec![line];
                        } else {
                            in_function = false;
                        }
                    }
                } else if lang == "ruby" {
                    // Ruby uses 'end' keyword to close blocks
                    // Track nested blocks (def, class, module, if, unless, case, while, until, for, begin, do)
                    let block_starters = [
                        "def ", "class ", "module ", "if ", "unless ", "case ", "while ", "until ",
                        "for ", "begin", "do",
                    ];
                    for starter in &block_starters {
                        if trimmed.starts_with(starter)
                            || trimmed.contains(&format!(" {} ", starter.trim()))
                        {
                            end_depth += 1;
                            break;
                        }
                    }
                    // Also check for inline block starters like "x.each do"
                    if trimmed.ends_with(" do") || trimmed.ends_with(" do |") {
                        end_depth += 1;
                    }
                    // Check for 'end' keyword
                    if trimmed == "end" || trimmed.starts_with("end ") || trimmed.ends_with(" end")
                    {
                        end_depth -= 1;
                        if end_depth <= 0 {
                            // End of function
                            if let Some(frag) =
                                self.create_fragment(path, func_start_line, i, &func_lines)
                            {
                                fragments.push(frag);
                            }
                            in_function = false;
                            func_lines.clear();
                            end_depth = 0;
                        }
                    }
                } else {
                    brace_depth +=
                        line.matches('{').count() as i32 - line.matches('}').count() as i32;
                    if brace_depth <= 0 {
                        // End of function
                        if let Some(frag) =
                            self.create_fragment(path, func_start_line, i, &func_lines)
                        {
                            fragments.push(frag);
                        }
                        in_function = false;
                        func_lines.clear();
                    }
                }
            }
        }

        // Handle unclosed function at end of file
        if in_function && !func_lines.is_empty() {
            if let Some(frag) = self.create_fragment(
                path,
                func_start_line,
                lines.len().saturating_sub(1),
                &func_lines,
            ) {
                fragments.push(frag);
            }
        }

        fragments
    }

    /// Create a code fragment from lines if it meets minimum token requirements.
    fn create_fragment(
        &self,
        path: &str,
        start_line: usize,
        end_line: usize,
        lines: &[&str],
    ) -> Option<CodeFragment> {
        let content = lines.join("\n");

        // Normalize and tokenize
        let normalized = self.normalize_code(&content);
        let tokens = tokenize(&normalized);

        // Normalize tokens with a FRESH identifier map for each fragment.
        // This ensures structurally identical code in different files produces
        // identical token sequences, enabling proper similarity detection.
        let normalized_tokens = normalize_tokens_fresh(&tokens, &self.config);

        // Check minimum token count
        if normalized_tokens.len() < self.config.min_tokens {
            return None;
        }

        Some(CodeFragment {
            id: 0, // Set later
            file: path.to_string(),
            start_line: (start_line + 1) as u32,
            end_line: (end_line + 1) as u32,
            content: normalized_tokens.join(" "),
            tokens: normalized_tokens,
            normalized_hash: 0, // Set later
            signature: None,    // Set later
        })
    }

    /// Normalize code for comparison.
    fn normalize_code(&self, code: &str) -> String {
        let mut result = String::new();

        for line in code.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.config.ignore_comments && is_comment(trimmed) {
                continue;
            }
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(trimmed);
        }

        result
    }

    /// Compute MinHash signature for a token sequence.
    fn compute_minhash(&self, tokens: &[String]) -> MinHashSignature {
        let shingles = generate_k_shingles(tokens, self.config.shingle_size);

        let mut values = vec![u64::MAX; self.config.num_hash_functions];

        for shingle_hash in &shingles {
            for (i, value) in values.iter_mut().enumerate() {
                let h = hash_u64_with_seed(*shingle_hash, i as u64);
                if h < *value {
                    *value = h;
                }
            }
        }

        MinHashSignature { values }
    }

    /// Compute normalized hash for a token sequence.
    fn compute_normalized_hash(&self, tokens: &[String]) -> u64 {
        let content = tokens.join(" ");
        xxhash_rust::xxh3::xxh3_64(content.as_bytes())
    }

    /// Find clone pairs using LSH for O(n) average-case candidate filtering.
    fn find_clone_pairs_lsh(&self, fragments: &[CodeFragment]) -> Vec<ClonePair> {
        let bands = self.config.num_bands;
        let rows_per_band = self.config.rows_per_band;

        // Create LSH buckets for each band
        let mut lsh_buckets: Vec<HashMap<u64, Vec<usize>>> =
            (0..bands).map(|_| HashMap::new()).collect();

        // Hash each fragment into buckets
        for (idx, fragment) in fragments.iter().enumerate() {
            let Some(ref sig) = fragment.signature else {
                continue;
            };
            if sig.values.is_empty() {
                continue;
            }

            for (band, bucket) in lsh_buckets.iter_mut().enumerate().take(bands) {
                let start = band * rows_per_band;
                let end = (start + rows_per_band).min(sig.values.len());
                if start >= end {
                    continue;
                }

                let band_hash = hash_band(&sig.values[start..end], band as u64);
                bucket.entry(band_hash).or_default().push(idx);
            }
        }

        // Find candidate pairs from buckets
        let mut candidate_pairs: HashSet<(usize, usize)> = HashSet::new();
        for band_buckets in &lsh_buckets {
            for bucket in band_buckets.values() {
                if bucket.len() < 2 {
                    continue;
                }
                for i in 0..bucket.len() {
                    for j in (i + 1)..bucket.len() {
                        let (a, b) = if bucket[i] < bucket[j] {
                            (bucket[i], bucket[j])
                        } else {
                            (bucket[j], bucket[i])
                        };
                        candidate_pairs.insert((a, b));
                    }
                }
            }
        }

        // Verify candidate pairs with actual Jaccard similarity
        let mut pairs = Vec::new();
        for (idx_a, idx_b) in candidate_pairs {
            let frag_a = &fragments[idx_a];
            let frag_b = &fragments[idx_b];

            // Skip if same file and overlapping
            if frag_a.file == frag_b.file
                && frag_a.start_line <= frag_b.end_line
                && frag_b.start_line <= frag_a.end_line
            {
                continue;
            }

            // Calculate actual similarity
            if let (Some(sig_a), Some(sig_b)) = (&frag_a.signature, &frag_b.signature) {
                let similarity = sig_a.jaccard_similarity(sig_b);
                if similarity >= self.config.similarity_threshold {
                    pairs.push(ClonePair {
                        idx_a,
                        idx_b,
                        similarity,
                    });
                }
            }
        }

        pairs
    }

    /// Group clone pairs using Union-Find algorithm.
    fn group_clones(&self, fragments: &[CodeFragment], pairs: &[ClonePair]) -> Vec<CloneGroup> {
        if pairs.is_empty() {
            return Vec::new();
        }

        // Initialize Union-Find
        let mut parent: Vec<usize> = (0..fragments.len()).collect();

        fn find(parent: &mut [usize], x: usize) -> usize {
            if parent[x] != x {
                parent[x] = find(parent, parent[x]);
            }
            parent[x]
        }

        fn union(parent: &mut [usize], x: usize, y: usize) {
            let px = find(parent, x);
            let py = find(parent, y);
            if px != py {
                parent[px] = py;
            }
        }

        // Union all clone pairs
        for pair in pairs {
            union(&mut parent, pair.idx_a, pair.idx_b);
        }

        // Group fragments by their root
        let mut group_map: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..fragments.len() {
            let root = find(&mut parent, i);
            group_map.entry(root).or_default().push(i);
        }

        // Build similarity map
        let mut similarity_map: HashMap<(usize, usize), f64> = HashMap::new();
        for pair in pairs {
            let key = if pair.idx_a < pair.idx_b {
                (pair.idx_a, pair.idx_b)
            } else {
                (pair.idx_b, pair.idx_a)
            };
            similarity_map.insert(key, pair.similarity);
        }

        // Convert to CloneGroup
        let mut groups = Vec::new();
        let mut group_id = 0u64;

        for member_indices in group_map.values() {
            if member_indices.len() < self.config.min_group_size {
                continue;
            }

            group_id += 1;
            let mut instances = Vec::new();
            let mut total_lines = 0;
            let mut total_tokens = 0;
            let mut similarity_sum = 0.0;
            let mut similarity_count = 0;

            for &idx in member_indices {
                let frag = &fragments[idx];
                let lines = (frag.end_line - frag.start_line + 1) as usize;
                instances.push(CloneInstance {
                    file: frag.file.clone(),
                    start_line: frag.start_line,
                    end_line: frag.end_line,
                    lines,
                    normalized_hash: frag.normalized_hash,
                    similarity: 1.0,
                });
                total_lines += lines;
                total_tokens += frag.tokens.len();
            }

            // Calculate average similarity
            for i in 0..member_indices.len() {
                for j in (i + 1)..member_indices.len() {
                    let key = if member_indices[i] < member_indices[j] {
                        (member_indices[i], member_indices[j])
                    } else {
                        (member_indices[j], member_indices[i])
                    };
                    if let Some(&sim) = similarity_map.get(&key) {
                        similarity_sum += sim;
                        similarity_count += 1;
                    }
                }
            }

            let avg_similarity = if similarity_count > 0 {
                similarity_sum / similarity_count as f64
            } else {
                1.0
            };

            groups.push(CloneGroup {
                id: group_id,
                clone_type: CloneType::from_similarity(avg_similarity),
                instances,
                total_lines,
                total_tokens,
                average_similarity: avg_similarity,
            });
        }

        groups
    }

    /// Compute duplication hotspots.
    fn compute_hotspots(&self, groups: &[CloneGroup]) -> Vec<Hotspot> {
        let mut file_stats: HashMap<String, (usize, HashSet<u64>)> = HashMap::new();

        for group in groups {
            for inst in &group.instances {
                let entry = file_stats.entry(inst.file.clone()).or_default();
                entry.0 += inst.lines;
                entry.1.insert(group.id);
            }
        }

        let mut hotspots: Vec<Hotspot> = file_stats
            .into_iter()
            .map(|(file, (lines, groups_set))| {
                let severity = (lines as f64 + 1.0).ln() * (groups_set.len() as f64).sqrt();
                Hotspot {
                    file,
                    duplicate_lines: lines,
                    clone_group_count: groups_set.len(),
                    severity,
                }
            })
            .collect();

        hotspots.sort_by(|a, b| {
            b.severity
                .partial_cmp(&a.severity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if hotspots.len() > 10 {
            hotspots.truncate(10);
        }

        hotspots
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "duplicates"
    }

    fn description(&self) -> &'static str {
        "Find duplicated code (Type 1, 2, 3 clones) using MinHash with LSH"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        // Extract fragments from all files
        let mut all_fragments: Vec<CodeFragment> = Vec::new();
        let mut total_lines = 0usize;
        let mut files_scanned = 0usize;

        for path in ctx.files.iter() {
            let content = match ctx.read_file(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if self.max_file_size > 0 && content.len() > self.max_file_size {
                continue;
            }

            files_scanned += 1;
            let path_str = path.to_string_lossy();
            let fragments = self.extract_fragments(&path_str, &content);
            for mut frag in fragments {
                total_lines += (frag.end_line - frag.start_line + 1) as usize;
                frag.id = all_fragments.len() as u64;
                all_fragments.push(frag);
            }
        }

        // Compute MinHash signatures in parallel
        all_fragments.par_iter_mut().for_each(|frag| {
            frag.signature = Some(self.compute_minhash(&frag.tokens));
            frag.normalized_hash = self.compute_normalized_hash(&frag.tokens);
        });

        // Find clone pairs using LSH
        let clone_pairs = self.find_clone_pairs_lsh(&all_fragments);

        // Group clones using Union-Find
        let groups = self.group_clones(&all_fragments, &clone_pairs);

        // Build summary
        let mut summary = AnalysisSummary {
            total_groups: groups.len(),
            ..Default::default()
        };

        // Calculate duplicated_lines from unique instances in groups
        // Each instance represents duplicated code - count each once
        use std::collections::HashSet;
        let mut seen_ranges: HashSet<(String, u32, u32)> = HashSet::new();
        for group in &groups {
            for inst in &group.instances {
                let key = (inst.file.clone(), inst.start_line, inst.end_line);
                if seen_ranges.insert(key) {
                    summary.duplicated_lines += inst.lines;
                }
            }
        }

        // Convert groups to pairwise clones for backward compatibility
        let mut clones = Vec::new();
        for group in &groups {
            for i in 0..group.instances.len() {
                for j in (i + 1)..group.instances.len() {
                    let inst_a = &group.instances[i];
                    let inst_b = &group.instances[j];

                    let clone = Clone {
                        clone_type: group.clone_type,
                        similarity: group.average_similarity,
                        file_a: inst_a.file.clone(),
                        file_b: inst_b.file.clone(),
                        start_line_a: inst_a.start_line,
                        end_line_a: inst_a.end_line,
                        start_line_b: inst_b.start_line,
                        end_line_b: inst_b.end_line,
                        lines_a: inst_a.lines,
                        lines_b: inst_b.lines,
                        group_id: group.id,
                    };

                    summary.add_clone_stats(&clone);
                    clones.push(clone);
                }
            }
        }

        // Calculate statistics
        if !clones.is_empty() {
            let mut similarities: Vec<f64> = clones.iter().map(|c| c.similarity).collect();
            summary.avg_similarity = similarities.iter().sum::<f64>() / similarities.len() as f64;

            similarities.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            summary.p50_similarity = percentile(&similarities, 50.0);
            summary.p95_similarity = percentile(&similarities, 95.0);
        }

        // Calculate duplication ratio
        summary.total_lines = total_lines;
        if total_lines > 0 {
            let ratio = summary.duplicated_lines as f64 / total_lines as f64;
            summary.duplication_ratio = ratio.min(1.0);
        }

        // Compute hotspots
        summary.hotspots = self.compute_hotspots(&groups);

        Ok(Analysis {
            clones,
            groups,
            summary,
            total_files_scanned: files_scanned,
            min_lines: self.config.min_tokens / 8,
            threshold: self.config.similarity_threshold,
        })
    }
}

/// Internal code fragment representation.
struct CodeFragment {
    id: u64,
    file: String,
    start_line: u32,
    end_line: u32,
    #[allow(dead_code)]
    content: String,
    tokens: Vec<String>,
    normalized_hash: u64,
    signature: Option<MinHashSignature>,
}

/// Internal clone pair representation.
struct ClonePair {
    idx_a: usize,
    idx_b: usize,
    similarity: f64,
}

/// MinHash signature for similarity estimation.
#[derive(Clone)]
struct MinHashSignature {
    values: Vec<u64>,
}

impl MinHashSignature {
    fn jaccard_similarity(&self, other: &MinHashSignature) -> f64 {
        if self.values.len() != other.values.len() || self.values.is_empty() {
            return 0.0;
        }

        let matches = self
            .values
            .iter()
            .zip(other.values.iter())
            .filter(|(a, b)| a == b)
            .count();

        matches as f64 / self.values.len() as f64
    }
}

// Output types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub clones: Vec<Clone>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<CloneGroup>,
    pub summary: AnalysisSummary,
    pub total_files_scanned: usize,
    pub min_lines: usize,
    pub threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clone {
    pub clone_type: CloneType,
    pub similarity: f64,
    pub file_a: String,
    pub file_b: String,
    pub start_line_a: u32,
    pub end_line_a: u32,
    pub start_line_b: u32,
    pub end_line_b: u32,
    pub lines_a: usize,
    pub lines_b: usize,
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub group_id: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneGroup {
    pub id: u64,
    pub clone_type: CloneType,
    pub instances: Vec<CloneInstance>,
    pub total_lines: usize,
    pub total_tokens: usize,
    pub average_similarity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneInstance {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub lines: usize,
    pub normalized_hash: u64,
    pub similarity: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_clones: usize,
    pub total_groups: usize,
    pub type1_count: usize,
    pub type2_count: usize,
    pub type3_count: usize,
    pub duplicated_lines: usize,
    pub total_lines: usize,
    pub duplication_ratio: f64,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub file_occurrences: HashMap<String, usize>,
    pub avg_similarity: f64,
    pub p50_similarity: f64,
    pub p95_similarity: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<Hotspot>,
}

impl AnalysisSummary {
    /// Add clone statistics (file occurrences and type counts).
    /// Note: duplicated_lines is calculated separately from unique group instances.
    fn add_clone_stats(&mut self, clone: &Clone) {
        self.total_clones += 1;
        *self
            .file_occurrences
            .entry(clone.file_a.clone())
            .or_default() += 1;
        if clone.file_a != clone.file_b {
            *self
                .file_occurrences
                .entry(clone.file_b.clone())
                .or_default() += 1;
        }

        match clone.clone_type {
            CloneType::Type1 => self.type1_count += 1,
            CloneType::Type2 => self.type2_count += 1,
            CloneType::Type3 => self.type3_count += 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotspot {
    pub file: String,
    pub duplicate_lines: usize,
    pub clone_group_count: usize,
    pub severity: f64,
}

// Helper functions

/// Detect programming language from file extension.
fn detect_language(path: &str) -> &'static str {
    let path_lower = path.to_lowercase();
    if path_lower.ends_with(".go") {
        "go"
    } else if path_lower.ends_with(".rs") {
        "rust"
    } else if path_lower.ends_with(".py") {
        "python"
    } else if path_lower.ends_with(".ts") || path_lower.ends_with(".tsx") {
        "typescript"
    } else if path_lower.ends_with(".js") || path_lower.ends_with(".jsx") {
        "javascript"
    } else if path_lower.ends_with(".c") || path_lower.ends_with(".h") {
        "c"
    } else if path_lower.ends_with(".cpp")
        || path_lower.ends_with(".hpp")
        || path_lower.ends_with(".cc")
        || path_lower.ends_with(".cxx")
    {
        "cpp"
    } else if path_lower.ends_with(".java") {
        "java"
    } else if path_lower.ends_with(".rb") {
        "ruby"
    } else if path_lower.ends_with(".php") {
        "php"
    } else {
        "unknown"
    }
}

/// Check if a line starts a function definition.
fn is_function_start(line: &str, lang: &str) -> bool {
    match lang {
        "go" => line.starts_with("func ") && line.contains('('),
        "rust" => line.contains("fn ") && line.contains('('),
        "python" => line.starts_with("def ") && line.contains('('),
        "ruby" => line.starts_with("def ") || line.starts_with("def self."),
        "typescript" | "javascript" => {
            line.contains("function ")
                || line.contains("=> {")
                || (line.contains('(') && line.contains(") {"))
        }
        "c" | "cpp" => line.contains('(') && (line.contains(") {") || line.ends_with('{')),
        "java" | "kotlin" => {
            (line.contains("void ")
                || line.contains("int ")
                || line.contains("String ")
                || line.contains("fun ")
                || line.contains("public ")
                || line.contains("private ")
                || line.contains("protected "))
                && line.contains('(')
        }
        "php" => line.contains("function ") && line.contains('('),
        _ => false,
    }
}

/// Check if a line is a comment.
fn is_comment(line: &str) -> bool {
    line.starts_with("//")
        || line.starts_with('#')
        || line.starts_with("/*")
        || line.starts_with('*')
        || line.starts_with("*/")
}

/// Keywords that should not be normalized.
fn is_keyword(token: &str) -> bool {
    matches!(
        token,
        // Go
        "func" | "return" | "if" | "else" | "for" | "range" | "switch" | "case" | "default"
        | "break" | "continue" | "goto" | "fallthrough" | "defer" | "go" | "select" | "chan"
        | "map" | "struct" | "interface" | "type" | "var" | "const" | "package" | "import"
        | "nil" | "true" | "false"
        // Rust
        | "fn" | "let" | "mut" | "match" | "loop" | "while" | "impl" | "trait" | "mod" | "use"
        | "pub" | "crate" | "self" | "Self" | "where" | "async" | "await" | "static" | "extern"
        | "unsafe" | "enum" | "move" | "ref" | "as" | "in"
        // Python
        | "def" | "class" | "elif" | "try" | "except" | "finally" | "with" | "lambda" | "yield"
        | "assert" | "raise" | "pass" | "del" | "global" | "nonlocal" | "and" | "or" | "not"
        | "is" | "from"
        // JavaScript/TypeScript
        | "function" | "new" | "this" | "super" | "extends" | "implements" | "export" | "throw"
        | "catch" | "instanceof" | "typeof" | "void" | "delete" | "debugger"
        // Common
        | "null" | "undefined"
    )
}

/// Check if a token is a literal value.
fn is_literal(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }

    let first = token.chars().next().unwrap();

    // String literal
    if first == '"' || first == '\'' || first == '`' {
        return true;
    }

    // Number literal
    if first.is_ascii_digit() {
        return true;
    }

    // Negative number
    if first == '-' && token.len() > 1 {
        if let Some(second) = token.chars().nth(1) {
            if second.is_ascii_digit() {
                return true;
            }
        }
    }

    false
}

/// Normalize tokens with a fresh identifier map.
/// Each fragment gets its own identifier numbering, so structurally identical
/// code in different files will produce identical token sequences.
fn normalize_tokens_fresh(tokens: &[String], config: &Config) -> Vec<String> {
    let mut identifier_map: HashMap<String, String> = HashMap::new();
    let mut counter = 0u32;

    tokens
        .iter()
        .filter_map(|token| {
            if token.is_empty() {
                return None;
            }

            // Keywords are not normalized
            if is_keyword(token) {
                return Some(token.clone());
            }

            // Literals
            if is_literal(token) {
                if config.normalize_literals {
                    return Some("LITERAL".to_string());
                }
                return Some(token.clone());
            }

            // Operators and delimiters are not normalized
            if is_operator_or_delimiter(token) {
                return Some(token.clone());
            }

            // Identifiers - use per-fragment canonical name
            if config.normalize_identifiers {
                if let Some(canonical) = identifier_map.get(token) {
                    return Some(canonical.clone());
                }
                let canonical = format!("VAR_{counter}");
                counter += 1;
                identifier_map.insert(token.clone(), canonical.clone());
                return Some(canonical);
            }

            Some(token.clone())
        })
        .collect()
}

/// Check if a token is an operator or delimiter.
fn is_operator_or_delimiter(token: &str) -> bool {
    matches!(
        token,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "="
            | "=="
            | "!="
            | "<"
            | ">"
            | "<="
            | ">="
            | "&&"
            | "||"
            | "!"
            | "&"
            | "|"
            | "^"
            | "<<"
            | ">>"
            | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "&="
            | "|="
            | "^="
            | "<<="
            | ">>="
            | "++"
            | "--"
            | "->"
            | "=>"
            | "::"
            | ".."
            | "..."
            | "?"
            | ":"
            | "("
            | ")"
            | "["
            | "]"
            | "{"
            | "}"
            | ","
            | ";"
            | "."
    )
}

/// Tokenize code into tokens.
fn tokenize(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Skip whitespace
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // String literals
        if c == '"' || c == '\'' || c == '`' {
            tokens.push(collect_string_literal(&chars, &mut i, c));
            continue;
        }

        // Numbers
        if c.is_ascii_digit() || (c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            tokens.push(collect_number(&chars, &mut i));
            continue;
        }

        // Identifiers and keywords
        if c.is_alphabetic() || c == '_' {
            tokens.push(collect_identifier(&chars, &mut i));
            continue;
        }

        // Multi-character operators
        if let Some(op) = collect_operator(&chars, &mut i) {
            tokens.push(op);
            continue;
        }

        // Single character
        tokens.push(c.to_string());
        i += 1;
    }

    tokens
}

fn collect_string_literal(chars: &[char], i: &mut usize, quote: char) -> String {
    let mut s = String::new();
    s.push(chars[*i]);
    *i += 1;

    while *i < chars.len() {
        let c = chars[*i];
        s.push(c);
        *i += 1;

        if c == quote {
            break;
        }
        // Handle escape sequences
        if c == '\\' && *i < chars.len() {
            s.push(chars[*i]);
            *i += 1;
        }
    }

    s
}

fn collect_number(chars: &[char], i: &mut usize) -> String {
    let mut s = String::new();

    // Handle negative sign
    if chars[*i] == '-' {
        s.push('-');
        *i += 1;
    }

    while *i < chars.len() {
        let c = chars[*i];
        if c.is_ascii_digit()
            || c == '.'
            || c == '_'
            || c == 'x'
            || c == 'X'
            || c == 'b'
            || c == 'B'
            || c == 'o'
            || c == 'O'
            || ('a'..='f').contains(&c)
            || ('A'..='F').contains(&c)
            || c == 'e'
            || c == 'E'
        {
            s.push(c);
            *i += 1;
        } else {
            break;
        }
    }

    s
}

fn collect_identifier(chars: &[char], i: &mut usize) -> String {
    let mut s = String::new();

    while *i < chars.len() {
        let c = chars[*i];
        if c.is_alphanumeric() || c == '_' {
            s.push(c);
            *i += 1;
        } else {
            break;
        }
    }

    s
}

fn collect_operator(chars: &[char], i: &mut usize) -> Option<String> {
    if *i >= chars.len() {
        return None;
    }

    // Try 3-character operators
    if *i + 2 < chars.len() {
        let op3: String = chars[*i..*i + 3].iter().collect();
        if matches!(op3.as_str(), "<<=" | ">>=" | "..." | "===" | "!==") {
            *i += 3;
            return Some(op3);
        }
    }

    // Try 2-character operators
    if *i + 1 < chars.len() {
        let op2: String = chars[*i..*i + 2].iter().collect();
        if matches!(
            op2.as_str(),
            "==" | "!="
                | "<="
                | ">="
                | "&&"
                | "||"
                | "<<"
                | ">>"
                | "+="
                | "-="
                | "*="
                | "/="
                | "%="
                | "&="
                | "|="
                | "^="
                | "++"
                | "--"
                | "->"
                | "=>"
                | "::"
                | ".."
                | "??"
        ) {
            *i += 2;
            return Some(op2);
        }
    }

    None
}

/// Generate k-shingles from tokens using blake3 hashing.
fn generate_k_shingles(tokens: &[String], k: usize) -> Vec<u64> {
    if tokens.len() < k {
        if !tokens.is_empty() {
            let mut hasher = blake3::Hasher::new();
            for t in tokens {
                hasher.update(t.as_bytes());
            }
            let hash = hasher.finalize();
            return vec![u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())];
        }
        return Vec::new();
    }

    let mut shingle_set: HashSet<u64> = HashSet::new();

    for window in tokens.windows(k) {
        let mut hasher = blake3::Hasher::new();
        for token in window {
            hasher.update(token.as_bytes());
        }
        let hash = hasher.finalize();
        let h = u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap());
        shingle_set.insert(h);
    }

    shingle_set.into_iter().collect()
}

/// Hash a u64 value with a seed using murmur-style mixing.
fn hash_u64_with_seed(value: u64, seed: u64) -> u64 {
    let mut h = value ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
    h
}

/// Hash a band portion of a MinHash signature.
fn hash_band(values: &[u64], seed: u64) -> u64 {
    const FNV_PRIME: u64 = 0x00000100000001B3;
    let mut h = seed ^ 0xcbf29ce484222325; // FNV offset basis
    for &v in values {
        h ^= v;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Calculate percentile from sorted values.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }

    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config as CoreConfig;
    use crate::core::FileSet;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "duplicates");
        assert_eq!(analyzer.config.min_tokens, 50);
        assert!((analyzer.config.similarity_threshold - 0.70).abs() < 0.001);
    }

    #[test]
    fn test_with_config() {
        let analyzer = Analyzer::new()
            .with_min_tokens(100)
            .with_similarity_threshold(0.8);
        assert_eq!(analyzer.config.min_tokens, 100);
        assert!((analyzer.config.similarity_threshold - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_clone_type_from_similarity() {
        assert_eq!(CloneType::from_similarity(0.99), CloneType::Type1);
        assert_eq!(CloneType::from_similarity(0.95), CloneType::Type1);
        assert_eq!(CloneType::from_similarity(0.90), CloneType::Type2);
        assert_eq!(CloneType::from_similarity(0.85), CloneType::Type2);
        assert_eq!(CloneType::from_similarity(0.70), CloneType::Type3);
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("func main() { x := 42 }");
        assert!(tokens.contains(&"func".to_string()));
        assert!(tokens.contains(&"main".to_string()));
        assert!(tokens.contains(&"42".to_string()));
        assert!(tokens.contains(&"{".to_string()));
        assert!(tokens.contains(&"}".to_string()));
    }

    #[test]
    fn test_tokenize_with_strings() {
        let tokens = tokenize(r#"x := "hello world""#);
        assert!(tokens.contains(&"x".to_string()));
        assert!(tokens.contains(&r#""hello world""#.to_string()));
    }

    #[test]
    fn test_is_keyword() {
        assert!(is_keyword("func"));
        assert!(is_keyword("fn"));
        assert!(is_keyword("def"));
        assert!(is_keyword("function"));
        assert!(!is_keyword("myFunction"));
        assert!(!is_keyword("variable"));
    }

    #[test]
    fn test_is_literal() {
        assert!(is_literal("42"));
        assert!(is_literal("-123"));
        assert!(is_literal(r#""hello""#));
        assert!(is_literal("'a'"));
        assert!(!is_literal("func"));
        assert!(!is_literal("myVar"));
    }

    #[test]
    fn test_is_operator() {
        assert!(is_operator_or_delimiter("+"));
        assert!(is_operator_or_delimiter("=="));
        assert!(is_operator_or_delimiter("("));
        assert!(is_operator_or_delimiter("}"));
        assert!(!is_operator_or_delimiter("func"));
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("main.go"), "go");
        assert_eq!(detect_language("lib.rs"), "rust");
        assert_eq!(detect_language("app.py"), "python");
        assert_eq!(detect_language("app.ts"), "typescript");
        assert_eq!(detect_language("app.js"), "javascript");
        assert_eq!(detect_language("app.rb"), "ruby");
        assert_eq!(detect_language("unknown.xyz"), "unknown");
    }

    #[test]
    fn test_generate_k_shingles() {
        let tokens: Vec<String> = vec!["a", "b", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect();
        let shingles = generate_k_shingles(&tokens, 3);
        assert_eq!(shingles.len(), 3); // "abc", "bcd", "cde"
    }

    #[test]
    fn test_generate_k_shingles_short() {
        let tokens: Vec<String> = vec!["a", "b"].into_iter().map(String::from).collect();
        let shingles = generate_k_shingles(&tokens, 5);
        assert_eq!(shingles.len(), 1); // Falls back to whole sequence
    }

    #[test]
    fn test_minhash_similarity() {
        let analyzer = Analyzer::new();

        // Identical tokens should have perfect similarity
        let tokens1: Vec<String> = (0..60).map(|i| format!("token{}", i)).collect();
        let sig1 = analyzer.compute_minhash(&tokens1);
        let sig2 = analyzer.compute_minhash(&tokens1);

        let similarity = sig1.jaccard_similarity(&sig2);
        assert!((similarity - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_minhash_similarity_different() {
        let analyzer = Analyzer::new();

        // Completely different tokens should have similarity 0
        let tokens1: Vec<String> = (0..60).map(|i| format!("alpha{}", i)).collect();
        let tokens2: Vec<String> = (0..60).map(|i| format!("beta{}", i)).collect();
        let sig1 = analyzer.compute_minhash(&tokens1);
        let sig2 = analyzer.compute_minhash(&tokens2);

        let similarity = sig1.jaccard_similarity(&sig2);
        assert!(similarity < 0.1); // Should be very low
    }

    #[test]
    fn test_percentile() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&values, 50.0) - 3.0).abs() < 0.001);
        assert!((percentile(&values, 0.0) - 1.0).abs() < 0.001);
        assert!((percentile(&values, 100.0) - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_empty() {
        let values: Vec<f64> = vec![];
        assert!((percentile(&values, 50.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_code() {
        let analyzer = Analyzer::new();
        let code = "  func main() {\n    // comment\n    x := 1\n  }";
        let normalized = analyzer.normalize_code(code);
        assert!(!normalized.contains("// comment"));
        assert!(normalized.contains("func main()"));
    }

    #[test]
    fn test_canonicalize_identifier_consistency() {
        let config = Config::default();

        // Same identifier within a fragment should get the same canonical name
        let tokens = vec![
            "myVariable".to_string(),
            "otherVariable".to_string(),
            "myVariable".to_string(),
            "myVariable".to_string(),
        ];
        let normalized = normalize_tokens_fresh(&tokens, &config);

        // myVariable appears first -> VAR_0
        // otherVariable appears second -> VAR_1
        assert_eq!(normalized[0], "VAR_0");
        assert_eq!(normalized[1], "VAR_1");
        assert_eq!(normalized[2], "VAR_0"); // Same as first
        assert_eq!(normalized[3], "VAR_0"); // Same as first
    }

    #[test]
    fn test_is_function_start_go() {
        assert!(is_function_start("func main() {", "go"));
        assert!(is_function_start("func (s *Server) Start()", "go"));
        assert!(!is_function_start("var x = 1", "go"));
    }

    #[test]
    fn test_is_function_start_rust() {
        assert!(is_function_start("fn main() {", "rust"));
        assert!(is_function_start("pub fn analyze(&self) {", "rust"));
        assert!(!is_function_start("let x = 1", "rust"));
    }

    #[test]
    fn test_is_function_start_python() {
        assert!(is_function_start("def my_func():", "python"));
        assert!(is_function_start("def __init__(self):", "python"));
        assert!(!is_function_start("class MyClass:", "python"));
    }

    #[test]
    fn test_is_function_start_ruby() {
        assert!(is_function_start("def my_method", "ruby"));
        assert!(is_function_start("def self.class_method", "ruby"));
        assert!(!is_function_start("class MyClass", "ruby"));
    }

    /// Test Ruby fragment extraction works correctly.
    #[test]
    fn test_ruby_fragment_extraction() {
        let analyzer = Analyzer::new().with_min_tokens(5);

        let code = r#"class UserService
  def find_user(email)
    return @cache[email] if @cache[email]
    user = @repo.find(email)
    if user
      @cache[email] = user
      return user
    end
    nil
  end
end
"#;
        let fragments = analyzer.extract_fragments("test.rb", code.as_bytes());

        // We should extract the find_user method as a fragment
        assert!(
            !fragments.is_empty(),
            "Should extract at least one fragment from Ruby file"
        );

        // The fragment should contain the full method (lines 2-11)
        let frag = &fragments[0];
        assert!(
            frag.tokens.len() >= 10,
            "Ruby method should have at least 10 tokens, got {}",
            frag.tokens.len()
        );
    }

    /// Test Ruby token normalization produces identical tokens for structurally identical code.
    #[test]
    fn test_ruby_token_similarity() {
        let analyzer = Analyzer::new().with_min_tokens(5);

        let code1 = r#"def find_user(email)
  return @cache[email] if @cache[email]
  user = @repo.find(email)
  if user
    @cache[email] = user
    return user
  end
  nil
end
"#;
        let code2 = r#"def find_product(sku)
  return @cache[sku] if @cache[sku]
  product = @repo.find(sku)
  if product
    @cache[sku] = product
    return product
  end
  nil
end
"#;
        let frags1 = analyzer.extract_fragments("a.rb", code1.as_bytes());
        let frags2 = analyzer.extract_fragments("b.rb", code2.as_bytes());

        assert!(!frags1.is_empty(), "Should extract fragment from code1");
        assert!(!frags2.is_empty(), "Should extract fragment from code2");

        // Per-fragment normalization means structurally identical code produces
        // identical token sequences
        assert_eq!(
            frags1[0].tokens, frags2[0].tokens,
            "Structurally identical Ruby methods should produce identical normalized tokens"
        );

        let sig1 = analyzer.compute_minhash(&frags1[0].tokens);
        let sig2 = analyzer.compute_minhash(&frags2[0].tokens);
        let similarity = sig1.jaccard_similarity(&sig2);

        // Identical tokens should produce perfect similarity
        assert!(
            (similarity - 1.0).abs() < 0.001,
            "Expected similarity = 1.0, got {:.2}",
            similarity
        );
    }

    /// Ruby functions should be detected as clones when they have similar structure
    #[test]
    fn test_analyze_ruby_clones() {
        let tmp_dir = TempDir::new().unwrap();

        // Two Ruby files with structurally similar methods
        let code1 = r#"class UserService
  def find_user_by_email(email)
    return @cache[email] if @cache[email]
    user = @repository.find_by(email: email)
    if user
      @cache[email] = user
      @logger.info("Found user")
      @metrics.increment(:user_found)
      return user
    end
    @logger.warn("User not found")
    @metrics.increment(:user_not_found)
    nil
  end
end
"#;
        let code2 = r#"class ProductService
  def find_product_by_sku(sku)
    return @cache[sku] if @cache[sku]
    product = @repository.find_by(sku: sku)
    if product
      @cache[sku] = product
      @logger.info("Found product")
      @metrics.increment(:product_found)
      return product
    end
    @logger.warn("Product not found")
    @metrics.increment(:product_not_found)
    nil
  end
end
"#;

        let file1 = tmp_dir.path().join("user_service.rb");
        let file2 = tmp_dir.path().join("product_service.rb");
        fs::write(&file1, code1).unwrap();
        fs::write(&file2, code2).unwrap();

        let analyzer = Analyzer::new()
            .with_min_tokens(15)
            .with_similarity_threshold(0.7);

        let config = CoreConfig::default();
        let file_set = FileSet::from_path(tmp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(tmp_dir.path()));

        let analysis = analyzer.analyze(&ctx).unwrap();

        assert_eq!(analysis.total_files_scanned, 2, "expected 2 files scanned");
        // Should detect the similar Ruby methods as clones
        assert!(
            !analysis.groups.is_empty(),
            "Expected at least 1 clone group for similar Ruby methods, got 0. \
             Summary: total_lines={}, duplicated_lines={}",
            analysis.summary.total_lines,
            analysis.summary.duplicated_lines
        );
    }

    #[test]
    fn test_is_function_start_javascript() {
        assert!(is_function_start("function hello() {", "javascript"));
        assert!(is_function_start("const x = () => {", "javascript"));
        assert!(!is_function_start("const x = 1", "javascript"));
    }

    /// Test from Go: exact clones should be detected
    #[test]
    fn test_analyze_exact_clones() {
        let tmp_dir = TempDir::new().unwrap();

        // Create two files with identical functions
        let code = r#"package main

func duplicate() int {
    x := 1
    y := 2
    z := 3
    result := x + y + z
    if result > 5 {
        return result
    }
    return 0
}
"#;
        let file1 = tmp_dir.path().join("a.go");
        let file2 = tmp_dir.path().join("b.go");
        fs::write(&file1, code).unwrap();
        fs::write(&file2, code).unwrap();

        let analyzer = Analyzer::new()
            .with_min_tokens(10)
            .with_similarity_threshold(0.8);

        let config = CoreConfig::default();
        let file_set = FileSet::from_path(tmp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(tmp_dir.path()));

        let analysis = analyzer.analyze(&ctx).unwrap();

        assert_eq!(analysis.total_files_scanned, 2);
        // Should find at least one clone group
        assert!(
            !analysis.groups.is_empty(),
            "Expected at least 1 clone group, got {}",
            analysis.groups.len()
        );
    }

    /// Test from Go: no clones should be found for different code
    #[test]
    fn test_analyze_no_clones() {
        let tmp_dir = TempDir::new().unwrap();

        let file1 = tmp_dir.path().join("a.go");
        let code1 = r#"package main

func funcA() int {
    return 1
}
"#;
        fs::write(&file1, code1).unwrap();

        let file2 = tmp_dir.path().join("b.go");
        let code2 = r#"package main

func funcB() string {
    return "hello"
}
"#;
        fs::write(&file2, code2).unwrap();

        let analyzer = Analyzer::new().with_min_tokens(50); // High threshold to avoid small matches

        let config = CoreConfig::default();
        let file_set = FileSet::from_path(tmp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(tmp_dir.path()));

        let analysis = analyzer.analyze(&ctx).unwrap();

        assert_eq!(analysis.clones.len(), 0, "expected no clones");
    }

    /// Test from Go: empty files should not produce clones
    #[test]
    fn test_analyze_empty_files() {
        let tmp_dir = TempDir::new().unwrap();

        let file1 = tmp_dir.path().join("a.go");
        fs::write(&file1, "package main\n").unwrap();

        let analyzer = Analyzer::new();

        let config = CoreConfig::default();
        let file_set = FileSet::from_path(tmp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(tmp_dir.path()));

        let analysis = analyzer.analyze(&ctx).unwrap();

        assert_eq!(
            analysis.clones.len(),
            0,
            "expected no clones from minimal file"
        );
    }

    #[test]
    fn test_summary_add_clone_stats() {
        let mut summary = AnalysisSummary::default();

        let clone = Clone {
            clone_type: CloneType::Type1,
            similarity: 0.95,
            file_a: "a.go".to_string(),
            file_b: "b.go".to_string(),
            start_line_a: 1,
            end_line_a: 10,
            start_line_b: 1,
            end_line_b: 10,
            lines_a: 10,
            lines_b: 10,
            group_id: 1,
        };

        summary.add_clone_stats(&clone);

        assert_eq!(summary.total_clones, 1);
        assert_eq!(summary.type1_count, 1);
        // duplicated_lines is now calculated separately from unique group instances
        assert_eq!(summary.duplicated_lines, 0);
    }

    #[test]
    fn test_config_defaults() {
        let cfg = Config::default();

        assert!(cfg.min_tokens > 0, "MinTokens should be positive");
        assert!(
            cfg.similarity_threshold > 0.0 && cfg.similarity_threshold <= 1.0,
            "SimilarityThreshold should be in (0, 1]"
        );
        assert!(
            cfg.num_hash_functions > 0,
            "NumHashFunctions should be positive"
        );
        assert!(cfg.num_bands > 0, "NumBands should be positive");
    }
}
