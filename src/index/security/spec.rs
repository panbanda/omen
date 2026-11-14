//! The table of sinks and the syntax facts of each language.
//!
//! A row names a category, the languages that it covers, a target, and an
//! argument predicate. A predicate looks at the syntax of the arguments only.
//! It does not follow a value from its source.
//!
//! A module path uses the separators of any language: `.`, `::`, `/`, and `\`.
//! `|` separates alternatives in a module and in a callee. A callee `*` matches each name.
//! An empty module means a name that the language gives without an import.

use super::SinkCategory;
use crate::core::Language;

/// A test on the arguments of a call. The values and the prefixes are lower case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Predicate {
    Always,
    /// A keyword argument or an object property has the name, and its value has the text.
    Keyword {
        name: &'static str,
        value: &'static str,
    },
    /// The argument at the index is one of the values, without its quotation marks.
    ArgIn {
        index: usize,
        values: &'static [&'static str],
    },
    ArgStartsWith {
        index: usize,
        prefixes: &'static [&'static str],
    },
    ArgContains {
        index: usize,
        needles: &'static [&'static str],
    },
    AnyArgIn(&'static [&'static str]),
    AnyArgStartsWith(&'static [&'static str]),
    /// The loader argument is missing, or the imports resolve it to a loader that is not safe.
    /// A loader that the imports cannot resolve is not a sink.
    NotSafeLoader,
    /// The argument at the index exists and is not a literal.
    ArgNotLiteral {
        index: usize,
    },
    /// The argument at the index, or any argument, is a concatenated,
    /// interpolated, or formatted string.
    BuiltString {
        index: Option<usize>,
    },
    All(&'static [Predicate]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A call or a construction. Import resolution gives the module of the callee.
    Path {
        module: &'static str,
        callee: &'static str,
    },
    /// A method call on a receiver of unknown type. Only the name gives the evidence.
    Method {
        callee: &'static str,
    },
    /// A member access that is not a call.
    Mention {
        module: &'static str,
        name: &'static str,
    },
    /// A field of a struct literal. The literal has the type `owner` of the module `module`.
    KeyedField {
        key: &'static str,
        value: &'static str,
        module: &'static str,
        owner: &'static str,
    },
    /// An assignment to a member. The assigned value, without white space and in
    /// lower case, contains one of the values.
    Assignment {
        module: &'static str,
        name: &'static str,
        values: &'static [&'static str],
    },
    UnsafeBlock,
    /// A string literal that is a token with a provider prefix.
    SecretLiteral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkSpec {
    pub category: SinkCategory,
    pub languages: &'static [Language],
    pub target: Target,
    pub predicate: Predicate,
}

const PYTHON: &[Language] = &[Language::Python];
const ECMA: &[Language] = &[
    Language::JavaScript,
    Language::TypeScript,
    Language::Tsx,
    Language::Jsx,
];
const GO: &[Language] = &[Language::Go];
const RUST: &[Language] = &[Language::Rust];
const JAVA: &[Language] = &[Language::Java];
const CSHARP: &[Language] = &[Language::CSharp];
const RUBY: &[Language] = &[Language::Ruby];
const PHP: &[Language] = &[Language::Php];
const C: &[Language] = &[Language::C, Language::Cpp];
const CPP: &[Language] = &[Language::Cpp];
const BASH: &[Language] = &[Language::Bash];
const ALL: &[Language] = &[
    Language::Go,
    Language::Rust,
    Language::Python,
    Language::TypeScript,
    Language::JavaScript,
    Language::Tsx,
    Language::Jsx,
    Language::Java,
    Language::C,
    Language::Cpp,
    Language::CSharp,
    Language::Ruby,
    Language::Php,
    Language::Bash,
];

use Predicate::{All, Always, AnyArgIn, AnyArgStartsWith, ArgContains, ArgIn, ArgStartsWith};
use SinkCategory::{
    CodeEvaluation, CommandExecution, HardcodedCredential, MemoryUnsafe, SqlStringConstruction,
    TlsVerificationDisabled, UnsafeDeserialization, WeakCipher, WeakHash,
};

const fn path(
    category: SinkCategory,
    languages: &'static [Language],
    module: &'static str,
    callee: &'static str,
    predicate: Predicate,
) -> SinkSpec {
    SinkSpec {
        category,
        languages,
        target: Target::Path { module, callee },
        predicate,
    }
}

const fn method(
    category: SinkCategory,
    languages: &'static [Language],
    callee: &'static str,
    predicate: Predicate,
) -> SinkSpec {
    SinkSpec {
        category,
        languages,
        target: Target::Method { callee },
        predicate,
    }
}

const fn mention(
    category: SinkCategory,
    languages: &'static [Language],
    module: &'static str,
    name: &'static str,
) -> SinkSpec {
    SinkSpec {
        category,
        languages,
        target: Target::Mention { module, name },
        predicate: Always,
    }
}

const fn keyed(
    category: SinkCategory,
    languages: &'static [Language],
    key: &'static str,
    value: &'static str,
    module: &'static str,
    owner: &'static str,
) -> SinkSpec {
    SinkSpec {
        category,
        languages,
        target: Target::KeyedField {
            key,
            value,
            module,
            owner,
        },
        predicate: Always,
    }
}

const fn assignment(
    category: SinkCategory,
    languages: &'static [Language],
    module: &'static str,
    name: &'static str,
    values: &'static [&'static str],
) -> SinkSpec {
    SinkSpec {
        category,
        languages,
        target: Target::Assignment {
            module,
            name,
            values,
        },
        predicate: Always,
    }
}

const fn keyword(name: &'static str, value: &'static str) -> Predicate {
    Predicate::Keyword { name, value }
}

const fn built(index: usize) -> Predicate {
    Predicate::BuiltString { index: Some(index) }
}

const ANY_BUILT: Predicate = Predicate::BuiltString { index: None };

const WEAK_DIGESTS: &[&str] = &["md5", "sha1", "sha-1", "md2", "md4"];
const WEAK_CIPHERS: &[&str] = &["des", "rc4", "rc2", "bf", "blowfish", "arcfour"];
const CURL_VERIFY: &[&str] = &["curlopt_ssl_verifypeer", "curlopt_ssl_verifyhost"];

/// Each row of the table, in the order in which a sink of a category is chosen.
pub const SPECS: &[SinkSpec] = &[
    // Python
    path(CommandExecution, PYTHON, "os", "system|popen", Always),
    path(
        CommandExecution,
        PYTHON,
        "subprocess",
        "run|call|check_call|check_output|Popen",
        keyword("shell", "true"),
    ),
    path(CommandExecution, PYTHON, "subprocess", "getoutput|getstatusoutput", Always),
    path(CodeEvaluation, PYTHON, "", "eval|exec", Always),
    path(UnsafeDeserialization, PYTHON, "pickle|_pickle|cPickle", "loads|load", Always),
    path(UnsafeDeserialization, PYTHON, "marshal", "loads|load", Always),
    path(
        UnsafeDeserialization,
        PYTHON,
        "yaml",
        "load|load_all",
        Predicate::NotSafeLoader,
    ),
    path(UnsafeDeserialization, PYTHON, "yaml", "unsafe_load|unsafe_load_all", Always),
    method(SqlStringConstruction, PYTHON, "execute|executemany|executescript", built(0)),
    path(WeakHash, PYTHON, "hashlib", "md5|sha1", Always),
    path(
        WeakHash,
        PYTHON,
        "hashlib",
        "new",
        ArgIn {
            index: 0,
            values: WEAK_DIGESTS,
        },
    ),
    path(
        WeakCipher,
        PYTHON,
        "Crypto.Cipher.DES|Crypto.Cipher.ARC4|Crypto.Cipher.ARC2|Crypto.Cipher.Blowfish",
        "new",
        Always,
    ),
    path(
        WeakCipher,
        PYTHON,
        "Cryptodome.Cipher.DES|Cryptodome.Cipher.ARC4|Cryptodome.Cipher.ARC2|Cryptodome.Cipher.Blowfish",
        "new",
        Always,
    ),
    path(
        TlsVerificationDisabled,
        PYTHON,
        "requests|httpx",
        "get|post|put|patch|delete|head|options|request",
        keyword("verify", "false"),
    ),
    path(TlsVerificationDisabled, PYTHON, "ssl", "_create_unverified_context", Always),
    // JavaScript and TypeScript
    path(
        CommandExecution,
        ECMA,
        "child_process|node:child_process",
        "exec|execSync",
        Always,
    ),
    path(
        CommandExecution,
        ECMA,
        "child_process|node:child_process",
        "spawn|spawnSync|execFile|execFileSync",
        keyword("shell", "true"),
    ),
    path(CodeEvaluation, ECMA, "", "eval|Function", Always),
    path(
        CodeEvaluation,
        ECMA,
        "vm|node:vm",
        "runInNewContext|runInThisContext|runInContext",
        Always,
    ),
    path(UnsafeDeserialization, ECMA, "node-serialize", "unserialize", Always),
    method(
        SqlStringConstruction,
        ECMA,
        "query|execute|raw|queryRawUnsafe|$queryRawUnsafe",
        built(0),
    ),
    path(
        WeakHash,
        ECMA,
        "crypto|node:crypto",
        "createHash",
        ArgIn {
            index: 0,
            values: WEAK_DIGESTS,
        },
    ),
    path(
        WeakCipher,
        ECMA,
        "crypto|node:crypto",
        "createCipheriv|createCipher|createDecipheriv|createDecipher",
        ArgStartsWith {
            index: 0,
            prefixes: WEAK_CIPHERS,
        },
    ),
    path(
        TlsVerificationDisabled,
        ECMA,
        "https|node:https|tls|node:tls",
        "request|get|connect|Agent",
        keyword("rejectunauthorized", "false"),
    ),
    // Go
    // A command is built here. The sink does not say that the command runs.
    path(CommandExecution, GO, "os/exec", "Command|CommandContext", Always),
    method(
        SqlStringConstruction,
        GO,
        "Query|QueryRow|Exec|QueryContext|QueryRowContext|ExecContext",
        ANY_BUILT,
    ),
    path(WeakHash, GO, "crypto/md5|crypto/sha1", "New|Sum", Always),
    path(WeakCipher, GO, "crypto/des", "NewCipher|NewTripleDESCipher", Always),
    path(WeakCipher, GO, "crypto/rc4", "NewCipher", Always),
    keyed(
        TlsVerificationDisabled,
        GO,
        "InsecureSkipVerify",
        "true",
        "crypto/tls",
        "Config",
    ),
    path(
        MemoryUnsafe,
        GO,
        "unsafe",
        "Pointer|Slice|SliceData|String|StringData|Add",
        Always,
    ),
    // Rust
    // A command is built here. The sink does not say that the command runs.
    path(CommandExecution, RUST, "std::process::Command", "new", Always),
    path(
        SqlStringConstruction,
        RUST,
        "sqlx",
        "query|query_as|query_scalar",
        built(0),
    ),
    path(SqlStringConstruction, RUST, "diesel", "sql_query", built(0)),
    path(WeakHash, RUST, "md5", "compute", Always),
    path(WeakHash, RUST, "md5::Md5|sha1::Sha1", "new|digest|new_with_prefix", Always),
    path(
        WeakCipher,
        RUST,
        "des::Des|rc4::Rc4",
        "new|new_from_slice",
        Always,
    ),
    method(
        TlsVerificationDisabled,
        RUST,
        "danger_accept_invalid_certs|danger_accept_invalid_hostnames",
        ArgIn {
            index: 0,
            values: &["true"],
        },
    ),
    SinkSpec {
        category: MemoryUnsafe,
        languages: RUST,
        target: Target::UnsafeBlock,
        predicate: Always,
    },
    path(MemoryUnsafe, RUST, "std::mem", "transmute|transmute_copy", Always),
    path(
        MemoryUnsafe,
        RUST,
        "std::slice",
        "from_raw_parts|from_raw_parts_mut",
        Always,
    ),
    path(MemoryUnsafe, RUST, "std::ffi::CStr", "from_ptr", Always),
    // Java
    path(
        CommandExecution,
        JAVA,
        "java.lang.Runtime.getRuntime()",
        "exec",
        Always,
    ),
    path(CommandExecution, JAVA, "java.lang", "ProcessBuilder", Always),
    path(CodeEvaluation, JAVA, "javax.script.ScriptEngine", "eval", Always),
    path(
        UnsafeDeserialization,
        JAVA,
        "java.io.ObjectInputStream",
        "readObject|readUnshared",
        Always,
    ),
    path(
        SqlStringConstruction,
        JAVA,
        "java.sql.Statement",
        "execute|executeQuery|executeUpdate|executeLargeUpdate|addBatch",
        built(0),
    ),
    path(
        SqlStringConstruction,
        JAVA,
        "java.sql.Connection",
        "prepareStatement|prepareCall|nativeSQL",
        built(0),
    ),
    path(
        WeakHash,
        JAVA,
        "java.security.MessageDigest",
        "getInstance",
        ArgIn {
            index: 0,
            values: WEAK_DIGESTS,
        },
    ),
    path(
        WeakCipher,
        JAVA,
        "javax.crypto.Cipher",
        "getInstance",
        ArgStartsWith {
            index: 0,
            prefixes: WEAK_CIPHERS,
        },
    ),
    path(
        TlsVerificationDisabled,
        JAVA,
        "javax.net.ssl.HttpsURLConnection",
        "setDefaultHostnameVerifier|setHostnameVerifier",
        ArgContains {
            index: 0,
            needles: &["-> true", "noophostnameverifier", "allow_all"],
        },
    ),
    // C#
    path(CommandExecution, CSHARP, "System.Diagnostics.Process", "Start", Always),
    path(CommandExecution, CSHARP, "System.Diagnostics", "ProcessStartInfo", Always),
    path(
        CodeEvaluation,
        CSHARP,
        "Microsoft.CodeAnalysis.CSharp.Scripting.CSharpScript",
        "EvaluateAsync|RunAsync",
        Always,
    ),
    path(
        UnsafeDeserialization,
        CSHARP,
        "System.Runtime.Serialization.Formatters.Binary.BinaryFormatter|System.Runtime.Serialization.Formatters.Soap.SoapFormatter|System.Runtime.Serialization.NetDataContractSerializer",
        "Deserialize",
        Always,
    ),
    path(
        SqlStringConstruction,
        CSHARP,
        "System.Data.SqlClient|Microsoft.Data.SqlClient",
        "SqlCommand",
        built(0),
    ),
    path(
        WeakHash,
        CSHARP,
        "System.Security.Cryptography.MD5|System.Security.Cryptography.SHA1",
        "Create",
        Always,
    ),
    path(
        WeakCipher,
        CSHARP,
        "System.Security.Cryptography.DES|System.Security.Cryptography.RC2",
        "Create",
        Always,
    ),
    assignment(
        TlsVerificationDisabled,
        CSHARP,
        "System.Net.Http.HttpClientHandler",
        "ServerCertificateCustomValidationCallback",
        &["dangerousacceptanyservercertificatevalidator", "=>true"],
    ),
    assignment(
        TlsVerificationDisabled,
        CSHARP,
        "System.Net.ServicePointManager",
        "ServerCertificateValidationCallback",
        &["=>true"],
    ),
    SinkSpec {
        category: MemoryUnsafe,
        languages: CSHARP,
        target: Target::UnsafeBlock,
        predicate: Always,
    },
    // Ruby
    path(CommandExecution, RUBY, "", "system|exec|spawn", Always),
    path(
        CommandExecution,
        RUBY,
        "Open3",
        "popen3|popen2|popen2e|capture2|capture2e|capture3",
        Always,
    ),
    path(CommandExecution, RUBY, "IO", "popen", Always),
    path(CodeEvaluation, RUBY, "", "eval|instance_eval|class_eval|module_eval", Always),
    path(UnsafeDeserialization, RUBY, "Marshal", "load|restore", Always),
    path(UnsafeDeserialization, RUBY, "YAML", "unsafe_load|unsafe_load_file", Always),
    method(
        SqlStringConstruction,
        RUBY,
        "execute|exec_query|find_by_sql|where|select_all",
        built(0),
    ),
    path(WeakHash, RUBY, "Digest::MD5|Digest::SHA1", "hexdigest|digest|base64digest|new", Always),
    path(
        WeakCipher,
        RUBY,
        "OpenSSL::Cipher",
        "new",
        ArgStartsWith {
            index: 0,
            prefixes: WEAK_CIPHERS,
        },
    ),
    path(
        WeakCipher,
        RUBY,
        "OpenSSL::Cipher::DES|OpenSSL::Cipher::RC4|OpenSSL::Cipher::RC2|OpenSSL::Cipher::Blowfish",
        "new",
        Always,
    ),
    mention(TlsVerificationDisabled, RUBY, "OpenSSL::SSL", "VERIFY_NONE"),
    // PHP
    path(
        CommandExecution,
        PHP,
        "",
        "exec|shell_exec|system|passthru|popen|proc_open|pcntl_exec",
        Always,
    ),
    path(CodeEvaluation, PHP, "", "eval|assert|create_function", Always),
    path(UnsafeDeserialization, PHP, "", "unserialize", Always),
    path(
        SqlStringConstruction,
        PHP,
        "",
        "mysqli_query|mysql_query|pg_query|sqlite_query",
        ANY_BUILT,
    ),
    method(SqlStringConstruction, PHP, "query|exec|multi_query|real_query", built(0)),
    path(WeakHash, PHP, "", "md5|sha1|md5_file|sha1_file", Always),
    path(
        WeakHash,
        PHP,
        "",
        "hash",
        ArgIn {
            index: 0,
            values: WEAK_DIGESTS,
        },
    ),
    path(WeakCipher, PHP, "", "mcrypt_encrypt|mcrypt_decrypt|mcrypt_generic", Always),
    path(
        WeakCipher,
        PHP,
        "",
        "openssl_encrypt|openssl_decrypt",
        ArgStartsWith {
            index: 1,
            prefixes: WEAK_CIPHERS,
        },
    ),
    path(
        TlsVerificationDisabled,
        PHP,
        "",
        "curl_setopt",
        All(&[
            ArgIn {
                index: 1,
                values: CURL_VERIFY,
            },
            ArgIn {
                index: 2,
                values: &["false", "0"],
            },
        ]),
    ),
    // C and C++
    path(
        CommandExecution,
        C,
        "",
        "system|popen|execl|execlp|execle|execv|execvp|execve",
        Always,
    ),
    path(CommandExecution, CPP, "std", "system", Always),
    path(WeakHash, C, "", "MD5|MD5_Init|MD4|SHA1|SHA1_Init|EVP_md5|EVP_sha1", Always),
    path(
        WeakCipher,
        C,
        "",
        "DES_ecb_encrypt|DES_cbc_encrypt|DES_ncbc_encrypt|DES_set_key|DES_set_key_checked|RC4|RC4_set_key|EVP_des_ecb|EVP_des_cbc|EVP_rc4",
        Always,
    ),
    path(
        TlsVerificationDisabled,
        C,
        "",
        "curl_easy_setopt",
        All(&[
            ArgIn {
                index: 1,
                values: CURL_VERIFY,
            },
            ArgIn {
                index: 2,
                values: &["0l", "0", "false"],
            },
        ]),
    ),
    path(
        SqlStringConstruction,
        C,
        "",
        "sqlite3_exec|mysql_query|mysql_real_query|PQexec",
        Predicate::ArgNotLiteral { index: 1 },
    ),
    path(MemoryUnsafe, C, "", "gets|strcpy|strcat|sprintf|vsprintf", Always),
    path(MemoryUnsafe, CPP, "std", "strcpy|strcat|sprintf|vsprintf", Always),
    // Bash
    path(CodeEvaluation, BASH, "", "eval", Always),
    path(WeakHash, BASH, "", "md5sum|sha1sum", Always),
    path(
        WeakCipher,
        BASH,
        "",
        "openssl",
        AnyArgStartsWith(&["-des", "-rc4", "-rc2", "-bf"]),
    ),
    path(TlsVerificationDisabled, BASH, "", "curl", AnyArgIn(&["-k", "--insecure"])),
    path(
        TlsVerificationDisabled,
        BASH,
        "",
        "wget",
        AnyArgIn(&["--no-check-certificate"]),
    ),
    // Each language
    SinkSpec {
        category: HardcodedCredential,
        languages: ALL,
        target: Target::SecretLiteral,
        predicate: Always,
    },
];

/// The rows that cover a language.
pub fn specs_for(language: Language) -> impl Iterator<Item = &'static SinkSpec> {
    SPECS
        .iter()
        .filter(move |spec| spec.languages.contains(&language))
}

/// A token format with a provider prefix. The whole string must match.
#[derive(Debug, Clone, Copy)]
pub struct TokenFormat {
    pub prefix: &'static str,
    pub min_tail: usize,
    pub max_tail: usize,
    pub tail: fn(char) -> bool,
}

fn alphanumeric(character: char) -> bool {
    character.is_ascii_alphanumeric()
}

fn upper_alphanumeric(character: char) -> bool {
    character.is_ascii_uppercase() || character.is_ascii_digit()
}

fn url_safe(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-'
}

pub const TOKEN_FORMATS: &[TokenFormat] = &[
    TokenFormat {
        prefix: "AKIA",
        min_tail: 16,
        max_tail: 16,
        tail: upper_alphanumeric,
    },
    TokenFormat {
        prefix: "ghp_",
        min_tail: 36,
        max_tail: 36,
        tail: alphanumeric,
    },
    TokenFormat {
        prefix: "gho_",
        min_tail: 36,
        max_tail: 36,
        tail: alphanumeric,
    },
    TokenFormat {
        prefix: "ghs_",
        min_tail: 36,
        max_tail: 36,
        tail: alphanumeric,
    },
    TokenFormat {
        prefix: "xoxb-",
        min_tail: 10,
        max_tail: 80,
        tail: url_safe,
    },
    TokenFormat {
        prefix: "xoxp-",
        min_tail: 10,
        max_tail: 80,
        tail: url_safe,
    },
    TokenFormat {
        prefix: "sk_live_",
        min_tail: 24,
        max_tail: 99,
        tail: alphanumeric,
    },
    TokenFormat {
        prefix: "AIza",
        min_tail: 35,
        max_tail: 35,
        tail: url_safe,
    },
    TokenFormat {
        prefix: "glpat-",
        min_tail: 20,
        max_tail: 20,
        tail: url_safe,
    },
];

/// The syntax facts of a language that the matcher needs.
#[derive(Debug, Clone, Copy)]
pub struct LanguageSyntax {
    /// The node kinds of a string literal.
    pub string_kinds: &'static [&'static str],
    /// The node kinds of a member access that is not a call.
    pub mention_kinds: &'static [&'static str],
    /// The node kinds of a key and a value in a literal that the syntax names with children, not fields.
    pub keyed_kinds: &'static [&'static str],
    /// The node kinds of an assignment to a member.
    pub assignment_kinds: &'static [&'static str],
    /// The node kind of a block that the language marks as unsafe.
    pub unsafe_kinds: &'static [&'static str],
    /// The modules that the language gives without an import. A callee in one of them can be written without the module.
    pub implicit_modules: &'static [&'static str],
    /// A name has one value in the whole repository, thus a definition in any file replaces the library name.
    pub global_namespace: bool,
    /// The names in the same directory are visible without an import.
    pub same_directory_names: bool,
    /// The first name of a path that the language never binds locally. Each name in the list is a root.
    /// With `*`, each first name that is not a repository name is a root.
    pub roots: &'static [&'static str],
}

pub fn syntax_of(language: Language) -> LanguageSyntax {
    let base = LanguageSyntax {
        string_kinds: &["string"],
        mention_kinds: &[],
        keyed_kinds: &[],
        assignment_kinds: &[],
        unsafe_kinds: &[],
        implicit_modules: &[],
        global_namespace: false,
        same_directory_names: false,
        roots: &[],
    };
    match language {
        Language::Python => LanguageSyntax {
            implicit_modules: &["builtins"],
            ..base
        },
        Language::JavaScript | Language::TypeScript | Language::Tsx | Language::Jsx => {
            LanguageSyntax {
                string_kinds: &["string", "template_string"],
                ..base
            }
        }
        Language::Go => LanguageSyntax {
            string_kinds: &["interpreted_string_literal", "raw_string_literal"],
            keyed_kinds: &["keyed_element"],
            ..base
        },
        Language::Rust => LanguageSyntax {
            string_kinds: &["string_literal", "raw_string_literal"],
            roots: &["*"],
            unsafe_kinds: &["unsafe_block"],
            ..base
        },
        Language::Java => LanguageSyntax {
            string_kinds: &["string_literal"],
            implicit_modules: &["java.lang"],
            same_directory_names: true,
            ..base
        },
        Language::CSharp => LanguageSyntax {
            string_kinds: &[
                "string_literal",
                "verbatim_string_literal",
                "raw_string_literal",
            ],
            assignment_kinds: &["assignment_expression"],
            unsafe_kinds: &["unsafe_statement"],
            same_directory_names: true,
            ..base
        },
        Language::C => LanguageSyntax {
            string_kinds: &["string_literal"],
            global_namespace: true,
            ..base
        },
        Language::Cpp => LanguageSyntax {
            string_kinds: &["string_literal", "raw_string_literal"],
            global_namespace: true,
            roots: &["std"],
            ..base
        },
        Language::Ruby => LanguageSyntax {
            mention_kinds: &["scope_resolution"],
            implicit_modules: &["Kernel"],
            global_namespace: true,
            ..base
        },
        Language::Php => LanguageSyntax {
            string_kinds: &["string", "encapsed_string"],
            global_namespace: true,
            ..base
        },
        Language::Bash => LanguageSyntax {
            string_kinds: &["string", "raw_string"],
            global_namespace: true,
            ..base
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_language_has_a_row_for_the_credential_and_for_its_main_categories() {
        for language in ALL {
            let categories: Vec<SinkCategory> = specs_for(*language).map(|s| s.category).collect();
            assert!(
                categories.contains(&HardcodedCredential),
                "{language:?} has no credential row"
            );
            assert!(
                categories.contains(&WeakHash),
                "{language:?} has no weak hash row"
            );
        }
    }

    #[test]
    fn a_module_or_callee_has_no_blank_alternative_except_the_empty_module() {
        for spec in SPECS {
            if let Target::Path { module, callee } = spec.target {
                assert!(!callee.is_empty());
                assert!(callee.split('|').all(|name| !name.is_empty()));
                assert!(module.is_empty() || module.split('|').all(|part| !part.is_empty()));
            }
        }
    }

    #[test]
    fn predicate_values_are_lower_case() {
        fn check(predicate: &Predicate) {
            let lists: Vec<&[&str]> = match predicate {
                Predicate::ArgIn { values, .. } => vec![values],
                Predicate::ArgStartsWith { prefixes, .. } => vec![prefixes],
                Predicate::ArgContains { needles, .. } => vec![needles],
                Predicate::AnyArgIn(values) | Predicate::AnyArgStartsWith(values) => vec![values],
                Predicate::All(inner) => {
                    inner.iter().for_each(check);
                    vec![]
                }
                _ => vec![],
            };
            for list in lists {
                assert!(list.iter().all(|text| *text == text.to_lowercase()));
            }
        }
        SPECS.iter().for_each(|spec| check(&spec.predicate));
    }
}
