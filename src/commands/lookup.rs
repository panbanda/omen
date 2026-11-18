//! The output of a command when its symbol name does not select one
//! definition. The command writes the near names or the choices to the
//! writer, and then fails with a non-zero exit status.

use std::ffi::OsString;
use std::io::Write;

use serde_json::{json, Value};

use crate::core::Result;
use crate::output::Format;
use crate::symbol::{LookupFailure, SymbolChoice};

/// The command line of a process that looks up a symbol. Each choice gets
/// this command line with the key of the choice in place of the name.
#[derive(Debug, Clone)]
pub struct CommandLine {
    arguments: Vec<String>,
    name_position: usize,
    /// The `--start-line` flag and its value, if the command line has them.
    start_line_positions: Vec<usize>,
}

impl CommandLine {
    /// The command line `omen <command> <name>`.
    pub fn plain(command: &str, name: &str) -> Self {
        Self {
            arguments: vec!["omen".to_string(), command.to_string(), name.to_string()],
            name_position: 2,
            start_line_positions: Vec::new(),
        }
    }

    /// The command line of the process. `None` when an argument is not
    /// UTF-8, because the text of the command would then name a different
    /// path, or when the command line has no symbol name.
    ///
    /// The user information of a URL, such as a token in
    /// `https://user:token@host/owner/repo`, is not in the command. Git gets
    /// the credentials from its credential helper.
    pub fn from_process(arguments: Vec<OsString>) -> Option<Self> {
        let arguments: Vec<String> = arguments
            .into_iter()
            .map(|argument| argument.into_string().ok())
            .collect::<Option<_>>()?;
        let name_position = crate::cli::symbol_name_position(&arguments)?;
        let start_line_positions = crate::cli::start_line_positions(&arguments);
        let arguments = arguments
            .into_iter()
            .enumerate()
            .map(|(position, argument)| {
                if position == name_position {
                    argument
                } else {
                    without_user_information(&argument)
                }
            })
            .collect();
        Some(Self {
            arguments,
            name_position,
            start_line_positions,
        })
    }

    /// The command line with `key` in place of the symbol name, with shell
    /// quotes where an argument needs them. When `keep_start_line` is false,
    /// the command has no `--start-line`.
    pub fn selecting(&self, key: &str, keep_start_line: bool) -> String {
        let after_double_dash = self.arguments[..self.name_position]
            .iter()
            .any(|argument| argument == "--");
        // A key such as `-old.py#helper` is an option for clap unless it
        // comes after `--`.
        let key_at_end = key.starts_with('-') && !after_double_dash;
        let mut words: Vec<&str> = Vec::with_capacity(self.arguments.len() + 1);
        for (position, argument) in self.arguments.iter().enumerate() {
            if position == self.name_position {
                if !key_at_end {
                    words.push(key);
                }
                continue;
            }
            let removed_start_line =
                !keep_start_line && self.start_line_positions.contains(&position);
            let removed_double_dash =
                key_at_end && position > self.name_position && argument == "--";
            if !removed_start_line && !removed_double_dash {
                words.push(argument);
            }
        }
        if key_at_end {
            words.extend(["--", key]);
        }
        words
            .into_iter()
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The argument without the user information of a URL in it.
fn without_user_information(argument: &str) -> String {
    let Some(scheme_end) = argument.find("://") else {
        return argument.to_string();
    };
    let authority_start = scheme_end + "://".len();
    let authority_end = argument[authority_start..]
        .find(['/', '?', '#'])
        .map_or(argument.len(), |end| authority_start + end);
    match argument[authority_start..authority_end].rfind('@') {
        Some(at) => format!(
            "{}{}",
            &argument[..authority_start],
            &argument[authority_start + at + 1..]
        ),
        None => argument.to_string(),
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
/// Without `command_line`, the choices have no command.
pub fn write_failure(
    failure: &LookupFailure,
    param: &str,
    command_line: Option<&CommandLine>,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    match format {
        Format::Markdown | Format::Text => write_text(failure, command_line, format, out),
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

/// The choices of the failure, and if the command of a choice keeps the
/// `--start-line` of the command line.
fn choices_of(failure: &LookupFailure) -> Option<(&[SymbolChoice], bool)> {
    match failure {
        LookupFailure::NotFound { .. } => None,
        LookupFailure::NotAtLine { choices, .. } => Some((choices, false)),
        LookupFailure::Ambiguous { choices, .. } => Some((choices, true)),
    }
}

fn failure_json(failure: &LookupFailure, param: &str, command_line: Option<&CommandLine>) -> Value {
    let suggestions = match choices_of(failure) {
        Some((choices, keep_start_line)) => choices
            .iter()
            .map(|choice| {
                let mut value = json!(choice);
                if let Some(command_line) = command_line {
                    value["command"] = json!(command_line.selecting(&choice.name, keep_start_line));
                }
                value
            })
            .collect(),
        None => Value::Null,
    };
    let use_a_choice = if command_line.is_some() {
        "run the `command` of one suggestion."
    } else {
        "use the key of one suggestion as the name."
    };
    let (code, param, expected, received, suggestions, next) = match failure {
        LookupFailure::NotFound {
            name,
            start_line,
            suggestions,
        } => {
            let mut next = if suggestions.is_empty() {
                "Find the symbol with `outline` or `search`.".to_string()
            } else {
                "Use a name from `suggestions`.".to_string()
            };
            if start_line.is_some() {
                next.push_str(" Also correct `--start-line` or remove it.");
            }
            (
                "not_found",
                param,
                Value::Null,
                json!(name),
                json!(suggestions),
                next,
            )
        }
        LookupFailure::NotAtLine { start_line, .. } => (
            "not_found",
            "start_line",
            json!("the start line of a definition of the name"),
            json!(start_line),
            suggestions,
            format!("Correct `--start-line` or remove it, or {use_a_choice}"),
        ),
        LookupFailure::Ambiguous { name, .. } => (
            "ambiguous",
            param,
            json!("a name, or a name and a start line, that select one definition"),
            json!(name),
            suggestions,
            format!("To select one definition, {use_a_choice}"),
        ),
    };
    json!({
        "error": {
            "code": code,
            "message": failure.to_string(),
            "param": param,
            "expected": expected,
            "received": received,
            "suggestions": suggestions,
            "next": next
        }
    })
}

fn write_text(
    failure: &LookupFailure,
    command_line: Option<&CommandLine>,
    format: Format,
    out: &mut dyn Write,
) -> Result<()> {
    say!(out, "{failure}");
    let Some((choices, keep_start_line)) = choices_of(failure) else {
        return Ok(());
    };
    say!(out);
    if command_line.is_some() {
        say!(out, "Run one of these commands:");
    } else {
        say!(out, "Use the key of one of these definitions as the name:");
    }
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
        let Some(command_line) = command_line else {
            continue;
        };
        let command = command_line.selecting(&choice.name, keep_start_line);
        // In Markdown, a code block keeps characters such as `*` literal.
        if matches!(format, Format::Markdown) {
            say!(out, "  ```sh\n  {command}\n  ```");
        } else {
            say!(out, "  {command}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(text: &[&str]) -> CommandLine {
        CommandLine::from_process(text.iter().map(OsString::from).collect()).unwrap()
    }

    fn choice(
        name: &str,
        kind: &str,
        container: Option<&str>,
        file: &str,
        line: u32,
    ) -> SymbolChoice {
        SymbolChoice {
            name: name.to_string(),
            kind: kind.to_string(),
            container: container.map(str::to_string),
            file: file.to_string(),
            start_line: line,
            signature: "def helper():".to_string(),
        }
    }

    fn ambiguous() -> LookupFailure {
        LookupFailure::Ambiguous {
            name: "helper".to_string(),
            choices: vec![
                choice("x.py#Shop.helper", "Method", Some("Shop"), "x.py", 2),
                choice("y.py#helper", "Function", None, "y.py", 1),
            ],
        }
    }

    fn written(
        failure: &LookupFailure,
        command_line: Option<&CommandLine>,
        format: Format,
    ) -> String {
        let mut out = Vec::new();
        write_failure(failure, "name", command_line, format, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_the_command_keeps_the_program_and_each_other_argument() {
        let line = process(&[
            "./target/debug/omen",
            "-p",
            "my repo",
            "-f",
            "json",
            "symbol",
            "helper",
            "--no-source",
        ]);
        assert_eq!(
            line.selecting("x.py#Shop.helper", true),
            "./target/debug/omen -p 'my repo' -f json symbol 'x.py#Shop.helper' --no-source"
        );
    }

    #[test]
    fn test_a_key_that_starts_with_a_dash_comes_after_a_double_dash() {
        let line = process(&["omen", "symbol", "helper", "--no-source"]);
        assert_eq!(
            line.selecting("-old.py#helper", true),
            "omen symbol --no-source -- '-old.py#helper'"
        );
        let line = process(&["omen", "symbol", "--", "-helper"]);
        assert_eq!(
            line.selecting("-old.py#helper", true),
            "omen symbol -- '-old.py#helper'"
        );
    }

    #[test]
    fn test_the_command_has_no_user_information_of_a_url() {
        let line = process(&[
            "omen",
            "-p",
            "https://user:token@github.com/owner/repo@main",
            "symbol",
            "helper",
        ]);
        assert_eq!(
            line.selecting("y.py#helper", true),
            "omen -p https://github.com/owner/repo@main symbol 'y.py#helper'"
        );
        let line = process(&["omen", "--path=https://token@host/repo", "impact", "helper"]);
        assert_eq!(
            line.selecting("y.py#helper", true),
            "omen --path=https://host/repo impact 'y.py#helper'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_an_argument_that_is_not_utf8_gives_no_command_line() {
        use std::os::unix::ffi::OsStringExt;
        let arguments = vec![
            OsString::from("omen"),
            OsString::from("-p"),
            OsString::from_vec(b"repo\xff".to_vec()),
            OsString::from("symbol"),
            OsString::from("helper"),
        ];
        assert!(CommandLine::from_process(arguments).is_none());
    }

    #[test]
    fn test_a_quote_in_an_argument_stays_one_word() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn test_json_has_the_shape_of_a_tool_error_and_a_command_for_each_choice() {
        let text = written(
            &ambiguous(),
            Some(&CommandLine::plain("symbol", "helper")),
            Format::Json,
        );
        let value: Value = serde_json::from_str(&text).unwrap();
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
                "signature": "def helper():",
                "command": "omen symbol 'x.py#Shop.helper'"
            })
        );
        assert_eq!(
            value["error"]["next"],
            "To select one definition, run the `command` of one suggestion."
        );
    }

    #[test]
    fn test_without_a_command_line_the_choices_have_no_command() {
        let value: Value =
            serde_json::from_str(&written(&ambiguous(), None, Format::Json)).unwrap();
        assert!(value["error"]["suggestions"][0].get("command").is_none());
        assert_eq!(
            value["error"]["next"],
            "To select one definition, use the key of one suggestion as the name."
        );
        assert_eq!(
            written(&ambiguous(), None, Format::Text),
            "2 definitions agree with the symbol `helper`. Select one with its key or its start line.\n\
             \n\
             Use the key of one of these definitions as the name:\n\
             \n\
             - `x.py#Shop.helper` (Method in `Shop`) at x.py:2\n\
             - `y.py#helper` (Function) at y.py:1\n"
        );
    }

    #[test]
    fn test_markdown_puts_each_command_in_a_code_block() {
        let line = process(&["omen", "-p", "./*repo*", "symbol", "helper"]);
        assert_eq!(
            written(&ambiguous(), Some(&line), Format::Markdown),
            "2 definitions agree with the symbol `helper`. Select one with its key or its start line.\n\
             \n\
             Run one of these commands:\n\
             \n\
             - `x.py#Shop.helper` (Method in `Shop`) at x.py:2\n\
             \x20 ```sh\n\
             \x20 omen -p './*repo*' symbol 'x.py#Shop.helper'\n\
             \x20 ```\n\
             - `y.py#helper` (Function) at y.py:1\n\
             \x20 ```sh\n\
             \x20 omen -p './*repo*' symbol 'y.py#helper'\n\
             \x20 ```\n"
        );
    }

    #[test]
    fn test_text_puts_each_command_on_its_line() {
        let line = process(&["omen", "impact", "helper"]);
        assert_eq!(
            written(&ambiguous(), Some(&line), Format::Text),
            "2 definitions agree with the symbol `helper`. Select one with its key or its start line.\n\
             \n\
             Run one of these commands:\n\
             \n\
             - `x.py#Shop.helper` (Method in `Shop`) at x.py:2\n  omen impact 'x.py#Shop.helper'\n\
             - `y.py#helper` (Function) at y.py:1\n  omen impact 'y.py#helper'\n"
        );
    }

    #[test]
    fn test_a_wrong_start_line_gives_commands_without_the_start_line() {
        let failure = LookupFailure::NotAtLine {
            name: "helper".to_string(),
            start_line: 9,
            choices: vec![choice("y.py#helper", "Function", None, "y.py", 1)],
        };
        let line = process(&[
            "omen",
            "symbol",
            "helper",
            "--start-line",
            "9",
            "--no-source",
        ]);
        let value: Value =
            serde_json::from_str(&written(&failure, Some(&line), Format::Json)).unwrap();
        assert_eq!(value["error"]["code"], "not_found");
        assert_eq!(value["error"]["param"], "start_line");
        assert_eq!(value["error"]["received"], 9);
        assert_eq!(
            value["error"]["suggestions"][0]["command"],
            "omen symbol 'y.py#helper' --no-source"
        );
        assert_eq!(
            value["error"]["next"],
            "Correct `--start-line` or remove it, or run the `command` of one suggestion."
        );
    }

    #[test]
    fn test_an_ambiguous_name_keeps_the_start_line_in_each_command() {
        let line = process(&["omen", "symbol", "helper", "--start-line=2"]);
        assert_eq!(
            line.selecting("x.py#Shop.helper", true),
            "omen symbol 'x.py#Shop.helper' --start-line=2"
        );
    }

    #[test]
    fn test_json_of_an_unknown_name_has_the_near_names() {
        let failure = LookupFailure::NotFound {
            name: "helpr".to_string(),
            start_line: Some(9),
            suggestions: vec!["helper".to_string()],
        };
        let value: Value = serde_json::from_str(&written(
            &failure,
            Some(&CommandLine::plain("symbol", "helpr")),
            Format::JsonCompact,
        ))
        .unwrap();
        assert_eq!(value["error"]["code"], "not_found");
        assert_eq!(value["error"]["param"], "name");
        assert_eq!(value["error"]["suggestions"], json!(["helper"]));
        assert_eq!(
            value["error"]["next"],
            "Use a name from `suggestions`. Also correct `--start-line` or remove it."
        );
    }
}
