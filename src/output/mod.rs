//! Output formatters for analysis results.

use std::io::Write;

use serde::Serialize;
use serde_json::Value;

use crate::core::Result;

/// Output format enum.
#[derive(Clone, Copy, Debug, Default)]
pub enum Format {
    #[default]
    Json,
    JsonCompact,
    Markdown,
    Text,
    Sarif,
}

impl Format {
    pub fn format_value<W: Write>(&self, value: &Value, writer: &mut W) -> Result<()> {
        match self {
            Format::Json => format_json(value, writer),
            Format::JsonCompact => format_json_compact(value, writer),
            Format::Markdown => format_markdown(value, writer),
            Format::Text => format_text(value, writer),
            Format::Sarif => format_sarif(value, writer),
        }
    }

    pub fn format<T: Serialize, W: Write>(&self, data: &T, writer: &mut W) -> Result<()> {
        let value = serde_json::to_value(data)?;
        self.format_value(&value, writer)
    }
}

fn format_json<W: Write>(value: &Value, writer: &mut W) -> Result<()> {
    serde_json::to_writer_pretty(&mut *writer, value)?;
    writeln!(writer)?;
    Ok(())
}

fn format_json_compact<W: Write>(value: &Value, writer: &mut W) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writeln!(writer)?;
    Ok(())
}

/// Truncate top-level arrays in a JSON value for token-efficient output.
/// `top`: max items per array (0 = unlimited). `offset`: skip first N items.
/// Adds `<field>_omitted` count for each truncated array.
/// Returns total items omitted across all truncated arrays.
pub fn truncate_lists(value: &mut Value, top: usize, offset: usize) -> usize {
    if top == 0 && offset == 0 {
        return 0;
    }
    let mut total_omitted = 0usize;
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            let mut omitted_fields: Vec<(String, usize)> = Vec::new();
            for key in keys {
                if let Some(Value::Array(arr)) = map.get_mut(&key) {
                    let original_len = arr.len();
                    // Apply offset first
                    if offset > 0 {
                        if offset < original_len {
                            arr.drain(0..offset);
                        } else {
                            arr.clear();
                        }
                    }
                    // Apply top limit
                    if top > 0 && arr.len() > top {
                        arr.truncate(top);
                    }
                    let after_offset = original_len.saturating_sub(offset);
                    let returned = if top == 0 {
                        after_offset
                    } else {
                        after_offset.min(top)
                    };
                    let omitted = after_offset.saturating_sub(returned);
                    if omitted > 0 {
                        total_omitted += omitted;
                        omitted_fields.push((format!("{}_omitted", key), omitted));
                    }
                }
            }
            for (k, v) in omitted_fields {
                map.insert(k, Value::Number(v.into()));
            }
        }
        Value::Array(arr) => {
            let original_len = arr.len();
            if offset > 0 {
                if offset < original_len {
                    arr.drain(0..offset);
                } else {
                    arr.clear();
                }
            }
            if top > 0 && arr.len() > top {
                arr.truncate(top);
            }
            let after_offset = original_len.saturating_sub(offset);
            let returned = if top == 0 {
                after_offset
            } else {
                after_offset.min(top)
            };
            total_omitted = after_offset.saturating_sub(returned);
        }
        _ => {}
    }
    total_omitted
}

/// Format a JSON value with optional truncation applied for JSON formats.
/// For non-JSON formats, truncation is not applied.
/// `top`: max items per array (None = unlimited). `offset`: skip first N items (None = 0).
/// When only `offset` is set, truncation is applied with top=0 (unlimited after offset).
pub fn format_with_limits<W: Write>(
    mut value: Value,
    format: Format,
    top: Option<usize>,
    offset: Option<usize>,
    writer: &mut W,
) -> Result<()> {
    if matches!(format, Format::Json | Format::JsonCompact) && (top.is_some() || offset.is_some()) {
        let limit = top.unwrap_or(0); // 0 means unlimited
        let off = offset.unwrap_or(0);
        truncate_lists(&mut value, limit, off);
    }
    format.format_value(&value, writer)
}

fn format_markdown<W: Write>(value: &Value, writer: &mut W) -> Result<()> {
    format_value_as_markdown(value, writer, 0)?;
    Ok(())
}

fn format_text<W: Write>(value: &Value, writer: &mut W) -> Result<()> {
    format_value_as_text(value, writer, 0)?;
    Ok(())
}

fn format_sarif<W: Write>(value: &Value, writer: &mut W) -> Result<()> {
    let mut findings = Vec::new();
    collect_sarif_findings(value, &mut findings);

    let rules = serde_json::json!([{
        "id": "omen.finding",
        "name": "Omen finding",
        "shortDescription": {"text": "Omen code analysis finding"},
        "helpUri": "https://github.com/panbanda/omen"
    }]);

    let results: Vec<Value> = findings
        .into_iter()
        .map(|finding| {
            serde_json::json!({
                "ruleId": "omen.finding",
                "level": finding.level,
                "message": {"text": finding.message},
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": {"uri": finding.file},
                        "region": {"startLine": finding.line}
                    }
                }]
            })
        })
        .collect();

    let sarif = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "omen",
                    "informationUri": "https://github.com/panbanda/omen",
                    "rules": rules
                }
            },
            "results": results
        }]
    });

    format_json(&sarif, writer)
}

#[derive(Debug)]
struct SarifFinding {
    file: String,
    line: u64,
    level: &'static str,
    message: String,
}

fn collect_sarif_findings(value: &Value, findings: &mut Vec<SarifFinding>) {
    match value {
        Value::Object(map) => {
            if let Some(finding) = sarif_finding_from_object(map) {
                findings.push(finding);
            }
            for child in map.values() {
                collect_sarif_findings(child, findings);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_sarif_findings(item, findings);
            }
        }
        _ => {}
    }
}

fn sarif_finding_from_object(map: &serde_json::Map<String, Value>) -> Option<SarifFinding> {
    let file = ["file", "path", "file_path"]
        .iter()
        .find_map(|key| map.get(*key).and_then(Value::as_str))?;
    let line = ["line", "start_line", "line_start"]
        .iter()
        .find_map(|key| map.get(*key).and_then(Value::as_u64))
        .unwrap_or(1)
        .max(1);
    let message = ["text", "reason", "message", "name", "marker"]
        .iter()
        .find_map(|key| map.get(*key).and_then(Value::as_str))
        .unwrap_or("Omen finding")
        .to_string();
    let level = map
        .get("severity")
        .and_then(Value::as_str)
        .map(sarif_level)
        .unwrap_or("warning");

    Some(SarifFinding {
        file: file.to_string(),
        line,
        level,
        message,
    })
}

fn sarif_level(severity: &str) -> &'static str {
    match severity.to_ascii_lowercase().as_str() {
        "critical" | "high" => "error",
        "medium" => "warning",
        "low" | "info" | "note" => "note",
        _ => "warning",
    }
}

fn format_value_as_markdown<W: Write>(value: &Value, writer: &mut W, depth: usize) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                let header_level = "#".repeat((depth + 1).min(6));
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        writeln!(writer, "{} {}\n", header_level, format_key(key))?;
                        format_value_as_markdown(val, writer, depth + 1)?;
                    }
                    _ => {
                        writeln!(writer, "**{}**: {}\n", format_key(key), format_scalar(val))?;
                    }
                }
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                writeln!(writer, "_No items_\n")?;
            } else if is_table_compatible(arr) {
                format_as_table(arr, writer)?;
            } else {
                for item in arr {
                    writeln!(writer, "---\n")?;
                    format_value_as_markdown(item, writer, depth)?;
                }
            }
        }
        _ => {
            writeln!(writer, "{}\n", format_scalar(value))?;
        }
    }
    Ok(())
}

fn format_key(key: &str) -> String {
    key.replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{:.2}", f)
                }
            } else {
                n.to_string()
            }
        }
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        Value::Null => "-".to_string(),
        _ => value.to_string(),
    }
}

fn is_table_compatible(arr: &[Value]) -> bool {
    if arr.is_empty() {
        return false;
    }
    arr.iter().all(|v| {
        if let Value::Object(map) = v {
            map.values()
                .all(|v| !matches!(v, Value::Object(_) | Value::Array(_)))
        } else {
            false
        }
    })
}

fn format_as_table<W: Write>(arr: &[Value], writer: &mut W) -> Result<()> {
    if arr.is_empty() {
        return Ok(());
    }

    // Get headers from first object
    let headers: Vec<&str> = if let Value::Object(map) = &arr[0] {
        map.keys().map(|s| s.as_str()).collect()
    } else {
        return Ok(());
    };

    // Write header row
    write!(writer, "|")?;
    for header in &headers {
        write!(writer, " {} |", format_key(header))?;
    }
    writeln!(writer)?;

    // Write separator
    write!(writer, "|")?;
    for _ in &headers {
        write!(writer, " --- |")?;
    }
    writeln!(writer)?;

    // Write data rows
    for item in arr {
        if let Value::Object(map) = item {
            write!(writer, "|")?;
            for header in &headers {
                let value = map.get(*header).unwrap_or(&Value::Null);
                write!(writer, " {} |", format_scalar(value))?;
            }
            writeln!(writer)?;
        }
    }

    writeln!(writer)?;
    Ok(())
}

fn format_value_as_text<W: Write>(value: &Value, writer: &mut W, indent: usize) -> Result<()> {
    let prefix = "  ".repeat(indent);
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        writeln!(writer, "{}{}:", prefix, format_key(key))?;
                        format_value_as_text(val, writer, indent + 1)?;
                    }
                    _ => {
                        writeln!(
                            writer,
                            "{}{}: {}",
                            prefix,
                            format_key(key),
                            format_scalar(val)
                        )?;
                    }
                }
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                writeln!(writer, "{}[{}]", prefix, i)?;
                format_value_as_text(item, writer, indent + 1)?;
            }
        }
        _ => {
            writeln!(writer, "{}{}", prefix, format_scalar(value))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_default_is_json() {
        assert!(matches!(Format::default(), Format::Json));
    }

    #[test]
    fn test_format_sarif_emits_findings_from_items_array() {
        let value = json!({
            "items": [{
                "file": "src/lib.rs",
                "line": 7,
                "severity": "high",
                "marker": "TODO",
                "text": "TODO: remove shortcut"
            }]
        });
        let mut buf = Vec::new();
        Format::Sarif.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("\"version\": \"2.1.0\""));
        assert!(output.contains("\"artifactLocation\""));
        assert!(output.contains("src/lib.rs"));
        assert!(output.contains("\"startLine\": 7"));
        assert!(output.contains("TODO: remove shortcut"));
    }

    #[test]
    fn test_format_json_simple_object() {
        let value = json!({"name": "test", "count": 42});
        let mut buf = Vec::new();
        Format::Json.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("\"name\": \"test\""));
        assert!(output.contains("\"count\": 42"));
    }

    #[test]
    fn test_format_json_nested() {
        let value = json!({"outer": {"inner": 123}});
        let mut buf = Vec::new();
        Format::Json.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("outer"));
        assert!(output.contains("inner"));
        assert!(output.contains("123"));
    }

    #[test]
    fn test_format_markdown_simple_object() {
        let value = json!({"file_name": "test.rs", "score": 95});
        let mut buf = Vec::new();
        Format::Markdown.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("File Name"));
        assert!(output.contains("test.rs"));
        assert!(output.contains("95"));
    }

    #[test]
    fn test_format_markdown_nested_object() {
        let value = json!({"summary": {"total": 10, "passed": 8}});
        let mut buf = Vec::new();
        Format::Markdown.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("# Summary"));
    }

    #[test]
    fn test_format_markdown_array_as_table() {
        let value = json!([
            {"name": "a", "value": 1},
            {"name": "b", "value": 2}
        ]);
        let mut buf = Vec::new();
        Format::Markdown.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Should be formatted as markdown table
        assert!(output.contains("|"));
        assert!(output.contains("---"));
    }

    #[test]
    fn test_format_markdown_empty_array() {
        let value = json!([]);
        let mut buf = Vec::new();
        Format::Markdown.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("_No items_"));
    }

    #[test]
    fn test_format_markdown_non_table_array() {
        let value = json!([
            {"nested": {"deep": 1}},
            {"nested": {"deep": 2}}
        ]);
        let mut buf = Vec::new();
        Format::Markdown.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("---"));
    }

    #[test]
    fn test_format_text_simple_object() {
        let value = json!({"file_name": "test.rs", "score": 95});
        let mut buf = Vec::new();
        Format::Text.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("File Name: test.rs"));
        assert!(output.contains("Score: 95"));
    }

    #[test]
    fn test_format_text_nested_object() {
        let value = json!({"summary": {"total": 10}});
        let mut buf = Vec::new();
        Format::Text.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Summary:"));
        assert!(output.contains("Total: 10"));
    }

    #[test]
    fn test_format_text_array() {
        let value = json!([{"name": "a"}, {"name": "b"}]);
        let mut buf = Vec::new();
        Format::Text.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("[0]"));
        assert!(output.contains("[1]"));
    }

    #[test]
    fn test_format_key_snake_case() {
        assert_eq!(format_key("file_name"), "File Name");
        assert_eq!(format_key("total_count"), "Total Count");
    }

    #[test]
    fn test_format_key_single_word() {
        assert_eq!(format_key("name"), "Name");
        assert_eq!(format_key("score"), "Score");
    }

    #[test]
    fn test_format_scalar_string() {
        let v = json!("hello");
        assert_eq!(format_scalar(&v), "hello");
    }

    #[test]
    fn test_format_scalar_integer() {
        let v = json!(42);
        assert_eq!(format_scalar(&v), "42");
    }

    #[test]
    fn test_format_scalar_float() {
        let v = json!(2.5);
        assert_eq!(format_scalar(&v), "2.50");
    }

    #[test]
    fn test_format_scalar_float_whole() {
        let v = json!(10.0);
        assert_eq!(format_scalar(&v), "10");
    }

    #[test]
    fn test_format_scalar_bool() {
        assert_eq!(format_scalar(&json!(true)), "Yes");
        assert_eq!(format_scalar(&json!(false)), "No");
    }

    #[test]
    fn test_format_scalar_null() {
        assert_eq!(format_scalar(&json!(null)), "-");
    }

    #[test]
    fn test_is_table_compatible_empty() {
        let arr: Vec<Value> = vec![];
        assert!(!is_table_compatible(&arr));
    }

    #[test]
    fn test_is_table_compatible_flat_objects() {
        let arr = vec![json!({"a": 1}), json!({"a": 2})];
        assert!(is_table_compatible(&arr));
    }

    #[test]
    fn test_is_table_compatible_nested_objects() {
        let arr = vec![json!({"a": {"b": 1}}), json!({"a": {"b": 2}})];
        assert!(!is_table_compatible(&arr));
    }

    #[test]
    fn test_is_table_compatible_non_objects() {
        let arr = vec![json!(1), json!(2)];
        assert!(!is_table_compatible(&arr));
    }

    #[test]
    fn test_format_as_table() {
        let arr = vec![
            json!({"name": "a", "value": 1}),
            json!({"name": "b", "value": 2}),
        ];
        let mut buf = Vec::new();
        format_as_table(&arr, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("| Name |"));
        assert!(output.contains("| --- |"));
        assert!(output.contains("| a |"));
        assert!(output.contains("| b |"));
    }

    #[test]
    fn test_format_as_table_empty() {
        let arr: Vec<Value> = vec![];
        let mut buf = Vec::new();
        format_as_table(&arr, &mut buf).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_format_serialize() {
        #[derive(Serialize)]
        struct TestData {
            name: String,
            count: i32,
        }
        let data = TestData {
            name: "test".to_string(),
            count: 42,
        };
        let mut buf = Vec::new();
        Format::Json.format(&data, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("\"name\": \"test\""));
        assert!(output.contains("\"count\": 42"));
    }

    #[test]
    fn test_format_text_scalar_at_root() {
        let value = json!("just a string");
        let mut buf = Vec::new();
        Format::Text.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("just a string"));
    }

    #[test]
    fn test_format_markdown_scalar_at_root() {
        let value = json!("just a string");
        let mut buf = Vec::new();
        Format::Markdown.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("just a string"));
    }

    #[test]
    fn test_format_json_compact_single_line() {
        let value = json!({"name": "test", "items": [1, 2, 3]});
        let mut buf = Vec::new();
        Format::JsonCompact.format_value(&value, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Compact output must not contain newlines within the JSON (only trailing newline)
        assert_eq!(output.trim().lines().count(), 1);
    }

    #[test]
    fn test_format_json_compact_identical_value() {
        let value = json!({"name": "test", "count": 42, "items": [1, 2, 3]});
        let mut buf_pretty = Vec::new();
        let mut buf_compact = Vec::new();
        Format::Json.format_value(&value, &mut buf_pretty).unwrap();
        Format::JsonCompact
            .format_value(&value, &mut buf_compact)
            .unwrap();
        // Parse both back and compare
        let v1: Value = serde_json::from_slice(&buf_pretty).unwrap();
        let v2: Value = serde_json::from_slice(&buf_compact).unwrap();
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_truncate_lists_limits_top_level_arrays() {
        let mut value = json!({"files": (0..100).collect::<Vec<_>>(), "summary": {"total": 100}});
        let omitted = truncate_lists(&mut value, 10, 0);
        assert_eq!(omitted, 90);
        assert_eq!(value["files"].as_array().unwrap().len(), 10);
        assert_eq!(value["files_omitted"].as_u64().unwrap(), 90);
        // summary (not an array) is untouched
        assert!(value["summary"].is_object());
    }

    #[test]
    fn test_truncate_lists_zero_means_unlimited() {
        let mut value = json!({"items": (0..100).collect::<Vec<_>>()});
        let omitted = truncate_lists(&mut value, 0, 0);
        assert_eq!(omitted, 0);
        assert_eq!(value["items"].as_array().unwrap().len(), 100);
    }

    #[test]
    fn test_truncate_lists_preserves_summary_fields() {
        let mut value = json!({"items": [1, 2, 3], "total": 99, "grade": "A"});
        truncate_lists(&mut value, 2, 0);
        assert_eq!(value["total"].as_u64().unwrap(), 99);
        assert_eq!(value["grade"].as_str().unwrap(), "A");
    }

    #[test]
    fn test_truncate_lists_with_offset() {
        let mut value = json!({"items": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]});
        let omitted = truncate_lists(&mut value, 3, 5);
        // items 5,6,7 are kept; 0-4 skipped, 8-9 omitted
        let items = value["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], 5);
        assert_eq!(items[2], 7);
        // omitted = original(10) - offset(5) - returned(3) = 2
        assert_eq!(omitted, 2);
    }

    #[test]
    fn test_truncate_lists_on_root_array() {
        let mut value = json!([0, 1, 2, 3, 4]);
        let omitted = truncate_lists(&mut value, 3, 0);
        assert_eq!(value.as_array().unwrap().len(), 3);
        assert_eq!(omitted, 2);
    }

    // --- Tests for format_with_limits ---

    #[test]
    fn test_format_with_limits_json_applies_top() {
        // JsonCompact format with top=3: only 3 items should appear in output.
        let value = json!({"items": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]});
        let mut buf = Vec::new();
        format_with_limits(value, Format::JsonCompact, Some(3), None, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(parsed["items"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_format_with_limits_offset_only_no_top() {
        // offset=2, top=None → unlimited after offset; items 2..9 (8 items).
        let value = json!({"items": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]});
        let mut buf = Vec::new();
        format_with_limits(value, Format::Json, None, Some(2), &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        let items = parsed["items"].as_array().unwrap();
        assert_eq!(
            items.len(),
            8,
            "offset=2, all remaining 8 items should be returned"
        );
        assert_eq!(items[0], 2, "first item after offset should be index 2");
    }

    #[test]
    fn test_format_with_limits_non_json_passes_through() {
        // Non-JSON format (Markdown) should not apply truncation.
        let value = json!({"items": [1, 2, 3, 4, 5]});
        let mut buf = Vec::new();
        format_with_limits(value, Format::Markdown, Some(2), None, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Markdown output should contain all 5 items (no truncation).
        assert!(output.contains("1"), "markdown should contain item 1");
        assert!(output.contains("5"), "markdown should contain item 5");
    }

    #[test]
    fn test_format_with_limits_no_top_no_offset_json_passthrough() {
        // No top and no offset: JSON format should pass through all items.
        let value = json!({"items": [1, 2, 3]});
        let mut buf = Vec::new();
        format_with_limits(value, Format::Json, None, None, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(parsed["items"].as_array().unwrap().len(), 3);
    }
}
