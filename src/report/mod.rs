//! HTML report generation module.
//!
//! This module generates interactive HTML reports matching the Go version exactly.

mod render;
mod types;

pub use render::Renderer;
pub use types::*;
