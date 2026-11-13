//! The envelope that holds the result of a tool, and its pagination.

use serde_json::{json, Value};

use super::compact;
use super::tools::Pagination;

/// The part of the primary list that a call requests.
#[derive(Clone, Copy, Debug)]
pub(super) struct Page {
    /// The maximum number of items. The value 0 removes the limit.
    pub limit: usize,
    pub offset: usize,
}

/// Applies the page to `result` and puts the result in the envelope.
pub(super) fn build(
    tool: &str,
    pagination: Pagination,
    mut result: Value,
    page: Page,
    git_skipped_reason: Option<&str>,
    compact: bool,
) -> Value {
    if matches!(pagination, Pagination::None) {
        return json!({"tool": tool, "result": result});
    }
    let total_items = count(pagination, &result);
    truncate(pagination, &mut result, page);
    let returned = count(pagination, &result);

    let mut envelope = json!({
        "tool": tool,
        "total_items": total_items,
        "returned": returned,
        "offset": page.offset,
        "result": result
    });
    if let Some(reason) = git_skipped_reason {
        envelope["git_skipped_reason"] = json!(reason);
    }
    if compact {
        envelope = compact::encode(envelope);
    }
    envelope
}

/// The sum of the lengths of all the arrays at the top level of the value.
/// Gives `None` when the value has no array.
fn count_all_lists(value: &Value) -> Option<usize> {
    match value {
        Value::Array(items) => Some(items.len()),
        Value::Object(entries) => {
            let lengths: Vec<usize> = entries
                .values()
                .filter_map(Value::as_array)
                .map(Vec::len)
                .collect();
            (!lengths.is_empty()).then(|| lengths.iter().sum())
        }
        _ => None,
    }
}

fn child_arrays<'a>(
    value: &'a Value,
    parents: &'static [&'static str],
    child: &'static str,
) -> Option<Vec<&'a Vec<Value>>> {
    let mut found_parent = false;
    let mut children = Vec::new();
    for parent in parents {
        let Some(elements) = value.get(*parent).and_then(Value::as_array) else {
            continue;
        };
        found_parent = true;
        children.extend(
            elements
                .iter()
                .filter_map(|element| element.get(child)?.as_array()),
        );
    }
    found_parent.then_some(children)
}

fn count(pagination: Pagination, value: &Value) -> Option<usize> {
    match pagination {
        Pagination::None => None,
        Pagination::Field(field) => value
            .get(field)
            .and_then(Value::as_array)
            .map(Vec::len)
            .or_else(|| count_all_lists(value)),
        Pagination::Nested { parents, child } => child_arrays(value, parents, child)
            .map(|children| children.into_iter().map(Vec::len).sum()),
        Pagination::Children(field) => value.get(field)?.as_array().map(|children| {
            children
                .iter()
                .filter_map(Value::as_array)
                .map(Vec::len)
                .sum()
        }),
        Pagination::AllLists => count_all_lists(value),
    }
}

/// The limit and the offset that stay for the next array of one sequence.
struct Budget {
    limit: usize,
    offset: usize,
}

impl Budget {
    fn new(page: Page) -> Self {
        Self {
            limit: if page.limit == 0 {
                usize::MAX
            } else {
                page.limit
            },
            offset: page.offset,
        }
    }

    fn apply(&mut self, items: &mut Vec<Value>) {
        let skip = self.offset.min(items.len());
        items.drain(0..skip);
        self.offset -= skip;
        items.truncate(self.limit);
        self.limit -= items.len();
    }
}

fn truncate(pagination: Pagination, value: &mut Value, page: Page) {
    match pagination {
        Pagination::None => {}
        Pagination::Field(field) => {
            if let Some(items) = value.get_mut(field).and_then(Value::as_array_mut) {
                Budget::new(page).apply(items);
            }
        }
        Pagination::Nested { parents, child } => {
            let mut budget = Budget::new(page);
            for parent in parents {
                let Some(elements) = value.get_mut(*parent).and_then(Value::as_array_mut) else {
                    continue;
                };
                for element in elements {
                    if let Some(items) = element.get_mut(child).and_then(Value::as_array_mut) {
                        budget.apply(items);
                    }
                }
            }
        }
        Pagination::Children(field) => {
            let mut budget = Budget::new(page);
            if let Some(children) = value.get_mut(field).and_then(Value::as_array_mut) {
                for items in children.iter_mut().filter_map(Value::as_array_mut) {
                    budget.apply(items);
                }
            }
        }
        Pagination::AllLists => {
            crate::output::truncate_lists(value, page.limit, page.offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_PAGE: Page = Page {
        limit: 50,
        offset: 0,
    };

    fn page(limit: usize, offset: usize) -> Page {
        Page { limit, offset }
    }

    #[test]
    fn envelope_has_the_pagination_fields() {
        let envelope = build(
            "satd",
            Pagination::Field("items"),
            json!({"items": [1, 2, 3, 4, 5]}),
            DEFAULT_PAGE,
            None,
            false,
        );
        assert_eq!(envelope["tool"], "satd");
        assert_eq!(envelope["total_items"], 5);
        assert_eq!(envelope["returned"], 5);
        assert_eq!(envelope["offset"], 0);
        assert_eq!(envelope["result"]["items"], json!([1, 2, 3, 4, 5]));
    }

    #[test]
    fn envelope_reports_skipped_git_data() {
        let envelope = build(
            "score",
            Pagination::AllLists,
            json!({"components": {}}),
            DEFAULT_PAGE,
            Some("not a git repository"),
            false,
        );
        assert_eq!(envelope["git_skipped_reason"], "not a git repository");
    }

    #[test]
    fn default_limit_keeps_the_first_50_items() {
        let items: Vec<i64> = (0..100).collect();
        let envelope = build(
            "satd",
            Pagination::Field("items"),
            json!({"items": items}),
            DEFAULT_PAGE,
            None,
            false,
        );
        assert_eq!(envelope["returned"], 50);
        assert_eq!(envelope["total_items"], 100);
    }

    #[test]
    fn zero_limit_keeps_all_items() {
        let items: Vec<i64> = (0..100).collect();
        let envelope = build(
            "satd",
            Pagination::Field("items"),
            json!({"items": items}),
            page(0, 0),
            None,
            false,
        );
        assert_eq!(envelope["returned"], 100);
    }

    #[test]
    fn offset_skips_items_before_the_limit_applies() {
        let items: Vec<i64> = (0..20).collect();
        let envelope = build(
            "satd",
            Pagination::Field("items"),
            json!({"items": items}),
            page(5, 10),
            None,
            false,
        );
        assert_eq!(envelope["returned"], 5);
        assert_eq!(envelope["offset"], 10);
        assert_eq!(envelope["result"]["items"], json!([10, 11, 12, 13, 14]));
    }

    #[test]
    fn field_pagination_does_not_cut_other_lists() {
        let envelope = build(
            "satd",
            Pagination::Field("items"),
            json!({"items": [1, 2, 3], "files": [1, 2, 3]}),
            page(1, 0),
            None,
            false,
        );
        assert_eq!(envelope["result"]["items"], json!([1]));
        assert_eq!(envelope["result"]["files"], json!([1, 2, 3]));
    }

    #[test]
    fn field_pagination_counts_all_lists_when_the_field_is_absent() {
        let envelope = build(
            "get_symbol",
            Pagination::Field("callers"),
            json!({"choices": [1, 2]}),
            page(1, 0),
            None,
            false,
        );
        assert_eq!(envelope["total_items"], 2);
        assert_eq!(envelope["result"]["choices"], json!([1, 2]));
    }

    #[test]
    fn all_lists_pagination_counts_and_cuts_each_list() {
        let components: Vec<i64> = (0..101).collect();
        let smells: Vec<i64> = (0..10).collect();
        let envelope = build(
            "score",
            Pagination::AllLists,
            json!({"components": components, "smells": smells}),
            page(5, 0),
            None,
            false,
        );
        assert_eq!(envelope["total_items"], 111);
        assert_eq!(envelope["returned"], 10);
    }

    #[test]
    fn a_result_without_a_list_has_null_counts() {
        let envelope = build(
            "score",
            Pagination::AllLists,
            json!({"score": 90}),
            DEFAULT_PAGE,
            None,
            false,
        );
        assert!(envelope["total_items"].is_null());
        assert!(envelope["returned"].is_null());
    }

    fn files_with_functions() -> Value {
        let files: Vec<Value> = (0..3)
            .map(|file| {
                let functions: Vec<Value> = (0..100)
                    .map(|function| json!({"name": format!("f{file}_{function}")}))
                    .collect();
                json!({"path": format!("{file}.rs"), "functions": functions})
            })
            .collect();
        json!({"files": files})
    }

    const FUNCTIONS: Pagination = Pagination::Nested {
        parents: &["files"],
        child: "functions",
    };

    #[test]
    fn nested_pagination_applies_one_limit_to_all_child_lists() {
        let envelope = build(
            "complexity",
            FUNCTIONS,
            files_with_functions(),
            DEFAULT_PAGE,
            None,
            false,
        );
        let lengths: Vec<usize> = envelope["result"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["functions"].as_array().unwrap().len())
            .collect();
        assert_eq!(envelope["total_items"], 300);
        assert_eq!(envelope["returned"], 50);
        assert_eq!(lengths, [50, 0, 0]);
    }

    #[test]
    fn nested_pagination_applies_one_offset_to_all_child_lists() {
        let envelope = build(
            "complexity",
            FUNCTIONS,
            files_with_functions(),
            page(20, 190),
            None,
            false,
        );
        let files = envelope["result"]["files"].as_array().unwrap();
        let lengths: Vec<usize> = files
            .iter()
            .map(|file| file["functions"].as_array().unwrap().len())
            .collect();
        assert_eq!(lengths, [0, 10, 10]);
        assert_eq!(files[1]["functions"][0]["name"], "f1_90");
        assert_eq!(files[2]["functions"][9]["name"], "f2_9");
    }

    #[test]
    fn nested_pagination_goes_through_each_parent_list() {
        let level = |names: &[&str]| json!({"symbols": names});
        let pagination = Pagination::Nested {
            parents: &["callers", "callees"],
            child: "symbols",
        };
        let envelope = build(
            "impact",
            pagination,
            json!({
                "callers": [level(&["a", "b"]), level(&["c"])],
                "callees": [level(&["d", "e"])]
            }),
            page(3, 1),
            None,
            false,
        );
        assert_eq!(envelope["total_items"], 5);
        assert_eq!(envelope["returned"], 3);
        assert_eq!(
            envelope["result"]["callers"],
            json!([level(&["b"]), level(&["c"])])
        );
        assert_eq!(envelope["result"]["callees"], json!([level(&["d"])]));
    }

    #[test]
    fn nested_pagination_has_null_counts_without_a_parent_list() {
        let envelope = build(
            "complexity",
            FUNCTIONS,
            json!({"summary": 1}),
            DEFAULT_PAGE,
            None,
            false,
        );
        assert!(envelope["total_items"].is_null());
    }

    #[test]
    fn children_pagination_applies_one_limit_to_all_child_lists() {
        let envelope = build(
            "graph",
            Pagination::Children("cycles"),
            json!({"cycles": [["a", "b"], ["c", "d", "e"]], "nodes": [1, 2, 3]}),
            page(2, 1),
            None,
            false,
        );
        assert_eq!(envelope["total_items"], 5);
        assert_eq!(envelope["returned"], 2);
        assert_eq!(envelope["result"]["cycles"], json!([["b"], ["c"]]));
        assert_eq!(envelope["result"]["nodes"], json!([1, 2, 3]));
    }

    #[test]
    fn compact_keeps_the_pagination_fields() {
        let records: Vec<_> = (0..100)
            .map(|i| {
                json!({
                    "qualified_name": format!("module/file.rs:symbol_{i}"),
                    "file": "module/file.rs", "line": i + 1
                })
            })
            .collect();
        let value = json!({"symbols": records});
        let packed = build(
            "repomap",
            Pagination::Field("symbols"),
            value.clone(),
            page(30, 10),
            None,
            true,
        );
        assert_eq!(packed["total_items"], 100);
        assert_eq!(packed["returned"], 30);
        assert_eq!(packed["offset"], 10);
        assert_eq!(packed["table_paths"], json!(["/symbols"]));
        let table = &packed["result"]["symbols"];
        let columns = table["columns"].as_array().unwrap();
        let line_column = columns.iter().position(|v| v == "line").unwrap();
        assert_eq!(table["rows"][0][line_column], 11);
        assert_eq!(table["rows"][29][line_column], 40);

        let normal = build(
            "repomap",
            Pagination::Field("symbols"),
            value,
            page(30, 10),
            None,
            false,
        );
        assert!(normal.get("encoding").is_none());
        assert!(normal["result"]["symbols"].is_array());
    }
}
