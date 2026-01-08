//! Embedding engine for generating vector embeddings from code.
//!
//! Uses ONNX Runtime with the all-MiniLM-L6-v2 model for local inference.

use std::path::Path;
use std::sync::Mutex;

use ndarray::Axis;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::core::{Error, Result};
use crate::parser::FunctionNode;

use super::model::ModelManager;

/// Embedding dimension for all-MiniLM-L6-v2.
pub const EMBEDDING_DIM: usize = 384;

/// Maximum sequence length for the model.
const MAX_SEQ_LENGTH: usize = 256;

/// Embedding engine for generating code embeddings.
pub struct EmbeddingEngine {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl EmbeddingEngine {
    /// Create a new embedding engine, downloading the model if necessary.
    pub fn new() -> Result<Self> {
        let manager = ModelManager::new()?;
        manager.ensure_model()?;
        Self::from_paths(&manager.model_path(), &manager.tokenizer_path())
    }

    /// Create an embedding engine from explicit model and tokenizer paths.
    pub fn from_paths(model_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        let session = Session::builder()
            .map_err(|e| Error::analysis(format!("Failed to create ONNX session builder: {}", e)))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| Error::analysis(format!("Failed to set optimization level: {}", e)))?
            .with_intra_threads(4)
            .map_err(|e| Error::analysis(format!("Failed to set intra threads: {}", e)))?
            .commit_from_file(model_path)
            .map_err(|e| {
                Error::analysis(format!(
                    "Failed to load ONNX model from {}: {}",
                    model_path.display(),
                    e
                ))
            })?;

        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|e| {
            Error::analysis(format!(
                "Failed to load tokenizer from {}: {}",
                tokenizer_path.display(),
                e
            ))
        })?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    /// Generate an embedding for a single text.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()])?;
        Ok(embeddings.into_iter().next().unwrap())
    }

    /// Generate embeddings for a batch of texts.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Tokenize all texts
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| Error::analysis(format!("Tokenization failed: {}", e)))?;

        let batch_size = encodings.len();

        // Prepare input tensors
        let mut input_ids: Vec<i64> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);
        let mut attention_mask: Vec<i64> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);
        let mut token_type_ids: Vec<i64> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let type_ids = encoding.get_type_ids();

            // Truncate or pad to MAX_SEQ_LENGTH
            for i in 0..MAX_SEQ_LENGTH {
                if i < ids.len() {
                    input_ids.push(ids[i] as i64);
                    attention_mask.push(mask[i] as i64);
                    token_type_ids.push(type_ids[i] as i64);
                } else {
                    input_ids.push(0);
                    attention_mask.push(0);
                    token_type_ids.push(0);
                }
            }
        }

        // Create input tensors using Tensor::from_array with (shape, data) tuple
        let shape = [batch_size as i64, MAX_SEQ_LENGTH as i64];

        let input_ids_tensor = Tensor::from_array((shape, input_ids.into_boxed_slice()))
            .map_err(|e| Error::analysis(format!("Failed to create input_ids tensor: {}", e)))?;

        let attention_mask_tensor = Tensor::from_array((shape, attention_mask.into_boxed_slice()))
            .map_err(|e| {
                Error::analysis(format!("Failed to create attention_mask tensor: {}", e))
            })?;

        let token_type_ids_tensor = Tensor::from_array((shape, token_type_ids.into_boxed_slice()))
            .map_err(|e| {
                Error::analysis(format!("Failed to create token_type_ids tensor: {}", e))
            })?;

        // Run inference with mutable session access through Mutex
        let mut session = self
            .session
            .lock()
            .map_err(|e| Error::analysis(format!("Failed to lock session: {}", e)))?;

        let outputs = session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor,
            ])
            .map_err(|e| Error::analysis(format!("ONNX inference failed: {}", e)))?;

        // Extract embeddings from output using try_extract_array (ndarray feature)
        let output_array = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| Error::analysis(format!("Failed to extract output tensor: {}", e)))?;

        // The output shape is [batch_size, seq_len, hidden_size]
        let output_shape = output_array.shape();
        let seq_len = output_shape[1];
        let hidden_size = output_shape[2];

        // Mean pooling with attention mask
        let mut embeddings = Vec::with_capacity(batch_size);

        for (batch_idx, encoding) in encodings.iter().enumerate() {
            let mask = encoding.get_attention_mask();
            let mask_sum: f32 = mask.iter().take(seq_len).map(|&x| x as f32).sum();

            let mut embedding = vec![0.0f32; hidden_size];

            // Get the slice for this batch item
            let batch_output = output_array.index_axis(Axis(0), batch_idx);

            for (seq_idx, &mask_val) in mask.iter().take(seq_len).enumerate() {
                if mask_val == 1 {
                    let seq_output = batch_output.index_axis(Axis(0), seq_idx);
                    for (emb_idx, emb_val) in embedding.iter_mut().enumerate() {
                        *emb_val += seq_output[emb_idx];
                    }
                }
            }

            // Normalize by mask sum
            if mask_sum > 0.0 {
                for val in &mut embedding {
                    *val /= mask_sum;
                }
            }

            // L2 normalize
            let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for val in &mut embedding {
                    *val /= norm;
                }
            }

            embeddings.push(embedding);
        }

        Ok(embeddings)
    }

    /// Generate an embedding for a function/symbol.
    pub fn embed_symbol(&self, func: &FunctionNode, source: &str) -> Result<Vec<f32>> {
        let text = format_symbol_text(func, source);
        self.embed(&text)
    }
}

/// Format a symbol for embedding generation.
pub fn format_symbol_text(func: &FunctionNode, source: &str) -> String {
    // Get function body from source by line numbers
    let lines: Vec<&str> = source.lines().collect();
    let start = (func.start_line as usize).saturating_sub(1);
    let end = (func.end_line as usize).min(lines.len());

    let body = if start < end && start < lines.len() {
        lines[start..end].join("\n")
    } else {
        func.signature.clone()
    };

    // Limit body length to avoid exceeding token limit
    let max_chars = 1500;
    if body.len() > max_chars {
        format!("{}\n{}", func.signature, &body[..max_chars])
    } else {
        body
    }
}

/// Compute cosine similarity between two embeddings.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_format_symbol_text() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 3,
            body: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        let source = "fn test_func() {\n    println!(\"hello\");\n}";
        let text = format_symbol_text(&func, source);
        assert!(text.contains("fn test_func()"));
        assert!(text.contains("println!"));
    }

    #[test]
    fn test_format_symbol_text_truncation() {
        let func = FunctionNode {
            name: "test_func".to_string(),
            start_line: 1,
            end_line: 1,
            body: None,
            is_exported: true,
            signature: "fn test_func()".to_string(),
        };
        // Create a very long source
        let source = "x".repeat(3000);
        let text = format_symbol_text(&func, &source);
        assert!(text.len() <= 1600); // signature + truncated body
    }
}
