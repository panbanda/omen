//! Model management for semantic search embeddings.
//!
//! Downloads and caches the all-MiniLM-L6-v2 model for local candle inference.

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::{Error, Result};

/// Hugging Face repository for all-MiniLM-L6-v2.
pub const MODEL_REPO: &str = "sentence-transformers/all-MiniLM-L6-v2";

/// Expected model version for cache invalidation.
const MODEL_VERSION: &str = "all-MiniLM-L6-v2-candle-v1";

/// Model weights file name.
const MODEL_FILE: &str = "model.safetensors";

/// Tokenizer file name.
const TOKENIZER_FILE: &str = "tokenizer.json";

/// Config file name.
const CONFIG_FILE: &str = "config.json";

/// Version file name.
const VERSION_FILE: &str = "version.txt";

/// Model manager for downloading and caching the embedding model.
pub struct ModelManager {
    cache_dir: PathBuf,
}

impl ModelManager {
    /// Create a new model manager with the default cache directory.
    pub fn new() -> Result<Self> {
        let cache_dir = Self::default_cache_dir()?;
        Ok(Self { cache_dir })
    }

    /// Create a model manager with a custom cache directory.
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Get the default cache directory (~/.cache/omen/models/).
    fn default_cache_dir() -> Result<PathBuf> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| Error::config("Could not determine cache directory"))?
            .join("omen")
            .join("models");
        Ok(cache_dir)
    }

    /// Get the cache directory.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Get the path to the model weights file.
    pub fn model_path(&self) -> PathBuf {
        self.cache_dir.join(MODEL_FILE)
    }

    /// Get the path to the tokenizer file.
    pub fn tokenizer_path(&self) -> PathBuf {
        self.cache_dir.join(TOKENIZER_FILE)
    }

    /// Get the path to the config file.
    pub fn config_path(&self) -> PathBuf {
        self.cache_dir.join(CONFIG_FILE)
    }

    /// Ensure the model, tokenizer, and config are downloaded and cached.
    pub fn ensure_model(&self) -> Result<()> {
        fs::create_dir_all(&self.cache_dir).map_err(|e| {
            Error::analysis(format!(
                "Failed to create cache directory {}: {}",
                self.cache_dir.display(),
                e
            ))
        })?;

        // Check if we need to download/update
        if !self.is_model_valid() {
            self.download_model_files()?;
            self.write_version()?;
        }

        Ok(())
    }

    /// Check if the cached model is valid and up-to-date.
    fn is_model_valid(&self) -> bool {
        let model_path = self.model_path();
        let tokenizer_path = self.tokenizer_path();
        let config_path = self.config_path();
        let version_path = self.cache_dir.join(VERSION_FILE);

        if !model_path.exists()
            || !tokenizer_path.exists()
            || !config_path.exists()
            || !version_path.exists()
        {
            return false;
        }

        // Check version
        match fs::read_to_string(&version_path) {
            Ok(version) => version.trim() == MODEL_VERSION,
            Err(_) => false,
        }
    }

    /// Download model files from Hugging Face Hub.
    fn download_model_files(&self) -> Result<()> {
        eprintln!("Downloading embedding model from Hugging Face Hub...");
        eprintln!("Repository: {}", MODEL_REPO);

        // Use hf-hub to download files
        let api = hf_hub::api::sync::Api::new().map_err(|e| {
            Error::analysis(format!("Failed to initialize Hugging Face Hub API: {}", e))
        })?;

        let repo = api.model(MODEL_REPO.to_string());

        // Download model weights (safetensors format)
        eprintln!("Downloading model weights...");
        let model_src = repo
            .get(MODEL_FILE)
            .map_err(|e| Error::analysis(format!("Failed to download model weights: {}", e)))?;
        fs::copy(&model_src, self.model_path())
            .map_err(|e| Error::analysis(format!("Failed to copy model weights: {}", e)))?;

        // Download tokenizer
        eprintln!("Downloading tokenizer...");
        let tokenizer_src = repo
            .get(TOKENIZER_FILE)
            .map_err(|e| Error::analysis(format!("Failed to download tokenizer: {}", e)))?;
        fs::copy(&tokenizer_src, self.tokenizer_path())
            .map_err(|e| Error::analysis(format!("Failed to copy tokenizer: {}", e)))?;

        // Download config
        eprintln!("Downloading config...");
        let config_src = repo
            .get(CONFIG_FILE)
            .map_err(|e| Error::analysis(format!("Failed to download config: {}", e)))?;
        fs::copy(&config_src, self.config_path())
            .map_err(|e| Error::analysis(format!("Failed to copy config: {}", e)))?;

        eprintln!("Model downloaded to: {}", self.cache_dir.display());
        Ok(())
    }

    /// Write the version file for cache invalidation.
    fn write_version(&self) -> Result<()> {
        let version_path = self.cache_dir.join(VERSION_FILE);
        fs::write(&version_path, MODEL_VERSION).map_err(|e| {
            Error::analysis(format!(
                "Failed to write version file {}: {}",
                version_path.display(),
                e
            ))
        })?;
        Ok(())
    }
}

impl Default for ModelManager {
    fn default() -> Self {
        Self::new().expect("Failed to create model manager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_model_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::with_cache_dir(temp_dir.path().to_path_buf());
        assert_eq!(manager.cache_dir, temp_dir.path());
    }

    #[test]
    fn test_model_path() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::with_cache_dir(temp_dir.path().to_path_buf());
        assert_eq!(
            manager.model_path(),
            temp_dir.path().join("model.safetensors")
        );
    }

    #[test]
    fn test_tokenizer_path() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::with_cache_dir(temp_dir.path().to_path_buf());
        assert_eq!(
            manager.tokenizer_path(),
            temp_dir.path().join("tokenizer.json")
        );
    }

    #[test]
    fn test_config_path() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::with_cache_dir(temp_dir.path().to_path_buf());
        assert_eq!(manager.config_path(), temp_dir.path().join("config.json"));
    }

    #[test]
    fn test_is_model_valid_missing_files() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::with_cache_dir(temp_dir.path().to_path_buf());
        assert!(!manager.is_model_valid());
    }

    #[test]
    fn test_is_model_valid_with_files() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::with_cache_dir(temp_dir.path().to_path_buf());

        // Create dummy files
        fs::write(manager.model_path(), "dummy").unwrap();
        fs::write(manager.tokenizer_path(), "dummy").unwrap();
        fs::write(manager.config_path(), "dummy").unwrap();
        fs::write(temp_dir.path().join(VERSION_FILE), MODEL_VERSION).unwrap();

        assert!(manager.is_model_valid());
    }

    #[test]
    fn test_is_model_valid_wrong_version() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ModelManager::with_cache_dir(temp_dir.path().to_path_buf());

        // Create dummy files with wrong version
        fs::write(manager.model_path(), "dummy").unwrap();
        fs::write(manager.tokenizer_path(), "dummy").unwrap();
        fs::write(manager.config_path(), "dummy").unwrap();
        fs::write(temp_dir.path().join(VERSION_FILE), "wrong-version").unwrap();

        assert!(!manager.is_model_valid());
    }
}
