//! Pure-Rust TF-IDF engine for semantic code search.
//!
//! Uses sublinear TF (1 + ln(tf)), smooth IDF (sklearn-compatible), unigram + bigram
//! tokenization, and L2-normalized sparse vectors for cosine similarity via dot product.

use std::collections::HashMap;

use regex::Regex;

/// Metadata for an indexed document.
#[derive(Debug, Clone)]
pub struct DocMeta {
    pub file_path: String,
    pub symbol_name: String,
    pub symbol_type: String,
    pub signature: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// Sparse vector: parallel arrays of column indices and values.
#[derive(Debug, Clone)]
struct SparseVec {
    indices: Vec<u32>,
    values: Vec<f32>,
}

impl SparseVec {
    fn dot(&self, other: &SparseVec) -> f32 {
        let mut sum = 0.0f32;
        let (mut i, mut j) = (0, 0);
        while i < self.indices.len() && j < other.indices.len() {
            match self.indices[i].cmp(&other.indices[j]) {
                std::cmp::Ordering::Equal => {
                    sum += self.values[i] * other.values[j];
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        sum
    }

    fn l2_normalize(&mut self) {
        let norm: f32 = self.values.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut self.values {
                *v /= norm;
            }
        }
    }
}

/// Maximum vocabulary size (top terms by document frequency).
const MAX_VOCAB: usize = 10_000;

/// TF-IDF engine. Fits a vocabulary from a corpus, then supports search queries.
pub struct TfidfEngine {
    vocab: HashMap<String, u32>,
    idf: Vec<f32>,
    doc_vectors: Vec<SparseVec>,
    doc_meta: Vec<DocMeta>,
}

impl TfidfEngine {
    /// Build a TF-IDF engine from a corpus of (text, metadata) pairs.
    pub fn fit(docs: &[(String, DocMeta)]) -> Self {
        if docs.is_empty() {
            return Self {
                vocab: HashMap::new(),
                idf: Vec::new(),
                doc_vectors: Vec::new(),
                doc_meta: Vec::new(),
            };
        }

        let n = docs.len() as f32;

        // Tokenize all docs
        let tokenized: Vec<Vec<String>> = docs.iter().map(|(text, _)| tokenize(text)).collect();

        // Count document frequency per term
        let mut df: HashMap<String, u32> = HashMap::new();
        for tokens in &tokenized {
            let mut seen = HashMap::new();
            for t in tokens {
                seen.entry(t.clone()).or_insert(true);
            }
            for term in seen.into_keys() {
                *df.entry(term).or_insert(0) += 1;
            }
        }

        // Select top MAX_VOCAB terms by DF (descending), breaking ties alphabetically
        let mut terms: Vec<(String, u32)> = df.into_iter().collect();
        terms.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        terms.truncate(MAX_VOCAB);

        let vocab: HashMap<String, u32> = terms
            .iter()
            .enumerate()
            .map(|(idx, (term, _))| (term.clone(), idx as u32))
            .collect();

        // Compute smooth IDF: ln(1 + n/(1+df)) + 1
        let idf: Vec<f32> = terms
            .iter()
            .map(|(_, doc_freq)| (1.0 + n / (1.0 + *doc_freq as f32)).ln() + 1.0)
            .collect();

        // Build TF-IDF vectors for each document
        let doc_vectors: Vec<SparseVec> = tokenized
            .iter()
            .map(|tokens| build_tfidf_vector(tokens, &vocab, &idf))
            .collect();

        let doc_meta: Vec<DocMeta> = docs.iter().map(|(_, meta)| meta.clone()).collect();

        Self {
            vocab,
            idf,
            doc_vectors,
            doc_meta,
        }
    }

    /// Search for the top-k documents most similar to the query.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<(DocMeta, f32)> {
        if self.doc_vectors.is_empty() {
            return Vec::new();
        }

        let tokens = tokenize(query);
        let query_vec = build_tfidf_vector(&tokens, &self.vocab, &self.idf);

        let mut scored: Vec<(usize, f32)> = self
            .doc_vectors
            .iter()
            .enumerate()
            .map(|(i, doc_vec)| (i, query_vec.dot(doc_vec)))
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);

        scored
            .into_iter()
            .map(|(i, score)| (self.doc_meta[i].clone(), score))
            .collect()
    }

    /// Search within a specific set of files.
    pub fn search_in_files(
        &self,
        query: &str,
        files: &[&str],
        top_k: usize,
    ) -> Vec<(DocMeta, f32)> {
        if self.doc_vectors.is_empty() {
            return Vec::new();
        }

        let tokens = tokenize(query);
        let query_vec = build_tfidf_vector(&tokens, &self.vocab, &self.idf);

        let mut scored: Vec<(usize, f32)> = self
            .doc_vectors
            .iter()
            .enumerate()
            .filter(|(i, _)| files.contains(&self.doc_meta[*i].file_path.as_str()))
            .map(|(i, doc_vec)| (i, query_vec.dot(doc_vec)))
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);

        scored
            .into_iter()
            .map(|(i, score)| (self.doc_meta[i].clone(), score))
            .collect()
    }
}

/// Tokenize text into lowercase unigrams + bigrams using word characters.
fn tokenize(text: &str) -> Vec<String> {
    // \w+ matches word characters (letters, digits, underscore)
    let re = Regex::new(r"\w+").expect("valid regex");
    let words: Vec<String> = re
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .collect();

    let mut tokens = words.clone();

    // Add bigrams
    for pair in words.windows(2) {
        tokens.push(format!("{} {}", pair[0], pair[1]));
    }

    tokens
}

/// Build a TF-IDF sparse vector from tokens, L2-normalized.
fn build_tfidf_vector(tokens: &[String], vocab: &HashMap<String, u32>, idf: &[f32]) -> SparseVec {
    if tokens.is_empty() {
        return SparseVec {
            indices: Vec::new(),
            values: Vec::new(),
        };
    }

    // Count term frequency
    let mut tf: HashMap<u32, u32> = HashMap::new();
    for token in tokens {
        if let Some(&idx) = vocab.get(token) {
            *tf.entry(idx).or_insert(0) += 1;
        }
    }

    // Build sparse vector with sublinear TF: 1 + ln(tf)
    let mut indices: Vec<u32> = tf.keys().copied().collect();
    indices.sort();

    let values: Vec<f32> = indices
        .iter()
        .map(|&idx| {
            let raw_tf = tf[&idx] as f32;
            let sublinear_tf = 1.0 + raw_tf.ln();
            sublinear_tf * idf[idx as usize]
        })
        .collect();

    let mut vec = SparseVec { indices, values };
    vec.l2_normalize();
    vec
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_meta(name: &str) -> DocMeta {
        DocMeta {
            file_path: "test.rs".to_string(),
            symbol_name: name.to_string(),
            symbol_type: "function".to_string(),
            signature: format!("fn {}()", name),
            start_line: 1,
            end_line: 5,
        }
    }

    #[test]
    fn test_tokenize_basic() {
        let tokens = tokenize("hello world");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"hello world".to_string()));
    }

    #[test]
    fn test_tokenize_lowercases() {
        let tokens = tokenize("Hello WORLD");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
    }

    #[test]
    fn test_tokenize_handles_underscores() {
        let tokens = tokenize("parse_file_content");
        assert!(tokens.contains(&"parse_file_content".to_string()));
    }

    #[test]
    fn test_tokenize_empty() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_sparse_vec_dot_identical() {
        let a = SparseVec {
            indices: vec![0, 1],
            values: vec![1.0, 0.0],
        };
        let b = SparseVec {
            indices: vec![0, 1],
            values: vec![1.0, 0.0],
        };
        assert!((a.dot(&b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_sparse_vec_dot_orthogonal() {
        let a = SparseVec {
            indices: vec![0],
            values: vec![1.0],
        };
        let b = SparseVec {
            indices: vec![1],
            values: vec![1.0],
        };
        assert!(a.dot(&b).abs() < 1e-6);
    }

    #[test]
    fn test_sparse_vec_l2_normalize() {
        let mut v = SparseVec {
            indices: vec![0, 1],
            values: vec![3.0, 4.0],
        };
        v.l2_normalize();
        let norm: f32 = v.values.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_sparse_vec_l2_normalize_zero() {
        let mut v = SparseVec {
            indices: vec![0],
            values: vec![0.0],
        };
        v.l2_normalize();
        assert_eq!(v.values[0], 0.0);
    }

    #[test]
    fn test_fit_empty_corpus() {
        let engine = TfidfEngine::fit(&[]);
        assert!(engine.doc_vectors.is_empty());
        assert!(engine.doc_meta.is_empty());
    }

    #[test]
    fn test_fit_single_doc() {
        let docs = vec![("fn parse_file() {}".to_string(), make_meta("parse_file"))];
        let engine = TfidfEngine::fit(&docs);
        assert_eq!(engine.doc_vectors.len(), 1);
        assert_eq!(engine.doc_meta.len(), 1);
        assert!(!engine.vocab.is_empty());
    }

    #[test]
    fn test_search_returns_results() {
        let docs = vec![
            (
                "fn parse_source_code(source: &str) { tree_sitter_parse(source) }".to_string(),
                make_meta("parse_source_code"),
            ),
            (
                "fn format_output(data: &Data) { println!(\"{}\", data) }".to_string(),
                make_meta("format_output"),
            ),
            (
                "fn compute_hash(input: &[u8]) -> Hash { blake3::hash(input) }".to_string(),
                make_meta("compute_hash"),
            ),
        ];

        let engine = TfidfEngine::fit(&docs);
        let results = engine.search("parse source code", 2);

        assert!(!results.is_empty());
        assert_eq!(results[0].0.symbol_name, "parse_source_code");
    }

    #[test]
    fn test_search_respects_top_k() {
        let docs: Vec<_> = (0..10)
            .map(|i| {
                (
                    format!("fn func_{i}() {{ code_{i} }}"),
                    make_meta(&format!("func_{i}")),
                )
            })
            .collect();

        let engine = TfidfEngine::fit(&docs);
        let results = engine.search("func", 3);
        assert!(results.len() <= 3);
    }

    #[test]
    fn test_search_in_files() {
        let docs = vec![
            ("fn parse_file() {}".to_string(), {
                let mut m = make_meta("parse_file");
                m.file_path = "src/parser.rs".to_string();
                m
            }),
            ("fn parse_config() {}".to_string(), {
                let mut m = make_meta("parse_config");
                m.file_path = "src/config.rs".to_string();
                m
            }),
        ];

        let engine = TfidfEngine::fit(&docs);
        let results = engine.search_in_files("parse", &["src/parser.rs"], 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.file_path, "src/parser.rs");
    }

    #[test]
    fn test_search_empty_query() {
        let docs = vec![("fn foo() {}".to_string(), make_meta("foo"))];
        let engine = TfidfEngine::fit(&docs);
        // Punctuation-only query produces no word tokens
        let results = engine.search("???", 10);
        // Should not panic; scores will be 0
        for (_, score) in &results {
            assert!(*score == 0.0);
        }
    }

    #[test]
    fn test_idf_discriminates() {
        // "common" appears in all docs, "rare" in only one.
        // Searching for "rare" should rank the doc containing it first.
        let docs = vec![
            ("common word alpha".to_string(), make_meta("a")),
            ("common word beta".to_string(), make_meta("b")),
            ("common word rare gamma".to_string(), make_meta("c")),
        ];

        let engine = TfidfEngine::fit(&docs);
        let results = engine.search("rare", 3);

        assert_eq!(results[0].0.symbol_name, "c");
    }

    #[test]
    fn test_normalized_vectors_have_unit_length() {
        let docs = vec![
            ("fn alpha() { code }".to_string(), make_meta("alpha")),
            ("fn beta() { more code }".to_string(), make_meta("beta")),
        ];

        let engine = TfidfEngine::fit(&docs);
        for vec in &engine.doc_vectors {
            if !vec.values.is_empty() {
                let norm: f32 = vec.values.iter().map(|v| v * v).sum::<f32>().sqrt();
                assert!(
                    (norm - 1.0).abs() < 1e-5,
                    "vector not unit length: {}",
                    norm
                );
            }
        }
    }

    #[test]
    fn test_vocab_bounded_by_max() {
        // Create enough unique terms to exceed MAX_VOCAB
        let mut text = String::new();
        for i in 0..12_000 {
            text.push_str(&format!("uniqueterm{} ", i));
        }
        let docs = vec![(text, make_meta("big"))];
        let engine = TfidfEngine::fit(&docs);
        assert!(engine.vocab.len() <= MAX_VOCAB);
    }
}
