//! The output of a command when its symbol name does not select one
//! definition. The command writes the near names or the choices to the
//! writer, and then fails with a non-zero exit status.

use std::io::Write;

use serde_json::{json, Value};

use crate::core::Result;
use crate::output::Format;
use crate::symbol::LookupFailure;

/// The command line of a process that looks up a symbol, with the position
/// of the symbol name. Each choice of an ambiguous name gets this command
/// line with the key of the choice in place of the name.
#[derive(Debug, Clone)]
pub struct CommandLine {
    arguments: Vec<String>,
    name_position: usize,
}

impl CommandLine {
    /// The command line `omen <command> <name>`.
    pub fn plain(command: &str, name: &str) -> Self {
        Self {
            arguments: vec!["omen".to_string(), command.to_string(), name.to_string()],
            name_position: 2,
        }
    }

    /// The command line of the process, with the symbol name at
    /// `name_position`. See [`crate::cli::symbol_name_position`].
    pub fn from_process(mut arguments: Vec<String>, name_position: usize) -> Self {
        // The program can have an absolute path. The choices name it `omen`.
        if let Some(program) = arguments.first_mut() {
            *program = "omen".to_string();
        }
        Self {
            arguments,
            name_position,
        }
    }

    /// The command line with `name` in place of the symbol name, with shell
    /// quotes where an argument needs them.
    pub fn with_name(&self, name: &str) -> String {
        self.arguments
            .iter()
            .enumerate()
            .map(|(position, argument)| {
                let argument = if position == self.name_position {
                    name
                } else {
                    argument.as_str()
                };
                shell_quote(argument)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The argument as one shell word. A key has `#`, which starts a comment in
/// some shells, thus each argument with a special character gets quotes.
fn shell_quote(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-./=:,@%+".contains(&byte));
    if plain {
        argument.to_string()
    } else {
        format!("'{}'", argument.replace('\'', r"'\''"))
    }
}

/// Writes the failure in `format`. `param` is the name of the argument that
/// holds the symbol name. The JSON form has the shape of an MCP tool error.
pub fn write_failure(
    failure: &LookupFailure,
    param: &str,
    command_line: &CommandLine,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    match format {
        Format::Markdown | Format::Text => write_text(failure, command_line, out),
        Format::JsonCompact => {
            let mut out = out;
            format.format_value(&failure_json(failure, param, command_line), &mut out)
        }
        _ => {
            let mut out = out;
            Format::Json.format_value(&failure_json(failure, param, command_line), &mut out)
        }
    }
}

fn failure_json(failure: &LookupFailure, param: &str, command_line: &CommandLine) -> Value {
    let (code, name, expected, suggestions, next) = match failure {
        LookupFailure::NotFound { name, suggestions } => (
            "not_found",
            name,
            Value::Null,
            json!(suggestions),
            if suggestions.is_empty() {
                "Find the symbol with `outline` or `search`."
            } else {
                "Use a name from `suggestions`."
            },
        ),
        LookupFailure::Ambiguous { name, choices } => (
            "ambiguous",
            name,
            json!("a name, or a name and a start line, that select one definition"),
            choices
                .iter()
                .map(|choice| {
                    let mut value = json!(choice);
                    value["command"] = json!(command_line.with_name(&choice.name));
                    value
                })
                .collect(),
            "Run the `command` of one suggestion.",
        ),
    };
    json!({
        "error": {
            "code": code,
            "message": failure.to_string(),
            "param": param,
            "expected": expected,
            "received": name,
            "suggestions": suggestions,
            "next": next
        }
    })
}

fn write_text(
    failure: &LookupFailure,
    command_line: &CommandLine,
    out: &mut dyn Write,
) -> Result<()> {
    say!(out, "{failure}");
    if let LookupFailure::Ambiguous { choices, .. } = failure {
        say!(out);
        say!(out, "Run one of these commands:");
        say!(out);
        for choice in choices {
            let place = match &choice.container {
                Some(container) => format!("{} in `{container}`", choice.kind),
                None => choice.kind.clone(),
            };
            say!(
                out,
                "- `{}` ({place}) at {}:{}",
                choice.name,
                choice.file,
                choice.start_line
            );
            say!(out, "  {}", command_line.with_name(&choice.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolChoice;

    fn arguments(text: &[&str]) -> Vec<String> {
        text.iter().map(|argument| argument.to_string()).collect()
    }

    fn ambiguous() -> LookupFailure {
        LookupFailure::Ambiguous {
            name: "helper".to_string(),
            choices: vec![
                SymbolChoice {
                    name: "x.py#Shop.helper".to_string(),
                    kind: "Method".to_string(),
                    container: Some("Shop".to_string()),
                    file: "x.py".to_string(),
                    start_line: 2,
                    signature: "def helper(self):".to_string(),
                },
                SymbolChoice {
                    name: "y.py#helper".to_string(),
                    kind: "Function".to_string(),
                    container: None,
                    file: "y.py".to_string(),
                    start_line: 1,
                    signature: "def helper():".to_string(),
                },
            ],
        }
    }

    #[test]
    fn test_the_command_keeps_each_other_argument() {
        let line = CommandLine::from_process(
            arguments(&[
                "/usr/local/bin/omen",
                "-p",
                "my repo",
                "-f",
                "json",
                "symbol",
                "helper",
                "--no-source",
            ]),
            6,
        );
        assert_eq!(
            line.with_name("x.py#Shop.helper"),
            "omen -p 'my repo' -f json symbol 'x.py#Shop.helper' --no-source"
        );
    }

    #[test]
    fn test_a_quote_in_an_argument_stays_one_word() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn test_json_has_the_shape_of_a_tool_error_and_a_command_for_each_choice() {
        let mut out = Vec::new();
        write_failure(
            &ambiguous(),
            "name",
            &CommandLine::plain("symbol", "helper"),
            Format::Json,
            &mut out,
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["error"]["code"], "ambiguous");
        assert_eq!(value["error"]["param"], "name");
        assert_eq!(value["error"]["received"], "helper");
        assert_eq!(
            value["error"]["suggestions"][0],
            json!({
                "name": "x.py#Shop.helper",
                "kind": "Method",
                "container": "Shop",
                "file": "x.py",
                "start_line": 2,
                "signature": "def helper(self):",
                "command": "omen symbol 'x.py#Shop.helper'"
            })
        );
        assert_eq!(
            value["error"]["suggestions"][1]["command"],
            "omen symbol 'y.py#helper'"
        );
    }

    #[test]
    fn test_text_lists_each_choice_with_its_command() {
        let mut out = Vec::new();
        write_failure(
            &ambiguous(),
            "symbol",
            &CommandLine::plain("impact", "helper"),
            Format::Markdown,
            &mut out,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "2 definitions agree with the symbol `helper`. Select one with its key or its start line.\n\
             \n\
             Run one of these commands:\n\
             \n\
             - `x.py#Shop.helper` (Method in `Shop`) at x.py:2\n  omen impact 'x.py#Shop.helper'\n\
             - `y.py#helper` (Function) at y.py:1\n  omen impact 'y.py#helper'\n"
        );
    }

    #[test]
    fn test_json_of_an_unknown_name_has_the_near_names() {
        let failure = LookupFailure::NotFound {
            name: "helpr".to_string(),
            suggestions: vec!["helper".to_string()],
        };
        let mut out = Vec::new();
        write_failure(
            &failure,
            "name",
            &CommandLine::plain("symbol", "helpr"),
            Format::JsonCompact,
            &mut out,
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(value["error"]["code"], "not_found");
        assert_eq!(value["error"]["suggestions"], json!(["helper"]));
        assert_eq!(value["error"]["next"], "Use a name from `suggestions`.");
    }
}
