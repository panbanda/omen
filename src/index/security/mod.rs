//! Security findings of the shared index.
//!
//! A sink is a call, a member access, a literal, or a block that the table in
//! [`spec`] names. The matcher reports the fact that the sink exists. It does
//! not say that the code has a vulnerability, and it does not follow data.

mod matcher;
pub mod spec;
mod syntax;

use serde::{Deserialize, Serialize};

use super::model::Span;

pub use matcher::find_sinks;

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
            "token = \"AKIAIOSFODNN7EXAMPLE\"\nname = \"AKIAEXAMPLE\"\n# AKIAIOSFODNN7EXAMPLE\n";
        assert_eq!(
            found(Language::Python, &[("a.py", source)]),
            [one("a.py", 1, "hardcoded_credential", "builtin")]
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
}
