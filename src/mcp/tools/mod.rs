//! The specification of each MCP tool.
//!
//! One [`ToolSpec`] describes one tool. The schema in `tools/list`, the
//! validation of arguments, and the list of tool names come from these
//! specifications. Do not keep a second list of tools or parameters.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use super::envelope::{ItemCount, Truncation};
use super::error::ToolError;
use super::McpServer;

mod analysis;
mod navigation;
pub(super) mod schema;
mod search;
pub(super) mod validate;

/// The largest value of an integer parameter that has no natural limit.
pub(super) const MAX_COUNT: u64 = u32::MAX as u64;

/// The default value of `limit`.
pub(super) const DEFAULT_LIMIT: u64 = 50;

pub(super) type HandlerResult = Result<ToolOutput, ToolError>;

/// The function that runs a tool. The arguments are valid for the specification.
pub(super) type Handler = fn(&McpServer, &Arguments<'_>) -> HandlerResult;

/// The single source of truth for one tool.
pub(super) struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// The parameters of this tool. `limit` and `offset` are not in this
    /// list: `paginated` adds them.
    pub params: &'static [Param],
    pub paginated: Pagination,
    pub handler: Handler,
}

/// One parameter of a tool.
#[derive(Clone, Copy, Debug)]
pub(super) struct Param {
    pub name: &'static str,
    pub kind: ParamType,
    pub required: bool,
    pub default: Option<ParamDefault>,
    pub description: &'static str,
}

/// The type of a parameter. The bounds are inclusive.
#[derive(Clone, Copy, Debug)]
pub(super) enum ParamType {
    String,
    Integer { minimum: u64, maximum: u64 },
    Number { minimum: f64, maximum: f64 },
    Boolean,
    Enum(&'static [&'static str]),
    StringArray,
}

/// The value that the server uses when a call does not give the parameter.
#[derive(Clone, Copy, Debug)]
pub(super) enum ParamDefault {
    String(&'static str),
    Integer(u64),
    Number(f64),
    Boolean(bool),
}

/// The list in the result to which `limit` and `offset` apply.
#[derive(Clone, Copy, Debug)]
pub(super) enum Pagination {
    /// The tool controls its own size. It has no `limit` and no `offset`.
    None,
    /// One array at the top level of the result.
    Field(&'static str),
    /// The `child` arrays in the elements of each `parents` array. The limit
    /// and the offset apply to all the child arrays as one sequence.
    Nested {
        parents: &'static [&'static str],
        child: &'static str,
    },
    /// The arrays that are the elements of one array, as one sequence.
    Children(&'static str),
    /// Each array at the top level of the result, independently.
    AllLists,
}

impl Param {
    const fn new(name: &'static str, kind: ParamType, description: &'static str) -> Self {
        Self {
            name,
            kind,
            required: false,
            default: None,
            description,
        }
    }

    pub(super) const fn string(name: &'static str, description: &'static str) -> Self {
        Self::new(name, ParamType::String, description)
    }

    pub(super) const fn integer(
        name: &'static str,
        minimum: u64,
        maximum: u64,
        description: &'static str,
    ) -> Self {
        Self::new(name, ParamType::Integer { minimum, maximum }, description)
    }

    pub(super) const fn number(
        name: &'static str,
        minimum: f64,
        maximum: f64,
        description: &'static str,
    ) -> Self {
        Self::new(name, ParamType::Number { minimum, maximum }, description)
    }

    pub(super) const fn boolean(name: &'static str, description: &'static str) -> Self {
        Self::new(name, ParamType::Boolean, description)
    }

    pub(super) const fn choice(
        name: &'static str,
        values: &'static [&'static str],
        description: &'static str,
    ) -> Self {
        Self::new(name, ParamType::Enum(values), description)
    }

    pub(super) const fn string_array(name: &'static str, description: &'static str) -> Self {
        Self::new(name, ParamType::StringArray, description)
    }

    pub(super) const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub(super) const fn with_default(mut self, default: ParamDefault) -> Self {
        self.default = Some(default);
        self
    }
}

pub(super) const PATH: Param = Param::string(
    "path",
    "The file or directory to analyze. The default is the repository root.",
);

pub(super) const COMPACT: Param = Param::boolean(
    "compact",
    "If true, the server writes each list of flat records as a table. \
     `table_paths` gives a JSON pointer to each table in `result`. \
     Join the `columns` of a table with each of its `rows` to get the records. \
     The server keeps the usual JSON when the table is not smaller.",
)
.with_default(ParamDefault::Boolean(false));

const LIMIT: Param = Param::integer(
    "limit",
    0,
    MAX_COUNT,
    "The maximum number of items in the result. The value 0 removes the limit.",
)
.with_default(ParamDefault::Integer(DEFAULT_LIMIT));

const OFFSET: Param = Param::integer(
    "offset",
    0,
    MAX_COUNT,
    "The number of items to skip. Use `next_offset` from the last result.",
)
.with_default(ParamDefault::Integer(0));

const PAGINATION_PARAMS: [Param; 2] = [LIMIT, OFFSET];

impl ToolSpec {
    /// All the parameters that a call can give, in a stable sequence.
    pub(super) fn parameters(&self) -> impl Iterator<Item = &Param> {
        let pagination: &[Param] = match self.paginated {
            Pagination::None => &[],
            _ => &PAGINATION_PARAMS,
        };
        self.params.iter().chain(pagination)
    }

    pub(super) fn parameter(&self, name: &str) -> Option<&Param> {
        self.parameters().find(|param| param.name == name)
    }
}

/// The tools in the sequence that `tools/list` shows them.
static TOOLS: [&ToolSpec; 26] = [
    &navigation::CONTEXT,
    &navigation::OUTLINE,
    &analysis::COMPLEXITY,
    &analysis::SATD,
    &analysis::DEADCODE,
    &analysis::STUBS,
    &analysis::CHURN,
    &analysis::CLONES,
    &analysis::DEFECT,
    &analysis::CHANGES,
    &analysis::DIFF,
    &analysis::TDG,
    &analysis::GRAPH,
    &analysis::HOTSPOT,
    &analysis::TEMPORAL,
    &analysis::OWNERSHIP,
    &analysis::COHESION,
    &analysis::REPOMAP,
    &analysis::SMELLS,
    &analysis::FLAGS,
    &analysis::SCORE,
    &search::SEMANTIC_SEARCH,
    &navigation::GET_SYMBOL,
    &navigation::TASK_CONTEXT,
    &navigation::IMPACT,
    &search::SEMANTIC_SEARCH_HYDE,
];

pub(super) fn all() -> &'static [&'static ToolSpec] {
    &TOOLS
}

pub(super) fn find(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().copied().find(|spec| spec.name == name)
}

pub(super) fn names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| TOOLS.iter().map(|spec| spec.name).collect())
}

/// The arguments of one call, read through the specification of the tool.
///
/// Each accessor gives the value from the call, or the default of the
/// parameter when the call does not give one.
pub(super) struct Arguments<'a> {
    spec: &'static ToolSpec,
    values: Option<&'a Map<String, Value>>,
}

impl<'a> Arguments<'a> {
    pub(super) fn new(spec: &'static ToolSpec, values: &'a Value) -> Self {
        Self {
            spec,
            values: values.as_object(),
        }
    }

    fn param(&self, name: &str) -> Option<&'static Param> {
        let param = self.spec.parameter(name);
        debug_assert!(
            param.is_some(),
            "tool `{}` reads `{name}`, which is not in its specification",
            self.spec.name
        );
        param
    }

    fn value(&self, name: &str) -> Option<&'a Value> {
        self.values.and_then(|values| values.get(name))
    }

    /// True when the call gives a value for the parameter.
    pub(super) fn given(&self, name: &str) -> bool {
        self.param(name).is_some() && self.value(name).is_some()
    }

    pub(super) fn string(&self, name: &str) -> Option<&'a str> {
        let param = self.param(name)?;
        self.value(name)
            .and_then(Value::as_str)
            .or(match param.default {
                Some(ParamDefault::String(default)) => Some(default),
                _ => None,
            })
    }

    pub(super) fn integer(&self, name: &str) -> Option<u64> {
        let param = self.param(name)?;
        self.value(name)
            .and_then(Value::as_u64)
            .or(match param.default {
                Some(ParamDefault::Integer(default)) => Some(default),
                _ => None,
            })
    }

    /// An integer parameter as a `usize`.
    pub(super) fn count(&self, name: &str) -> Option<usize> {
        self.integer(name)
            .and_then(|value| usize::try_from(value).ok())
    }

    /// An integer parameter as a `u32`. The bounds of each integer parameter
    /// are not larger than `u32::MAX`, thus the conversion keeps the value.
    pub(super) fn count_u32(&self, name: &str) -> Option<u32> {
        self.integer(name)
            .and_then(|value| u32::try_from(value).ok())
    }

    pub(super) fn number(&self, name: &str) -> Option<f64> {
        let param = self.param(name)?;
        self.value(name)
            .and_then(Value::as_f64)
            .or(match param.default {
                Some(ParamDefault::Number(default)) => Some(default),
                _ => None,
            })
    }

    pub(super) fn boolean(&self, name: &str) -> Option<bool> {
        let param = self.param(name)?;
        self.value(name)
            .and_then(Value::as_bool)
            .or(match param.default {
                Some(ParamDefault::Boolean(default)) => Some(default),
                _ => None,
            })
    }

    pub(super) fn strings(&self, name: &str) -> Option<Vec<&'a str>> {
        self.param(name)?;
        self.value(name)
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
    }
}

/// What a handler gives back to the server.
pub(super) enum ToolOutput {
    /// Text that the server sends as it is, without an envelope.
    Text(String),
    /// A JSON result that the server puts in the envelope.
    Data(ToolData),
}

pub(super) struct ToolData {
    pub result: Value,
    pub git_skipped_reason: Option<String>,
    /// The lists that the tool cut. The envelope adds the lists that the
    /// pagination cut.
    pub truncated: Vec<Truncation>,
    /// The item counts of a tool without pagination.
    pub items: Option<ItemCount>,
}

impl ToolData {
    pub(super) fn new(result: Value) -> Self {
        Self {
            result,
            git_skipped_reason: None,
            truncated: Vec::new(),
            items: None,
        }
    }
}

impl ToolOutput {
    pub(super) fn data(result: Value) -> Self {
        Self::Data(ToolData::new(result))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn names_come_from_the_specifications() {
        let from_specifications: Vec<&str> = all().iter().map(|spec| spec.name).collect();
        assert_eq!(names(), from_specifications.as_slice());
        assert_eq!(McpServer::tool_names(), from_specifications.as_slice());
    }

    #[test]
    fn each_tool_name_is_unique() {
        let unique: BTreeSet<&str> = names().iter().copied().collect();
        assert_eq!(unique.len(), all().len());
    }

    #[test]
    fn each_parameter_name_is_unique_in_its_tool() {
        for spec in all() {
            let names: Vec<&str> = spec.parameters().map(|param| param.name).collect();
            let unique: BTreeSet<&str> = names.iter().copied().collect();
            assert_eq!(unique.len(), names.len(), "tool {}", spec.name);
        }
    }

    #[test]
    fn find_gives_the_specification_of_a_known_tool_only() {
        assert_eq!(find("impact").map(|spec| spec.name), Some("impact"));
        assert!(find("no_such_tool").is_none());
    }

    #[test]
    fn a_paginated_tool_has_limit_and_offset() {
        for spec in all() {
            let paginated = !matches!(spec.paginated, Pagination::None);
            assert_eq!(
                spec.parameter("limit").is_some(),
                paginated,
                "{}",
                spec.name
            );
            assert_eq!(
                spec.parameter("offset").is_some(),
                paginated,
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn task_context_is_the_only_tool_without_pagination() {
        let without: Vec<&str> = all()
            .iter()
            .filter(|spec| matches!(spec.paginated, Pagination::None))
            .map(|spec| spec.name)
            .collect();
        assert_eq!(without, ["task_context"]);
    }

    #[test]
    fn each_default_has_the_type_of_its_parameter() {
        for spec in all() {
            for param in spec.parameters() {
                let agrees = matches!(
                    (param.kind, param.default),
                    (_, None)
                        | (ParamType::String, Some(ParamDefault::String(_)))
                        | (ParamType::Integer { .. }, Some(ParamDefault::Integer(_)))
                        | (ParamType::Number { .. }, Some(ParamDefault::Number(_)))
                        | (ParamType::Boolean, Some(ParamDefault::Boolean(_)))
                        | (ParamType::Enum(_), Some(ParamDefault::String(_)))
                );
                assert!(agrees, "{}.{}", spec.name, param.name);
                assert!(
                    !(param.required && param.default.is_some()),
                    "{}.{} is required, thus it cannot have a default",
                    spec.name,
                    param.name
                );
            }
        }
    }

    #[test]
    fn each_default_is_in_the_bounds_of_its_parameter() {
        for spec in all() {
            for param in spec.parameters() {
                let in_bounds = match (param.kind, param.default) {
                    (ParamType::Integer { minimum, maximum }, Some(ParamDefault::Integer(v))) => {
                        (minimum..=maximum).contains(&v)
                    }
                    (ParamType::Number { minimum, maximum }, Some(ParamDefault::Number(v))) => {
                        (minimum..=maximum).contains(&v)
                    }
                    (ParamType::Enum(values), Some(ParamDefault::String(v))) => values.contains(&v),
                    _ => true,
                };
                assert!(in_bounds, "{}.{}", spec.name, param.name);
            }
        }
    }

    #[test]
    fn each_integer_bound_fits_in_a_u32() {
        for spec in all() {
            for param in spec.parameters() {
                if let ParamType::Integer { minimum, maximum } = param.kind {
                    assert!(minimum <= maximum, "{}.{}", spec.name, param.name);
                    assert!(maximum <= MAX_COUNT, "{}.{}", spec.name, param.name);
                }
            }
        }
    }

    #[test]
    fn each_description_is_a_sentence() {
        for spec in all() {
            assert!(spec.description.ends_with('.'), "{}", spec.name);
            for param in spec.parameters() {
                assert!(
                    param.description.ends_with('.'),
                    "{}.{}",
                    spec.name,
                    param.name
                );
            }
        }
    }

    static SAMPLE: ToolSpec = ToolSpec {
        name: "sample",
        description: "A sample.",
        params: &[
            Param::string("name", "A name.").required(),
            Param::string("mode", "A mode.").with_default(ParamDefault::String("fast")),
            Param::integer("depth", 0, 4, "A depth.").with_default(ParamDefault::Integer(2)),
            Param::number("score", 0.0, 1.0, "A score.").with_default(ParamDefault::Number(0.5)),
            Param::boolean("deep", "A flag.").with_default(ParamDefault::Boolean(true)),
            Param::string_array("files", "Some files."),
            Param::integer("days", 1, 10, "Some days."),
        ],
        paginated: Pagination::Field("items"),
        handler: |_, _| Ok(ToolOutput::Text(String::new())),
    };

    #[test]
    fn arguments_give_the_value_from_the_call() {
        let values = serde_json::json!({
            "name": "run", "mode": "slow", "depth": 3, "score": 0.25, "deep": false,
            "files": ["a.rs", "b.rs"], "limit": 7
        });
        let arguments = Arguments::new(&SAMPLE, &values);
        assert_eq!(arguments.string("name"), Some("run"));
        assert_eq!(arguments.string("mode"), Some("slow"));
        assert_eq!(arguments.integer("depth"), Some(3));
        assert_eq!(arguments.count("depth"), Some(3));
        assert_eq!(arguments.count_u32("depth"), Some(3));
        assert_eq!(arguments.number("score"), Some(0.25));
        assert_eq!(arguments.boolean("deep"), Some(false));
        assert_eq!(arguments.strings("files"), Some(vec!["a.rs", "b.rs"]));
        assert_eq!(arguments.count("limit"), Some(7));
    }

    #[test]
    fn arguments_give_the_default_when_the_call_has_no_value() {
        let values = serde_json::json!({});
        let arguments = Arguments::new(&SAMPLE, &values);
        assert_eq!(arguments.string("name"), None);
        assert_eq!(arguments.string("mode"), Some("fast"));
        assert_eq!(arguments.integer("depth"), Some(2));
        assert_eq!(arguments.number("score"), Some(0.5));
        assert_eq!(arguments.boolean("deep"), Some(true));
        assert_eq!(arguments.strings("files"), None);
        assert_eq!(arguments.integer("days"), None);
        assert_eq!(arguments.count("limit"), Some(50));
        assert_eq!(arguments.count("offset"), Some(0));
    }
}
