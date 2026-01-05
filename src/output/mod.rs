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
}

impl Format {
    pub fn format_value<W: Write>(&self, value: &Value, writer: &mut W) -> Result<()> {
        match self {
            Format::Json => format_json(value, writer),
            Format::Markdown => format_markdown(value, writer),
            Format::Text => format_text(value, writer),
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
