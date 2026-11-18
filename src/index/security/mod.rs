//! Security findings of the shared index.
//!
//! A sink is a call, a member access, a literal, or a block that the table in
//! [`spec`] names. The matcher reports the fact that the sink exists. It does
//! not say that the code has a vulnerability, and it does not follow data.

pub mod analysis;
pub mod entry;
mod matcher;
pub mod reach;
pub mod spec;
pub mod suppress;
mod syntax;

use serde::{Deserialize, Serialize};

use super::model::Span;

pub use matcher::{find_sinks, find_sinks_with};

/// What the security analysis says about one symbol. The analysis gives facts. A path is a
/// path in the call graph that the index resolves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolSecurity {
    /// The entry point that the symbol is, if it is the handler of one.
    pub entry_point: Option<EntrySummary>,
    /// The sinks in the body of the symbol.
    pub sinks_in_body: Vec<SinkSummary>,
    /// The keys of the entry points that have a path through the symbol.
    pub reachable_from: Vec<String>,
    /// The keys of the sinks that a path through the symbol reaches.
    pub reaches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySummary {
    pub key: String,
    pub kind: String,
    pub framework: String,
    pub route: Option<String>,
    pub guards: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SinkSummary {
    pub key: String,
    pub category: String,
    pub cwe: u32,
    pub line: u32,
    pub basis: String,
}

/// The security facts of the symbol with the key `symbol_key`.
pub fn symbol_security(analysis: &analysis::Analysis, symbol_key: &str) -> SymbolSecurity {
    let entry_point = analysis
        .entry_points
        .iter()
        .find(|entry| entry.symbol.as_deref() == Some(symbol_key))
        .map(|entry| EntrySummary {
            key: entry.key.clone(),
            kind: entry.kind.name().to_string(),
            framework: entry.framework.clone(),
            route: entry
                .route
                .as_ref()
                .map(|route| format!("{} {}", route.method, route.path)),
            guards: entry.guards.clone(),
        });
    let sinks_in_body = analysis
        .sinks
        .iter()
        .filter(|sink| sink.enclosing.as_deref() == Some(symbol_key))
        .map(|sink| SinkSummary {
            key: sink.key.clone(),
            category: sink.category.name().to_string(),
            cwe: sink.cwe,
            line: sink.range.start_line,
            basis: sink.basis.name().to_string(),
        })
        .collect();
    let through: Vec<&reach::SinkPath> = analysis
        .reachability
        .paths
        .iter()
        .filter(|path| path.members.iter().any(|member| member == symbol_key))
        .collect();
    let mut reachable_from: Vec<String> = through.iter().map(|path| path.entry.clone()).collect();
    reachable_from.sort();
    reachable_from.dedup();
    let mut reaches: Vec<String> = through.iter().map(|path| path.sink.clone()).collect();
    reaches.sort();
    reaches.dedup();
    SymbolSecurity {
        entry_point,
        sinks_in_body,
        reachable_from,
        reaches,
    }
}

/// The sinks of one parsed file, with the imports and the references of that file only.
///
/// The index of one file cannot see a repository definition. Thus the result can have
/// a sink that a repository index does not have.
pub fn sinks_of_source(parsed: &crate::parser::ParseResult, file: &str) -> Vec<Sink> {
    use super::extract::{extract_file, link_files};
    use super::imports::{resolve_imports, Manifests};
    use super::resolve::resolve_references;
    let mut files = vec![extract_file(parsed, file)];
    link_files(&mut files);
    resolve_imports(&mut files, &Manifests::default());
    resolve_references(&mut files);
    find_sinks(std::slice::from_ref(parsed), &files)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SinkCategory {
    /// A command is built or run. The sink does not say that the command runs.
    CommandExecution,
    CodeEvaluation,
    UnsafeDeserialization,
    SqlStringConstruction,
    WeakHash,
    WeakCipher,
    TlsVerificationDisabled,
    HardcodedCredential,
    MemoryUnsafe,
    /// Code marks a string as safe HTML, and the template does not escape it.
    UnsafeHtmlOutput,
}

impl SinkCategory {
    pub const ALL: [Self; 10] = [
        Self::CommandExecution,
        Self::CodeEvaluation,
        Self::UnsafeDeserialization,
        Self::SqlStringConstruction,
        Self::WeakHash,
        Self::WeakCipher,
        Self::TlsVerificationDisabled,
        Self::HardcodedCredential,
        Self::MemoryUnsafe,
        Self::UnsafeHtmlOutput,
    ];

    /// The name of the category in the output, for example `weak_hash`.
    pub fn name(self) -> &'static str {
        match self {
            Self::CommandExecution => "command_execution",
            Self::CodeEvaluation => "code_evaluation",
            Self::UnsafeDeserialization => "unsafe_deserialization",
            Self::SqlStringConstruction => "sql_string_construction",
            Self::WeakHash => "weak_hash",
            Self::WeakCipher => "weak_cipher",
            Self::TlsVerificationDisabled => "tls_verification_disabled",
            Self::HardcodedCredential => "hardcoded_credential",
            Self::MemoryUnsafe => "memory_unsafe",
            Self::UnsafeHtmlOutput => "unsafe_html_output",
        }
    }

    /// Tells if a sink of this category is a critical defect for `tdg`. Weak
    /// cryptography, a disabled certificate check, and a token in the source are
    /// findings of `security`, and they do not fail the score.
    pub fn is_critical_defect(self) -> bool {
        matches!(
            self,
            Self::CommandExecution
                | Self::CodeEvaluation
                | Self::UnsafeDeserialization
                | Self::MemoryUnsafe
        )
    }

    /// Tells if the category is off unless the caller asks for it. The review of
    /// real repositories found that almost each token in the source of a test is
    /// a made-up value. Thus the category is below the precision that omen needs.
    pub fn is_experimental(self) -> bool {
        matches!(self, Self::HardcodedCredential)
    }

    /// The number of the Common Weakness Enumeration entry for the category.
    pub fn cwe(self) -> u32 {
        match self {
            Self::CommandExecution => 78,
            Self::CodeEvaluation => 95,
            Self::UnsafeDeserialization => 502,
            Self::SqlStringConstruction => 89,
            Self::WeakHash => 328,
            Self::WeakCipher => 327,
            Self::TlsVerificationDisabled => 295,
            Self::HardcodedCredential => 798,
            Self::MemoryUnsafe => 119,
            Self::UnsafeHtmlOutput => 79,
        }
    }
}

/// The evidence that the callee is the one that the table names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SinkBasis {
    /// Only the name gives the evidence. The receiver has no known type.
    NameOnly,
    /// The language or the standard library gives the name without an import,
    /// or the syntax itself is the sink, as an `unsafe` block is.
    Builtin,
    /// An import binds the callee to the module that the table names.
    ImportedSymbol,
}

impl SinkBasis {
    /// The name of the basis in the output, for example `imported_symbol`.
    pub fn name(self) -> &'static str {
        match self {
            Self::NameOnly => "name_only",
            Self::Builtin => "builtin",
            Self::ImportedSymbol => "imported_symbol",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SinkFlags {
    /// The call has arguments, and each argument is a literal.
    pub constant_argument: bool,
    /// The sink is in a test file or in a Rust `#[cfg(test)]` module.
    pub in_test: bool,
    /// The sink is in a Go file that has the standard header of generated code,
    /// `// Code generated ... DO NOT EDIT.`, before the package clause.
    #[serde(default)]
    pub in_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sink {
    /// `file:line:column:category`. The line and the column are those of `range`.
    pub key: String,
    pub file: String,
    pub category: SinkCategory,
    pub cwe: u32,
    /// The name of the callee for a call. The expression or the literal for other sinks.
    pub range: Span,
    /// The key of the nearest function that contains the sink.
    pub enclosing: Option<String>,
    pub callee_text: String,
    /// What the sink is, not how the source writes it: the resolved path of the callee, such
    /// as `os.system` for `operating_system.system` after `import os as operating_system`.
    /// A sink that is not a call has the text of the construct.
    #[serde(default)]
    pub target: String,
    pub basis: SinkBasis,
    pub flags: SinkFlags,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::core::Language;
    use crate::index::extract::{extract_file, link_files, FileSymbols};
    use crate::index::imports::{resolve_imports, Manifests};
    use crate::index::resolve::resolve_references;
    use crate::parser::{ParseResult, Parser};

    type Found = (String, u32, &'static str, &'static str);

    fn sinks(language: Language, files: &[(&str, &str)]) -> Vec<Sink> {
        let parsed: Vec<ParseResult> = files
            .iter()
            .map(|(name, text)| {
                Parser::new()
                    .parse(text.as_bytes(), language, Path::new(name))
                    .unwrap()
            })
            .collect();
        let mut index: Vec<FileSymbols> = files
            .iter()
            .zip(&parsed)
            .map(|((name, _), parsed)| extract_file(parsed, name))
            .collect();
        link_files(&mut index);
        resolve_imports(&mut index, &Manifests::default());
        resolve_references(&mut index);
        find_sinks(&parsed, &index)
    }

    fn found(language: Language, files: &[(&str, &str)]) -> Vec<Found> {
        sinks(language, files)
            .into_iter()
            .map(|sink| {
                (
                    sink.file,
                    sink.range.start_line,
                    sink.category.name(),
                    sink.basis.name(),
                )
            })
            .collect()
    }

    fn one(file: &str, line: u32, category: &'static str, basis: &'static str) -> Found {
        (file.to_string(), line, category, basis)
    }

    #[test]
    fn category_names_and_cwe_numbers_follow_the_design() {
        let table: Vec<(&str, u32)> = SinkCategory::ALL
            .iter()
            .map(|category| (category.name(), category.cwe()))
            .collect();
        assert_eq!(
            table,
            [
                ("command_execution", 78),
                ("code_evaluation", 95),
                ("unsafe_deserialization", 502),
                ("sql_string_construction", 89),
                ("weak_hash", 328),
                ("weak_cipher", 327),
                ("tls_verification_disabled", 295),
                ("hardcoded_credential", 798),
                ("memory_unsafe", 119),
                ("unsafe_html_output", 79),
            ]
        );
    }

    #[test]
    fn an_alias_of_a_module_resolves_to_the_sink() {
        let source =
            "import subprocess as sp\n\ndef run(command):\n    sp.run(command, shell=True)\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [one("a.py", 4, "command_execution", "imported_symbol")]
        );
    }

    #[test]
    fn an_alias_of_a_name_resolves_to_the_sink() {
        let source = "from pickle import loads as revive\n\ndef run(blob):\n    revive(blob)\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [one("a.py", 4, "unsafe_deserialization", "imported_symbol")]
        );
    }

    #[test]
    fn a_call_without_the_dangerous_argument_is_not_a_sink() {
        let source = "import subprocess\nimport yaml\n\ndef run(command, text):\n    subprocess.run([\"ls\"])\n    yaml.load(text, Loader=yaml.SafeLoader)\n";
        assert_eq!(found(Language::Python, &[("a.py", source)]), []);
    }

    #[test]
    fn a_repository_function_with_the_name_of_a_sink_is_not_a_sink() {
        let helpers = "def system(command):\n    return command\n";
        let main = "from helpers import system\nimport os\n\ndef run(command):\n    system(command)\n    os.system(command)\n";
        assert_eq!(
            found(
                Language::Python,
                &[("helpers.py", helpers), ("main.py", main)]
            ),
            [one("main.py", 6, "command_execution", "imported_symbol")]
        );
    }

    #[test]
    fn a_name_in_a_string_or_a_comment_is_not_a_sink() {
        let source = "import os\n\ndef run(command):\n    text = \"os.system(command)\"\n    # os.system(command)\n    return text\n";
        assert_eq!(found(Language::Python, &[("a.py", source)]), []);
    }

    #[test]
    fn a_builtin_has_the_basis_builtin() {
        let source = "def run(expression):\n    eval(expression)\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [one("a.py", 2, "code_evaluation", "builtin")]
        );
    }

    #[test]
    fn a_built_string_makes_a_sql_sink_and_a_constant_string_does_not() {
        let source = "def run(cursor, name):\n    cursor.execute(\"SELECT \" + name)\n    cursor.execute(f\"SELECT {name}\")\n    cursor.execute(\"SELECT 1\")\n    cursor.execute(\"SELECT %s\", (name,))\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [
                one("a.py", 2, "sql_string_construction", "name_only"),
                one("a.py", 3, "sql_string_construction", "name_only"),
            ]
        );
    }

    #[test]
    fn a_receiver_of_a_repository_type_is_not_a_sql_sink() {
        let helpers =
            "class Gateway:\n    def execute(self, statement):\n        return statement\n";
        let main = "from helpers import Gateway\n\ndef run(name):\n    gateway = Gateway()\n    gateway.execute(\"SELECT \" + name)\n";
        assert_eq!(
            found(
                Language::Python,
                &[("helpers.py", helpers), ("main.py", main)]
            ),
            []
        );
    }

    #[test]
    fn a_token_with_a_provider_prefix_is_a_credential() {
        let source =
            "token = \"AKIAJ3KD92LQ7MZP5XWB\"\nname = \"AKIAEXAMPLE\"\n# AKIAJ3KD92LQ7MZP5XWB\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [one("a.py", 1, "hardcoded_credential", "builtin")]
        );
    }

    #[test]
    fn the_example_token_of_the_provider_documentation_is_not_a_credential() {
        let source = "token = \"AKIAIOSFODNN7EXAMPLE\"\nother = \"AKIAJ3KD92LQ7MZP5XWB\"\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [one("a.py", 2, "hardcoded_credential", "builtin")]
        );
    }

    #[test]
    fn a_sql_string_that_the_driver_composes_is_not_a_sql_sink() {
        let source = "from psycopg import sql\n\ndef run(cursor, name, query):\n    cursor.execute(sql.SQL(\"DROP TABLE {}\").format(sql.Identifier(name)))\n    cursor.execute(DELETE.format(table=sql.Identifier(name)))\n    cursor.execute(sql.SQL(query).format(sql.Identifier(name)))\n    cursor.execute(\"DROP TABLE {}\".format(name))\n    cursor.execute(sql.SQL(\"SELECT {} WHERE \").format(sql.Identifier(name)) + condition)\n    cursor.execute(sql.SQL(\"SELECT \") + condition)\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [one("a.py", 7, "sql_string_construction", "name_only")]
        );
    }

    #[test]
    fn a_digest_that_declares_it_is_not_for_security_is_not_a_weak_hash() {
        let source = "import hashlib\n\ndef run(data):\n    hashlib.md5(data, usedforsecurity=False)\n    hashlib.sha1(data)\n    hashlib.new(\"md5\", data, usedforsecurity=False)\n    hashlib.new(\"md5\", data)\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [
                one("a.py", 5, "weak_hash", "imported_symbol"),
                one("a.py", 7, "weak_hash", "imported_symbol"),
            ]
        );
    }

    #[test]
    fn the_curl_option_for_a_configuration_file_is_not_the_insecure_option() {
        let source = "curl -K - http://a\ncurl -k http://b\ncurl --insecure http://c\n";
        assert_eq!(
            found(Language::Bash, &[("a.sh", source)]),
            [
                one("a.sh", 2, "tls_verification_disabled", "builtin"),
                one("a.sh", 3, "tls_verification_disabled", "builtin"),
            ]
        );
    }

    #[test]
    fn a_ruby_eval_of_a_block_is_not_code_evaluation() {
        let source = "def run(block, text)\n  instance_eval(&block)\n  instance_eval { 1 }\n  instance_eval(text)\n  eval(text)\nend\n";
        assert_eq!(
            found(Language::Ruby, &[("a.rb", source)]),
            [
                one("a.rb", 4, "code_evaluation", "builtin"),
                one("a.rb", 5, "code_evaluation", "builtin"),
            ]
        );
    }

    #[test]
    fn a_file_in_a_test_directory_has_the_flag_in_test() {
        let source = "import os\n\ndef test_run(command):\n    os.system(\"ls\")\n";
        let result = sinks(Language::Python, &[("tests/test_a.py", source)]);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].flags,
            SinkFlags {
                constant_argument: true,
                in_test: true,
                in_generated: false,
            }
        );
        let result = sinks(Language::Python, &[("a.py", source)]);
        assert!(!result[0].flags.in_test);
    }

    #[test]
    fn a_rust_unsafe_block_is_a_sink_and_a_cfg_test_module_has_the_flag() {
        let source = "pub fn first(pointer: *const u8) -> u8 {\n    unsafe { *pointer }\n}\n\n#[cfg(test)]\nmod tests {\n    fn second(pointer: *const u8) -> u8 {\n        unsafe { *pointer }\n    }\n}\n";
        let result = sinks(Language::Rust, &[("a.rs", source)]);
        let flags: Vec<(u32, bool)> = result
            .iter()
            .map(|sink| (sink.range.start_line, sink.flags.in_test))
            .collect();
        assert_eq!(flags, [(2, false), (8, true)]);
        assert!(result.iter().all(|sink| sink.basis == SinkBasis::Builtin));
        assert_eq!(result[0].enclosing.as_deref(), Some("a.rs#first"));
    }

    #[test]
    fn a_rust_use_declaration_binds_the_command_type() {
        let source = "use std::process::Command as Spawn;\n\npub fn run(program: &str) {\n    Spawn::new(program);\n    std::process::id();\n}\n";
        assert_eq!(
            found(Language::Rust, &[("a.rs", source)]),
            [one("a.rs", 4, "command_execution", "imported_symbol")]
        );
    }

    #[test]
    fn a_java_receiver_chain_resolves_unless_the_class_is_a_repository_class() {
        let real = "public class A {\n    void run(String command) throws Exception {\n        Runtime.getRuntime().exec(command);\n    }\n}\n";
        assert_eq!(
            found(Language::Java, &[("A.java", real)]),
            [one("A.java", 3, "command_execution", "builtin")]
        );
        let runtime = "package helpers;\n\npublic class Runtime {\n    public static Runtime getRuntime() {\n        return new Runtime();\n    }\n\n    public void exec(String command) {\n    }\n}\n";
        let decoy = "import helpers.Runtime;\n\npublic class B {\n    void run(String command) {\n        Runtime.getRuntime().exec(command);\n    }\n}\n";
        assert_eq!(
            found(
                Language::Java,
                &[("helpers/Runtime.java", runtime), ("B.java", decoy)]
            ),
            []
        );
    }

    #[test]
    fn a_constructed_receiver_gives_its_type() {
        let csharp = "using System.Runtime.Serialization.Formatters.Binary;\n\npublic class A {\n    public void Run(System.IO.Stream stream) {\n        new BinaryFormatter().Deserialize(stream);\n    }\n}\n";
        assert_eq!(
            found(Language::CSharp, &[("A.cs", csharp)]),
            [one("A.cs", 5, "unsafe_deserialization", "imported_symbol")]
        );
        let java = "import java.io.ObjectInputStream;\nimport java.io.InputStream;\n\npublic class A {\n    void run(InputStream stream) throws Exception {\n        new ObjectInputStream(stream).readObject();\n    }\n}\n";
        assert_eq!(
            found(Language::Java, &[("A.java", java)]),
            [one(
                "A.java",
                6,
                "unsafe_deserialization",
                "imported_symbol"
            )]
        );
    }

    #[test]
    fn the_callee_text_of_a_constructed_receiver_has_no_arguments() {
        let java = "import java.io.ObjectInputStream;\n\npublic class A {\n    void run() throws Exception {\n        (new ObjectInputStream(new Secret(\"AKIAJ3KD92LQ7MZP5XWB\"))).readObject();\n    }\n}\n";
        let result = sinks(Language::Java, &[("A.java", java)]);
        let callees: Vec<(&str, &str)> = result
            .iter()
            .map(|sink| (sink.category.name(), sink.callee_text.as_str()))
            .collect();
        assert_eq!(
            callees,
            [
                ("hardcoded_credential", "AKIA"),
                ("unsafe_deserialization", "ObjectInputStream.readObject"),
            ]
        );
    }

    #[test]
    fn a_literal_in_parentheses_is_a_literal() {
        let source =
            "int run(void *db) {\n    return sqlite3_exec(db, (\"SELECT 1\"), 0, 0, 0);\n}\n";
        assert_eq!(found(Language::C, &[("a.c", source)]), []);
    }

    #[test]
    fn a_validator_is_a_sink_only_when_its_whole_body_is_true() {
        let source = "using System.Net.Http;\n\npublic class A {\n    void Run(HttpClientHandler handler) {\n        handler.ServerCertificateCustomValidationCallback = (m, c, ch, e) => true && e == 0;\n        handler.ServerCertificateCustomValidationCallback = (m, c, ch, e) => { return true; };\n        handler.ServerCertificateCustomValidationCallback = (m, c, ch, e) => (true);\n    }\n}\n";
        let lines: Vec<u32> = sinks(Language::CSharp, &[("A.cs", source)])
            .iter()
            .map(|sink| sink.range.start_line)
            .collect();
        assert_eq!(lines, [6, 7]);
    }

    #[test]
    fn the_last_import_before_the_call_binds_the_name() {
        let unsafe_last = "import yaml\nfrom yaml import SafeLoader as Chosen\nfrom yaml import FullLoader as Chosen\n\ndef run(text):\n    yaml.load(text, Loader=Chosen)\n";
        assert_eq!(
            found(Language::Python, &[("a.py", unsafe_last)]),
            [one("a.py", 6, "unsafe_deserialization", "imported_symbol")]
        );
        let safe_last = "import yaml\nfrom yaml import FullLoader as Chosen\nfrom yaml import SafeLoader as Chosen\n\ndef run(text):\n    yaml.load(text, Loader=Chosen)\n";
        assert_eq!(found(Language::Python, &[("b.py", safe_last)]), []);
    }

    #[test]
    fn a_csharp_using_directive_gives_the_namespace_of_a_type() {
        let source = "using System.Diagnostics;\n\npublic class A {\n    public void Run(string command) {\n        Process.Start(command);\n    }\n}\n";
        assert_eq!(
            found(Language::CSharp, &[("A.cs", source)]),
            [one("A.cs", 5, "command_execution", "imported_symbol")]
        );
    }

    #[test]
    fn a_go_struct_field_and_a_ruby_constant_are_sinks_that_are_not_calls() {
        let go = "package main\n\nimport \"crypto/tls\"\n\nfunc run() {\n\t_ = &tls.Config{InsecureSkipVerify: true}\n\t_ = &tls.Config{InsecureSkipVerify: false}\n}\n";
        assert_eq!(
            found(Language::Go, &[("a.go", go)]),
            [one("a.go", 6, "tls_verification_disabled", "name_only")]
        );
        let ruby = "def run(http)\n  http.verify_mode = OpenSSL::SSL::VERIFY_NONE\n  http.verify_mode = OpenSSL::SSL::VERIFY_PEER\nend\n";
        assert_eq!(
            found(Language::Ruby, &[("a.rb", ruby)]),
            [one("a.rb", 2, "tls_verification_disabled", "name_only")]
        );
    }

    #[test]
    fn a_c_function_is_a_builtin_unless_the_repository_defines_it() {
        let library = "int run(const char *command) {\n    return system(command);\n}\n";
        assert_eq!(
            found(Language::C, &[("a.c", library)]),
            [one("a.c", 2, "command_execution", "builtin")]
        );
        let own = "int system(const char *command) {\n    return 0;\n}\n\nint run(const char *command) {\n    return system(command);\n}\n";
        assert_eq!(found(Language::C, &[("b.c", own)]), []);
    }

    #[test]
    fn a_symbol_has_its_entry_point_its_sinks_and_the_paths_through_it() {
        let source = "import os\nfrom flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/run\")\ndef entry():\n    return helper()\n\ndef helper():\n    os.system(\"x\" + os.environ[\"A\"])\n\ndef other():\n    return 1\n";
        let analysis =
            analysis::analyze_sources(&[("app.py", source)], crate::index::view::DEFAULT_MIN_BASIS);
        let entry = symbol_security(&analysis, "app.py#entry");
        assert_eq!(
            entry.entry_point.as_ref().unwrap().route.as_deref(),
            Some("GET /run")
        );
        assert_eq!(entry.reaches.len(), 1);
        let helper = symbol_security(&analysis, "app.py#helper");
        assert!(helper.entry_point.is_none());
        assert_eq!(helper.sinks_in_body.len(), 1);
        assert_eq!(helper.sinks_in_body[0].category, "command_execution");
        assert_eq!(helper.reachable_from.len(), 1);
        let other = symbol_security(&analysis, "app.py#other");
        assert_eq!(other, SymbolSecurity::default());
    }

    #[test]
    fn the_sinks_are_sorted_and_have_a_key() {
        let source = "import os\nimport pickle\n\ndef run(command, blob):\n    pickle.loads(blob)\n    os.system(command)\n";
        let result = sinks(Language::Python, &[("a.py", source)]);
        let keys: Vec<&str> = result.iter().map(|sink| sink.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "a.py:5:11:unsafe_deserialization",
                "a.py:6:7:command_execution"
            ]
        );
        assert_eq!(result[0].cwe, 502);
        assert_eq!(result[0].enclosing.as_deref(), Some("a.py#run"));
        assert_eq!(result[0].callee_text, "pickle.loads");
    }

    #[test]
    fn a_local_in_an_inner_block_after_the_call_does_not_hide_the_sink_go() {
        let source = "package main\n\nimport \"os/exec\"\n\nfunc run(cmd string) {\n\texec.Command(cmd)\n\t{\n\t\texec := struct{}{}\n\t\t_ = exec\n\t}\n}\n";
        assert_eq!(
            found(Language::Go, &[("a.go", source)]),
            [one("a.go", 6, "command_execution", "imported_symbol")]
        );
    }

    #[test]
    fn a_local_declared_after_the_call_does_not_hide_the_sink_go() {
        let source = "package main\n\nimport \"os/exec\"\n\nfunc run(cmd string) {\n\texec.Command(cmd)\n\texec := 1\n\t_ = exec\n}\n";
        assert_eq!(
            found(Language::Go, &[("a.go", source)]),
            [one("a.go", 6, "command_execution", "imported_symbol")]
        );
    }

    #[test]
    fn only_the_sql_argument_makes_a_built_string_a_sink_go() {
        let source = "package main\n\nimport (\n\t\"database/sql\"\n\t\"fmt\"\n)\n\nfunc run(db *sql.DB, ctx int, v string) {\n\tdb.Exec(\"INSERT INTO t VALUES (?)\", fmt.Sprintf(\"%s\", v))\n\tdb.Exec(fmt.Sprintf(\"INSERT INTO t VALUES (%s)\", v))\n\tdb.ExecContext(ctx, \"INSERT INTO t VALUES (?)\", fmt.Sprintf(\"%s\", v))\n\tdb.ExecContext(ctx, fmt.Sprintf(\"INSERT INTO t VALUES (%s)\", v))\n\tdb.QueryContext(fmt.Sprintf(\"%s\", v), \"SELECT 1\")\n}\n";
        assert_eq!(
            found(Language::Go, &[("a.go", source)]),
            [
                one("a.go", 10, "sql_string_construction", "name_only"),
                one("a.go", 12, "sql_string_construction", "name_only"),
            ]
        );
    }

    #[test]
    fn only_the_sql_argument_makes_a_built_string_a_sink_php() {
        let source = "<?php\nfunction run($link, $name) {\n    mysql_query(\"SELECT 1\", \"a\" . $name);\n    mysql_query(\"SELECT \" . $name, $link);\n    mysqli_query($link, \"SELECT '$name'\");\n    mysqli_query(\"a\" . $name, \"SELECT 1\");\n    pg_query(\"SELECT \" . $name);\n    pg_query($link, \"SELECT \" . $name);\n    sqlite_query($link, \"SELECT 1\");\n}\n";
        assert_eq!(
            found(Language::Php, &[("a.php", source)]),
            [
                one("a.php", 4, "sql_string_construction", "builtin"),
                one("a.php", 5, "sql_string_construction", "builtin"),
                one("a.php", 7, "sql_string_construction", "builtin"),
                one("a.php", 8, "sql_string_construction", "builtin"),
            ]
        );
    }

    #[test]
    fn the_sql_argument_of_a_c_call_is_the_second_argument() {
        let source = "int run(void *db, const char *query, const char *name) {\n    sqlite3_exec(db, query, 0, 0, 0);\n    mysql_query(db, query);\n    mysql_real_query(db, query, 3);\n    PQexec(db, query);\n    mysql_query(db, \"SELECT 1\");\n    return 0;\n}\n";
        let lines: Vec<u32> = found(Language::C, &[("a.c", source)])
            .iter()
            .map(|sink| sink.1)
            .collect();
        assert_eq!(lines, [2, 3, 4, 5]);
    }

    #[test]
    fn a_hostname_verifier_that_always_gives_true_is_a_sink_without_regard_to_spacing_java() {
        let source = "import javax.net.ssl.HttpsURLConnection;\n\nclass A {\n    void run() {\n        HttpsURLConnection.setDefaultHostnameVerifier((host, session)->true);\n        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> true);\n        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> { return true; });\n        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> trueish(host));\n        HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> host.equals(\"a\"));\n    }\n}\n";
        let lines: Vec<u32> = found(Language::Java, &[("A.java", source)])
            .iter()
            .map(|sink| sink.1)
            .collect();
        assert_eq!(lines, [5, 6, 7]);
    }

    /// The line, the category, and the flag `constant_argument` of each sink.
    fn constants(language: Language, files: &[(&str, &str)]) -> Vec<(u32, &'static str, bool)> {
        sinks(language, files)
            .into_iter()
            .map(|sink| {
                (
                    sink.range.start_line,
                    sink.category.name(),
                    sink.flags.constant_argument,
                )
            })
            .collect()
    }

    #[test]
    fn a_process_that_os_or_syscall_starts_is_a_command_sink_go() {
        let source = "package main\n\nimport (\n\t\"os\"\n\tsys \"syscall\"\n)\n\nfunc run(program string, arguments []string) {\n\tos.StartProcess(program, arguments, nil)\n\tsys.Exec(program, arguments, nil)\n\tsys.ForkExec(program, arguments, nil)\n\tsys.StartProcess(program, arguments, nil)\n\tos.Getpid()\n\ttext := \"os.StartProcess(program)\"\n\t// syscall.Exec(program)\n\t_ = text\n}\n";
        assert_eq!(
            found(Language::Go, &[("a.go", source)]),
            [
                one("a.go", 9, "command_execution", "imported_symbol"),
                one("a.go", 10, "command_execution", "imported_symbol"),
                one("a.go", 11, "command_execution", "imported_symbol"),
                one("a.go", 12, "command_execution", "imported_symbol"),
            ]
        );
    }

    #[test]
    fn a_concatenation_of_literals_is_a_constant_argument() {
        let go = "package main\n\nimport \"database/sql\"\n\nfunc run(db *sql.DB, id int, name string) {\n\tdb.Query(\"SELECT a \"+\n\t\t\"FROM t WHERE id = ?\", id)\n\tdb.Query(\"SELECT a FROM t WHERE name = '\" + name + \"'\")\n\tdb.Query((\"SELECT \" + \"a \") + \"FROM t\")\n}\n";
        assert_eq!(
            constants(Language::Go, &[("a.go", go)]),
            [
                (6, "sql_string_construction", true),
                (8, "sql_string_construction", false),
                (9, "sql_string_construction", true),
            ]
        );
        let python = "def run(cursor, name):\n    cursor.execute(\"SELECT a \" + \"FROM t WHERE id = %s\", (name,))\n    cursor.execute(\"SELECT a \" + name)\n";
        assert_eq!(
            constants(Language::Python, &[("a.py", python)]),
            [
                (2, "sql_string_construction", true),
                (3, "sql_string_construction", false),
            ]
        );
        let java = "import java.sql.Statement;\n\nclass A {\n    void run(Statement statement, String name) throws Exception {\n        statement.executeQuery(\"SELECT a \" + \"FROM t\");\n        statement.executeQuery(\"SELECT a \" + name);\n    }\n}\n";
        assert_eq!(
            constants(Language::Java, &[("A.java", java)]),
            [
                (5, "sql_string_construction", true),
                (6, "sql_string_construction", false),
            ]
        );
        let php = "<?php\nfunction run($name) {\n    mysql_query(\"SELECT a \" . \"FROM t\");\n    mysql_query(\"SELECT a \" . $name);\n}\n";
        assert_eq!(
            constants(Language::Php, &[("a.php", php)]),
            [
                (3, "sql_string_construction", true),
                (4, "sql_string_construction", false),
            ]
        );
    }

    #[test]
    fn the_sql_methods_of_xorm_sqlx_and_gorm_take_a_built_string_go() {
        let source = "package main\n\nimport \"fmt\"\n\nfunc run(engine Engine, ctx int, table string) {\n\tengine.SQL(fmt.Sprintf(\"SELECT * FROM %s\", table)).Get(nil)\n\tengine.SQL(\"SELECT * FROM t WHERE id = ?\", table)\n\tengine.Raw(fmt.Sprintf(\"SELECT * FROM %s\", table))\n\tengine.Prepare(\"SELECT * FROM \" + table)\n\tengine.PrepareContext(ctx, \"SELECT * FROM \" + table)\n\tengine.Queryx(\"SELECT * FROM \" + table)\n\tengine.QueryRowx(\"SELECT * FROM \" + table)\n\tengine.MustExec(\"DELETE FROM \" + table)\n\tengine.NamedExec(\"DELETE FROM \" + table, nil)\n\tengine.NamedQuery(\"SELECT * FROM \" + table, nil)\n\tengine.QueryxContext(ctx, \"SELECT * FROM \" + table)\n\tengine.Get(fmt.Sprintf(\"key-%s\", table))\n\tengine.Select(fmt.Sprintf(\"a-%s\", table))\n}\n";
        let lines: Vec<u32> = found(Language::Go, &[("a.go", source)])
            .iter()
            .map(|sink| sink.1)
            .collect();
        assert_eq!(lines, [6, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    }

    #[test]
    fn a_tls_setting_that_is_not_a_constant_is_a_sink_with_the_flag_go() {
        let source = "package main\n\nimport \"crypto/tls\"\n\nfunc run(skip bool, options Options) {\n\t_ = &tls.Config{InsecureSkipVerify: true}\n\t_ = &tls.Config{InsecureSkipVerify: false}\n\t_ = &tls.Config{InsecureSkipVerify: skip}\n\t_ = &tls.Config{InsecureSkipVerify: options.Skip}\n\t_ = &Options{InsecureSkipVerify: skip}\n\tconfig := &tls.Config{}\n\tconfig.InsecureSkipVerify = skip\n\tconfig.InsecureSkipVerify = false\n\tconfig.InsecureSkipVerify = true\n\toptions.InsecureSkipVerify = skip\n}\n";
        assert_eq!(
            constants(Language::Go, &[("a.go", source)]),
            [
                (6, "tls_verification_disabled", true),
                (8, "tls_verification_disabled", false),
                (9, "tls_verification_disabled", false),
                (12, "tls_verification_disabled", false),
                (14, "tls_verification_disabled", true),
            ]
        );
    }

    #[test]
    fn a_tls_setting_that_is_not_a_constant_is_a_sink_with_the_flag_python() {
        let source = "import requests\n\ndef run(url, check):\n    requests.get(url, verify=False)\n    requests.get(url, verify=True)\n    requests.get(url, verify=check)\n    requests.get(url, verify=\"/etc/ca.pem\")\n    requests.get(url)\n";
        assert_eq!(
            constants(Language::Python, &[("a.py", source)]),
            [
                (4, "tls_verification_disabled", true),
                (6, "tls_verification_disabled", false),
            ]
        );
    }

    #[test]
    fn a_tls_setting_that_is_not_a_constant_is_a_sink_with_the_flag_ecma() {
        let source = "import https from \"https\";\n\nexport function run(url, check) {\n  https.request(url, { rejectUnauthorized: false });\n  https.request(url, { rejectUnauthorized: true });\n  https.request(url, { rejectUnauthorized: check });\n  https.request(url, { timeout: 1 });\n}\n";
        assert_eq!(
            constants(Language::JavaScript, &[("a.js", source)]),
            [
                (4, "tls_verification_disabled", true),
                (6, "tls_verification_disabled", false),
            ]
        );
    }

    #[test]
    fn a_tls_setting_that_is_not_a_constant_is_a_sink_with_the_flag_rust() {
        let source = "pub fn run(builder: reqwest::ClientBuilder, check: bool) {\n    builder.danger_accept_invalid_certs(true);\n    builder.danger_accept_invalid_certs(false);\n    builder.danger_accept_invalid_certs(check);\n    builder.danger_accept_invalid_hostnames(check);\n}\n";
        assert_eq!(
            constants(Language::Rust, &[("a.rs", source)]),
            [
                (2, "tls_verification_disabled", true),
                (4, "tls_verification_disabled", false),
                (5, "tls_verification_disabled", false),
            ]
        );
    }

    #[test]
    fn a_tls_setting_that_is_not_a_constant_is_a_sink_with_the_flag_php_and_c() {
        let php = "<?php\nfunction run($handle, $check) {\n    curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, false);\n    curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, true);\n    curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, $check);\n    curl_setopt($handle, CURLOPT_TIMEOUT, $check);\n}\n";
        assert_eq!(
            constants(Language::Php, &[("a.php", php)]),
            [
                (3, "tls_verification_disabled", true),
                (5, "tls_verification_disabled", false),
            ]
        );
        let c = "void run(void *handle, long check) {\n    curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 0L);\n    curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 1L);\n    curl_easy_setopt(handle, CURLOPT_SSL_VERIFYHOST, check);\n}\n";
        assert_eq!(
            constants(Language::C, &[("a.c", c)]),
            [
                (2, "tls_verification_disabled", true),
                (4, "tls_verification_disabled", false),
            ]
        );
    }

    #[test]
    fn a_go_file_with_the_generated_header_has_the_flag_in_generated() {
        let body = "package main\n\nimport \"os/exec\"\n\nfunc run(program string) {\n\texec.Command(program)\n}\n";
        let generated = |header: &str| -> Vec<bool> {
            let source = format!("{header}{body}");
            sinks(Language::Go, &[("a.go", source.as_str())])
                .iter()
                .map(|sink| sink.flags.in_generated)
                .collect()
        };
        assert_eq!(
            generated("// Code generated by protoc-gen-go. DO NOT EDIT.\n\n"),
            [true]
        );
        assert_eq!(
            generated("// Copyright 2024 The Authors.\n\n// Code generated by mockery v2. DO NOT EDIT.\n\n"),
            [true]
        );
        assert_eq!(
            generated("// Code generated by protoc-gen-go. DO NOT EDIT.\r\n\r\n"),
            [true]
        );
        assert_eq!(generated(""), [false]);
        assert_eq!(
            generated("// Code generated by hand. Edit it.\n\n"),
            [false]
        );
        assert_eq!(generated("// Code generated DO NOT EDIT.\n\n"), [false]);
        assert_eq!(
            generated("/* Code generated by x. DO NOT EDIT. */\n\n"),
            [false]
        );
        assert_eq!(
            generated("// code generated by x. DO NOT EDIT.\n\n"),
            [false]
        );
        let late = format!("{body}\n// Code generated by x. DO NOT EDIT.\n");
        let flags: Vec<bool> = sinks(Language::Go, &[("b.go", late.as_str())])
            .iter()
            .map(|sink| sink.flags.in_generated)
            .collect();
        assert_eq!(flags, [false]);
        let python = "# Code generated by x. DO NOT EDIT.\nimport os\n\ndef run(command):\n    os.system(command)\n";
        let flags: Vec<bool> = sinks(Language::Python, &[("a.py", python)])
            .iter()
            .map(|sink| sink.flags.in_generated)
            .collect();
        assert_eq!(flags, [false]);
    }

    /// The line, the basis, and the flag `constant_argument` of each sink of the category
    /// `unsafe_html_output`.
    fn html_sinks(language: Language, files: &[(&str, &str)]) -> Vec<(u32, &'static str, bool)> {
        sinks(language, files)
            .into_iter()
            .filter(|sink| sink.category == SinkCategory::UnsafeHtmlOutput)
            .map(|sink| {
                (
                    sink.range.start_line,
                    sink.basis.name(),
                    sink.flags.constant_argument,
                )
            })
            .collect()
    }

    #[test]
    fn a_conversion_to_a_trusted_template_type_is_unsafe_html_output_go() {
        let source = "package main\n\nimport (\n\t\"html/template\"\n\ttrusted \"html/template\"\n)\n\nfunc run(text string) {\n\t_ = template.HTML(text)\n\t_ = trusted.JS(text)\n\t_ = template.URL(text)\n\t_ = template.HTMLAttr(text)\n\t_ = template.CSS(text)\n\t_ = template.HTML(\"<br>\")\n\t_ = template.HTML(\"<a>\" + \"</a>\")\n\t_ = template.HTMLEscapeString(text)\n\t_ = \"template.HTML(text)\"\n\t// template.HTML(text)\n}\n";
        assert_eq!(
            html_sinks(Language::Go, &[("a.go", source)]),
            [
                (9, "imported_symbol", false),
                (10, "imported_symbol", false),
                (11, "imported_symbol", false),
                (12, "imported_symbol", false),
                (13, "imported_symbol", false),
                (14, "imported_symbol", true),
                (15, "imported_symbol", true),
            ]
        );
        let text_template = "package main\n\nimport \"text/template\"\n\nfunc run(text string) {\n\t_ = template.HTMLEscapeString(text)\n}\n";
        assert_eq!(html_sinks(Language::Go, &[("b.go", text_template)]), []);
    }

    #[test]
    fn a_string_that_code_marks_as_safe_is_unsafe_html_output_python() {
        let source = "from django.utils.safestring import mark_safe\nfrom markupsafe import Markup as Trusted, escape\n\ndef run(text):\n    mark_safe(text)\n    Trusted(text)\n    mark_safe(\"<br>\")\n    escape(text)\n";
        assert_eq!(
            html_sinks(Language::Python, &[("a.py", source)]),
            [
                (5, "imported_symbol", false),
                (6, "imported_symbol", false),
                (7, "imported_symbol", true),
            ]
        );
    }

    #[test]
    fn a_string_that_code_marks_as_safe_is_unsafe_html_output_ruby() {
        let source = "def run(text)\n  text.html_safe\n  \"<br>\".html_safe\n  ERB::Util.html_escape(text)\nend\n";
        assert_eq!(
            html_sinks(Language::Ruby, &[("a.rb", source)]),
            [(2, "name_only", false), (3, "name_only", true)]
        );
    }

    #[test]
    fn raw_html_in_the_dom_or_in_react_is_unsafe_html_output_ecma() {
        let source = "export function run(element, text) {\n  element.innerHTML = text;\n  element.outerHTML += text;\n  element.innerHTML = \"<br>\";\n  element.textContent = text;\n  element.insertAdjacentHTML(\"beforeend\", text);\n  element.insertAdjacentText(\"beforeend\", text);\n  return <div dangerouslySetInnerHTML={{ __html: text }} />;\n}\n\nexport function constant() {\n  return <div dangerouslySetInnerHTML={{ __html: \"<br>\" }} title=\"innerHTML\" />;\n}\n";
        assert_eq!(
            html_sinks(Language::Jsx, &[("a.jsx", source)]),
            [
                (2, "name_only", false),
                (3, "name_only", false),
                (4, "name_only", true),
                (6, "name_only", false),
                (8, "name_only", false),
                (12, "name_only", true),
            ]
        );
    }

    #[test]
    fn the_callee_text_of_a_member_assignment_has_no_literal() {
        let source = "export function run(element, text) {\n  element.querySelector(\"AKIAJ3KD92LQ7MZP5XWB\").innerHTML = text;\n  element.body.innerHTML = text;\n}\n";
        let callees: Vec<String> = sinks(Language::JavaScript, &[("a.js", source)])
            .into_iter()
            .filter(|sink| sink.category == SinkCategory::UnsafeHtmlOutput)
            .map(|sink| sink.callee_text)
            .collect();
        assert_eq!(callees, ["*.innerHTML", "element.body.innerHTML"]);
    }

    #[test]
    fn the_callee_text_of_a_struct_field_has_no_literal_go() {
        let source = "package main\n\nimport \"crypto/tls\"\n\nfunc run(settings Settings) {\n\t_ = &tls.Config{InsecureSkipVerify: lookup(\"AKIAJ3KD92LQ7MZP5XWB\")}\n\t_ = &tls.Config{InsecureSkipVerify: settings.Skip}\n\t_ = &tls.Config{InsecureSkipVerify: true}\n}\n";
        let callees: Vec<String> = sinks(Language::Go, &[("a.go", source)])
            .into_iter()
            .filter(|sink| sink.category == SinkCategory::TlsVerificationDisabled)
            .map(|sink| sink.callee_text)
            .collect();
        assert_eq!(
            callees,
            [
                "InsecureSkipVerify: <expression>",
                "InsecureSkipVerify: settings.Skip",
                "InsecureSkipVerify: true",
            ]
        );
    }

    #[test]
    fn a_shell_with_the_option_c_is_a_command_sink_ecma() {
        let source = "import { execFile, spawn, spawnSync, execFileSync } from \"node:child_process\";\n\nexport function run(command, program) {\n  execFile(\"sh\", [\"-c\", command]);\n  spawn(\"/bin/bash\", [\"-c\", command]);\n  spawnSync(\"zsh\", [\"-c\", \"ls\"]);\n  execFileSync(\"sh\", [\"-c\", command], { cwd: \"/\" });\n  spawn(\"sh\", [\"script.sh\", command]);\n  spawn(program, [\"-c\", command]);\n  spawn(\"git\", [\"-c\", command]);\n  execFile(program, []);\n  spawn(program, [\"-x\"]);\n  spawn(program, { shell: true });\n}\n";
        assert_eq!(
            constants(Language::TypeScript, &[("a.ts", source)]),
            [
                (4, "command_execution", false),
                (5, "command_execution", false),
                (6, "command_execution", true),
                (7, "command_execution", false),
                (13, "command_execution", false),
            ]
        );
    }

    #[test]
    fn a_shell_with_the_option_c_is_a_command_sink_python() {
        let source = "import subprocess\n\ndef run(command):\n    subprocess.run([\"sh\", \"-c\", command])\n    subprocess.check_output([\"/bin/bash\", \"-c\", \"ls\"])\n    subprocess.run([\"ls\", \"-c\", command])\n    subprocess.run([\"sh\", \"script.sh\"])\n";
        assert_eq!(
            constants(Language::Python, &[("a.py", source)]),
            [
                (4, "command_execution", false),
                (5, "command_execution", true),
            ]
        );
    }

    #[test]
    fn a_promisified_command_function_is_the_command_sink_ecma() {
        let source = "import { exec, execFile } from \"node:child_process\";\nimport * as childProcess from \"child_process\";\nimport { promisify } from \"node:util\";\nimport util from \"util\";\n\nconst asyncExec = promisify(exec);\nconst asyncExecFile = promisify(execFile);\nconst other = util.promisify(childProcess.exec);\nlet changed = promisify(exec);\n\nexport async function run(command) {\n  await asyncExec(command);\n  await asyncExecFile(command, []);\n  await asyncExecFile(\"sh\", [\"-c\", command]);\n  await other(command);\n  await changed(command);\n}\n";
        assert_eq!(
            found(Language::TypeScript, &[("a.ts", source)]),
            [
                one("a.ts", 12, "command_execution", "imported_symbol"),
                one("a.ts", 14, "command_execution", "imported_symbol"),
                one("a.ts", 15, "command_execution", "imported_symbol"),
            ]
        );
    }

    #[test]
    fn a_promisified_function_of_the_repository_is_not_a_sink_ecma() {
        let helpers = "export function exec(command) {\n  return command;\n}\n";
        let main = "import { exec } from \"./helpers\";\nimport { promisify } from \"node:util\";\n\nconst asyncExec = promisify(exec);\n\nexport async function run(command) {\n  await asyncExec(command);\n}\n";
        assert_eq!(
            found(
                Language::TypeScript,
                &[("helpers.ts", helpers), ("main.ts", main)]
            ),
            []
        );
    }

    #[test]
    fn a_script_of_the_vm_module_is_code_evaluation_ecma() {
        let source = "import { Script, createContext, compileFunction } from \"node:vm\";\nimport * as vm from \"vm\";\nimport type ivm from \"isolated-vm\";\n\nexport function run(code: string, context: ivm.Context, isolate: ivm.Isolate) {\n  const script = new Script(code);\n  script.runInContext(createContext({}));\n  new vm.Script(code).runInThisContext();\n  compileFunction(code, []);\n  vm.compileFunction(\"return 1\");\n  context.evalSync(code);\n  isolate.compileScriptSync(code);\n  createContext({});\n}\n";
        assert_eq!(
            constants(Language::TypeScript, &[("a.ts", source)]),
            [
                (6, "code_evaluation", false),
                (8, "code_evaluation", false),
                (9, "code_evaluation", false),
                (10, "code_evaluation", true),
                (11, "code_evaluation", false),
                (12, "code_evaluation", false),
            ]
        );
    }

    #[test]
    fn a_tls_property_in_any_object_literal_is_a_sink_ecma() {
        let source = "import https from \"node:https\";\nimport { Pool } from \"pg\";\n\nexport function run(url: string, allow: boolean, options: { rejectUnauthorized?: boolean }) {\n  https.request(url, { rejectUnauthorized: false });\n  new Pool({ ssl: { rejectUnauthorized: false } });\n  const tls = { host: url, rejectUnauthorized: !allow };\n  const safe = { rejectUnauthorized: true };\n  const { rejectUnauthorized } = options;\n  const quoted = { \"rejectUnauthorized\": false };\n  const text = \"rejectUnauthorized: false\";\n  // rejectUnauthorized: false\n  const env = { NODE_TLS_REJECT_UNAUTHORIZED: \"0\" };\n  return [tls, safe, rejectUnauthorized, quoted, text, env];\n}\n";
        assert_eq!(
            constants(Language::TypeScript, &[("a.ts", source)]),
            [
                (5, "tls_verification_disabled", true),
                (6, "tls_verification_disabled", true),
                (7, "tls_verification_disabled", false),
                (10, "tls_verification_disabled", true),
                (13, "tls_verification_disabled", true),
            ]
        );
    }

    #[test]
    fn the_text_of_a_tls_property_has_only_names_ecma() {
        let source = "export function run(options) {\n  return [\n    { rejectUnauthorized: options.verify },\n    { rejectUnauthorized: options.ssl as boolean },\n    { rejectUnauthorized: check(\"key\") },\n    { rejectUnauthorized: false },\n  ];\n}\n";
        let texts: Vec<String> = sinks(Language::TypeScript, &[("a.ts", source)])
            .into_iter()
            .map(|sink| sink.callee_text)
            .collect();
        assert_eq!(
            texts,
            [
                "rejectUnauthorized: options.verify",
                "rejectUnauthorized: <expression>",
                "rejectUnauthorized: <expression>",
                "rejectUnauthorized: false",
            ]
        );
    }

    #[test]
    fn a_tls_property_for_a_repository_function_is_not_a_sink_ecma() {
        let helpers = "export function request(options) {\n  return options;\n}\n";
        let main = "import { request } from \"./helpers\";\n\nexport function run() {\n  request({ rejectUnauthorized: false });\n}\n";
        assert_eq!(
            found(
                Language::TypeScript,
                &[("helpers.ts", helpers), ("main.ts", main)]
            ),
            []
        );
    }

    #[test]
    fn the_environment_variable_that_turns_off_tls_checks_is_a_sink_ecma() {
        let source = "export function run(value: string) {\n  process.env.NODE_TLS_REJECT_UNAUTHORIZED = \"0\";\n  process.env[\"NODE_TLS_REJECT_UNAUTHORIZED\"] = \"0\";\n  process.env.NODE_TLS_REJECT_UNAUTHORIZED = value;\n  process.env.NODE_TLS_REJECT_UNAUTHORIZED = \"1\";\n  settings.NODE_TLS_REJECT_UNAUTHORIZED = \"0\";\n}\n";
        assert_eq!(
            constants(Language::TypeScript, &[("a.ts", source)]),
            [
                (2, "tls_verification_disabled", true),
                (3, "tls_verification_disabled", true),
                (4, "tls_verification_disabled", false),
            ]
        );
    }

    #[test]
    fn an_array_argument_is_not_a_built_sql_string_ecma() {
        let source = "export async function run(git, db, branch: string) {\n  await git.raw([\"init\", `--initial-branch=${branch}`]);\n  await db.raw(`SELECT * FROM t WHERE name = '${branch}'`);\n  await db.query([`SELECT ${branch}`]);\n  await db.query`SELECT * FROM t WHERE name = ${branch}`;\n  return String.raw`${branch}\\.json`;\n}\n";
        assert_eq!(
            constants(Language::TypeScript, &[("a.ts", source)]),
            [(3, "sql_string_construction", false)]
        );
    }

    #[test]
    fn a_receiver_of_a_library_that_is_not_sql_is_not_a_sql_sink_ecma() {
        let source = "import type { SimpleGit } from \"simple-git\";\nimport type { Knex } from \"knex\";\nimport type { QueryRunner } from \"@n8n/typeorm\";\n\nexport async function run(git: SimpleGit, knex: Knex, runner: QueryRunner, other, name: string) {\n  await git.raw(`log ${name}`);\n  await knex.raw(`SELECT * FROM t WHERE name = '${name}'`);\n  await runner.query(`SELECT * FROM t WHERE name = '${name}'`);\n  await other.query(`SELECT * FROM t WHERE name = '${name}'`);\n}\n";
        assert_eq!(
            found(Language::TypeScript, &[("a.ts", source)]),
            [
                one("a.ts", 7, "sql_string_construction", "name_only"),
                one("a.ts", 8, "sql_string_construction", "name_only"),
                one("a.ts", 9, "sql_string_construction", "name_only"),
            ]
        );
    }

    #[test]
    fn an_inherited_sql_method_of_a_repository_class_is_a_sink_ecma() {
        let repository = "import { Repository } from \"typeorm\";\n\nexport class Statistics extends Repository<object> {\n  count() {\n    return 1;\n  }\n}\n\nexport class Own {\n  query(text: string) {\n    return text;\n  }\n}\n";
        let main = "import { Statistics, Own } from \"./repository\";\n\nexport async function run(statistics: Statistics, own: Own, id: string) {\n  await statistics.query(`SELECT * FROM t WHERE id = ${id}`);\n  await own.query(`SELECT * FROM t WHERE id = ${id}`);\n}\n";
        assert_eq!(
            found(
                Language::TypeScript,
                &[("main.ts", main), ("repository.ts", repository)]
            ),
            [one("main.ts", 4, "sql_string_construction", "name_only")]
        );
    }

    #[test]
    fn a_template_with_only_literal_substitutions_is_constant_ecma() {
        let source = "export async function run(db, prefix: string) {\n  await db.query(`SELECT * FROM ${\"users\"} WHERE id = ${1}`);\n  await db.query(`SELECT * FROM ${prefix}users`);\n}\n";
        assert_eq!(
            constants(Language::TypeScript, &[("a.ts", source)]),
            [
                (2, "sql_string_construction", true),
                (3, "sql_string_construction", false),
            ]
        );
    }

    #[test]
    fn a_trusted_html_string_is_unsafe_html_output_csharp_and_rust() {
        let csharp = "using Microsoft.AspNetCore.Html;\n\npublic class A {\n    public object Run(string text) {\n        new HtmlString(text);\n        return new HtmlString(\"<br>\");\n    }\n}\n";
        assert_eq!(
            html_sinks(Language::CSharp, &[("A.cs", csharp)]),
            [(5, "imported_symbol", false), (6, "imported_symbol", true)]
        );
        let rust = "use maud::PreEscaped;\n\npub fn run(text: String) {\n    PreEscaped(text);\n    maud::PreEscaped(\"<br>\");\n}\n";
        assert_eq!(
            html_sinks(Language::Rust, &[("a.rs", rust)]),
            [(4, "imported_symbol", false), (5, "imported_symbol", true)]
        );
    }

    #[test]
    fn a_local_in_a_block_that_holds_the_call_hides_the_sink_go() {
        let source = "package main\n\nimport \"os/exec\"\n\nfunc run(cmd string) {\n\t{\n\t\texec := struct{}{}\n\t\texec.Command(cmd)\n\t}\n}\n";
        assert_eq!(found(Language::Go, &[("a.go", source)]), []);
    }
}
