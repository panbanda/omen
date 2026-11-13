//! The error that a tool gives when it cannot make a result.

use serde_json::{json, Value};

/// A failure of one tool call. It is not a JSON-RPC protocol error.
#[derive(Debug)]
pub(super) struct ToolError {
    pub message: String,
}

impl ToolError {
    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The MCP tool result for this error.
    pub(super) fn to_result(&self) -> Value {
        json!({
            "content": [{"type": "text", "text": self.message}],
            "isError": true
        })
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(format!("Serialization failed: {error}"))
    }
}

impl From<crate::core::Error> for ToolError {
    fn from(error: crate::core::Error) -> Self {
        Self::internal(error.to_string())
    }
}
