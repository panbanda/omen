//! Shared index of a repository.
//!
//! The analyzers use this module for the facts and the graph algorithms that they share.

pub mod build;
pub mod calls;
pub mod extract;
pub mod graph;
pub mod headers;
pub mod imports;
pub mod lang;
pub mod model;
pub mod resolve;
pub mod security;
pub mod view;
