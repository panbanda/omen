//! Model management for semantic search embeddings.
//!
//! Downloads and caches the all-MiniLM-L6-v2 ONNX model for local inference.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::{Error, Result};

/// URL for the all-MiniLM-L6-v2 ONNX model.
const MODEL_URL: &str =
    "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx";

/// URL for the tokenizer.json file.
const TOKENIZER_URL: &str =
    "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json";

/// Expected model version for cache invalidation.
const MODEL_VERSION: &str = "all-MiniLM-L6-v2-v1";

/// Model file name.
const MODEL_FILE: &str = "model.onnx";

/// Tokenizer file name.
const TOKENIZER_FILE: &str = "tokenizer.json";

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

    /// Get the path to the ONNX model file.
    pub fn model_path(&self) -> PathBuf {
        self.cache_dir.join(MODEL_FILE)
    }

    /// Get the path to the tokenizer file.
    pub fn tokenizer_path(&self) -> PathBuf {
        self.cache_dir.join(TOKENIZER_FILE)
    }

    /// Ensure the model and tokenizer are downloaded and cached.
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
            self.download_model()?;
            self.download_tokenizer()?;
            self.write_version()?;
        }

        Ok(())
    }

    /// Check if the cached model is valid and up-to-date.
    fn is_model_valid(&self) -> bool {
        let model_path = self.model_path();
        let tokenizer_path = self.tokenizer_path();
        let version_path = self.cache_dir.join(VERSION_FILE);

        if !model_path.exists() || !tokenizer_path.exists() || !version_path.exists() {
            return false;
        }

        // Check version
        match fs::read_to_string(&version_path) {
            Ok(version) => version.trim() == MODEL_VERSION,
            Err(_) => false,
        }
    }

    /// Download the ONNX model.
    fn download_model(&self) -> Result<()> {
        eprintln!("Downloading embedding model...");
        let model_path = self.model_path();
        download_file(MODEL_URL, &model_path)?;
        eprintln!("Model downloaded to: {}", model_path.display());
        Ok(())
    }

    /// Download the tokenizer.
    fn download_tokenizer(&self) -> Result<()> {
        eprintln!("Downloading tokenizer...");
        let tokenizer_path = self.tokenizer_path();
        download_file(TOKENIZER_URL, &tokenizer_path)?;
        eprintln!("Tokenizer downloaded to: {}", tokenizer_path.display());
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

/// Download a file from a URL to a local path.
fn download_file(url: &str, path: &Path) -> Result<()> {
    let response = reqwest::blocking::get(url)
        .map_err(|e| Error::analysis(format!("Failed to download {}: {}", url, e)))?;

    if !response.status().is_success() {
        return Err(Error::analysis(format!(
            "Failed to download {}: HTTP {}",
            url,
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .map_err(|e| Error::analysis(format!("Failed to read response body: {}", e)))?;

    let mut file = fs::File::create(path)
        .map_err(|e| Error::analysis(format!("Failed to create file {}: {}", path.display(), e)))?;

    file.write_all(&bytes)
        .map_err(|e| Error::analysis(format!("Failed to write file {}: {}", path.display(), e)))?;

    Ok(())
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
        assert_eq!(manager.model_path(), temp_dir.path().join("model.onnx"));
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
        fs::write(temp_dir.path().join(VERSION_FILE), "wrong-version").unwrap();

        assert!(!manager.is_model_valid());
    }
}
