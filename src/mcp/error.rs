//! The structured error that a tool gives when it cannot make a result.
//!
//! A tool error is a usual MCP tool result with `isError: true`. JSON-RPC
//! error objects are only for protocol faults: a parse error, an unknown
//! method, an unknown tool, and a `tools/call` request without a tool name.

use serde_json::{json, Value};

/// The stable identifier of a class of tool errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ErrorCode {
    MissingArgument,
    InvalidArgument,
    UnknownArgument,
    Internal,
}

impl ErrorCode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::MissingArgument => "missing_argument",
            Self::InvalidArgument => "invalid_argument",
            Self::UnknownArgument => "unknown_argument",
            Self::Internal => "internal",
        }
    }
}

/// A failure of one tool call.
///
/// The details are in a box, thus a `Result` with this error stays small.
#[derive(Debug)]
pub(super) struct ToolError(Box<ErrorDetails>);

#[derive(Debug)]
pub(super) struct ErrorDetails {
    pub code: ErrorCode,
    pub message: String,
    /// The argument that caused the error.
    pub param: Option<String>,
    /// A description of a correct value.
    pub expected: Option<String>,
    /// The value that the call gave.
    pub received: Option<Value>,
    /// Values that the caller can use in the next call.
    pub suggestions: Vec<Value>,
    /// The step that corrects the error.
    pub next: Option<String>,
}

impl std::ops::Deref for ToolError {
    type Target = ErrorDetails;

    fn deref(&self) -> &ErrorDetails {
        &self.0
    }
}

impl ToolError {
    pub(super) fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self(Box::new(ErrorDetails {
            code,
            message: message.into(),
            param: None,
            expected: None,
            received: None,
            suggestions: Vec::new(),
            next: None,
        }))
    }

    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub(super) fn with_param(mut self, param: impl Into<String>) -> Self {
        self.0.param = Some(param.into());
        self
    }

    pub(super) fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.0.expected = Some(expected.into());
        self
    }

    pub(super) fn with_received(mut self, received: &Value) -> Self {
        self.0.received = Some(shorten(received));
        self
    }

    pub(super) fn with_suggestions<T: Into<Value>>(
        mut self,
        suggestions: impl IntoIterator<Item = T>,
    ) -> Self {
        self.0.suggestions = suggestions.into_iter().map(Into::into).collect();
        self
    }

    pub(super) fn with_next(mut self, next: impl Into<String>) -> Self {
        self.0.next = Some(next.into());
        self
    }

    /// The error object. Each key is always present, thus the shape is stable.
    pub(super) fn to_json(&self) -> Value {
        json!({
            "code": self.code.as_str(),
            "message": self.message,
            "param": self.param,
            "expected": self.expected,
            "received": self.received,
            "suggestions": self.suggestions,
            "next": self.next
        })
    }

    pub(super) fn to_result(&self) -> Value {
        error_result(std::slice::from_ref(self))
    }
}

/// The largest JSON text of a received value that an error shows.
const MAX_RECEIVED_BYTES: usize = 200;

/// Replaces a large value with a description, because an error must be small.
fn shorten(value: &Value) -> Value {
    if value.to_string().len() <= MAX_RECEIVED_BYTES {
        return value.clone();
    }
    let kind = match value {
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        _ => "value",
    };
    json!(format!("<{kind} that is too long to show>"))
}

/// The MCP tool result for one or more errors of one call.
///
/// `error` is the first error. `errors` is present only when the call has
/// more than one error, and it holds all of them.
pub(super) fn error_result(errors: &[ToolError]) -> Value {
    let mut body = json!({"error": errors.first().map(ToolError::to_json)});
    if errors.len() > 1 {
        body["errors"] = errors.iter().map(ToolError::to_json).collect();
    }
    json!({
        "content": [{"type": "text", "text": body.to_string()}],
        "structuredContent": body,
        "isError": true
    })
}

impl From<serde_json::Error> for ToolError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(format!(
            "The server cannot write the result as JSON: {error}"
        ))
    }
}

impl From<crate::core::Error> for ToolError {
    fn from(error: crate::core::Error) -> Self {
        Self::internal(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_code_has_a_stable_name() {
        let names: Vec<&str> = [
            ErrorCode::MissingArgument,
            ErrorCode::InvalidArgument,
            ErrorCode::UnknownArgument,
            ErrorCode::Internal,
        ]
        .into_iter()
        .map(ErrorCode::as_str)
        .collect();
        assert_eq!(
            names,
            [
                "missing_argument",
                "invalid_argument",
                "unknown_argument",
                "internal"
            ]
        );
    }

    #[test]
    fn error_object_always_has_each_key() {
        let error = ToolError::internal("The analysis failed.");
        assert_eq!(
            error.to_json(),
            json!({
                "code": "internal",
                "message": "The analysis failed.",
                "param": null,
                "expected": null,
                "received": null,
                "suggestions": [],
                "next": null
            })
        );
    }

    #[test]
    fn error_object_holds_the_details() {
        let error = ToolError::new(ErrorCode::InvalidArgument, "The value is not correct.")
            .with_param("depth")
            .with_expected("an integer from 0 to 4")
            .with_received(&json!("two"))
            .with_suggestions(["a", "b"])
            .with_next("Send the call again.");
        assert_eq!(
            error.to_json(),
            json!({
                "code": "invalid_argument",
                "message": "The value is not correct.",
                "param": "depth",
                "expected": "an integer from 0 to 4",
                "received": "two",
                "suggestions": ["a", "b"],
                "next": "Send the call again."
            })
        );
    }

    #[test]
    fn a_long_received_value_becomes_a_description() {
        let long = json!("x".repeat(500));
        let error = ToolError::internal("m").with_received(&long);
        assert_eq!(
            error.received,
            Some(json!("<string that is too long to show>"))
        );
        let items = json!(vec![1; 500]);
        let error = ToolError::internal("m").with_received(&items);
        assert_eq!(
            error.received,
            Some(json!("<array that is too long to show>"))
        );
    }

    #[test]
    fn result_of_one_error_is_an_error_result_with_structured_content() {
        let result = ToolError::internal("The analysis failed.").to_result();
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["error"]["code"], "internal");
        assert!(result["structuredContent"].get("errors").is_none());
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
    }

    #[test]
    fn result_of_two_errors_lists_all_of_them() {
        let errors = [
            ToolError::new(ErrorCode::MissingArgument, "first"),
            ToolError::new(ErrorCode::UnknownArgument, "second"),
        ];
        let result = error_result(&errors);
        let body = &result["structuredContent"];
        assert_eq!(body["error"]["code"], "missing_argument");
        assert_eq!(body["errors"][0]["code"], "missing_argument");
        assert_eq!(body["errors"][1]["code"], "unknown_argument");
        assert_eq!(body["errors"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_core_error_becomes_an_internal_error() {
        let error = ToolError::from(crate::core::Error::analysis("no data"));
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(error.message.contains("no data"));
    }
}
