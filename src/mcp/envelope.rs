//! The envelope that holds the result of a tool, and its pagination.
//!
//! Each tool gives the same envelope: `tool`, `total_items`, `returned`,
//! `offset`, `has_more`, `next_offset`, `truncated`, and `result`.

use serde::Serialize;
use serde_json::{json, Value};

use super::compact;
use super::tools::{Pagination, ToolData};

/// The part of the primary list that a call requests.
#[derive(Clone, Copy, Debug)]
pub(super) struct Page {
    /// The maximum number of items. The value 0 removes the limit.
    pub limit: usize,
    pub offset: usize,
}

/// One list in the result that does not hold all its items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Truncation {
    /// A JSON pointer from `result` to the list. The segment `*` means each
    /// element of an array.
    pub path: String,
    pub total: usize,
    pub returned: usize,
}

/// The number of items that a tool without pagination has and gives.
#[derive(Clone, Copy, Debug)]
pub(super) struct ItemCount {
    pub total: usize,
    pub returned: usize,
}

/// Applies the page to the result and puts the result in the envelope.
pub(super) fn build(
    tool: &str,
    pagination: Pagination,
    data: ToolData,
    page: Page,
    compact: bool,
) -> Value {
    let mut result = data.result;
    let mut paging = paginate(pagination, &mut result, page);
    if let Some(items) = data.items {
        paging.total = Some(items.total);
        paging.returned = Some(items.returned);
    }
    for entry in data.truncated {
        // When the tool and the pagination cut the same list, one entry
        // gives the total before the two cuts.
        match paging
            .truncated
            .iter_mut()
            .find(|existing| existing.path == entry.path)
        {
            Some(existing) => existing.total = existing.total.max(entry.total),
            None => paging.truncated.push(entry),
        }
    }

    let mut envelope = json!({
        "tool": tool,
        "total_items": paging.total,
        "returned": paging.returned,
        "offset": page.offset,
        "has_more": paging.next_offset.is_some(),
        "next_offset": paging.next_offset,
        "truncated": paging.truncated,
        "result": result
    });
    if let Some(reason) = data.git_skipped_reason {
        envelope["git_skipped_reason"] = json!(reason);
    }
    if compact {
        envelope = compact::encode(envelope);
    }
    envelope
}

/// What the pagination did to one result.
#[derive(Debug, Default)]
struct Paging {
    total: Option<usize>,
    returned: Option<usize>,
    /// The offset of the next page. `None` when no more items follow.
    next_offset: Option<usize>,
    truncated: Vec<Truncation>,
}

impl Paging {
    /// The paging of a result in which the pagination found no list to cut.
    fn untouched(count: Option<usize>) -> Self {
        Self {
            total: count,
            returned: count,
            ..Self::default()
        }
    }
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

/// One limit and one offset that apply to many arrays as one sequence.
struct Sequence {
    page: Page,
    remaining_limit: usize,
    remaining_offset: usize,
    total: usize,
    returned: usize,
    truncated: Vec<Truncation>,
}

impl Sequence {
    fn new(page: Page) -> Self {
        Self {
            page,
            remaining_limit: if page.limit == 0 {
                usize::MAX
            } else {
                page.limit
            },
            remaining_offset: page.offset,
            total: 0,
            returned: 0,
            truncated: Vec::new(),
        }
    }

    /// Cuts the arrays, which all have the same `path`.
    fn apply<'a>(&mut self, path: String, arrays: impl Iterator<Item = &'a mut Vec<Value>>) {
        let mut total = 0;
        let mut returned = 0;
        for items in arrays {
            total += items.len();
            let skip = self.remaining_offset.min(items.len());
            items.drain(0..skip);
            self.remaining_offset -= skip;
            items.truncate(self.remaining_limit);
            self.remaining_limit -= items.len();
            returned += items.len();
        }
        self.total += total;
        self.returned += returned;
        if returned < total {
            self.truncated.push(Truncation {
                path,
                total,
                returned,
            });
        }
    }

    fn finish(self) -> Paging {
        let end = self.page.offset.saturating_add(self.returned);
        Paging {
            total: Some(self.total),
            returned: Some(self.returned),
            next_offset: (end < self.total).then_some(end),
            truncated: self.truncated,
        }
    }
}

fn paginate(pagination: Pagination, value: &mut Value, page: Page) -> Paging {
    match pagination {
        Pagination::None => Paging::default(),
        Pagination::Field(field) => {
            let Some(items) = value.get_mut(field).and_then(Value::as_array_mut) else {
                return Paging::untouched(count_all_lists(value));
            };
            let mut sequence = Sequence::new(page);
            sequence.apply(format!("/{field}"), std::iter::once(items));
            sequence.finish()
        }
        Pagination::Nested { parents, child } => {
            let mut sequence = Sequence::new(page);
            let mut found_parent = false;
            for parent in parents {
                let Some(elements) = value.get_mut(*parent).and_then(Value::as_array_mut) else {
                    continue;
                };
                found_parent = true;
                sequence.apply(
                    format!("/{parent}/*/{child}"),
                    elements
                        .iter_mut()
                        .filter_map(|element| element.get_mut(child)?.as_array_mut()),
                );
            }
            if found_parent {
                sequence.finish()
            } else {
                Paging::untouched(None)
            }
        }
        Pagination::Children(field) => {
            let Some(children) = value.get_mut(field).and_then(Value::as_array_mut) else {
                return Paging::untouched(None);
            };
            let mut sequence = Sequence::new(page);
            sequence.apply(
                format!("/{field}/*"),
                children.iter_mut().filter_map(Value::as_array_mut),
            );
            sequence.finish()
        }
        Pagination::Entries(field) => {
            let Some(entries) = value.get_mut(field).and_then(Value::as_object_mut) else {
                return Paging::untouched(None);
            };
            // The map has its keys in sorted sequence, thus the pages are stable.
            let mut items: Vec<Value> = std::mem::take(entries)
                .into_iter()
                .map(|(key, entry)| json!([key, entry]))
                .collect();
            let mut sequence = Sequence::new(page);
            sequence.apply(format!("/{field}"), std::iter::once(&mut items));
            for item in items {
                if let Value::Array(mut pair) = item {
                    if let (Some(entry), Some(Value::String(key))) = (pair.pop(), pair.pop()) {
                        entries.insert(key, entry);
                    }
                }
            }
            sequence.finish()
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

    fn envelope(tool: &str, pagination: Pagination, result: Value, page: Page) -> Value {
        build(tool, pagination, ToolData::new(result), page, false)
    }

    const ITEMS: Pagination = Pagination::Field("items");

    #[test]
    fn envelope_has_the_same_keys_for_each_tool() {
        let paginated = envelope(
            "satd",
            ITEMS,
            json!({"items": [1, 2, 3, 4, 5]}),
            DEFAULT_PAGE,
        );
        let unpaginated = envelope(
            "task_context",
            Pagination::None,
            json!({"symbols": [1]}),
            page(0, 0),
        );
        let keys =
            |value: &Value| -> Vec<String> { value.as_object().unwrap().keys().cloned().collect() };
        assert_eq!(
            keys(&paginated),
            [
                "has_more",
                "next_offset",
                "offset",
                "result",
                "returned",
                "tool",
                "total_items",
                "truncated"
            ]
        );
        assert_eq!(keys(&paginated), keys(&unpaginated));
    }

    #[test]
    fn envelope_of_a_full_list_has_no_more_items() {
        let envelope = envelope(
            "satd",
            ITEMS,
            json!({"items": [1, 2, 3, 4, 5]}),
            DEFAULT_PAGE,
        );
        assert_eq!(envelope["tool"], "satd");
        assert_eq!(envelope["total_items"], 5);
        assert_eq!(envelope["returned"], 5);
        assert_eq!(envelope["offset"], 0);
        assert_eq!(envelope["has_more"], false);
        assert!(envelope["next_offset"].is_null());
        assert_eq!(envelope["truncated"], json!([]));
        assert_eq!(envelope["result"]["items"], json!([1, 2, 3, 4, 5]));
    }

    #[test]
    fn envelope_reports_skipped_git_data() {
        let data = ToolData {
            git_skipped_reason: Some("not a git repository".to_string()),
            ..ToolData::new(json!({"components": {}}))
        };
        let envelope = build(
            "score",
            Pagination::Entries("components"),
            data,
            DEFAULT_PAGE,
            false,
        );
        assert_eq!(envelope["git_skipped_reason"], "not a git repository");
    }

    #[test]
    fn default_limit_keeps_the_first_50_items_and_gives_the_next_offset() {
        let items: Vec<i64> = (0..100).collect();
        let envelope = envelope("satd", ITEMS, json!({"items": items}), DEFAULT_PAGE);
        assert_eq!(envelope["returned"], 50);
        assert_eq!(envelope["total_items"], 100);
        assert_eq!(envelope["has_more"], true);
        assert_eq!(envelope["next_offset"], 50);
        assert_eq!(
            envelope["truncated"],
            json!([{"path": "/items", "total": 100, "returned": 50}])
        );
    }

    #[test]
    fn the_last_page_has_no_next_offset() {
        let items: Vec<i64> = (0..100).collect();
        let envelope = envelope("satd", ITEMS, json!({"items": items}), page(50, 50));
        assert_eq!(envelope["returned"], 50);
        assert_eq!(envelope["has_more"], false);
        assert!(envelope["next_offset"].is_null());
        assert_eq!(
            envelope["truncated"],
            json!([{"path": "/items", "total": 100, "returned": 50}])
        );
    }

    #[test]
    fn an_offset_after_the_end_gives_no_items_and_no_next_offset() {
        let envelope = envelope("satd", ITEMS, json!({"items": [1, 2]}), page(50, 7));
        assert_eq!(envelope["returned"], 0);
        assert_eq!(envelope["has_more"], false);
        assert!(envelope["next_offset"].is_null());
    }

    #[test]
    fn zero_limit_keeps_all_items() {
        let items: Vec<i64> = (0..100).collect();
        let envelope = envelope("satd", ITEMS, json!({"items": items}), page(0, 0));
        assert_eq!(envelope["returned"], 100);
        assert_eq!(envelope["has_more"], false);
        assert_eq!(envelope["truncated"], json!([]));
    }

    #[test]
    fn offset_skips_items_before_the_limit_applies() {
        let items: Vec<i64> = (0..20).collect();
        let envelope = envelope("satd", ITEMS, json!({"items": items}), page(5, 10));
        assert_eq!(envelope["returned"], 5);
        assert_eq!(envelope["offset"], 10);
        assert_eq!(envelope["has_more"], true);
        assert_eq!(envelope["next_offset"], 15);
        assert_eq!(envelope["result"]["items"], json!([10, 11, 12, 13, 14]));
    }

    #[test]
    fn field_pagination_does_not_cut_other_lists() {
        let envelope = envelope(
            "satd",
            ITEMS,
            json!({"items": [1, 2, 3], "files": [1, 2, 3]}),
            page(1, 0),
        );
        assert_eq!(envelope["result"]["items"], json!([1]));
        assert_eq!(envelope["result"]["files"], json!([1, 2, 3]));
    }

    #[test]
    fn field_pagination_counts_all_lists_when_the_field_is_absent() {
        let envelope = envelope(
            "get_symbol",
            Pagination::Field("callers"),
            json!({"choices": [1, 2]}),
            page(1, 0),
        );
        assert_eq!(envelope["total_items"], 2);
        assert_eq!(envelope["returned"], 2);
        assert_eq!(envelope["has_more"], false);
        assert_eq!(envelope["result"]["choices"], json!([1, 2]));
    }

    const COMPONENTS: Pagination = Pagination::Entries("components");

    fn score_result() -> Value {
        json!({
            "overall_score": 80,
            "components": {
                "cohesion": {"score": 90}, "complexity": {"score": 70}, "satd": {"score": 60}
            },
            "skipped_components": [{"name": "coupling"}, {"name": "smells"}]
        })
    }

    #[test]
    fn entries_pagination_with_limit_1_gives_the_first_entry_and_each_envelope_field() {
        let envelope = envelope("score", COMPONENTS, score_result(), page(1, 0));
        assert_eq!(
            envelope,
            json!({
                "tool": "score",
                "total_items": 3,
                "returned": 1,
                "offset": 0,
                "has_more": true,
                "next_offset": 1,
                "truncated": [{"path": "/components", "total": 3, "returned": 1}],
                "result": {
                    "overall_score": 80,
                    "components": {"cohesion": {"score": 90}},
                    "skipped_components": [{"name": "coupling"}, {"name": "smells"}]
                }
            })
        );
    }

    #[test]
    fn entries_pagination_applies_the_offset_in_the_sequence_of_the_keys() {
        let envelope = envelope("score", COMPONENTS, score_result(), page(5, 1));
        assert_eq!(
            envelope["result"]["components"],
            json!({"complexity": {"score": 70}, "satd": {"score": 60}})
        );
        assert_eq!(envelope["total_items"], 3);
        assert_eq!(envelope["returned"], 2);
        assert_eq!(envelope["has_more"], false);
        assert!(envelope["next_offset"].is_null());
    }

    #[test]
    fn entries_pagination_has_null_counts_without_the_object() {
        let envelope = envelope("score", COMPONENTS, json!({"score": 90}), DEFAULT_PAGE);
        assert!(envelope["total_items"].is_null());
        assert!(envelope["returned"].is_null());
        assert_eq!(envelope["has_more"], false);
        assert_eq!(envelope["truncated"], json!([]));
    }

    /// The items of the primary list of a result, as one sequence.
    fn primary_items(pagination: Pagination, result: &Value) -> Vec<Value> {
        let arrays = |values: Vec<&Value>| -> Vec<Value> {
            values
                .into_iter()
                .filter_map(Value::as_array)
                .flatten()
                .cloned()
                .collect()
        };
        match pagination {
            Pagination::None => Vec::new(),
            Pagination::Field(field) => arrays(vec![&result[field]]),
            Pagination::Nested { parents, child } => arrays(
                parents
                    .iter()
                    .filter_map(|parent| result[*parent].as_array())
                    .flatten()
                    .map(|element| &element[child])
                    .collect(),
            ),
            Pagination::Children(field) => {
                arrays(result[field].as_array().into_iter().flatten().collect())
            }
            Pagination::Entries(field) => result[field]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(key, entry)| json!([key, entry]))
                .collect(),
        }
    }

    #[test]
    fn the_pages_of_each_pagination_kind_give_each_item_one_time() {
        let level = |names: &[&str]| json!({"symbols": names});
        let cases = [
            (
                ITEMS,
                json!({"items": [1, 2, 3, 4, 5, 6, 7], "other": [1, 2]}),
            ),
            (FUNCTIONS, files_with_functions()),
            (
                Pagination::Nested {
                    parents: &["callers", "callees"],
                    child: "symbols",
                },
                json!({
                    "callers": [level(&["a", "b"]), level(&[]), level(&["c"])],
                    "callees": [level(&["d", "e", "f"])]
                }),
            ),
            (
                Pagination::Children("cycles"),
                json!({"cycles": [["a", "b"], [], ["c", "d", "e"]], "nodes": [1, 2, 3]}),
            ),
            (COMPONENTS, score_result()),
        ];
        for (pagination, result) in cases {
            let all = primary_items(pagination, &result);
            for limit in [1, 2, 3, 7, 1000] {
                let mut collected = Vec::new();
                let mut offset = 0;
                loop {
                    let page = envelope("tool", pagination, result.clone(), page(limit, offset));
                    let items = primary_items(pagination, &page["result"]);
                    assert_eq!(page["returned"], items.len(), "{pagination:?}");
                    assert_eq!(page["total_items"], all.len(), "{pagination:?}");
                    assert_eq!(page["offset"], offset);
                    collected.extend(items);
                    if page["has_more"] == false {
                        assert!(page["next_offset"].is_null());
                        break;
                    }
                    let next = page["next_offset"].as_u64().expect("next offset") as usize;
                    assert!(next > offset, "{pagination:?}: the offset must increase");
                    offset = next;
                }
                assert_eq!(collected, all, "{pagination:?} with limit {limit}");
            }
        }
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
        let envelope = envelope(
            "complexity",
            FUNCTIONS,
            files_with_functions(),
            DEFAULT_PAGE,
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
        assert_eq!(envelope["next_offset"], 50);
        assert_eq!(
            envelope["truncated"],
            json!([{"path": "/files/*/functions", "total": 300, "returned": 50}])
        );
    }

    #[test]
    fn nested_pagination_applies_one_offset_to_all_child_lists() {
        let envelope = envelope(
            "complexity",
            FUNCTIONS,
            files_with_functions(),
            page(20, 190),
        );
        let files = envelope["result"]["files"].as_array().unwrap();
        let lengths: Vec<usize> = files
            .iter()
            .map(|file| file["functions"].as_array().unwrap().len())
            .collect();
        assert_eq!(lengths, [0, 10, 10]);
        assert_eq!(files[1]["functions"][0]["name"], "f1_90");
        assert_eq!(files[2]["functions"][9]["name"], "f2_9");
        assert_eq!(envelope["next_offset"], 210);
    }

    #[test]
    fn nested_pagination_goes_through_each_parent_list() {
        let level = |names: &[&str]| json!({"symbols": names});
        let pagination = Pagination::Nested {
            parents: &["callers", "callees"],
            child: "symbols",
        };
        let envelope = envelope(
            "impact",
            pagination,
            json!({
                "callers": [level(&["a", "b"]), level(&["c"])],
                "callees": [level(&["d", "e"])]
            }),
            page(3, 1),
        );
        assert_eq!(envelope["total_items"], 5);
        assert_eq!(envelope["returned"], 3);
        assert_eq!(
            envelope["result"]["callers"],
            json!([level(&["b"]), level(&["c"])])
        );
        assert_eq!(envelope["result"]["callees"], json!([level(&["d"])]));
        assert_eq!(envelope["next_offset"], 4);
        assert_eq!(
            envelope["truncated"],
            json!([
                {"path": "/callers/*/symbols", "total": 3, "returned": 2},
                {"path": "/callees/*/symbols", "total": 2, "returned": 1}
            ])
        );
    }

    #[test]
    fn nested_pagination_has_null_counts_without_a_parent_list() {
        let envelope = envelope("complexity", FUNCTIONS, json!({"summary": 1}), DEFAULT_PAGE);
        assert!(envelope["total_items"].is_null());
        assert_eq!(envelope["has_more"], false);
    }

    #[test]
    fn children_pagination_applies_one_limit_to_all_child_lists() {
        let envelope = envelope(
            "graph",
            Pagination::Children("cycles"),
            json!({"cycles": [["a", "b"], ["c", "d", "e"]], "nodes": [1, 2, 3]}),
            page(2, 1),
        );
        assert_eq!(envelope["total_items"], 5);
        assert_eq!(envelope["returned"], 2);
        assert_eq!(envelope["result"]["cycles"], json!([["b"], ["c"]]));
        assert_eq!(envelope["result"]["nodes"], json!([1, 2, 3]));
        assert_eq!(envelope["next_offset"], 3);
        assert_eq!(
            envelope["truncated"],
            json!([{"path": "/cycles/*", "total": 5, "returned": 2}])
        );
    }

    #[test]
    fn children_pagination_has_null_counts_without_the_list() {
        let envelope = envelope(
            "graph",
            Pagination::Children("cycles"),
            json!({"nodes": [1]}),
            DEFAULT_PAGE,
        );
        assert!(envelope["total_items"].is_null());
    }

    #[test]
    fn a_tool_without_pagination_gives_its_own_counts_and_truncations() {
        let data = ToolData {
            items: Some(ItemCount {
                total: 9,
                returned: 2,
            }),
            truncated: vec![Truncation {
                path: "/symbols".to_string(),
                total: 9,
                returned: 2,
            }],
            ..ToolData::new(json!({"symbols": [1, 2]}))
        };
        let envelope = build("task_context", Pagination::None, data, page(0, 0), false);
        assert_eq!(envelope["total_items"], 9);
        assert_eq!(envelope["returned"], 2);
        assert_eq!(envelope["offset"], 0);
        assert_eq!(envelope["has_more"], false);
        assert!(envelope["next_offset"].is_null());
        assert_eq!(
            envelope["truncated"],
            json!([{"path": "/symbols", "total": 9, "returned": 2}])
        );
        assert_eq!(envelope["result"]["symbols"], json!([1, 2]));
    }

    #[test]
    fn a_list_that_the_tool_and_the_pagination_cut_has_one_entry() {
        let data = ToolData {
            truncated: vec![Truncation {
                path: "/items".to_string(),
                total: 9,
                returned: 4,
            }],
            ..ToolData::new(json!({"items": [1, 2, 3, 4]}))
        };
        let envelope = build("search", ITEMS, data, page(2, 0), false);
        assert_eq!(
            envelope["truncated"],
            json!([{"path": "/items", "total": 9, "returned": 2}])
        );
        assert_eq!(envelope["total_items"], 4);
        assert_eq!(envelope["next_offset"], 2);
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
        let symbols = Pagination::Field("symbols");
        let packed = build(
            "repomap",
            symbols,
            ToolData::new(value.clone()),
            page(30, 10),
            true,
        );
        assert_eq!(packed["total_items"], 100);
        assert_eq!(packed["returned"], 30);
        assert_eq!(packed["offset"], 10);
        assert_eq!(packed["next_offset"], 40);
        assert_eq!(packed["table_paths"], json!(["/symbols"]));
        let table = &packed["result"]["symbols"];
        let columns = table["columns"].as_array().unwrap();
        let line_column = columns.iter().position(|v| v == "line").unwrap();
        assert_eq!(table["rows"][0][line_column], 11);
        assert_eq!(table["rows"][29][line_column], 40);

        let normal = envelope("repomap", symbols, value, page(30, 10));
        assert!(normal.get("encoding").is_none());
        assert!(normal["result"]["symbols"].is_array());
    }
}
