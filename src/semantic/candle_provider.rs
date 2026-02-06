//! Candle-based embedding provider using BAAI/bge-small-en-v1.5.
//!
//! Runs local inference using the candle ML framework.

use std::path::Path;
use std::sync::Mutex;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig, DTYPE};
use tokenizers::Tokenizer;

use crate::core::{Error, Result};

use super::model::ModelManager;
use super::provider::{EmbeddingProvider, BGE_SMALL_EMBEDDING_DIM};

/// Maximum sequence length for the model.
const MAX_SEQ_LENGTH: usize = 512;

/// Candle-based embedding provider using BAAI/bge-small-en-v1.5.
pub struct CandleProvider {
    model: Mutex<BertModel>,
    tokenizer: Tokenizer,
    device: Device,
}

impl CandleProvider {
    /// Create a new candle provider, downloading the model if necessary.
    pub fn new() -> Result<Self> {
        let manager = ModelManager::new()?;
        manager.ensure_model()?;
        Self::from_paths(
            &manager.model_path(),
            &manager.tokenizer_path(),
            &manager.config_path(),
        )
    }

    /// Create a provider from explicit model, tokenizer, and config paths.
    pub fn from_paths(
        model_path: &Path,
        tokenizer_path: &Path,
        config_path: &Path,
    ) -> Result<Self> {
        let device = Device::Cpu;

        // Load config
        let config_str = std::fs::read_to_string(config_path).map_err(|e| {
            Error::analysis(format!(
                "Failed to read config from {}: {}",
                config_path.display(),
                e
            ))
        })?;
        let config: BertConfig = serde_json::from_str(&config_str)
            .map_err(|e| Error::analysis(format!("Failed to parse config: {}", e)))?;

        // Load model weights
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[model_path], DTYPE, &device).map_err(|e| {
                Error::analysis(format!(
                    "Failed to load model from {}: {}",
                    model_path.display(),
                    e
                ))
            })?
        };

        let model = BertModel::load(vb, &config)
            .map_err(|e| Error::analysis(format!("Failed to build BERT model: {}", e)))?;

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|e| {
            Error::analysis(format!(
                "Failed to load tokenizer from {}: {}",
                tokenizer_path.display(),
                e
            ))
        })?;

        Ok(Self {
            model: Mutex::new(model),
            tokenizer,
            device,
        })
    }

    /// Mean pool the hidden states with attention mask.
    fn mean_pool(&self, hidden_states: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
        // Expand attention mask to match hidden states dimensions
        let mask = attention_mask
            .unsqueeze(2)
            .map_err(|e| Error::analysis(format!("Failed to unsqueeze mask: {}", e)))?
            .to_dtype(DType::F32)
            .map_err(|e| Error::analysis(format!("Failed to convert mask dtype: {}", e)))?;

        // Mask the hidden states
        let masked = hidden_states
            .broadcast_mul(&mask)
            .map_err(|e| Error::analysis(format!("Failed to apply mask: {}", e)))?;

        // Sum along sequence dimension
        let summed = masked
            .sum(1)
            .map_err(|e| Error::analysis(format!("Failed to sum hidden states: {}", e)))?;

        // Sum mask for averaging
        let mask_sum = mask
            .sum(1)
            .map_err(|e| Error::analysis(format!("Failed to sum mask: {}", e)))?
            .clamp(1e-9, f64::MAX)
            .map_err(|e| Error::analysis(format!("Failed to clamp mask: {}", e)))?;

        // Average
        let pooled = summed
            .broadcast_div(&mask_sum)
            .map_err(|e| Error::analysis(format!("Failed to divide for mean: {}", e)))?;

        Ok(pooled)
    }

    /// L2 normalize embeddings.
    fn l2_normalize(&self, embeddings: &Tensor) -> Result<Tensor> {
        let norm = embeddings
            .sqr()
            .map_err(|e| Error::analysis(format!("Failed to square: {}", e)))?
            .sum_keepdim(1)
            .map_err(|e| Error::analysis(format!("Failed to sum for norm: {}", e)))?
            .sqrt()
            .map_err(|e| Error::analysis(format!("Failed to sqrt: {}", e)))?
            .clamp(1e-12, f64::MAX)
            .map_err(|e| Error::analysis(format!("Failed to clamp norm: {}", e)))?;

        let normalized = embeddings
            .broadcast_div(&norm)
            .map_err(|e| Error::analysis(format!("Failed to normalize: {}", e)))?;

        Ok(normalized)
    }
}

impl EmbeddingProvider for CandleProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| Error::analysis("embed_batch returned empty for single input"))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
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
        let mut input_ids: Vec<u32> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);
        let mut attention_mask: Vec<u32> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);
        let mut token_type_ids: Vec<u32> = Vec::with_capacity(batch_size * MAX_SEQ_LENGTH);

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let type_ids = encoding.get_type_ids();

            // Truncate or pad to MAX_SEQ_LENGTH
            for i in 0..MAX_SEQ_LENGTH {
                if i < ids.len() {
                    input_ids.push(ids[i]);
                    attention_mask.push(mask[i]);
                    token_type_ids.push(type_ids[i]);
                } else {
                    input_ids.push(0);
                    attention_mask.push(0);
                    token_type_ids.push(0);
                }
            }
        }

        // Create tensors
        let input_ids_tensor =
            Tensor::from_vec(input_ids, (batch_size, MAX_SEQ_LENGTH), &self.device).map_err(
                |e| Error::analysis(format!("Failed to create input_ids tensor: {}", e)),
            )?;

        let attention_mask_tensor =
            Tensor::from_vec(attention_mask, (batch_size, MAX_SEQ_LENGTH), &self.device).map_err(
                |e| Error::analysis(format!("Failed to create attention_mask tensor: {}", e)),
            )?;

        let token_type_ids_tensor =
            Tensor::from_vec(token_type_ids, (batch_size, MAX_SEQ_LENGTH), &self.device).map_err(
                |e| Error::analysis(format!("Failed to create token_type_ids tensor: {}", e)),
            )?;

        // Run inference
        let model = self
            .model
            .lock()
            .map_err(|e| Error::analysis(format!("Failed to lock model: {}", e)))?;

        let hidden_states = model
            .forward(
                &input_ids_tensor,
                &token_type_ids_tensor,
                Some(&attention_mask_tensor),
            )
            .map_err(|e| Error::analysis(format!("Model forward pass failed: {}", e)))?;

        // Mean pooling
        let attention_mask_f32 = attention_mask_tensor
            .to_dtype(DType::F32)
            .map_err(|e| Error::analysis(format!("Failed to convert mask: {}", e)))?;
        let pooled = self.mean_pool(&hidden_states, &attention_mask_f32)?;

        // L2 normalize
        let normalized = self.l2_normalize(&pooled)?;

        // Convert to Vec<Vec<f32>>
        let embeddings_flat: Vec<f32> = normalized
            .flatten_all()
            .map_err(|e| Error::analysis(format!("Failed to flatten: {}", e)))?
            .to_vec1()
            .map_err(|e| Error::analysis(format!("Failed to convert to vec: {}", e)))?;

        let embeddings: Vec<Vec<f32>> = embeddings_flat
            .chunks(BGE_SMALL_EMBEDDING_DIM)
            .map(|chunk| chunk.to_vec())
            .collect();

        Ok(embeddings)
    }

    fn embedding_dim(&self) -> usize {
        BGE_SMALL_EMBEDDING_DIM
    }

    fn name(&self) -> &str {
        "candle (bge-small-en-v1.5)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_dim() {
        assert_eq!(BGE_SMALL_EMBEDDING_DIM, 384);
    }

    #[test]
    fn test_max_seq_length() {
        assert_eq!(MAX_SEQ_LENGTH, 512);
    }
}
