//! Lossless, opt-in tables for repeated flat records. No source or evidence is dropped.
use serde_json::{json, Value};

use crate::core::{Error, Result};

pub(super) fn encode(envelope: Value) -> Value {
    let mut candidate = envelope.clone();
    let mut paths = Vec::new();
    visit(&mut candidate["result"], "", &mut paths);
    if paths.is_empty() {
        return envelope;
    }
    candidate["encoding"] = json!("omen.tables.v1");
    candidate["table_paths"] = json!(paths);
    // Marginal byte savings can increase tokenizer cost. Require headroom for
    // metadata/token-boundary overhead; actual tokenizer savings are benchmarked.
    if candidate.to_string().len().saturating_add(128) <= envelope.to_string().len() {
        candidate
    } else {
        envelope
    }
}

/// Invert [`encode`], expanding every declared table back into its records.
///
/// This is the reference decoder for `omen.tables.v1`. An envelope without that
/// encoding is returned untouched. A declared path that is missing, or a row
/// whose arity disagrees with its columns, is rejected rather than silently
/// reshaped: a decoder that guesses would defeat the losslessness it exists to
/// demonstrate.
pub fn decode(mut envelope: Value) -> Result<Value> {
    if envelope.get("encoding") != Some(&json!("omen.tables.v1")) {
        return Ok(envelope);
    }
    let paths: Vec<String> = serde_json::from_value(envelope["table_paths"].clone())
        .map_err(|e| Error::Mcp(format!("unreadable table_paths: {e}")))?;

    for path in paths {
        let table = envelope["result"]
            .pointer_mut(&path)
            .ok_or_else(|| Error::Mcp(format!("no table at {path:?}")))?;
        let columns: Vec<String> = serde_json::from_value(table["columns"].clone())
            .map_err(|e| Error::Mcp(format!("unreadable columns at {path:?}: {e}")))?;
        let rows = table["rows"]
            .as_array()
            .ok_or_else(|| Error::Mcp(format!("no rows at {path:?}")))?;

        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let values = row
                .as_array()
                .ok_or_else(|| Error::Mcp(format!("row is not an array at {path:?}")))?;
            if values.len() != columns.len() {
                return Err(Error::Mcp(format!(
                    "row has {} values for {} columns at {path:?}",
                    values.len(),
                    columns.len()
                )));
            }
            records.push(Value::Object(
                columns
                    .iter()
                    .cloned()
                    .zip(values.iter().cloned())
                    .collect(),
            ));
        }
        *table = Value::Array(records);
    }

    if let Some(object) = envelope.as_object_mut() {
        object.remove("encoding");
        object.remove("table_paths");
    }
    Ok(envelope)
}

fn visit(value: &mut Value, path: &str, paths: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            if items.len() >= 2 {
                if let Some(first) = items[0].as_object() {
                    let columns: Vec<_> = first.keys().cloned().collect();
                    if !columns.is_empty()
                        && items.iter().all(|item| {
                            item.as_object().is_some_and(|obj| {
                                obj.keys().eq(columns.iter())
                                    && obj.values().all(|v| !v.is_array() && !v.is_object())
                            })
                        })
                    {
                        let rows: Vec<Vec<_>> = items
                            .iter()
                            .map(|item| columns.iter().map(|key| item[key].clone()).collect())
                            .collect();
                        let table = json!({"columns": columns, "rows": rows});
                        if table.to_string().len() < value.to_string().len() {
                            *value = table;
                            paths.push(path.to_string());
                        }
                        return;
                    }
                }
            }
            for (i, item) in items.iter_mut().enumerate() {
                visit(item, &format!("{path}/{i}"), paths);
            }
        }
        Value::Object(obj) => {
            for (key, child) in obj {
                let key = key.replace('~', "~0").replace('/', "~1");
                visit(child, &format!("{path}/{key}"), paths);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(count: usize) -> Vec<Value> {
        (0..count)
            .map(|i| {
                json!({
                    "qualified_name": format!("lib/example.rb:method_{i}"),
                    "file": "lib/example.rb", "line": i + 1,
                    "uncertainty": null, "resolved": false
                })
            })
            .collect()
    }

    #[test]
    fn decode_inverts_encode_including_escaped_paths() {
        let original = json!({"result": {"a/b~c": records(100),
            "evidence": [{"status": "ambiguous", "candidates": ["a", "b"]}],
            "source": "columns rows are just source text"}, "returned": 100});

        let compact = encode(original.clone());
        assert!(compact.to_string().len() < original.to_string().len());
        assert_eq!(compact["table_paths"], json!(["/a~1b~0c"]));
        assert_eq!(decode(compact).expect("decodable"), original);
    }

    #[test]
    fn decode_round_trips_every_scalar_type() {
        let rows: Vec<Value> = (0..40)
            .map(|i| json!({"s": format!("v{i}"), "n": i, "f": 1.5, "b": i % 2 == 0, "z": null}))
            .collect();
        let original = json!({"result": {"rows~/odd": rows}});
        let compact = encode(original.clone());
        assert_eq!(compact["encoding"], json!("omen.tables.v1"));
        assert_eq!(decode(compact).expect("decodable"), original);
    }

    #[test]
    fn decode_passes_through_unencoded_envelopes() {
        let plain = json!({"result": [{"a": 1}, {"b": 2}], "returned": 2});
        assert_eq!(decode(plain.clone()).expect("unchanged"), plain);
    }

    #[test]
    fn decode_restores_a_root_level_table() {
        let compact = json!({"result": {"columns": ["a", "b"], "rows": [[1, null], [2, true]]},
            "encoding": "omen.tables.v1", "table_paths": [""]});
        assert_eq!(
            decode(compact).expect("decodable"),
            json!({"result": [{"a": 1, "b": null}, {"a": 2, "b": true}]})
        );
    }

    #[test]
    fn decode_rejects_row_and_column_disagreement() {
        let bad = json!({"result": {"columns": ["x", "y"], "rows": [[1]]},
            "encoding": "omen.tables.v1", "table_paths": [""]});
        assert!(decode(bad).is_err());
    }

    #[test]
    fn decode_rejects_a_table_path_that_is_not_there() {
        let bad = json!({"result": {"a": 1},
            "encoding": "omen.tables.v1", "table_paths": ["/nope"]});
        assert!(decode(bad).is_err());
    }

    #[test]
    fn small_or_heterogeneous_results_are_unchanged() {
        for result in [
            json!([]),
            json!([{"a": 1}, {"b": 2}]),
            json!([{"a": 1}, {"a": null}]),
            json!({"columns": ["x"], "rows": [[1]]}),
        ] {
            let original = json!({"result": result});
            assert_eq!(encode(original.clone()), original);
        }
    }
}
