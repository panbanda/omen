//! Error types for the omen library.

use std::path::PathBuf;

use thiserror::Error;

/// Result type alias using omen's Error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during code analysis.
#[derive(Error, Debug)]
pub enum Error {
    /// I/O error reading or writing files.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// File not found.
    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    /// Unsupported language for the given file.
    #[error("Unsupported language for file: {path}")]
    UnsupportedLanguage { path: PathBuf },

    /// Parse error from tree-sitter.
    #[error("Parse error in {path}: {message}")]
    Parse { path: PathBuf, message: String },

    /// Git operation error.
    #[error("Git error: {0}")]
    Git(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// TOML parsing error.
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    /// Analysis-specific error.
    #[error("Analysis error: {message}")]
    Analysis { message: String },

    /// Invalid argument.
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Remote repository error.
    #[error("Remote repository error: {0}")]
    Remote(String),

    /// MCP server error.
    #[error("MCP server error: {0}")]
    Mcp(String),

    /// Threshold violation (for CI/CD integration).
    #[error("Threshold violation: {message}")]
    ThresholdViolation { message: String, score: f64 },

    /// Template rendering error.
    #[error("Template error: {0}")]
    Template(String),
}

impl From<minijinja::Error> for Error {
    fn from(err: minijinja::Error) -> Self {
        Self::Template(err.to_string())
    }
}

impl Error {
    /// Create a new analysis error.
    pub fn analysis(message: impl Into<String>) -> Self {
        Self::Analysis {
            message: message.into(),
        }
    }

    /// Create a new git error.
    pub fn git(message: impl Into<String>) -> Self {
        Self::Git(message.into())
    }

    /// Create a new config error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// Create a threshold violation error.
    pub fn threshold_violation(message: impl Into<String>, score: f64) -> Self {
        Self::ThresholdViolation {
            message: message.into(),
            score,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::analysis("test error");
        assert_eq!(err.to_string(), "Analysis error: test error");

        let err = Error::FileNotFound {
            path: PathBuf::from("test.rs"),
        };
        assert_eq!(err.to_string(), "File not found: test.rs");
    }

    #[test]
    fn test_threshold_violation() {
        let err = Error::threshold_violation("Score below minimum", 45.0);
        match err {
            Error::ThresholdViolation { message, score } => {
                assert_eq!(message, "Score below minimum");
                assert!((score - 45.0).abs() < f64::EPSILON);
            }
            _ => panic!("Expected ThresholdViolation"),
        }
    }
}
