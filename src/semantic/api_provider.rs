//! Third-party API embedding providers.
//!
//! Supports Ollama (local), OpenAI, Cohere, and Voyage AI embeddings APIs.

use std::env;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{Error, Result};

use super::provider::EmbeddingProvider;

/// Ollama embeddings API provider (local).
pub struct OllamaProvider {
    url: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl OllamaProvider {
    /// Create a new Ollama provider.
    pub fn new(url: String, model: String) -> Self {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");
        Self { url, model, client }
    }

    fn embedding_dim_for_model(&self) -> usize {
        match self.model.as_str() {
            "bge-m3" => 1024,
            "nomic-embed-text" => 768,
            "mxbai-embed-large" => 1024,
            "all-minilm" => 384,
            "bge-small" | "snowflake-arctic-embed:xs" => 384,
            _ => 384, // Default to small model dimensions
        }
    }
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct OllamaResponse {
    embeddings: Vec<Vec<f32>>,
}

impl EmbeddingProvider for OllamaProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| Error::analysis("Ollama returned empty embeddings for single input"))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = OllamaRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };

        let response = self
            .client
            .post(format!("{}/api/embed", self.url))
            .json(&request)
            .send()
            .map_err(|e| Error::analysis(format!("Ollama API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(Error::analysis(format!(
                "Ollama API error ({}): {}",
                status, body
            )));
        }

        let response: OllamaResponse = response
            .json()
            .map_err(|e| Error::analysis(format!("Failed to parse Ollama response: {}", e)))?;

        Ok(response.embeddings)
    }

    fn embedding_dim(&self) -> usize {
        self.embedding_dim_for_model()
    }

    fn name(&self) -> &str {
        "ollama"
    }
}

/// OpenAI embeddings API provider.
pub struct OpenAIProvider {
    api_key: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl OpenAIProvider {
    /// Create a new OpenAI provider.
    pub fn new(api_key: Option<String>, model: String) -> Result<Self> {
        let api_key = api_key
            .or_else(|| env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| {
                Error::config(
                    "OpenAI API key not provided. Set OPENAI_API_KEY environment variable or provide api_key in config.",
                )
            })?;

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| Error::analysis(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            api_key,
            model,
            client,
        })
    }

    fn embedding_dim_for_model(&self) -> usize {
        match self.model.as_str() {
            "text-embedding-3-small" => 1536,
            "text-embedding-3-large" => 3072,
            "text-embedding-ada-002" => 1536,
            _ => 1536, // Default to small model dimensions
        }
    }
}

#[derive(Serialize)]
struct OpenAIRequest {
    input: Vec<String>,
    model: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    data: Vec<OpenAIEmbedding>,
}

#[derive(Deserialize)]
struct OpenAIEmbedding {
    embedding: Vec<f32>,
}

impl EmbeddingProvider for OpenAIProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()])?;
        embeddings.into_iter().next().ok_or_else(|| {
            crate::core::Error::analysis("embed_batch returned empty for single input")
        })
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = OpenAIRequest {
            input: texts.to_vec(),
            model: self.model.clone(),
        };

        let response = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .map_err(|e| Error::analysis(format!("OpenAI API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(Error::analysis(format!(
                "OpenAI API error ({}): {}",
                status, body
            )));
        }

        let response: OpenAIResponse = response
            .json()
            .map_err(|e| Error::analysis(format!("Failed to parse OpenAI response: {}", e)))?;

        Ok(response.data.into_iter().map(|e| e.embedding).collect())
    }

    fn embedding_dim(&self) -> usize {
        self.embedding_dim_for_model()
    }

    fn name(&self) -> &str {
        "openai"
    }
}

/// Cohere embeddings API provider.
pub struct CohereProvider {
    api_key: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl CohereProvider {
    /// Create a new Cohere provider.
    pub fn new(api_key: Option<String>, model: String) -> Result<Self> {
        let api_key = api_key
            .or_else(|| env::var("COHERE_API_KEY").ok())
            .ok_or_else(|| {
                Error::config(
                    "Cohere API key not provided. Set COHERE_API_KEY environment variable or provide api_key in config.",
                )
            })?;

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| Error::analysis(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            api_key,
            model,
            client,
        })
    }
}

#[derive(Serialize)]
struct CohereRequest {
    texts: Vec<String>,
    model: String,
    input_type: String,
    truncate: String,
}

#[derive(Deserialize)]
struct CohereResponse {
    embeddings: Vec<Vec<f32>>,
}

impl EmbeddingProvider for CohereProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()])?;
        embeddings.into_iter().next().ok_or_else(|| {
            crate::core::Error::analysis("embed_batch returned empty for single input")
        })
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = CohereRequest {
            texts: texts.to_vec(),
            model: self.model.clone(),
            input_type: "search_document".to_string(),
            truncate: "END".to_string(),
        };

        let response = self
            .client
            .post("https://api.cohere.ai/v1/embed")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .map_err(|e| Error::analysis(format!("Cohere API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(Error::analysis(format!(
                "Cohere API error ({}): {}",
                status, body
            )));
        }

        let response: CohereResponse = response
            .json()
            .map_err(|e| Error::analysis(format!("Failed to parse Cohere response: {}", e)))?;

        Ok(response.embeddings)
    }

    fn embedding_dim(&self) -> usize {
        match self.model.as_str() {
            "embed-english-v3.0" => 1024,
            "embed-multilingual-v3.0" => 1024,
            "embed-english-light-v3.0" => 384,
            "embed-multilingual-light-v3.0" => 384,
            _ => 1024, // Default to v3 dimensions
        }
    }

    fn name(&self) -> &str {
        "cohere"
    }
}

/// Voyage AI embeddings API provider.
pub struct VoyageProvider {
    api_key: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl VoyageProvider {
    /// Create a new Voyage provider.
    pub fn new(api_key: Option<String>, model: String) -> Result<Self> {
        let api_key = api_key
            .or_else(|| env::var("VOYAGE_API_KEY").ok())
            .ok_or_else(|| {
                Error::config(
                    "Voyage API key not provided. Set VOYAGE_API_KEY environment variable or provide api_key in config.",
                )
            })?;

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| Error::analysis(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            api_key,
            model,
            client,
        })
    }
}

#[derive(Serialize)]
struct VoyageRequest {
    input: Vec<String>,
    model: String,
    input_type: String,
}

#[derive(Deserialize)]
struct VoyageResponse {
    data: Vec<VoyageEmbedding>,
}

#[derive(Deserialize)]
struct VoyageEmbedding {
    embedding: Vec<f32>,
}

impl EmbeddingProvider for VoyageProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()])?;
        embeddings.into_iter().next().ok_or_else(|| {
            crate::core::Error::analysis("embed_batch returned empty for single input")
        })
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = VoyageRequest {
            input: texts.to_vec(),
            model: self.model.clone(),
            input_type: "document".to_string(),
        };

        let response = self
            .client
            .post("https://api.voyageai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .map_err(|e| Error::analysis(format!("Voyage API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(Error::analysis(format!(
                "Voyage API error ({}): {}",
                status, body
            )));
        }

        let response: VoyageResponse = response
            .json()
            .map_err(|e| Error::analysis(format!("Failed to parse Voyage response: {}", e)))?;

        Ok(response.data.into_iter().map(|e| e.embedding).collect())
    }

    fn embedding_dim(&self) -> usize {
        match self.model.as_str() {
            "voyage-code-2" => 1536,
            "voyage-2" => 1024,
            "voyage-large-2" => 1536,
            "voyage-lite-02-instruct" => 1024,
            _ => 1536, // Default to code-2 dimensions
        }
    }

    fn name(&self) -> &str {
        "voyage"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_embedding_dim() {
        let provider =
            OllamaProvider::new("http://localhost:11434".to_string(), "bge-m3".to_string());
        assert_eq!(provider.embedding_dim(), 1024);

        let provider = OllamaProvider::new(
            "http://localhost:11434".to_string(),
            "all-minilm".to_string(),
        );
        assert_eq!(provider.embedding_dim(), 384);
    }

    #[test]
    fn test_ollama_embed_empty_batch() {
        let provider =
            OllamaProvider::new("http://localhost:11434".to_string(), "bge-m3".to_string());
        let result = provider.embed_batch(&[]);
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_ollama_provider_name() {
        let provider =
            OllamaProvider::new("http://localhost:11434".to_string(), "bge-m3".to_string());
        assert_eq!(provider.name(), "ollama");
    }

    #[test]
    fn test_openai_embedding_dim() {
        // Can't create without API key, but we can test the model dim mapping
        assert_eq!(
            OpenAIProvider {
                api_key: "test".to_string(),
                model: "text-embedding-3-small".to_string(),
                client: reqwest::blocking::Client::new(),
            }
            .embedding_dim(),
            1536
        );
    }

    #[test]
    fn test_cohere_embedding_dim() {
        assert_eq!(
            CohereProvider {
                api_key: "test".to_string(),
                model: "embed-english-v3.0".to_string(),
                client: reqwest::blocking::Client::new(),
            }
            .embedding_dim(),
            1024
        );
    }

    #[test]
    fn test_voyage_embedding_dim() {
        assert_eq!(
            VoyageProvider {
                api_key: "test".to_string(),
                model: "voyage-code-2".to_string(),
                client: reqwest::blocking::Client::new(),
            }
            .embedding_dim(),
            1536
        );
    }

    #[test]
    fn test_openai_embed_empty_batch_returns_error() {
        let provider = OpenAIProvider {
            api_key: "test".to_string(),
            model: "text-embedding-3-small".to_string(),
            client: reqwest::blocking::Client::new(),
        };
        let result = provider.embed_batch(&[]);
        // Empty input should return Ok with empty vec, not panic
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_cohere_embed_empty_batch_returns_error() {
        let provider = CohereProvider {
            api_key: "test".to_string(),
            model: "embed-english-v3.0".to_string(),
            client: reqwest::blocking::Client::new(),
        };
        let result = provider.embed_batch(&[]);
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_voyage_embed_empty_batch_returns_error() {
        let provider = VoyageProvider {
            api_key: "test".to_string(),
            model: "voyage-code-2".to_string(),
            client: reqwest::blocking::Client::new(),
        };
        let result = provider.embed_batch(&[]);
        assert!(result.unwrap().is_empty());
    }
}
