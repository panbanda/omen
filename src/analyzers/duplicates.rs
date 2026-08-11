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
use std::ops::Range;
use std::path::Path;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{
    is_test_file, percentile, AnalysisContext, Analyzer as AnalyzerTrait, Language as CoreLanguage,
    Result,
};
use crate::parser::{cfg_test_module_ranges, Parser as TreeSitterParser};

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
    /// Exclude test code from duplication detection: whole test files
    /// (`is_test_file`) and, for Rust, fragments inside `#[cfg(test)]`
    /// modules. Defaults to true since repetitive test setup is expected
    /// and low-value to flag as duplication.
    pub exclude_tests: bool,
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
            exclude_tests: true,
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

    pub fn with_exclude_tests(mut self, exclude_tests: bool) -> Self {
        self.config.exclude_tests = exclude_tests;
        self
    }

    /// Extract code fragments from file content.
    fn extract_fragments(&self, path: &str, content: &[u8]) -> Vec<CodeFragment> {
        let content_str = match std::str::from_utf8(content) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let lines: Vec<&str> = content_str.lines().collect();
        let lang = detect_language(path);
        let mut fragments = Vec::new();

        // For Rust, find `#[cfg(test)] mod ...` line ranges so fragments
        // inside them are excluded below -- most of omen's own test
        // duplication is inline `#[cfg(test)] mod tests { ... }`, so
        // file-level exclusion (skipped whole files, see `analyze`) alone
        // is not enough.
        let test_line_ranges = if self.config.exclude_tests && lang == "rust" {
            rust_cfg_test_line_ranges(content_str)
        } else {
            Vec::new()
        };

        // Try function-level extraction first
        let (func_fragments, excluded_as_test) =
            self.extract_function_fragments(path, &lines, &test_line_ranges);
        if !func_fragments.is_empty() {
            fragments.extend(func_fragments);
        }

        // Fall back to whole file as single fragment if no functions were
        // found. If a function was dropped because it fell inside a
        // #[cfg(test)] range, do NOT fall back to the whole file -- that
        // would re-include the very test content the caller asked to
        // exclude via the file's other lines.
        if fragments.is_empty() && !excluded_as_test {
            if let Some(frag) =
                self.create_fragment(path, 0, lines.len().saturating_sub(1), &lines, lang)
            {
                fragments.push(frag);
            }
        }

        fragments
    }

    /// Extract function-level code fragments.
    ///
    /// `test_line_ranges` are 0-indexed `[start, end)` line ranges (typically
    /// Rust `#[cfg(test)]` modules) whose fragments should be dropped.
    ///
    /// Returns the fragments plus whether at least one function was dropped
    /// specifically because it fell inside `test_line_ranges` (as opposed to
    /// being dropped for being below `min_tokens`). Callers use this to
    /// avoid falling back to a whole-file fragment that would re-include the
    /// excluded test code.
    fn extract_function_fragments(
        &self,
        path: &str,
        lines: &[&str],
        test_line_ranges: &[Range<usize>],
    ) -> (Vec<CodeFragment>, bool) {
        let mut fragments = Vec::new();
        let mut excluded_as_test = false;
        let lang = detect_language(path);

        let mut in_function = false;
        let mut func_start_line = 0;
        let mut func_lines: Vec<&str> = Vec::new();
        let mut brace_depth = 0;
        let mut end_depth: i32 = 0;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            if !in_function {
                if !is_function_start(trimmed, lang) {
                    continue;
                }
                in_function = true;
                func_start_line = i;
                func_lines = vec![line];
                brace_depth = line.matches('{').count() as i32 - line.matches('}').count() as i32;
                if lang == "python" {
                    brace_depth = 1;
                } else if lang == "ruby" {
                    end_depth = 1;
                }
                continue;
            }

            func_lines.push(line);

            if lang == "python" {
                let is_dedent =
                    !trimmed.is_empty() && !line.starts_with(' ') && !line.starts_with('\t');
                let is_new_block = trimmed.starts_with("def ")
                    || trimmed.starts_with("class ")
                    || i == lines.len() - 1;

                if !(is_dedent && is_new_block) {
                    continue;
                }
                let end = if func_lines.len() > 1 { i - 1 } else { i };
                if line_in_ranges(func_start_line, test_line_ranges) {
                    excluded_as_test = true;
                } else if let Some(frag) = self.create_fragment(
                    path,
                    func_start_line,
                    end,
                    &func_lines[..func_lines.len().saturating_sub(1)],
                    lang,
                ) {
                    fragments.push(frag);
                }
                if is_function_start(trimmed, lang) {
                    func_start_line = i;
                    func_lines = vec![line];
                } else {
                    in_function = false;
                }
                continue;
            }

            let ended = match lang {
                "ruby" => process_ruby_line(trimmed, &mut end_depth),
                _ => {
                    brace_depth +=
                        line.matches('{').count() as i32 - line.matches('}').count() as i32;
                    brace_depth <= 0
                }
            };

            if !ended {
                continue;
            }

            if line_in_ranges(func_start_line, test_line_ranges) {
                excluded_as_test = true;
            } else if let Some(frag) =
                self.create_fragment(path, func_start_line, i, &func_lines, lang)
            {
                fragments.push(frag);
            }
            in_function = false;
            func_lines.clear();
            if lang == "ruby" {
                end_depth = 0;
            }
        }

        // Handle unclosed function at end of file
        if in_function && !func_lines.is_empty() {
            if line_in_ranges(func_start_line, test_line_ranges) {
                excluded_as_test = true;
            } else if let Some(frag) = self.create_fragment(
                path,
                func_start_line,
                lines.len().saturating_sub(1),
                &func_lines,
                lang,
            ) {
                fragments.push(frag);
            }
        }

        (fragments, excluded_as_test)
    }

    /// Create a code fragment from lines if it meets minimum token requirements.
    fn create_fragment(
        &self,
        path: &str,
        start_line: usize,
        end_line: usize,
        lines: &[&str],
        lang: &str,
    ) -> Option<CodeFragment> {
        // Normalize and tokenize
        let normalized = self.normalize_code(lines, lang);
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
            tokens: normalized_tokens,
            normalized_hash: 0, // Set later
            signature: None,    // Set later
        })
    }

    /// Normalize code for comparison.
    fn normalize_code(&self, lines: &[&str], lang: &str) -> String {
        let mut result = String::new();

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.config.ignore_comments && is_comment(trimmed, lang) {
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
        let mut sizes = vec![1usize; fragments.len()];

        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }

        fn union(parent: &mut [usize], sizes: &mut [usize], x: usize, y: usize) {
            let mut px = find(parent, x);
            let mut py = find(parent, y);
            if px == py {
                return;
            }
            if sizes[px] < sizes[py] {
                std::mem::swap(&mut px, &mut py);
            }
            parent[py] = px;
            sizes[px] += sizes[py];
        }

        // Union all clone pairs
        for pair in pairs {
            union(&mut parent, &mut sizes, pair.idx_a, pair.idx_b);
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
        // Extract fragments from all files in parallel
        let max_file_size = self.max_file_size;
        let exclude_tests = self.config.exclude_tests;
        let files_scanned = std::sync::atomic::AtomicUsize::new(0);
        let mut all_fragments: Vec<CodeFragment> = ctx
            .files
            .files()
            .par_iter()
            .filter_map(|path| {
                // Skip whole test files (mirrors cohesion.rs's skip_test_files).
                if exclude_tests && is_test_file(path) {
                    return None;
                }
                let content = ctx.read_file(path).ok()?;
                if max_file_size > 0 && content.len() > max_file_size {
                    return None;
                }
                files_scanned.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path_str = path.to_string_lossy();
                let fragments = self.extract_fragments(&path_str, &content);
                if fragments.is_empty() {
                    None
                } else {
                    Some(fragments)
                }
            })
            .flatten()
            .collect();
        let files_scanned = files_scanned.into_inner();

        // Sort by file path then line number for deterministic output
        all_fragments.sort_by(|a, b| a.file.cmp(&b.file).then(a.start_line.cmp(&b.start_line)));

        // Assign sequential IDs and compute totals
        let mut total_lines = 0usize;
        for (i, fragment) in all_fragments.iter_mut().enumerate() {
            fragment.id = i as u64;
            total_lines += (fragment.end_line - fragment.start_line + 1) as usize;
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

/// Find Rust `#[cfg(test)]` module bodies and convert them to 0-indexed
/// `[start, end)` line ranges, reusing the tree-sitter based detector shared
/// with `analyzers::tdg` (`crate::parser::cfg_test_module_ranges`) so both
/// analyzers agree on what counts as test code and avoid `not(test)` /
/// `contest`-style false positives.
fn rust_cfg_test_line_ranges(content: &str) -> Vec<Range<usize>> {
    let parser = TreeSitterParser::new();
    let Ok(parsed) = parser.parse(
        content.as_bytes(),
        CoreLanguage::Rust,
        Path::new("<duplicates>"),
    ) else {
        return Vec::new();
    };

    cfg_test_module_ranges(&parsed.tree.root_node(), content)
        .into_iter()
        .map(|byte_range| {
            byte_offset_to_line(content, byte_range.start)
                ..byte_offset_to_line(content, byte_range.end)
        })
        .collect()
}

/// Count newlines before `offset` to get the 0-indexed line number, matching
/// tree-sitter's own row convention.
fn byte_offset_to_line(content: &str, offset: usize) -> usize {
    content.as_bytes()[..offset.min(content.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// Check whether `line` (0-indexed) falls inside any of `ranges`.
fn line_in_ranges(line: usize, ranges: &[Range<usize>]) -> bool {
    ranges.iter().any(|r| line >= r.start && line < r.end)
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

/// Process a line inside a Ruby function body. Returns true if the function ended.
///
/// Tracks Ruby's block nesting via `end_depth`: block-starting keywords increment
/// the depth, and `end` decrements it. The function ends when depth reaches zero.
fn process_ruby_line(trimmed: &str, end_depth: &mut i32) -> bool {
    const BLOCK_STARTERS: &[&str] = &[
        "def ", "class ", "module ", "if ", "unless ", "case ", "while ", "until ", "for ",
        "begin", "do",
    ];

    let starts_block = BLOCK_STARTERS.iter().any(|starter| {
        trimmed.starts_with(starter) || trimmed.contains(&format!(" {} ", starter.trim()))
    });
    if starts_block {
        *end_depth += 1;
    }

    if trimmed.ends_with(" do") || trimmed.ends_with(" do |") {
        *end_depth += 1;
    }

    let is_end = trimmed == "end" || trimmed.starts_with("end ") || trimmed.ends_with(" end");
    if !is_end {
        return false;
    }

    *end_depth -= 1;
    *end_depth <= 0
}

/// Check if a line is a comment, using language-specific comment prefixes.
///
/// The `#` character is only a comment prefix in languages that use it
/// (Python, Ruby, Bash, PHP). In Rust, `#` starts attributes like
/// `#[derive(Debug)]` and must not be stripped.
fn is_comment(line: &str, lang: &str) -> bool {
    if line.starts_with("//")
        || line.starts_with("/*")
        || line.starts_with('*')
        || line.starts_with("*/")
    {
        return true;
    }

    if line.starts_with('#') {
        return matches!(lang, "python" | "ruby" | "bash" | "php");
    }

    false
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

    let first = token.chars().next().expect("non-empty token checked above");

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
    let mut chars = content.char_indices().peekable();

    while let Some((start, c)) = chars.next() {
        // Skip whitespace
        if c.is_whitespace() {
            continue;
        }

        // String literals
        if c == '"' || c == '\'' || c == '`' {
            let mut escaped = false;
            for (_, current) in chars.by_ref() {
                if escaped {
                    escaped = false;
                } else if current == '\\' {
                    escaped = true;
                } else if current == c {
                    break;
                }
            }
            let end = chars.peek().map_or(content.len(), |(index, _)| *index);
            tokens.push(content[start..end].to_string());
            continue;
        }

        // Numbers
        if c.is_ascii_digit()
            || (c == '-' && chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()))
        {
            while chars.peek().is_some_and(|(_, next)| {
                next.is_ascii_digit()
                    || matches!(
                        next,
                        '.' | '_'
                            | 'x'
                            | 'X'
                            | 'b'
                            | 'B'
                            | 'o'
                            | 'O'
                            | 'a'..='f'
                            | 'A'..='F'
                    )
            }) {
                chars.next();
            }
            let end = chars.peek().map_or(content.len(), |(index, _)| *index);
            tokens.push(content[start..end].to_string());
            continue;
        }

        // Identifiers and keywords
        if c.is_alphabetic() || c == '_' {
            while chars
                .peek()
                .is_some_and(|(_, next)| next.is_alphanumeric() || *next == '_')
            {
                chars.next();
            }
            let end = chars.peek().map_or(content.len(), |(index, _)| *index);
            tokens.push(content[start..end].to_string());
            continue;
        }

        // Guard operator matching by the first byte, then allocate only the
        // operator that is actually emitted.
        let rest = &content[start..];
        let operator_len = if matches!(c, '<' | '>' | '.' | '=' | '!')
            && ["<<=", ">>=", "...", "===", "!=="]
                .iter()
                .any(|operator| rest.starts_with(operator))
        {
            3
        } else if matches!(
            c,
            '=' | '!' | '<' | '>' | '&' | '|' | '+' | '-' | '*' | '/' | '%' | '^' | ':' | '.' | '?'
        ) && [
            "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=",
            "|=", "^=", "++", "--", "->", "=>", "::", "..", "??",
        ]
        .iter()
        .any(|operator| rest.starts_with(operator))
        {
            2
        } else {
            0
        };
        if operator_len > 0 {
            for _ in 1..operator_len {
                chars.next();
            }
            tokens.push(content[start..start + operator_len].to_string());
            continue;
        }

        // Single character
        tokens.push(c.to_string());
    }

    tokens
}

/// Generate k-shingles from tokens using BLAKE3 hashing.
fn generate_k_shingles(tokens: &[String], k: usize) -> Vec<u64> {
    if tokens.len() < k {
        if !tokens.is_empty() {
            let mut bytes = Vec::new();
            for t in tokens {
                bytes.extend_from_slice(t.as_bytes());
            }
            let hash = blake3::hash(&bytes);
            return vec![u64::from_le_bytes(
                hash.as_bytes()[..8]
                    .try_into()
                    .expect("blake3 hash is always 32 bytes"),
            )];
        }
        return Vec::new();
    }

    let mut shingle_set: HashSet<u64> = HashSet::new();
    let mut bytes = Vec::new();

    for window in tokens.windows(k) {
        bytes.clear();
        for token in window {
            bytes.extend_from_slice(token.as_bytes());
        }
        let hash = blake3::hash(&bytes);
        shingle_set.insert(u64::from_le_bytes(
            hash.as_bytes()[..8]
                .try_into()
                .expect("blake3 hash is always 32 bytes"),
        ));
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
        let values: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
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
        let lines: Vec<&str> = code.lines().collect();
        let normalized = analyzer.normalize_code(&lines, "go");
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
        assert!(cfg.exclude_tests, "exclude_tests should default to true");
    }

    #[test]
    fn test_rust_attribute_not_treated_as_comment() {
        assert!(
            !is_comment("#[derive(Debug)]", "rust"),
            "Rust #[derive(Debug)] should not be treated as a comment"
        );
        assert!(
            !is_comment("#![allow(unused)]", "rust"),
            "Rust #![allow(unused)] should not be treated as a comment"
        );
        assert!(
            !is_comment("#[cfg(test)]", "rust"),
            "Rust #[cfg(test)] should not be treated as a comment"
        );
    }

    #[test]
    fn test_python_hash_treated_as_comment() {
        assert!(
            is_comment("# TODO: fix this", "python"),
            "Python # TODO should be treated as a comment"
        );
        assert!(
            is_comment("# a regular comment", "python"),
            "Python # comment should be treated as a comment"
        );
    }

    #[test]
    fn test_go_slash_comment_treated_as_comment() {
        assert!(
            is_comment("// this is a comment", "go"),
            "Go // comment should be treated as a comment"
        );
        assert!(
            !is_comment("#include <stdio.h>", "go"),
            "# should not be a comment in Go"
        );
    }

    #[test]
    fn test_is_comment_language_specific() {
        // Hash is a comment in Ruby and Bash
        assert!(is_comment("# comment", "ruby"));
        assert!(is_comment("# comment", "bash"));
        assert!(is_comment("# comment", "php"));

        // Hash is NOT a comment in C-family or Rust
        assert!(!is_comment("#include <stdio.h>", "c"));
        assert!(!is_comment("#include <vector>", "cpp"));
        assert!(!is_comment("#[derive(Clone)]", "rust"));
        assert!(!is_comment("# not a comment", "java"));
        assert!(!is_comment("# not a comment", "typescript"));
        assert!(!is_comment("# not a comment", "javascript"));

        // Double-slash is a comment everywhere
        assert!(is_comment("// comment", "rust"));
        assert!(is_comment("// comment", "c"));
        assert!(is_comment("// comment", "python"));
    }

    /// A fragment inside `#[cfg(test)] mod tests { ... }` must be excluded
    /// when `exclude_tests` is true (the default), and included when false
    /// (mirrors the `omen clones --include-tests` CLI override).
    #[test]
    fn test_cfg_test_fragment_excluded_by_default() {
        let code = r#"
#[cfg(test)]
mod tests {
    fn helper_one() {
        let a = 1;
        let b = 2;
        let c = 3;
        let d = a + b + c;
        let e = d * 2;
        let f = e - 1;
        let g = f + a;
        let h = g - b;
        println!("{}", h);
    }
}
"#;

        let excluding = Analyzer::new().with_min_tokens(10);
        let fragments = excluding.extract_fragments("lib.rs", code.as_bytes());
        assert!(
            fragments.is_empty(),
            "fragment inside #[cfg(test)] should be excluded by default, got {} fragments",
            fragments.len()
        );

        let including = Analyzer::new()
            .with_min_tokens(10)
            .with_exclude_tests(false);
        let fragments = including.extract_fragments("lib.rs", code.as_bytes());
        assert!(
            !fragments.is_empty(),
            "fragment inside #[cfg(test)] should be included with exclude_tests=false"
        );
    }

    /// A whole test FILE (matched by `crate::core::is_test_file`) should be
    /// skipped entirely by default, and analyzed with `--include-tests`.
    #[test]
    fn test_exclude_tests_skips_test_files() {
        let tmp_dir = TempDir::new().unwrap();

        let code = r#"package main

func duplicateHelper() int {
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
        let file1 = tmp_dir.path().join("a_test.go");
        let file2 = tmp_dir.path().join("b_test.go");
        fs::write(&file1, code).unwrap();
        fs::write(&file2, code).unwrap();

        let config = CoreConfig::default();
        let file_set = FileSet::from_path(tmp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(tmp_dir.path()));

        let analyzer = Analyzer::new()
            .with_min_tokens(10)
            .with_similarity_threshold(0.8);
        let analysis = analyzer.analyze(&ctx).unwrap();
        assert_eq!(
            analysis.total_files_scanned, 0,
            "*_test.go files should be skipped by default"
        );
        assert!(analysis.groups.is_empty());

        let including = Analyzer::new()
            .with_min_tokens(10)
            .with_similarity_threshold(0.8)
            .with_exclude_tests(false);
        let analysis = including.analyze(&ctx).unwrap();
        assert_eq!(
            analysis.total_files_scanned, 2,
            "test files should be scanned with exclude_tests=false"
        );
        assert!(!analysis.groups.is_empty());
    }

    /// A production file merely named like a test keyword substring
    /// (`contest.rs`) and a `#[cfg(not(test))]` block must NOT be excluded --
    /// only real test files/modules should be dropped.
    #[test]
    fn test_contest_file_and_cfg_not_test_are_not_excluded() {
        assert!(
            !is_test_file(Path::new("contest.rs")),
            "contest.rs should not be treated as a test file"
        );

        let code = r#"
#[cfg(not(test))]
mod real_impl {
    fn helper_two() {
        let a = 1;
        let b = 2;
        let c = 3;
        let d = a + b + c;
        let e = d * 2;
        let f = e - 1;
        let g = f + a;
        let h = g - b;
        println!("{}", h);
    }
}
"#;
        let analyzer = Analyzer::new().with_min_tokens(10);
        let fragments = analyzer.extract_fragments("lib.rs", code.as_bytes());
        assert!(
            !fragments.is_empty(),
            "#[cfg(not(test))] fragment must not be excluded"
        );
    }

    /// Two identical PRODUCTION functions (outside any #[cfg(test)] module,
    /// in non-test files) must still be detected as a clone when
    /// exclude_tests is at its default of true.
    #[test]
    fn test_exclude_tests_still_detects_production_clones() {
        let tmp_dir = TempDir::new().unwrap();

        let code = r#"pub fn compute_totals() -> i32 {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = a + b + c;
    let e = d * 2;
    let f = e - 1;
    let g = f + a;
    let h = g - b;
    h
}
"#;
        let file1 = tmp_dir.path().join("a.rs");
        let file2 = tmp_dir.path().join("b.rs");
        fs::write(&file1, code).unwrap();
        fs::write(&file2, code).unwrap();

        let config = CoreConfig::default();
        let file_set = FileSet::from_path(tmp_dir.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&file_set, &config, Some(tmp_dir.path()));

        let analyzer = Analyzer::new()
            .with_min_tokens(10)
            .with_similarity_threshold(0.8);
        let analysis = analyzer.analyze(&ctx).unwrap();

        assert_eq!(analysis.total_files_scanned, 2);
        assert!(
            !analysis.groups.is_empty(),
            "identical production functions should still be detected as clones"
        );
    }

    #[test]
    fn test_max_nesting_depth_acceptable() {
        let source = include_str!("duplicates.rs");
        let max_nesting = source
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| (l.len() - l.trim_start().len()) / 4)
            .max()
            .unwrap_or(0);
        assert!(
            max_nesting <= 7,
            "duplicates.rs nesting depth {max_nesting} exceeds 7"
        );
    }
}
