//! Code analyzers for various metrics and issues.

pub mod changes;
pub mod churn;
pub mod cohesion;
pub mod complexity;
pub mod deadcode;
pub mod defect;
pub mod duplicates;
pub mod flags;
pub mod graph;
pub mod hotspot;
pub mod ownership;
pub mod repomap;
pub mod satd;
pub mod smells;
pub mod tdg;
pub mod temporal;

// Re-export analyzer types for convenience
pub use complexity::Analyzer as ComplexityAnalyzer;
pub use satd::Analyzer as SatdAnalyzer;
