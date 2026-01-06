//! Omen - Multi-language code analysis library for AI assistants.
//!
//! Omen analyzes codebases for complexity, technical debt, dead code,
//! code duplication, defect prediction, and dependency graphs.
//!
//! # Supported Languages
//!
//! Go, Rust, Python, TypeScript, JavaScript, TSX/JSX, Java, C, C++, C#, Ruby, PHP, Bash
//!
//! # Example
//!
//! ```no_run
//! use omen::analyzers::complexity::Analyzer as ComplexityAnalyzer;
//! use omen::core::{AnalysisContext, Analyzer, FileSet};
//! use omen::config::Config;
//!
//! let config = Config::default();
//! let files = FileSet::from_path(".", &config).unwrap();
//! let ctx = AnalysisContext::new(&files, &config, None);
//! let analyzer = ComplexityAnalyzer::new();
//! let result = analyzer.analyze(&ctx).unwrap();
//! println!("Analyzed {} functions", result.summary.total_functions);
//! ```

pub mod analyzers;
pub mod cli;
pub mod config;
pub mod core;
pub mod git;
pub mod mcp;
pub mod output;
pub mod parser;
pub mod report;
pub mod score;

pub use core::{AnalysisContext, AnalysisResult, Analyzer};
