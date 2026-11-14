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
}

impl SinkCategory {
    pub const ALL: [Self; 9] = [
        Self::CommandExecution,
        Self::CodeEvaluation,
        Self::UnsafeDeserialization,
        Self::SqlStringConstruction,
        Self::WeakHash,
        Self::WeakCipher,
        Self::TlsVerificationDisabled,
        Self::HardcodedCredential,
        Self::MemoryUnsafe,
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
                in_test: true
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

    #[test]
    fn a_local_in_a_block_that_holds_the_call_hides_the_sink_go() {
        let source = "package main\n\nimport \"os/exec\"\n\nfunc run(cmd string) {\n\t{\n\t\texec := struct{}{}\n\t\texec.Command(cmd)\n\t}\n}\n";
        assert_eq!(found(Language::Go, &[("a.go", source)]), []);
    }
}
