//! The validation of the arguments of a call against a tool specification.

use serde_json::Value;

use super::{integer_value, Arguments, Param, ParamType, ToolSpec};
use crate::mcp::error::{ErrorCode, ToolError};

/// Checks the arguments of one call.
///
/// The result has all the problems of the call, not only the first. The
/// sequence is stable: first the parameters in the sequence of the
/// specification, then the unknown arguments in the sequence of their names.
/// A problem is never changed into a default value.
pub(in crate::mcp) fn validate<'a>(
    spec: &'static ToolSpec,
    arguments: &'a Value,
) -> Result<Arguments<'a>, Vec<ToolError>> {
    let Some(values) = arguments.as_object() else {
        return Err(vec![ToolError::new(
            ErrorCode::InvalidArgument,
            "The arguments of a tool call must be a JSON object.",
        )
        .with_expected("an object")
        .with_received(arguments)
        .with_next("Send the call again with an object as `arguments`.")]);
    };

    let mut problems = Vec::new();
    for param in spec.parameters() {
        match values.get(param.name) {
            None if param.required => problems.push(missing(param)),
            None => {}
            Some(value) if check(param, value) => {}
            Some(value) => problems.push(invalid(param, value)),
        }
    }
    let names: Vec<&str> = spec.parameters().map(|param| param.name).collect();
    // The map has its keys in sorted sequence, thus this sequence is stable.
    for name in values.keys().filter(|name| !names.contains(&name.as_str())) {
        problems.push(unknown(spec, name, &names));
    }

    if problems.is_empty() {
        Ok(Arguments::new(spec, arguments))
    } else {
        Err(problems)
    }
}

fn missing(param: &Param) -> ToolError {
    ToolError::new(
        ErrorCode::MissingArgument,
        format!("The argument `{}` is necessary.", param.name),
    )
    .with_param(param.name)
    .with_expected(expected(param))
    .with_next(format!("Send the call again with `{}`.", param.name))
}

fn invalid(param: &Param, value: &Value) -> ToolError {
    let suggestions = match (param.kind, value.as_str()) {
        (ParamType::Enum(values), Some(text)) => nearest(text, values),
        _ => Vec::new(),
    };
    ToolError::new(
        ErrorCode::InvalidArgument,
        format!("The value of `{}` is not correct.", param.name),
    )
    .with_param(param.name)
    .with_expected(expected(param))
    .with_received(value)
    .with_suggestions(suggestions)
    .with_next(format!(
        "Send the call again with a correct value for `{}`.",
        param.name
    ))
}

fn unknown(spec: &ToolSpec, name: &str, names: &[&str]) -> ToolError {
    let suggestions = nearest(name, names);
    let next = if suggestions.is_empty() {
        format!("Remove `{name}` from the call.")
    } else {
        "Use a name from `suggestions`.".to_string()
    };
    ToolError::new(
        ErrorCode::UnknownArgument,
        format!("The tool `{}` has no argument `{name}`.", spec.name),
    )
    .with_param(name)
    .with_expected(format!("one of: {}", names.join(", ")))
    .with_suggestions(suggestions)
    .with_next(next)
}

/// The text that describes a correct value of the parameter.
fn expected(param: &Param) -> String {
    match param.kind {
        ParamType::String => "a string".to_string(),
        ParamType::Integer { minimum, maximum } => {
            format!("an integer from {minimum} to {maximum}")
        }
        ParamType::Number { minimum, maximum } => {
            format!("a number from {minimum} to {maximum}")
        }
        ParamType::Boolean => "true or false".to_string(),
        ParamType::Enum(values) => format!("one of: {}", values.join(", ")),
        ParamType::StringArray => "an array of strings".to_string(),
    }
}

fn check(param: &Param, value: &Value) -> bool {
    match param.kind {
        ParamType::String => value.is_string(),
        ParamType::Integer { minimum, maximum } => {
            integer_value(value).is_some_and(|number| (minimum..=maximum).contains(&number))
        }
        ParamType::Number { minimum, maximum } => value
            .as_f64()
            .is_some_and(|number| (minimum..=maximum).contains(&number)),
        ParamType::Boolean => value.is_boolean(),
        ParamType::Enum(values) => value.as_str().is_some_and(|text| values.contains(&text)),
        ParamType::StringArray => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string)),
    }
}

/// The largest number of suggestions in one error.
const MAX_SUGGESTIONS: usize = 3;

/// The candidates that are almost the same as `target`.
///
/// The sequence is the edit distance, then the name. Thus the result is the
/// same for each run.
pub(in crate::mcp) fn nearest<'a>(target: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let target = target.to_lowercase();
    let length = target.chars().count();
    let threshold = (length / 3).max(1);
    let mut near: Vec<(usize, &str)> = candidates
        .iter()
        .map(|candidate| {
            (
                edit_distance(&target, &candidate.to_lowercase()),
                *candidate,
            )
        })
        .filter(|(distance, candidate)| {
            *distance <= threshold && *distance < length.max(candidate.chars().count())
        })
        .collect();
    near.sort();
    near.dedup();
    near.into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, candidate)| candidate)
        .collect()
}

/// The Levenshtein distance between two strings, in characters.
fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_character) in left.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_character) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_character != *right_character);
            let deletion = previous[column + 1] + 1;
            let insertion = current[column] + 1;
            current.push(substitution.min(deletion).min(insertion));
        }
        previous = current;
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{find, Pagination, ParamDefault, ToolOutput};
    use super::*;

    static SAMPLE: ToolSpec = ToolSpec {
        name: "sample",
        description: "A sample.",
        params: &[
            Param::string("name", "A name.").required(),
            Param::integer("depth", 0, 4, "A depth.").with_default(ParamDefault::Integer(2)),
            Param::number("score", 0.0, 1.0, "A score."),
            Param::boolean("deep", "A flag."),
            Param::choice("direction", &["callers", "callees", "both"], "A direction."),
            Param::string_array("files", "Some files."),
        ],
        paginated: Pagination::Field("items"),
        handler: |_, _| Ok(ToolOutput::Text(String::new())),
    };

    fn problems(arguments: Value) -> Vec<ToolError> {
        match validate(&SAMPLE, &arguments) {
            Ok(_) => Vec::new(),
            Err(problems) => problems,
        }
    }

    fn one_problem(arguments: Value) -> ToolError {
        let mut problems = problems(arguments);
        assert_eq!(problems.len(), 1, "{problems:?}");
        problems.remove(0)
    }

    #[test]
    fn valid_arguments_pass_and_keep_their_values() {
        let values = json!({
            "name": "run", "depth": 4, "score": 1, "deep": true, "direction": "both",
            "files": ["a.rs"], "limit": 0, "offset": 3
        });
        let arguments = validate(&SAMPLE, &values).expect("valid arguments");
        assert_eq!(arguments.string("name"), Some("run"));
        assert_eq!(arguments.integer("depth"), Some(4));
        assert_eq!(arguments.number("score"), Some(1.0));
        assert_eq!(arguments.count("offset"), Some(3));
    }

    #[test]
    fn absent_optional_arguments_pass() {
        assert!(problems(json!({"name": "run"})).is_empty());
    }

    #[test]
    fn a_missing_required_argument_is_an_error() {
        let problem = one_problem(json!({}));
        assert_eq!(problem.code, ErrorCode::MissingArgument);
        assert_eq!(problem.param.as_deref(), Some("name"));
        assert_eq!(problem.expected.as_deref(), Some("a string"));
        assert_eq!(problem.received, None);
        assert_eq!(problem.message, "The argument `name` is necessary.");
        assert_eq!(
            problem.next.as_deref(),
            Some("Send the call again with `name`.")
        );
    }

    #[test]
    fn a_number_of_the_wrong_type_is_an_error() {
        for wrong in [json!("3"), json!(1.5), json!(-1), json!(true), json!(null)] {
            let problem = one_problem(json!({"name": "run", "depth": wrong}));
            assert_eq!(problem.code, ErrorCode::InvalidArgument, "{wrong}");
            assert_eq!(problem.param.as_deref(), Some("depth"));
            assert_eq!(problem.expected.as_deref(), Some("an integer from 0 to 4"));
            assert_eq!(problem.received, Some(wrong));
        }
    }

    #[test]
    fn a_number_without_a_fraction_is_a_valid_integer() {
        let values = json!({"name": "run", "depth": 3.0, "limit": 7.0});
        let arguments = validate(&SAMPLE, &values).expect("3.0 is an integer in JSON Schema");
        assert_eq!(arguments.integer("depth"), Some(3));
        assert_eq!(arguments.count("limit"), Some(7));

        for wrong in [json!(3.5), json!(5.0), json!(-1.0), json!(1e300)] {
            let problem = one_problem(json!({"name": "run", "depth": wrong}));
            assert_eq!(problem.code, ErrorCode::InvalidArgument, "{wrong}");
        }
    }

    #[test]
    fn an_integer_out_of_its_bounds_is_an_error() {
        let problem = one_problem(json!({"name": "run", "depth": 5}));
        assert_eq!(problem.code, ErrorCode::InvalidArgument);
        assert_eq!(problem.received, Some(json!(5)));
        assert!(problems(json!({"name": "run", "depth": 0})).is_empty());
        assert!(problems(json!({"name": "run", "depth": 4})).is_empty());
    }

    #[test]
    fn a_number_out_of_its_bounds_or_of_the_wrong_type_is_an_error() {
        for wrong in [json!(1.01), json!(-0.1), json!("0.5")] {
            let problem = one_problem(json!({"name": "run", "score": wrong}));
            assert_eq!(problem.code, ErrorCode::InvalidArgument, "{wrong}");
            assert_eq!(problem.expected.as_deref(), Some("a number from 0 to 1"));
        }
        assert!(problems(json!({"name": "run", "score": 0.25})).is_empty());
    }

    #[test]
    fn a_string_or_a_boolean_of_the_wrong_type_is_an_error() {
        let problem = one_problem(json!({"name": 7}));
        assert_eq!(problem.code, ErrorCode::InvalidArgument);
        assert_eq!(problem.expected.as_deref(), Some("a string"));

        let problem = one_problem(json!({"name": "run", "deep": "yes"}));
        assert_eq!(problem.code, ErrorCode::InvalidArgument);
        assert_eq!(problem.expected.as_deref(), Some("true or false"));
    }

    #[test]
    fn a_value_that_is_not_in_the_enum_is_an_error_with_suggestions() {
        let problem = one_problem(json!({"name": "run", "direction": "caller"}));
        assert_eq!(problem.code, ErrorCode::InvalidArgument);
        assert_eq!(problem.param.as_deref(), Some("direction"));
        assert_eq!(
            problem.expected.as_deref(),
            Some("one of: callers, callees, both")
        );
        assert_eq!(problem.received, Some(json!("caller")));
        assert_eq!(problem.suggestions, [json!("callers"), json!("callees")]);

        let problem = one_problem(json!({"name": "run", "direction": 1}));
        assert_eq!(problem.code, ErrorCode::InvalidArgument);
        assert!(problem.suggestions.is_empty());
    }

    #[test]
    fn an_array_with_an_element_that_is_not_a_string_is_an_error() {
        for wrong in [json!("a.rs,b.rs"), json!(["a.rs", 1])] {
            let problem = one_problem(json!({"name": "run", "files": wrong}));
            assert_eq!(problem.code, ErrorCode::InvalidArgument, "{wrong}");
            assert_eq!(problem.expected.as_deref(), Some("an array of strings"));
        }
        assert!(problems(json!({"name": "run", "files": []})).is_empty());
    }

    #[test]
    fn an_unknown_argument_is_an_error_with_the_nearest_names() {
        let problem = one_problem(json!({"name": "run", "dept": 1}));
        assert_eq!(problem.code, ErrorCode::UnknownArgument);
        assert_eq!(problem.param.as_deref(), Some("dept"));
        assert_eq!(problem.suggestions, [json!("depth")]);
        assert_eq!(
            problem.expected.as_deref(),
            Some("one of: name, depth, score, deep, direction, files, limit, offset")
        );
        assert_eq!(problem.message, "The tool `sample` has no argument `dept`.");
        assert_eq!(
            problem.next.as_deref(),
            Some("Use a name from `suggestions`.")
        );
    }

    #[test]
    fn an_unknown_argument_without_a_near_name_has_no_suggestion() {
        let problem = one_problem(json!({"name": "run", "zzzzzzzz": 1}));
        assert_eq!(problem.code, ErrorCode::UnknownArgument);
        assert!(problem.suggestions.is_empty());
        assert_eq!(
            problem.next.as_deref(),
            Some("Remove `zzzzzzzz` from the call.")
        );
    }

    #[test]
    fn one_call_reports_all_its_problems_in_a_stable_sequence() {
        let found = problems(json!({
            "zeta": 1, "depth": "deep", "alpha": 2, "direction": "up", "limit": -1
        }));
        let summary: Vec<(&str, &str)> = found
            .iter()
            .map(|problem| (problem.code.as_str(), problem.param.as_deref().unwrap()))
            .collect();
        assert_eq!(
            summary,
            [
                ("missing_argument", "name"),
                ("invalid_argument", "depth"),
                ("invalid_argument", "direction"),
                ("invalid_argument", "limit"),
                ("unknown_argument", "alpha"),
                ("unknown_argument", "zeta"),
            ]
        );
    }

    #[test]
    fn arguments_that_are_not_an_object_are_an_error() {
        let problem = one_problem(json!(["run"]));
        assert_eq!(problem.code, ErrorCode::InvalidArgument);
        assert_eq!(problem.param, None);
        assert_eq!(problem.expected.as_deref(), Some("an object"));
        assert_eq!(problem.received, Some(json!(["run"])));
    }

    #[test]
    fn a_tool_without_pagination_rejects_limit() {
        let spec = find("task_context").expect("task_context specification");
        let Err(problems) = validate(spec, &json!({"name": "run", "limit": 5})) else {
            panic!("limit is not a parameter of task_context");
        };
        assert_eq!(problems[0].code, ErrorCode::UnknownArgument);
        assert_eq!(problems[0].param.as_deref(), Some("limit"));
    }

    #[test]
    fn nearest_gives_the_candidates_by_distance_and_then_by_name() {
        assert_eq!(nearest("pth", &["path", "depth", "limit"]), ["path"]);
        assert_eq!(nearest("PATH", &["path"]), ["path"]);
        assert_eq!(
            nearest("ab", &["ac", "ab1", "aa", "zz"]),
            ["aa", "ab1", "ac"]
        );
        assert_eq!(nearest("a", &["b", "c", "ab"]), ["ab"]);
        assert!(nearest("target", &["base", "head", "path"]).is_empty());
        assert_eq!(nearest("x", &["a", "b", "c", "d"]).len(), 0);
    }

    #[test]
    fn nearest_gives_a_maximum_of_three_candidates() {
        assert_eq!(
            nearest("name", &["name4", "name3", "name2", "name1"]),
            ["name1", "name2", "name3"]
        );
    }

    #[test]
    fn edit_distance_counts_single_character_changes() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("depth", "dept"), 1);
        assert_eq!(edit_distance("naïve", "naive"), 1);
    }
}
