//! Core types and traits for code analysis.

mod analyzer;
mod error;
mod file_set;
mod language;
pub mod progress;
mod source_file;

pub use analyzer::{AnalysisContext, AnalysisResult, Analyzer, Summary};
pub use error::{Error, Result};
pub use file_set::FileSet;
pub use language::Language;
pub use progress::{create_progress, create_spinner, is_tty, ProgressBuilder, ProgressTracker};
pub use source_file::SourceFile;
