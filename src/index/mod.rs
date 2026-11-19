//! Shared index of a repository.
//!
//! The analyzers use this module for the facts and the graph algorithms that they share.

pub mod build;
pub mod calls;
#[cfg(test)]
mod deep_tests;
pub mod dependencies;
pub mod extract;
pub mod graph;
pub mod headers;
pub mod imports;
pub mod lang;
pub mod members;
pub mod model;
pub mod resolve;
pub mod security;
pub mod test_code;
#[cfg(test)]
pub(crate) mod timing;
#[cfg(test)]
mod timing_tests;
pub mod view;
