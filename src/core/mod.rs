//! Core types and traits for code analysis.

mod analyzer;
mod error;
mod file_set;
mod language;
mod source_file;

pub use analyzer::{AnalysisContext, AnalysisResult, Analyzer, Summary};
pub use error::{Error, Result};
pub use file_set::FileSet;
pub use language::Language;
pub use source_file::SourceFile;
