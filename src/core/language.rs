//! Language detection and enumeration.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Supported programming languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Go,
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Tsx,
    Jsx,
    Java,
    C,
    Cpp,
    CSharp,
    Ruby,
    Php,
    Bash,
}

impl Language {
    /// Detect language from file path based on extension.
    pub fn detect(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;
        Self::from_extension(extension)
    }

    /// Get language from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "go" => Some(Self::Go),
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "tsx" => Some(Self::Tsx),
            "jsx" => Some(Self::Jsx),
            "java" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(Self::Cpp),
            "cs" => Some(Self::CSharp),
            "rb" | "rake" | "gemspec" => Some(Self::Ruby),
            "php" => Some(Self::Php),
            "sh" | "bash" => Some(Self::Bash),
            _ => None,
        }
    }

    /// Get the display name for the language.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Go => "Go",
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::TypeScript => "TypeScript",
            Self::JavaScript => "JavaScript",
            Self::Tsx => "TSX",
            Self::Jsx => "JSX",
            Self::Java => "Java",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::CSharp => "C#",
            Self::Ruby => "Ruby",
            Self::Php => "PHP",
            Self::Bash => "Bash",
        }
    }

    /// Check if the language supports classes/OOP constructs.
    pub fn supports_classes(&self) -> bool {
        matches!(
            self,
            Self::Java
                | Self::CSharp
                | Self::TypeScript
                | Self::JavaScript
                | Self::Tsx
                | Self::Jsx
                | Self::Python
                | Self::Ruby
                | Self::Php
                | Self::Cpp
        )
    }

    /// Check if the language uses explicit imports.
    pub fn has_imports(&self) -> bool {
        !matches!(self, Self::C | Self::Cpp | Self::Bash)
    }

    /// Get common file patterns for this language.
    pub fn glob_patterns(&self) -> &'static [&'static str] {
        match self {
            Self::Go => &["**/*.go"],
            Self::Rust => &["**/*.rs"],
            Self::Python => &["**/*.py", "**/*.pyi"],
            Self::TypeScript => &["**/*.ts", "**/*.mts", "**/*.cts"],
            Self::JavaScript => &["**/*.js", "**/*.mjs", "**/*.cjs"],
            Self::Tsx => &["**/*.tsx"],
            Self::Jsx => &["**/*.jsx"],
            Self::Java => &["**/*.java"],
            Self::C => &["**/*.c", "**/*.h"],
            Self::Cpp => &[
                "**/*.cpp", "**/*.cc", "**/*.cxx", "**/*.hpp", "**/*.hxx", "**/*.hh",
            ],
            Self::CSharp => &["**/*.cs"],
            Self::Ruby => &["**/*.rb", "**/*.rake", "**/*.gemspec"],
            Self::Php => &["**/*.php"],
            Self::Bash => &["**/*.sh", "**/*.bash"],
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(Language::detect(Path::new("main.go")), Some(Language::Go));
        assert_eq!(Language::detect(Path::new("lib.rs")), Some(Language::Rust));
        assert_eq!(
            Language::detect(Path::new("script.py")),
            Some(Language::Python)
        );
        assert_eq!(
            Language::detect(Path::new("app.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::detect(Path::new("component.tsx")),
            Some(Language::Tsx)
        );
        assert_eq!(
            Language::detect(Path::new("Main.java")),
            Some(Language::Java)
        );
        assert_eq!(Language::detect(Path::new("file.c")), Some(Language::C));
        assert_eq!(Language::detect(Path::new("file.cpp")), Some(Language::Cpp));
        assert_eq!(
            Language::detect(Path::new("Program.cs")),
            Some(Language::CSharp)
        );
        assert_eq!(Language::detect(Path::new("app.rb")), Some(Language::Ruby));
        assert_eq!(
            Language::detect(Path::new("index.php")),
            Some(Language::Php)
        );
        assert_eq!(
            Language::detect(Path::new("script.sh")),
            Some(Language::Bash)
        );
        assert_eq!(Language::detect(Path::new("README.md")), None);
    }

    #[test]
    fn test_from_extension() {
        assert_eq!(Language::from_extension("go"), Some(Language::Go));
        assert_eq!(Language::from_extension("GO"), Some(Language::Go));
        assert_eq!(Language::from_extension("unknown"), None);
    }

    #[test]
    fn test_display_name() {
        assert_eq!(Language::Go.display_name(), "Go");
        assert_eq!(Language::Cpp.display_name(), "C++");
        assert_eq!(Language::CSharp.display_name(), "C#");
    }

    #[test]
    fn test_cpp_glob_patterns_include_hh() {
        let patterns = Language::Cpp.glob_patterns();
        assert!(
            patterns.contains(&"**/*.hh"),
            "C++ glob patterns must include .hh header extension"
        );
    }

    #[test]
    fn test_supports_classes() {
        assert!(Language::Java.supports_classes());
        assert!(Language::Python.supports_classes());
        assert!(!Language::Go.supports_classes());
        assert!(!Language::C.supports_classes());
    }
}
