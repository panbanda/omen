//! Lossless, opt-in tables for repeated flat records. No source or evidence is dropped.
use serde_json::{json, Value};

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

    #[test]
    fn round_trip_preserves_graph_evidence_and_escaped_paths() {
        let records: Vec<_> = (0..100)
            .map(|i| {
                json!({
                    "qualified_name": format!("lib/example.rb:method_{i}"),
                    "file": "lib/example.rb", "line": i + 1,
                    "uncertainty": null, "resolved": false
                })
            })
            .collect();
        let original = json!({"result": {"a/b~c": records,
            "evidence": [{"status": "ambiguous", "candidates": ["a", "b"]}],
            "source": "columns rows are just source text"}, "returned": 100});
        let mut compact = encode(original.clone());
        assert!(compact.to_string().len() < original.to_string().len());
        let paths: Vec<String> = serde_json::from_value(compact["table_paths"].clone()).unwrap();
        assert_eq!(paths, ["/a~1b~0c"]);
        for path in paths {
            let table = compact["result"].pointer_mut(&path).unwrap();
            let columns = table["columns"].as_array().unwrap();
            let rows = table["rows"].as_array().unwrap();
            let restored: Vec<Value> = rows
                .iter()
                .map(|row| {
                    Value::Object(
                        columns
                            .iter()
                            .zip(row.as_array().unwrap())
                            .map(|(key, val)| (key.as_str().unwrap().to_string(), val.clone()))
                            .collect(),
                    )
                })
                .collect();
            *table = json!(restored);
        }
        compact.as_object_mut().unwrap().remove("encoding");
        compact.as_object_mut().unwrap().remove("table_paths");
        assert_eq!(compact, original);
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
