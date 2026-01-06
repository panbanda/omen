//! Source file representation.

use std::path::{Path, PathBuf};

use super::{Language, Result};

/// A source file with its content loaded.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Path to the file.
    pub path: PathBuf,
    /// Detected language.
    pub language: Language,
    /// File content as bytes.
    pub content: Vec<u8>,
}

impl SourceFile {
    /// Load a source file from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let language = Language::detect(path).ok_or_else(|| super::Error::UnsupportedLanguage {
            path: path.to_path_buf(),
        })?;
        let content = std::fs::read(path)?;

        Ok(Self {
            path: path.to_path_buf(),
            language,
            content,
        })
    }

    /// Create from existing content.
    pub fn from_content(path: impl Into<PathBuf>, language: Language, content: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            language,
            content,
        }
    }

    /// Get content as string (lossy conversion).
    pub fn content_str(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.content)
    }

    /// Count lines of code (non-empty, non-comment lines).
    pub fn lines_of_code(&self) -> usize {
        let content = self.content_str();
        content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !is_comment_line(trimmed, self.language)
            })
            .count()
    }

    /// Count total lines.
    pub fn total_lines(&self) -> usize {
        self.content_str().lines().count()
    }
}

/// Check if a line is a comment (simple heuristic).
fn is_comment_line(line: &str, lang: Language) -> bool {
    match lang {
        Language::Go
        | Language::Rust
        | Language::Java
        | Language::CSharp
        | Language::C
        | Language::Cpp
        | Language::JavaScript
        | Language::TypeScript
        | Language::Tsx
        | Language::Jsx
        | Language::Php => {
            line.starts_with("//") || line.starts_with("/*") || line.starts_with('*')
        }
        Language::Python | Language::Ruby | Language::Bash => {
            line.starts_with('#') || line.starts_with("'''") || line.starts_with("\"\"\"")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_file_from_content() {
        let content = b"fn main() {\n    println!(\"Hello\");\n}\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        assert_eq!(file.language, Language::Rust);
        assert_eq!(file.total_lines(), 3);
        assert_eq!(file.lines_of_code(), 3);
    }

    #[test]
    fn test_lines_of_code_excludes_comments() {
        let content = b"// This is a comment\nfn main() {}\n// Another comment\n".to_vec();
        let file = SourceFile::from_content("test.rs", Language::Rust, content);

        assert_eq!(file.total_lines(), 3);
        assert_eq!(file.lines_of_code(), 1);
    }
}
