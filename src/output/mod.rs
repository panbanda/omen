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
    Markdown,
    Text,
    Sarif,
}

impl Format {
    pub fn format_value<W: Write>(&self, value: &Value, writer: &mut W) -> Result<()> {
        match self {
            Format::Json => format_json(value, writer),
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
}
