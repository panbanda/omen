//! Core types and traits for code analysis.

mod analyzer;
mod content_source;
mod error;
mod file_set;
mod language;
pub mod progress;
mod source_file;
mod utils;

pub use analyzer::{AnalysisContext, AnalysisResult, Analyzer, Summary};
pub use content_source::{ContentSource, FilesystemSource, TreeSource};
pub use error::{Error, Result};
pub use file_set::FileSet;
pub use language::Language;
pub use progress::{create_progress, create_spinner, is_tty, ProgressBuilder, ProgressTracker};
pub use source_file::SourceFile;
pub use utils::{is_test_file, percentile};

#[cfg(test)]
mod shared_helper_tests {
    use super::{is_test_file, percentile};
    use std::path::Path;

    #[test]
    fn test_percentile_uses_rounded_len_minus_one_index() {
        let values: Vec<u32> = (1..=10).collect();
        assert_eq!(percentile(&values, 90.0), 9);
    }

    #[test]
    fn test_percentile_handles_empty_and_endpoints() {
        assert_eq!(percentile::<u32>(&[], 90.0), 0);
        assert_eq!(percentile(&[10, 20, 30], 0.0), 10);
        assert_eq!(percentile(&[10, 20, 30], 100.0), 30);
    }

    #[test]
    fn test_is_test_file_uses_path_components_not_substrings() {
        assert!(!is_test_file(Path::new("src/contest_runner.rs")));
        assert!(!is_test_file(Path::new("test.rs")));
        assert!(is_test_file(Path::new("src/test_utils.rs")));
        assert!(is_test_file(Path::new("packages/app/__tests__/app.ts")));
        assert!(is_test_file(Path::new("src/test/java/FooTest.java")));
        assert!(is_test_file(Path::new("spec/models/user_spec.rb")));
    }
}
