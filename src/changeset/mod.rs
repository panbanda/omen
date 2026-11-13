//! The changes between two commits: the symbols that changed, the calls
//! that changed, and the comment for a pull request.
//!
//! Omen builds the shared index at the base and at the head, and compares
//! the two indexes by symbol key.

pub mod graph;
pub mod markdown;
pub mod mermaid;
pub mod model;
