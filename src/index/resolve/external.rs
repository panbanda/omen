//! Evidence that a call names a definition outside the repository.
//!
//! The rules run only for a call that has no candidate. A call with a
//! candidate keeps the status that the other rules gave. A rule gives the
//! status `External` only when the source shows one of these facts:
//!
//! - An import that the repository does not contain binds the first name of
//!   the qualifier or of the receiver.
//! - The call has no qualifier and no receiver, and its name is in the list of
//!   the builtins of the language. No symbol of the repository, no local name,
//!   and no import has the name.
//! - The receiver is a local name with a declared type. The declared type is
//!   a builtin type or an imported external type. No repository type has the
//!   name, and no repository member can extend the type.
//!
//! The lists are small and static. Change `LIST_VERSION` when a list changes.

use super::{
    is_callable, segments, split_type, Id, ImportBinding, ImportKind, ImportTarget, Local, Outcome,
    Receiver, ReferenceKind, Resolver, Status, SymbolKind,
};
use crate::core::Language;
use crate::index::calls::{one_result, required_type};
use crate::index::imports::TestRunners;

/// The resolution of the value of a Go local name stops at this depth.
const MAXIMUM_DEPTH: usize = 4;

/// The version of the lists in this module.
pub const LIST_VERSION: u32 = 3;

/// A list of names, or the reason why the rule does not apply to the language.
#[derive(Debug, Clone, Copy)]
pub enum Names {
    List(&'static [&'static str]),
    NotApplicable(&'static str),
}

impl Names {
    fn contains(self, name: &str) -> bool {
        match self {
            Self::List(names) => names.contains(&name),
            Self::NotApplicable(_) => false,
        }
    }
}

/// The names that a module provides. A file that imports the module from
/// outside the repository can use these names without a repository definition.
#[derive(Debug, Clone, Copy)]
pub enum Modules {
    Table(&'static [(&'static str, &'static [&'static str])]),
    NotApplicable(&'static str),
}

impl Modules {
    /// The names that `module` provides.
    fn provided_by(self, module: &str) -> impl Iterator<Item = &'static str> + '_ {
        let table = match self {
            Self::Table(table) => table,
            Self::NotApplicable(_) => &[][..],
        };
        table
            .iter()
            .filter(move |(path, _)| *path == module)
            .flat_map(|(_, names)| names.iter().copied())
    }
}

/// The rules of one language.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// The functions that a call can name without a qualifier.
    pub bare: Names,
    /// The names of classes, namespaces, and objects that a qualifier or a
    /// receiver can start with, and that the language provides.
    pub receivers: Names,
    /// The builtin types that a declared type can name.
    pub types: Names,
    /// The names that an imported module provides.
    pub modules: Modules,
}

const PYTHON_BUILTINS: &[&str] = &[
    "AssertionError",
    "AttributeError",
    "Exception",
    "ImportError",
    "IndexError",
    "KeyError",
    "NotImplementedError",
    "OSError",
    "RuntimeError",
    "StopIteration",
    "TypeError",
    "ValueError",
    "abs",
    "all",
    "any",
    "bool",
    "bytes",
    "callable",
    "chr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "hasattr",
    "hash",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "list",
    "map",
    "max",
    "min",
    "next",
    "open",
    "ord",
    "pow",
    "print",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "sorted",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
];

const PYTHON_TYPES: &[&str] = &[
    "bytes", "dict", "float", "int", "list", "set", "str", "tuple",
];

/// The predeclared functions of Go, and the predeclared types, whose call is a conversion.
const GO_BUILTINS: &[&str] = &[
    "any",
    "append",
    "bool",
    "byte",
    "cap",
    "clear",
    "close",
    "comparable",
    "complex",
    "complex128",
    "complex64",
    "copy",
    "delete",
    "error",
    "float32",
    "float64",
    "imag",
    "int",
    "int16",
    "int32",
    "int64",
    "int8",
    "len",
    "make",
    "max",
    "min",
    "new",
    "panic",
    "print",
    "println",
    "real",
    "recover",
    "rune",
    "string",
    "uint",
    "uint16",
    "uint32",
    "uint64",
    "uint8",
    "uintptr",
];

const RUST_BUILTINS: &[&str] = &["Err", "Ok", "Some", "drop"];

const RUST_RECEIVERS: &[&str] = &["Box", "Option", "Result", "String", "Vec"];

const RUST_TYPES: &[&str] = &[
    "Option", "Result", "String", "Vec", "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64",
    "i8", "isize", "str", "u128", "u16", "u32", "u64", "u8", "usize",
];

/// The names that a Rust path can start with, and that no `use` binds.
const RUST_ROOTS: &[&str] = &["alloc", "core", "proc_macro", "std", "test"];

const ECMA_BUILTINS: &[&str] = &[
    "Array",
    "BigInt",
    "Boolean",
    "Error",
    "Number",
    "Object",
    "String",
    "Symbol",
    "TypeError",
    "atob",
    "btoa",
    "clearInterval",
    "clearTimeout",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "fetch",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "queueMicrotask",
    "require",
    "setInterval",
    "setTimeout",
    "structuredClone",
];

/// The compiler of Vue replaces these calls in the script of a component.
/// No module gives them, and no import binds them.
const VUE_COMPILER_MACROS: &[&str] = &[
    "defineEmits",
    "defineExpose",
    "defineModel",
    "defineOptions",
    "defineProps",
    "defineSlots",
    "withDefaults",
];

const ECMA_RECEIVERS: &[&str] = &[
    "Array",
    "Buffer",
    "Date",
    "Intl",
    "JSON",
    "Map",
    "Math",
    "Number",
    "Object",
    "Promise",
    "Reflect",
    "Set",
    "String",
    "Symbol",
    "console",
    "document",
    "globalThis",
    "process",
    "window",
];

const ECMA_TYPES: &[&str] = &[
    "Array",
    "Date",
    "Map",
    "Promise",
    "ReadonlyArray",
    "RegExp",
    "Set",
    "WeakMap",
    "WeakSet",
    "bigint",
    "boolean",
    "number",
    "string",
];

/// The global classes, namespaces, and objects of ECMAScript, of Node.js, and
/// of the browser. A module of the repository that declares one of these
/// names does not make that declaration global. Thus a file uses the
/// repository declaration only when it imports or declares the name.
pub(super) const ECMA_GLOBAL_NAMES: &[&str] = &[
    "AbortController",
    "AbortSignal",
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "Atomics",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Blob",
    "Boolean",
    "Buffer",
    "DOMException",
    "DataView",
    "Date",
    "Document",
    "Element",
    "Error",
    "EvalError",
    "Event",
    "EventTarget",
    "File",
    "FinalizationRegistry",
    "Float32Array",
    "Float64Array",
    "FormData",
    "Function",
    "HTMLElement",
    "Headers",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "JSON",
    "Map",
    "Math",
    "MessageChannel",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReadableStream",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Request",
    "Response",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TextDecoder",
    "TextEncoder",
    "TypeError",
    "URIError",
    "URL",
    "URLSearchParams",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "WebAssembly",
    "WebSocket",
    "Worker",
    "WritableStream",
    "console",
    "document",
    "globalThis",
    "navigator",
    "process",
    "window",
];

/// Tells if `name` is a global of ECMAScript, Node.js, or the browser.
pub(super) fn is_ecma_global(name: &str) -> bool {
    ECMA_GLOBAL_NAMES.binary_search(&name).is_ok()
}

/// The standard static members of the ECMAScript, Node.js, and browser
/// globals, by the global. A declaration in the repository can add a member
/// to a global, as `reflect-metadata` adds `Reflect.getMetadata`, but it
/// cannot change a standard member. The globals and each list are sorted.
const ECMA_GLOBAL_MEMBERS: &[(&str, &[&str])] = &[
    ("Array", &["from", "fromAsync", "isArray", "of"]),
    ("ArrayBuffer", &["isView"]),
    ("BigInt", &["asIntN", "asUintN"]),
    (
        "Buffer",
        &[
            "alloc",
            "allocUnsafe",
            "allocUnsafeSlow",
            "byteLength",
            "compare",
            "concat",
            "from",
            "isBuffer",
            "isEncoding",
        ],
    ),
    ("Date", &["UTC", "now", "parse"]),
    ("Error", &["captureStackTrace"]),
    (
        "Intl",
        &[
            "Collator",
            "DateTimeFormat",
            "DisplayNames",
            "ListFormat",
            "Locale",
            "NumberFormat",
            "PluralRules",
            "RelativeTimeFormat",
            "Segmenter",
            "getCanonicalLocales",
            "supportedValuesOf",
        ],
    ),
    ("JSON", &["parse", "stringify"]),
    ("Map", &["groupBy"]),
    (
        "Math",
        &[
            "abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt", "ceil",
            "clz32", "cos", "cosh", "exp", "expm1", "floor", "fround", "hypot", "imul", "log",
            "log10", "log1p", "log2", "max", "min", "pow", "random", "round", "sign", "sin",
            "sinh", "sqrt", "tan", "tanh", "trunc",
        ],
    ),
    (
        "Number",
        &[
            "isFinite",
            "isInteger",
            "isNaN",
            "isSafeInteger",
            "parseFloat",
            "parseInt",
        ],
    ),
    (
        "Object",
        &[
            "assign",
            "create",
            "defineProperties",
            "defineProperty",
            "entries",
            "freeze",
            "fromEntries",
            "getOwnPropertyDescriptor",
            "getOwnPropertyDescriptors",
            "getOwnPropertyNames",
            "getOwnPropertySymbols",
            "getPrototypeOf",
            "groupBy",
            "hasOwn",
            "is",
            "isExtensible",
            "isFrozen",
            "isSealed",
            "keys",
            "preventExtensions",
            "seal",
            "setPrototypeOf",
            "values",
        ],
    ),
    (
        "Promise",
        &[
            "all",
            "allSettled",
            "any",
            "race",
            "reject",
            "resolve",
            "try",
            "withResolvers",
        ],
    ),
    ("Proxy", &["revocable"]),
    (
        "Reflect",
        &[
            "apply",
            "construct",
            "defineProperty",
            "deleteProperty",
            "get",
            "getOwnPropertyDescriptor",
            "getPrototypeOf",
            "has",
            "isExtensible",
            "ownKeys",
            "preventExtensions",
            "set",
            "setPrototypeOf",
        ],
    ),
    ("String", &["fromCharCode", "fromCodePoint", "raw"]),
    ("Symbol", &["for", "keyFor"]),
    (
        "URL",
        &["canParse", "createObjectURL", "parse", "revokeObjectURL"],
    ),
    (
        "console",
        &[
            "assert",
            "clear",
            "count",
            "countReset",
            "debug",
            "dir",
            "dirxml",
            "error",
            "group",
            "groupCollapsed",
            "groupEnd",
            "info",
            "log",
            "table",
            "time",
            "timeEnd",
            "timeLog",
            "trace",
            "warn",
        ],
    ),
    (
        "document",
        &[
            "addEventListener",
            "createElement",
            "createElementNS",
            "createEvent",
            "createRange",
            "createTextNode",
            "dispatchEvent",
            "elementFromPoint",
            "execCommand",
            "getElementById",
            "getElementsByClassName",
            "getElementsByName",
            "getElementsByTagName",
            "getSelection",
            "hasFocus",
            "querySelector",
            "querySelectorAll",
            "removeEventListener",
        ],
    ),
    (
        "process",
        &[
            "abort",
            "chdir",
            "cpuUsage",
            "cwd",
            "emit",
            "emitWarning",
            "exit",
            "getActiveResourcesInfo",
            "getegid",
            "geteuid",
            "getgid",
            "getgroups",
            "getuid",
            "hrtime",
            "kill",
            "listenerCount",
            "memoryUsage",
            "nextTick",
            "off",
            "on",
            "once",
            "removeAllListeners",
            "removeListener",
            "resourceUsage",
            "setUncaughtExceptionCaptureCallback",
            "umask",
            "uptime",
        ],
    ),
    (
        "window",
        &[
            "addEventListener",
            "alert",
            "blur",
            "cancelAnimationFrame",
            "clearInterval",
            "clearTimeout",
            "close",
            "confirm",
            "dispatchEvent",
            "fetch",
            "focus",
            "getComputedStyle",
            "getSelection",
            "matchMedia",
            "open",
            "postMessage",
            "print",
            "prompt",
            "removeEventListener",
            "requestAnimationFrame",
            "scroll",
            "scrollBy",
            "scrollTo",
            "setInterval",
            "setTimeout",
        ],
    ),
];

/// Tells if `member` is a standard static member of the global `global`.
fn is_standard_member(global: &str, member: &str) -> bool {
    ECMA_GLOBAL_MEMBERS
        .binary_search_by(|(name, _)| (*name).cmp(global))
        .is_ok_and(|position| {
            ECMA_GLOBAL_MEMBERS[position]
                .1
                .binary_search(&member)
                .is_ok()
        })
}

/// The globals that each test runner gives to a test file, sorted. A
/// package that depends on the runner can use them without an import.
const JEST_GLOBALS: &[&str] = &[
    "afterAll",
    "afterEach",
    "beforeAll",
    "beforeEach",
    "describe",
    "expect",
    "fdescribe",
    "fit",
    "it",
    "jest",
    "test",
    "xdescribe",
    "xit",
    "xtest",
];
const VITEST_GLOBALS: &[&str] = &[
    "afterAll",
    "afterEach",
    "assert",
    "assertType",
    "beforeAll",
    "beforeEach",
    "bench",
    "chai",
    "describe",
    "expect",
    "expectTypeOf",
    "it",
    "onTestFailed",
    "onTestFinished",
    "suite",
    "test",
    "vi",
    "vitest",
];
const MOCHA_GLOBALS: &[&str] = &[
    "after",
    "afterEach",
    "before",
    "beforeEach",
    "context",
    "describe",
    "it",
    "run",
    "setup",
    "specify",
    "suite",
    "suiteSetup",
    "suiteTeardown",
    "teardown",
    "test",
    "xcontext",
    "xdescribe",
    "xit",
    "xspecify",
];

/// The globals of each test runner.
const TEST_RUNNER_GLOBALS: &[(TestRunners, &[&str])] = &[
    (TestRunners::JEST, JEST_GLOBALS),
    (TestRunners::VITEST, VITEST_GLOBALS),
    (TestRunners::MOCHA, MOCHA_GLOBALS),
];

const JAVA_RECEIVERS: &[&str] = &[
    "Boolean",
    "Character",
    "Double",
    "Float",
    "Integer",
    "Long",
    "Math",
    "Runtime",
    "String",
    "StringBuilder",
    "System",
    "Thread",
];

const JAVA_TYPES: &[&str] = &[
    "Boolean",
    "Character",
    "Double",
    "Float",
    "Integer",
    "Long",
    "String",
    "StringBuilder",
];

const CSHARP_TYPES: &[&str] = &["string"];

const CSHARP_MODULES: &[(&str, &[&str])] = &[
    (
        "System",
        &[
            "Activator",
            "Array",
            "BitConverter",
            "Buffer",
            "Console",
            "Convert",
            "DateTime",
            "Enum",
            "Environment",
            "GC",
            "Guid",
            "Math",
            "String",
            "TimeSpan",
        ],
    ),
    (
        "System.Collections.Generic",
        &[
            "Dictionary",
            "HashSet",
            "LinkedList",
            "List",
            "Queue",
            "SortedDictionary",
            "SortedSet",
            "Stack",
        ],
    ),
    (
        "System.IO",
        &[
            "Directory",
            "File",
            "MemoryStream",
            "Path",
            "Stream",
            "StreamReader",
            "StreamWriter",
        ],
    ),
    ("System.Linq", &["Enumerable"]),
    ("System.Text", &["Encoding", "StringBuilder"]),
];

const PHP_BUILTINS: &[&str] = &[
    "array_filter",
    "array_key_exists",
    "array_keys",
    "array_map",
    "array_merge",
    "array_pop",
    "array_push",
    "array_reverse",
    "array_search",
    "array_shift",
    "array_slice",
    "array_unique",
    "array_values",
    "count",
    "explode",
    "file_get_contents",
    "file_put_contents",
    "function_exists",
    "implode",
    "in_array",
    "intval",
    "is_array",
    "is_callable",
    "is_null",
    "is_numeric",
    "is_string",
    "json_decode",
    "json_encode",
    "ltrim",
    "max",
    "min",
    "preg_match",
    "preg_replace",
    "printf",
    "rtrim",
    "sprintf",
    "str_contains",
    "str_replace",
    "str_starts_with",
    "strlen",
    "strpos",
    "strtolower",
    "strtoupper",
    "substr",
    "trim",
    "var_dump",
];

const PHP_CLASSES: &[&str] = &[
    "ArrayObject",
    "Closure",
    "DateInterval",
    "DateTime",
    "DateTimeImmutable",
    "Exception",
    "InvalidArgumentException",
    "RuntimeException",
    "SplQueue",
    "SplStack",
    "stdClass",
];

const RUBY_BUILTINS: &[&str] = &[
    "Array",
    "Float",
    "Integer",
    "String",
    "abort",
    "at_exit",
    "block_given?",
    "catch",
    "exit",
    "format",
    "gets",
    "lambda",
    "loop",
    "p",
    "print",
    "printf",
    "proc",
    "putc",
    "puts",
    "raise",
    "rand",
    "require",
    "require_relative",
    "sleep",
    "sprintf",
    "throw",
];

const RUBY_RECEIVERS: &[&str] = &[
    "Array",
    "Comparable",
    "Dir",
    "File",
    "Float",
    "Hash",
    "IO",
    "Integer",
    "Kernel",
    "Math",
    "Process",
    "Random",
    "Regexp",
    "String",
    "Struct",
    "Symbol",
    "Thread",
    "Time",
];

/// The classes and modules of the Ruby core, sorted. Each Ruby program has
/// them without `require`. A repository can open one of them again to add
/// methods, but the class stays the core class. A class of the standard
/// library needs its `require`, see `RUBY_MODULES`.
const RUBY_CORE_CLASSES: &[&str] = &[
    "ArgumentError",
    "Array",
    "BasicObject",
    "Binding",
    "Class",
    "Comparable",
    "Complex",
    "ConditionVariable",
    "Data",
    "Dir",
    "Encoding",
    "Enumerable",
    "Enumerator",
    "Errno",
    "Exception",
    "FalseClass",
    "Fiber",
    "File",
    "Float",
    "FrozenError",
    "Hash",
    "IO",
    "IOError",
    "IndexError",
    "Integer",
    "Kernel",
    "KeyError",
    "Marshal",
    "MatchData",
    "Math",
    "Method",
    "Module",
    "Mutex",
    "NameError",
    "NilClass",
    "NoMethodError",
    "NotImplementedError",
    "Numeric",
    "Object",
    "ObjectSpace",
    "Proc",
    "Process",
    "Queue",
    "Random",
    "Range",
    "RangeError",
    "Rational",
    "Regexp",
    "RuntimeError",
    "ScriptError",
    "Signal",
    "SizedQueue",
    "StandardError",
    "StopIteration",
    "String",
    "Struct",
    "Symbol",
    "SystemExit",
    "Thread",
    "Time",
    "TrueClass",
    "TypeError",
    "UnboundMethod",
    "ZeroDivisionError",
];

/// Tells if `name` is a class or a module of the Ruby core.
pub(super) fn is_ruby_core_class(name: &str) -> bool {
    RUBY_CORE_CLASSES.binary_search(&name).is_ok()
}

const RUBY_MODULES: &[(&str, &[&str])] = &[
    ("fileutils", &["FileUtils"]),
    ("json", &["JSON"]),
    ("logger", &["Logger"]),
    ("pathname", &["Pathname"]),
    ("securerandom", &["SecureRandom"]),
    ("set", &["Set"]),
    ("stringio", &["StringIO"]),
    ("tempfile", &["Tempfile"]),
    ("yaml", &["YAML"]),
];

const C_STDIO: &[&str] = &[
    "fclose", "fflush", "fgets", "fopen", "fprintf", "fputs", "fread", "fscanf", "fwrite",
    "getchar", "perror", "printf", "putchar", "puts", "remove", "rename", "scanf", "snprintf",
    "sprintf", "sscanf",
];
const C_STDLIB: &[&str] = &[
    "abort", "abs", "atexit", "atof", "atoi", "atol", "bsearch", "calloc", "exit", "free",
    "getenv", "malloc", "qsort", "rand", "realloc", "srand", "strtod", "strtol", "strtoul",
    "system",
];
const C_STRING: &[&str] = &[
    "memchr", "memcmp", "memcpy", "memmove", "memset", "strcat", "strchr", "strcmp", "strcpy",
    "strdup", "strerror", "strlen", "strncat", "strncmp", "strncpy", "strrchr", "strstr", "strtok",
];
const C_CTYPE: &[&str] = &[
    "isalnum", "isalpha", "isdigit", "islower", "isspace", "isupper", "tolower", "toupper",
];
const C_MATH: &[&str] = &[
    "ceil", "cos", "exp", "fabs", "floor", "fmod", "log", "pow", "sin", "sqrt", "tan",
];

const C_MODULES: &[(&str, &[&str])] = &[
    ("cctype", C_CTYPE),
    ("cmath", C_MATH),
    ("cstdio", C_STDIO),
    ("cstdlib", C_STDLIB),
    ("cstring", C_STRING),
    ("ctype.h", C_CTYPE),
    ("math.h", C_MATH),
    ("stdio.h", C_STDIO),
    ("stdlib.h", C_STDLIB),
    ("string.h", C_STRING),
];

const BASH_BUILTINS: &[&str] = &[
    "alias", "builtin", "cd", "command", "declare", "echo", "eval", "exec", "exit", "export",
    "getopts", "kill", "local", "popd", "printf", "pushd", "pwd", "read", "readonly", "return",
    "set", "shift", "test", "trap", "type", "typeset", "unalias", "unset", "wait",
];

/// The rules of a language. Each language has an entry.
pub fn rules(language: Language) -> Rules {
    const NO_DECLARED_TYPES: &str = "The language has no declared types.";
    match language {
        Language::Python => Rules {
            bare: Names::List(PYTHON_BUILTINS),
            receivers: Names::List(PYTHON_TYPES),
            types: Names::List(PYTHON_TYPES),
            modules: Modules::NotApplicable("An import binds the name of each module that a file uses."),
        },
        Language::Go => Rules {
            bare: Names::List(GO_BUILTINS),
            receivers: Names::NotApplicable("An import binds the name of each package that a file uses."),
            types: Names::NotApplicable("The predeclared types of Go have no methods."),
            modules: Modules::NotApplicable("An import binds the name of each package that a file uses."),
        },
        Language::Rust => Rules {
            bare: Names::List(RUST_BUILTINS),
            receivers: Names::List(RUST_RECEIVERS),
            types: Names::List(RUST_TYPES),
            modules: Modules::NotApplicable("A `use` binds the name of each item that a file uses."),
        },
        Language::TypeScript | Language::Tsx => Rules {
            bare: Names::List(ECMA_BUILTINS),
            receivers: Names::List(ECMA_RECEIVERS),
            types: Names::List(ECMA_TYPES),
            modules: Modules::NotApplicable("An import binds the name of each module that a file uses."),
        },
        Language::JavaScript | Language::Jsx => Rules {
            bare: Names::List(ECMA_BUILTINS),
            receivers: Names::List(ECMA_RECEIVERS),
            types: Names::NotApplicable(NO_DECLARED_TYPES),
            modules: Modules::NotApplicable("An import binds the name of each module that a file uses."),
        },
        Language::Java => Rules {
            bare: Names::NotApplicable(
                "A Java call without a receiver names a method of the enclosing class or of a parent class.",
            ),
            receivers: Names::List(JAVA_RECEIVERS),
            types: Names::List(JAVA_TYPES),
            modules: Modules::NotApplicable("An import binds the simple name of each class that a file uses."),
        },
        Language::CSharp => Rules {
            bare: Names::NotApplicable(
                "A C# call without a receiver names a method of the enclosing class or of a parent class.",
            ),
            receivers: Names::NotApplicable("A `using` gives the names. See the module table."),
            types: Names::List(CSHARP_TYPES),
            modules: Modules::Table(CSHARP_MODULES),
        },
        Language::Php => Rules {
            bare: Names::List(PHP_BUILTINS),
            receivers: Names::List(PHP_CLASSES),
            types: Names::List(PHP_CLASSES),
            modules: Modules::NotApplicable("A `use` binds the name of each class that a file uses."),
        },
        Language::Ruby => Rules {
            bare: Names::List(RUBY_BUILTINS),
            receivers: Names::List(RUBY_RECEIVERS),
            types: Names::NotApplicable(NO_DECLARED_TYPES),
            modules: Modules::Table(RUBY_MODULES),
        },
        Language::C => Rules {
            bare: Names::NotApplicable("A header gives the names. See the module table."),
            receivers: Names::NotApplicable("C has no class and no namespace."),
            types: Names::NotApplicable("C has no method."),
            modules: Modules::Table(C_MODULES),
        },
        Language::Cpp => Rules {
            bare: Names::NotApplicable("A header gives the names. See the module table."),
            receivers: Names::List(&["std"]),
            types: Names::NotApplicable("The qualifier `std` gives a builtin type."),
            modules: Modules::Table(C_MODULES),
        },
        Language::Bash => Rules {
            bare: Names::List(BASH_BUILTINS),
            receivers: Names::NotApplicable("Bash has no receiver."),
            types: Names::NotApplicable(NO_DECLARED_TYPES),
            modules: Modules::NotApplicable("A `source` loads files of the repository, not names."),
        },
    }
}

/// The qualifier and the name of a declared type, or `None` when the text
/// does not name one type. A Go slice, a Go map, and a function type name none.
fn declared_head(text: &str) -> Option<(Option<String>, String)> {
    let text = required_type(text).trim();
    if text.starts_with('[') || text.starts_with("map[") || text.starts_with("func") {
        return None;
    }
    if let Some(element) = text.strip_suffix("[]") {
        return (!element.trim().is_empty()).then(|| (None, "Array".to_string()));
    }
    let (qualifier, name) = split_type(text);
    (!name.is_empty()).then_some((qualifier, name))
}

/// The place of a call: its file, the definitions that contain it, and its
/// start byte, with the depth of the resolution that asks for it.
#[derive(Clone, Copy)]
struct Scope<'s> {
    file: usize,
    chain: &'s [Id],
    start: usize,
    depth: usize,
}

impl<'a> Resolver<'a> {
    /// The outcome `External` when the source shows that the call names a
    /// definition outside the repository. The caller gives only a call that has no candidate.
    pub(super) fn external(&self, file: usize, position: usize, depth: usize) -> Option<Outcome> {
        let facts = &self.files[file];
        let reference = &facts.references[position];
        let rules = rules(facts.language);
        let chain = self.chain(file, facts.reference_facts[position].owner);
        let scope = Scope {
            file,
            chain: &chain,
            start: facts.reference_facts[position].start_byte,
            depth,
        };
        let name = reference.name.as_str();
        let evidence = if reference.kind == ReferenceKind::Construct {
            reference.qualifier.is_none() && self.is_external_name(scope, name, rules)
        } else {
            match (&reference.receiver, &reference.qualifier) {
                (Some(Receiver::Name(receiver)), _) => {
                    self.external_receiver(scope, receiver, name, rules)
                }
                (None, Some(qualifier)) => {
                    let standard = self.is_ecma_global_in(file, qualifier)
                        && is_standard_member(qualifier, name);
                    self.external_qualifier(scope, qualifier, rules)
                        && (standard
                            || (!self.can_extend_external_type(name, file)
                                && !self.unbound_qualifier_can_name(scope, qualifier, name)))
                }
                (None, None) => !name.starts_with('$') && self.external_bare(scope, name, rules),
                (Some(Receiver::Expression), _) => self.test_runner_value(file, position, depth),
                _ => false,
            }
        };
        evidence.then(|| Outcome::none(Status::External))
    }

    fn binding_of(&self, file: usize, name: &str) -> Option<&'a ImportBinding> {
        self.imports_named(file, name).next()
    }

    fn is_external_binding(&self, binding: &ImportBinding) -> bool {
        binding.target_file == ImportTarget::External && self.target_files(binding).is_empty()
    }

    fn local_in_scope(&self, scope: Scope<'_>, name: &str) -> Option<&'a Local> {
        self.local(scope.chain, scope.file, name, Some(scope.start))
    }

    /// A wildcard import of the file names a module of the repository. The
    /// module can bind any name, also with an assignment that is not a symbol.
    fn has_repository_wildcard(&self, file: usize) -> bool {
        self.repository_wildcards.get_or_init(|| {
            self.files
                .iter()
                .map(|facts| {
                    facts.imports.iter().any(|binding| {
                        binding.kind == ImportKind::Wildcard && !self.is_external_binding(binding)
                    })
                })
                .collect()
        })[file]
    }

    /// A file of the repository binds the name again with a macro, an alias,
    /// an assignment to the global object, or a singleton class.
    pub(super) fn is_rebound(&self, file: usize, name: &str) -> bool {
        self.rebound.contains(&(self.family(file), name))
    }

    /// A function or a method of the repository, in the language family of
    /// the file, has the name.
    fn has_callable_named(&self, name: &str, file: usize) -> bool {
        self.named(name, file)
            .any(|id| is_callable(self.symbol(id)))
    }

    /// No symbol of the repository and no import of the file has the name,
    /// and no wildcard import of the repository can bind it.
    fn is_free_type_name(&self, file: usize, name: &str) -> bool {
        let php_name = self.files[file].language == Language::Php
            && self.php_names.contains(&name.to_ascii_lowercase());
        // A declaration of an ECMAScript global in a different module is not global.
        let declared = self.is_declared_for(file, name);
        let taken = self.is_rebound(file, name)
            || declared
            || php_name
            || self.binding_of(file, name).is_some()
            || self.has_repository_wildcard(file);
        !taken
    }

    /// No symbol of the repository, no local name that the call sees, and no
    /// import of the file has the name.
    fn is_free_name(&self, scope: Scope<'_>, name: &str) -> bool {
        self.is_free_type_name(scope.file, name) && self.local_in_scope(scope, name).is_none()
    }

    /// An import of the file, from outside the repository, provides the name.
    /// The rules are those of the language of the file.
    fn is_provided(&self, file: usize, name: &str) -> bool {
        let provided = self.provided_names.get_or_init(|| {
            self.files
                .iter()
                .map(|facts| {
                    let modules = rules(facts.language).modules;
                    facts
                        .imports
                        .iter()
                        .filter(|binding| self.is_external_binding(binding))
                        .flat_map(|binding| modules.provided_by(&binding.module_path))
                        .collect()
                })
                .collect()
        });
        provided[file].contains(name)
    }

    /// A class, a namespace, or an object that the language or an imported
    /// module provides, and that nothing in the repository replaces.
    fn is_external_name(&self, scope: Scope<'_>, name: &str, rules: Rules) -> bool {
        if let Some(binding) = self.binding_of(scope.file, name) {
            return self.is_external_binding(binding);
        }
        self.is_free_name(scope, name)
            && (rules.receivers.contains(name)
                || self.is_ecma_global_in(scope.file, name)
                || self.is_test_global(scope.file, name)
                || self.is_provided(scope.file, name))
    }

    /// An ECMAScript, Node.js, or browser global, in a file of ECMAScript.
    fn is_ecma_global_in(&self, file: usize, name: &str) -> bool {
        self.family(file) == super::Family::Ecma && is_ecma_global(name)
    }

    /// A global of a test runner that the package of `file` depends on.
    fn is_test_global(&self, file: usize, name: &str) -> bool {
        if self.family(file) != super::Family::Ecma {
            return false;
        }
        let runners = self.test_runners.get_or_init(|| {
            self.files
                .iter()
                .map(|facts| TestRunners::of_file(self.manifests, &facts.file))
                .collect()
        })[file];
        TEST_RUNNER_GLOBALS
            .iter()
            .any(|(runner, names)| runners.contains(*runner) && names.binary_search(&name).is_ok())
    }

    /// A symbol of the repository can be the global `name` for `file`. A
    /// declaration of an ECMAScript global or of a test global in a
    /// different module is not global, thus only the file counts.
    fn is_declared_for(&self, file: usize, name: &str) -> bool {
        if self.is_ecma_global_in(file, name) || self.is_test_global(file, name) {
            self.named_in(name, file).next().is_some()
        } else {
            self.by_name.contains_key(name)
        }
    }

    /// The call at `position` gives a value of a test runner: a call of a
    /// test global, or a call on such a value whose name no function or
    /// method of the repository has. A custom matcher of the repository can
    /// have the name of the call.
    fn test_runner_value(&self, file: usize, position: usize, depth: usize) -> bool {
        if depth > MAXIMUM_DEPTH {
            return false;
        }
        let facts = &self.files[file];
        let reference = &facts.references[position];
        let site = &facts.reference_facts[position];
        let chain = self.chain(file, site.owner);
        let scope = Scope {
            file,
            chain: &chain,
            start: site.start_byte,
            depth,
        };
        let rules = rules(facts.language);
        let head = match (&reference.receiver, &reference.qualifier) {
            (Some(Receiver::Name(receiver)), _) => receiver.as_str(),
            (None, Some(qualifier)) => segments(qualifier).first().copied().unwrap_or_default(),
            (None, None) => reference.name.as_str(),
            (Some(Receiver::Expression), _) => {
                return !self.has_callable_named(&reference.name, file)
                    && self
                        .receiver_call(file, position)
                        .is_some_and(|call| self.test_runner_value(file, call, depth + 1));
            }
            _ => return false,
        };
        self.is_test_global(file, head) && self.is_external_name(scope, head, rules)
    }

    /// The call that starts the receiver of the call at `position`: of the
    /// calls that start where the receiver starts, the longest one that ends
    /// in the receiver. `expect(a)` for `expect(a).not.toBe(b)`.
    fn receiver_call(&self, file: usize, position: usize) -> Option<usize> {
        let (start, end) = self.files[file].reference_facts[position].receiver?;
        self.references_within(file, start, end)
            .iter()
            .rev()
            .map(|(_, other)| *other)
            .find(|other| *other != position)
    }

    fn external_bare(&self, scope: Scope<'_>, name: &str, rules: Rules) -> bool {
        // Only a local name, an import, or a definition of the package hides
        // a Go builtin. A dot import gives only exported names, and no
        // builtin is exported.
        if self.files[scope.file].language == Language::Go {
            return rules.bare.contains(name)
                && self.local_in_scope(scope, name).is_none()
                && self.binding_of(scope.file, name).is_none()
                && !self.is_rebound(scope.file, name)
                && !self.has_package_top_level(name, scope.file);
        }
        if !self.is_free_name(scope, name) {
            return false;
        }
        let name = if self.files[scope.file].language == Language::Php {
            name.to_ascii_lowercase()
        } else {
            name.to_string()
        };
        let compiler_macro = self.files[scope.file].component.is_some()
            && VUE_COMPILER_MACROS.contains(&name.as_str());
        rules.bare.contains(&name)
            || compiler_macro
            || self.is_provided(scope.file, &name)
            || self.is_test_global(scope.file, &name)
    }

    fn external_qualifier(&self, scope: Scope<'_>, qualifier: &str, rules: Rules) -> bool {
        let Some(head) = segments(qualifier).first().copied() else {
            return false;
        };
        if self.local_in_scope(scope, head).is_some() || self.is_rebound(scope.file, head) {
            return false;
        }
        if let Some(binding) = self.binding_of(scope.file, head) {
            return self.is_external_binding(binding);
        }
        if self.is_declared_for(scope.file, head) {
            return false;
        }
        if self.files[scope.file].language == Language::Rust {
            return !self.is_rebound(scope.file, crate::index::extract::RUST_MACRO_IMPORT)
                && !qualifier.contains('.')
                && (RUST_ROOTS.contains(&head)
                    || RUST_RECEIVERS.contains(&head)
                    || self.is_declared_dependency(scope.file, head));
        }
        !self.has_repository_wildcard(scope.file)
            && (rules.receivers.contains(head)
                || self.is_ecma_global_in(scope.file, head)
                || self.is_test_global(scope.file, head)
                || self.is_provided(scope.file, head))
    }

    /// The head of the qualifier has no import, and a function or a method of
    /// the repository has the name of the call. A Rust path is exempt: a
    /// repository type has methods that a prelude type does not have.
    fn unbound_qualifier_can_name(&self, scope: Scope<'_>, qualifier: &str, name: &str) -> bool {
        let head = segments(qualifier).first().copied().unwrap_or_default();
        self.files[scope.file].language != Language::Rust
            && self.binding_of(scope.file, head).is_none()
            && self.has_callable_named(name, scope.file)
    }

    /// A Rust path that starts with a name that nothing in the module binds.
    /// The name is external only when the nearest `Cargo.toml` declares a
    /// dependency with the name. A macro can bind any other name.
    fn is_declared_dependency(&self, file: usize, head: &str) -> bool {
        !self.is_rebound(file, crate::index::extract::RUST_MACRO_IMPORT)
            && self
                .manifests
                .declares_rust_dependency(&self.files[file].file, head)
            && !["crate", "self", "super"].contains(&head)
    }

    fn external_receiver(
        &self,
        scope: Scope<'_>,
        receiver: &str,
        name: &str,
        rules: Rules,
    ) -> bool {
        if self.files[scope.file].language == Language::Go {
            if let Some(local) = self.local_in_scope(scope, receiver) {
                return self
                    .go_local_type(scope, local)
                    .is_some_and(|(text, text_file)| {
                        self.is_external_go_type(text_file, text, name)
                    });
            }
        }
        if let Some(local) = self.local_in_scope(scope, receiver) {
            return local
                .name
                .type_name
                .as_deref()
                .is_some_and(|text| self.is_external_type(scope.file, text, name, rules));
        }
        if let Some((text, text_file)) = self.own_field(scope.file, scope.chain, receiver) {
            return self.is_external_type(text_file, text, name, rules);
        }
        self.binding_of(scope.file, receiver)
            .is_some_and(|binding| self.is_external_binding(binding))
            || (self.is_external_name(scope, receiver, rules)
                && (!self.has_callable_named(name, scope.file)
                    || (self.is_ecma_global_in(scope.file, receiver)
                        && is_standard_member(receiver, name))))
    }

    /// The text of the type of a Go local name and the file of the text: the
    /// declared type, or the declared result of the one function of the
    /// repository whose call gives the value.
    fn go_local_type(&self, scope: Scope<'_>, local: &'a Local) -> Option<(&'a str, usize)> {
        let file = scope.file;
        if let Some(declared) = local.name.type_name.as_deref() {
            return Some((declared, file));
        }
        if local.name.element || scope.depth > MAXIMUM_DEPTH {
            return None;
        }
        let position = *self.references_at(file, local.name.value_start?).first()?;
        let outcome = self.resolve(file, position, scope.depth + 1);
        let [callee] = outcome.candidates.as_slice() else {
            return None;
        };
        if outcome.status != Status::UniqueCandidate {
            return None;
        }
        let declared = self.facts(*callee).return_type.as_deref()?;
        Some((one_result(declared, local.name.result_index)?, callee.0))
    }

    /// A Go type of a package outside the repository, or `error` with its
    /// method `Error`. Go has no extension methods, thus a method of the
    /// repository with the name of the call is not a method of the type.
    fn is_external_go_type(&self, file: usize, text: &str, call: &str) -> bool {
        let Some((qualifier, name)) = declared_head(text) else {
            return false;
        };
        match qualifier {
            Some(qualifier) => self
                .binding_of(file, &qualifier)
                .is_some_and(|binding| self.is_external_binding(binding)),
            None => name == "error" && call == "Error" && !self.has_package_top_level(&name, file),
        }
    }

    /// A member of the repository with the name `call` can belong to a type
    /// that the repository does not define: a method of an `impl` for an
    /// external type, a member of a trait or an interface, or a C# extension method.
    fn can_extend_external_type(&self, call: &str, file: usize) -> bool {
        let csharp = self.files[file].language == Language::CSharp;
        self.by_name.get(call).is_some_and(|ids| {
            ids.iter().any(|id| {
                let symbol = self.symbol(*id);
                is_callable(symbol)
                    && (csharp
                        || symbol.receiver_type.as_deref().is_some_and(|text| {
                            // The container path keeps the qualifier of the type: `io.Error`.
                            let path = &self.facts(*id).container_path;
                            if path.len() > 1 {
                                self.is_external_type_text(id.0, &path.join("::"))
                            } else {
                                self.is_external_type_text(id.0, text)
                            }
                        })
                        || self.facts(*id).parent.is_some_and(|parent| {
                            matches!(
                                self.symbol((id.0, parent)).kind,
                                SymbolKind::Trait | SymbolKind::Interface
                            )
                        }))
            })
        })
    }

    /// The text names a type that the repository does not define: an import
    /// from outside the repository binds its first name, or no repository type has the name.
    fn is_external_type_text(&self, file: usize, text: &str) -> bool {
        let Some((qualifier, name)) = declared_head(text) else {
            return false;
        };
        let bound = qualifier
            .as_deref()
            .and_then(|qualifier| segments(qualifier).first().copied())
            .or(Some(name.as_str()))
            .and_then(|head| self.binding_of(file, head));
        match bound {
            Some(binding) => self.is_external_binding(binding),
            None => self.types(&name, qualifier.as_deref(), file).is_empty(),
        }
    }

    /// The declared type is a builtin type or an imported external type, and
    /// the repository has no type with its name. A member of the repository
    /// with the name of the call stops the rule: a pattern can hide the
    /// local name, and the index does not model the hiding.
    fn is_external_type(&self, file: usize, text: &str, call: &str, rules: Rules) -> bool {
        let Some((qualifier, name)) = declared_head(text) else {
            return false;
        };
        if !self.types(&name, qualifier.as_deref(), file).is_empty()
            || !self.methods_by_name(call, file).is_empty()
            || self.can_extend_external_type(call, file)
        {
            return false;
        }
        let language = self.files[file].language;
        match qualifier {
            Some(qualifier) => {
                let head = segments(&qualifier).first().copied().unwrap_or_default();
                match self.binding_of(file, head) {
                    Some(binding) => self.is_external_binding(binding),
                    None => {
                        (language == Language::Cpp && head == "std")
                            || (language == Language::Rust && RUST_ROOTS.contains(&head))
                    }
                }
            }
            None => match self.binding_of(file, &name) {
                Some(binding) => self.is_external_binding(binding),
                None => {
                    self.is_free_type_name(file, &name)
                        && (rules.types.contains(&name) || self.is_provided(file, &name))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::resolve::testing::rows;

    const LANGUAGES: [Language; 14] = [
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

    fn lists_of(language: Language) -> Vec<(&'static str, &'static [&'static str])> {
        let rules = rules(language);
        let mut lists = Vec::new();
        for (kind, names) in [
            ("bare", rules.bare),
            ("receivers", rules.receivers),
            ("types", rules.types),
        ] {
            if let Names::List(names) = names {
                lists.push((kind, names));
            }
        }
        lists
    }

    #[test]
    fn test_every_language_has_rules_and_each_list_is_sorted_and_unique() {
        for language in LANGUAGES {
            let rules = rules(language);
            assert!(
                matches!(rules.bare, Names::List(_))
                    || matches!(rules.receivers, Names::List(_))
                    || matches!(rules.modules, Modules::Table(_)),
                "{language:?} has no rule"
            );
            for (kind, names) in lists_of(language) {
                let mut sorted = names.to_vec();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted, names, "{language:?} {kind}");
            }
            if let Modules::Table(table) = rules.modules {
                let paths: Vec<&str> = table.iter().map(|(path, _)| *path).collect();
                let mut sorted = paths.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted, paths, "{language:?} modules");
                for (_, names) in table {
                    let mut sorted = names.to_vec();
                    sorted.sort_unstable();
                    sorted.dedup();
                    assert_eq!(sorted, *names, "{language:?} module names");
                }
            }
        }
    }

    #[test]
    fn test_ecma_global_members_are_sorted_and_unique() {
        let globals: Vec<&str> = ECMA_GLOBAL_MEMBERS.iter().map(|(name, _)| *name).collect();
        assert!(globals.windows(2).all(|pair| pair[0] < pair[1]));
        for (global, members) in ECMA_GLOBAL_MEMBERS {
            assert!(is_ecma_global(global), "{global}");
            assert!(members.windows(2).all(|pair| pair[0] < pair[1]), "{global}");
        }
        assert!(is_standard_member("Object", "keys"));
        assert!(!is_standard_member("Reflect", "getMetadata"));
    }

    #[test]
    fn test_ruby_core_classes_are_sorted_and_unique() {
        let mut sorted = RUBY_CORE_CLASSES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, RUBY_CORE_CLASSES);
        assert!(is_ruby_core_class("Thread"));
        assert!(!is_ruby_core_class("Circle"));
    }

    #[test]
    fn test_ecma_global_names_are_sorted_and_unique() {
        let mut sorted = ECMA_GLOBAL_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, ECMA_GLOBAL_NAMES);
        assert!(is_ecma_global("Set"));
        assert!(!is_ecma_global("Circle"));
    }

    #[test]
    fn test_the_lists_are_small() {
        for language in LANGUAGES {
            for (kind, names) in lists_of(language) {
                assert!(names.len() <= 80, "{language:?} {kind} has {}", names.len());
            }
        }
    }

    #[test]
    fn test_list_version_is_one() {
        assert_eq!(LIST_VERSION, 3);
    }

    #[test]
    fn test_declared_head() {
        let head = |text: &str| declared_head(text);
        assert_eq!(head("Vec<i32>"), Some((None, "Vec".to_string())));
        assert_eq!(head("&mut String"), Some((None, "String".to_string())));
        assert_eq!(
            head("*strings.Builder"),
            Some((Some("strings".to_string()), "Builder".to_string()))
        );
        assert_eq!(
            head("const std::string&"),
            Some((Some("std".to_string()), "string".to_string()))
        );
        assert_eq!(head("number[]"), Some((None, "Array".to_string())));
        assert_eq!(head("[]string"), None);
        assert_eq!(head("map[string]int"), None);
        assert_eq!(head("func(int) int"), None);
    }

    fn statuses(files: &[(&str, &str)], file: &str, name: &str) -> Vec<String> {
        rows(files, file)
            .into_iter()
            .filter(|row| {
                let callee = row.split(' ').nth(1).unwrap_or_default();
                callee.rsplit(['>', '.', ':']).next() == Some(name)
            })
            .map(|row| row.split(' ').nth(2).unwrap_or_default().to_string())
            .collect()
    }

    fn external(files: &[(&str, &str)], file: &str, name: &str) -> bool {
        let found = statuses(files, file, name);
        assert!(!found.is_empty(), "no call {name}");
        found.iter().all(|status| status == "External")
    }

    #[test]
    fn test_python_builtin_and_shadowing() {
        let app = "def run(items, len):\n    print(1)\n    max(1)\n    len(2)\n";
        let files = [("app.py", app), ("util.py", "def max(a):\n    return a\n")];
        assert!(external(&files, "app.py", "print"));
        assert!(!external(&files, "app.py", "max"));
        assert!(!external(&files, "app.py", "len"));
    }

    #[test]
    fn test_python_builtin_hidden_by_a_definition_in_the_repository() {
        let files = [
            ("app.py", "def run():\n    print(1)\n"),
            ("util.py", "def print(a):\n    return a\n"),
        ];
        assert!(!external(&files, "app.py", "print"));
    }

    #[test]
    fn test_python_qualifier_bound_by_an_external_import() {
        let files = [("app.py", "import os\n\ndef run():\n    os.path.join('a')\n")];
        assert!(external(&files, "app.py", "join"));
        let local = [("app.py", "def run(os):\n    os.path.join('a')\n")];
        assert!(!external(&local, "app.py", "join"));
    }

    #[test]
    fn test_python_declared_builtin_type() {
        let files = [("app.py", "def run(items: list):\n    items.append(1)\n")];
        assert!(external(&files, "app.py", "append"));
        let shadow = [
            ("app.py", "def run(items: list):\n    items.append(1)\n"),
            (
                "list.py",
                "class list:\n    def append(self, v):\n        pass\n",
            ),
        ];
        assert!(!external(&shadow, "app.py", "append"));
    }

    #[test]
    fn test_go_builtins_and_package_function() {
        let main =
            "package main\n\nfunc run(items []int) {\n\t_ = len(items)\n\tcopy(items, items)\n}\n";
        let other = "package main\n\nfunc copy(a []int, b []int) int { return 0 }\n";
        let files = [("main.go", main), ("util.go", other)];
        assert!(external(&files, "main.go", "len"));
        assert!(!external(&files, "main.go", "copy"));
    }

    /// Only a local name or a definition of the package hides a Go builtin.
    /// A method, a definition in a different package, or a definition in a
    /// different language does not. A conversion to a predeclared type is a
    /// call of a definition outside the repository.
    #[test]
    fn test_go_builtin_is_hidden_only_in_its_package() {
        let main = "package main\n\nfunc run(items []int, value int) string {\n\titems = append(items, 1)\n\t_ = int64(value)\n\treturn string(rune(value))\n}\n";
        let files = [
            ("go.mod", "module example.com/m\n"),
            ("main.go", main),
            ("list/list.go", "package list\n\ntype List struct{}\n\nfunc (l *List) append() {}\n\nfunc string() {}\n"),
            ("web/list.js", "function append() {}\n"),
        ];
        assert!(external(&files, "main.go", "append"));
        assert!(external(&files, "main.go", "int64"));
        assert!(external(&files, "main.go", "string"));
        assert!(external(&files, "main.go", "rune"));
        let hidden = [
            ("go.mod", "module example.com/m\n"),
            ("main.go", main),
            ("util.go", "package main\n\ntype int64 int\n\nfunc append(items []int, value int) []int { return items }\n"),
        ];
        assert!(!external(&hidden, "main.go", "append"));
        assert!(!external(&hidden, "main.go", "int64"));
        assert!(external(&hidden, "main.go", "string"));
        let local = "package main\n\nfunc run(string func() int) int {\n\treturn string()\n}\n";
        assert!(!external(&[("main.go", local)], "main.go", "string"));
    }

    /// Go has no extension methods: a method of the repository cannot be a
    /// method of a type of a different package. `error` has the method `Error`.
    #[test]
    fn test_go_method_of_an_external_type_with_a_repository_method_of_the_name() {
        let main = "package main\n\nimport \"net/http\"\n\nfunc load() error { return nil }\n\nfunc run(w http.ResponseWriter, failure error) string {\n\tw.Write(nil)\n\terr := load()\n\t_ = err.Error()\n\treturn failure.Error()\n}\n";
        let files = [
            ("go.mod", "module example.com/m\n"),
            ("main.go", main),
            (
                "errs/errs.go",
                "package errs\n\ntype Missing struct{}\n\nfunc (m Missing) Error() string { return \"\" }\n\nfunc (m Missing) Write(data []byte) {}\n",
            ),
        ];
        assert!(external(&files, "main.go", "Write"));
        assert!(external(&files, "main.go", "Error"));
    }

    #[test]
    fn test_go_declared_type_of_an_external_package() {
        let main = "package main\n\nimport \"strings\"\n\nfunc run() {\n\tvar builder strings.Builder\n\tbuilder.WriteString(\"x\")\n}\n";
        let files = [("main.go", main), ("go.mod", "module example.com/m\n")];
        assert!(external(&files, "main.go", "WriteString"));
    }

    #[test]
    fn test_rust_prelude_and_paths() {
        let main = "fn run() {\n    let v: Vec<i32> = Vec::new();\n    v.len();\n    serde_json::from_str(\"1\");\n    std::fs::read(\"x\");\n    Some(1);\n}\n";
        let cargo = "[dependencies]\nserde_json = \"1\"\n";
        let files = [("main.rs", main), ("Cargo.toml", cargo)];
        assert!(external(&files, "main.rs", "new"));
        assert!(external(&files, "main.rs", "len"));
        assert!(external(&files, "main.rs", "from_str"));
        assert!(external(&files, "main.rs", "read"));
        assert!(external(&files, "main.rs", "Some"));
    }

    #[test]
    fn test_rust_path_of_a_module_of_the_repository_is_not_external() {
        let main = "mod util;\n\nfn run() {\n    util::missing();\n}\n";
        let files = [("main.rs", main), ("util.rs", "pub fn other() {}\n")];
        assert!(!external(&files, "main.rs", "missing"));
    }

    #[test]
    fn test_rust_trait_member_can_extend_a_builtin_type() {
        let main = "trait Shout { fn shout(&self); }\nimpl Shout for String { fn shout(&self) {} }\nfn run(text: String) {\n    text.shout();\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "shout"));
    }

    #[test]
    fn test_ecma_globals_and_shadowing() {
        for file in ["app.ts", "app.js", "app.tsx", "app.jsx"] {
            let app = "export function run(console) {\n  console.log(1);\n  Math.max(1);\n  parseInt('1');\n}\n";
            let files = [(file, app)];
            assert!(!external(&files, file, "log"), "{file}");
            assert!(external(&files, file, "max"), "{file}");
            assert!(external(&files, file, "parseInt"), "{file}");
            let own = "export function parseInt(a) { return a; }\nexport function run() { parseInt('1'); }\n";
            let files = [(file, own)];
            assert!(!external(&files, file, "parseInt"), "{file}");
        }
    }

    /// A standard member of an ECMAScript or Node.js global is outside the
    /// repository, also when a method of the repository has its name, and a
    /// global class is outside the repository in `new`. A member that the
    /// standard does not define can come from the repository.
    #[test]
    fn test_ecma_standard_members_of_globals() {
        let other =
            "export class Store {\n  keys() {}\n  log() {}\n  now() {}\n  getMetadata() {}\n}\nexport class Math {\n  static max() {}\n}\n";
        for (file, other_file) in [
            ("app.ts", "store.ts"),
            ("app.js", "store.js"),
            ("app.tsx", "store.tsx"),
            ("app.jsx", "store.jsx"),
        ] {
            let app = "export function run(value) {\n  Object.keys(value);\n  console.log(1);\n  Date.now();\n  Math.max(1);\n  Reflect.getMetadata('a', value);\n  throw new Error('x');\n}\nexport function link() {\n  return new URL('a');\n}\n";
            let files = [(file, app), (other_file, other)];
            assert!(external(&files, file, "keys"), "{file}");
            assert!(external(&files, file, "log"), "{file}");
            assert!(external(&files, file, "now"), "{file}");
            // A class of a different module with the name of a global is not global.
            assert!(external(&files, file, "max"), "{file}");
            assert!(external(&files, file, "Error"), "{file}");
            assert!(external(&files, file, "URL"), "{file}");
            // `reflect-metadata` adds `getMetadata` to `Reflect`.
            assert!(!external(&files, file, "getMetadata"), "{file}");
            // A local name, an import, or a declaration of the file hides the global.
            let hidden = "import { Error } from './errors';\nexport function run(console) {\n  console.log(1);\n  throw new Error('x');\n}\n";
            let errors = "export class Error {}\n";
            let files = [(file, hidden), (other_file, other), ("errors.ts", errors)];
            assert!(!external(&files, file, "log"), "{file}");
            assert!(!external(&files, file, "Error"), "{file}");
        }
    }

    /// The globals of a test runner are outside the repository in a package
    /// that depends on the runner, when nothing in the file binds the name.
    /// A call on the result of such a call is outside the repository too,
    /// unless a function or a method of the repository has its name.
    #[test]
    fn test_vue_component_gets_the_test_runner_globals_of_its_package() {
        let vitest = "{\"name\": \"app\", \"devDependencies\": {\"vitest\": \"1.0.0\"}}";
        let none = "{\"name\": \"app\", \"dependencies\": {\"vue\": \"3.0.0\"}}";
        let component = "<script setup lang=\"ts\">\ndescribe('a', () => {\n  expect(1).toBe(1);\n});\n</script>\n";
        let files = [("package.json", vitest), ("src/Probe.vue", component)];
        assert!(external(&files, "src/Probe.vue", "describe"));
        assert!(external(&files, "src/Probe.vue", "expect"));
        let files = [("package.json", none), ("src/Probe.vue", component)];
        assert!(!external(&files, "src/Probe.vue", "describe"));
    }

    #[test]
    fn test_ecma_test_runner_globals() {
        let vitest = "{\"name\": \"app\", \"devDependencies\": {\"vitest\": \"1.0.0\"}}";
        let mocha = "{\"name\": \"app\", \"devDependencies\": {\"mocha\": \"1.0.0\"}}";
        let none = "{\"name\": \"app\", \"dependencies\": {\"zod\": \"1.0.0\"}}";
        let matchers = "export class Matchers {\n  toBeShaped() {}\n}\n";
        for (file, other) in [
            ("src/app.test.ts", "src/matchers.ts"),
            ("src/app.test.js", "src/matchers.js"),
            ("src/app.test.tsx", "src/matchers.tsx"),
            ("src/app.test.jsx", "src/matchers.jsx"),
        ] {
            let app = "describe('a', () => {\n  it('b', () => {\n    const run = vi.fn();\n    vi.fn().mockResolvedValue(1);\n    expect(run).toHaveBeenCalled();\n    expect(1).not.toEqual(2);\n    expect(1).toBeShaped();\n  });\n});\n";
            let files = [("package.json", vitest), (file, app), (other, matchers)];
            for name in [
                "describe",
                "it",
                "fn",
                "mockResolvedValue",
                "expect",
                "toHaveBeenCalled",
                "toEqual",
            ] {
                assert!(external(&files, file, name), "{file} {name}");
            }
            // A custom matcher of the repository can be the callee.
            assert!(!external(&files, file, "toBeShaped"), "{file}");
            // Without the runner, the names can come from a setup file.
            let files = [("package.json", none), (file, app), (other, matchers)];
            assert!(!external(&files, file, "describe"), "{file}");
            assert!(!external(&files, file, "toHaveBeenCalled"), "{file}");
            // `vi` is a global of Vitest, not of Mocha. `context` is a global of Mocha.
            let files = [("package.json", mocha), (file, app), (other, matchers)];
            assert!(external(&files, file, "describe"), "{file}");
            assert!(!external(&files, file, "fn"), "{file}");
            // An import, a local name, or an assignment to the global object binds the name.
            let bound = "import { it } from './helpers';\nexport function run(expect) {\n  it('a');\n  expect(1).toBe(1);\n}\nglobalThis.describe = run;\ndescribe('a');\n";
            let files = [
                ("package.json", vitest),
                (file, bound),
                ("src/helpers.ts", "export function it(name: string) {}\n"),
            ];
            for name in ["it", "expect", "toBe", "describe"] {
                assert!(!external(&files, file, name), "{file} {name}");
            }
        }
    }

    #[test]
    fn test_ecma_declared_global_type_typescript() {
        for file in ["app.ts", "app.tsx"] {
            let app =
                "export function run(cache: Map<string, number>) {\n  cache.set('a', 1);\n}\n";
            assert!(external(&[(file, app)], file, "set"), "{file}");
        }
    }

    #[test]
    fn test_java_lang_receivers_and_imported_types() {
        let main = "import java.util.List;\nclass Main {\n    void run(List<String> names) {\n        Integer.parseInt(\"3\");\n        names.add(\"a\");\n        Math.max(1, 2);\n    }\n}\n";
        let math = "class Math {\n    static int max(int a, int b) { return a; }\n}\n";
        let files = [("Main.java", main), ("Math.java", math)];
        assert!(external(&files, "Main.java", "parseInt"));
        assert!(external(&files, "Main.java", "add"));
        assert!(!external(&files, "Main.java", "max"));
    }

    #[test]
    fn test_csharp_names_from_a_system_namespace() {
        let program = "using System;\nclass Program {\n    static void Run() {\n        Console.WriteLine(\"x\");\n        Math.Max(1, 2);\n    }\n}\n";
        let math = "class Math {\n    public static int Max(int a, int b) { return a; }\n}\n";
        let files = [("Program.cs", program), ("Math.cs", math)];
        assert!(external(&files, "Program.cs", "WriteLine"));
        assert!(!external(&files, "Program.cs", "Max"));
        let without = "class Program {\n    static void Run() {\n        Console.WriteLine(\"x\");\n    }\n}\n";
        assert!(!external(
            &[("Program.cs", without)],
            "Program.cs",
            "WriteLine"
        ));
    }

    #[test]
    fn test_php_internal_functions_and_shadowing() {
        let app = "<?php\nfunction run($x) {\n    strlen($x);\n    str_replace('a', 'b', $x);\n}\n";
        let util = "<?php\nfunction str_replace($a, $b, $c) { return $c; }\n";
        let files = [("app.php", app), ("util.php", util)];
        assert!(external(&files, "app.php", "strlen"));
        assert!(!external(&files, "app.php", "str_replace"));
    }

    #[test]
    fn test_ruby_kernel_and_core_classes() {
        let app = "def run\n  puts 'x'\n  File.join('a', 'b')\n  format('%d', 1)\nend\n";
        let util = "def format(a, b)\n  a\nend\n";
        let files = [("app.rb", app), ("util.rb", util)];
        assert!(external(&files, "app.rb", "puts"));
        assert!(external(&files, "app.rb", "join"));
        assert!(!external(&files, "app.rb", "format"));
    }

    #[test]
    fn test_ruby_constant_from_a_required_library() {
        let app = "require 'json'\n\ndef run\n  JSON.parse('{}')\nend\n";
        assert!(external(&[("app.rb", app)], "app.rb", "parse"));
        let without = "def run\n  JSON.parse('{}')\nend\n";
        assert!(!external(&[("app.rb", without)], "app.rb", "parse"));
    }

    #[test]
    fn test_c_function_needs_its_header() {
        let with = "#include <stdio.h>\nint run(void) {\n    printf(\"x\");\n    return 0;\n}\n";
        assert!(external(&[("main.c", with)], "main.c", "printf"));
        let without = "int run(void) {\n    printf(\"x\");\n    return 0;\n}\n";
        assert!(!external(&[("main.c", without)], "main.c", "printf"));
        let own = "#include <stdio.h>\nint printf(const char *f) { return 0; }\nint run(void) {\n    printf(\"x\");\n    return 0;\n}\n";
        assert!(!external(&[("main.c", own)], "main.c", "printf"));
    }

    #[test]
    fn test_cpp_std_namespace_and_declared_type() {
        let main = "#include <vector>\nvoid run(std::vector<int> xs) {\n    xs.push_back(1);\n    std::sort(xs.begin(), xs.end());\n}\n";
        let files = [("main.cpp", main)];
        assert!(external(&files, "main.cpp", "push_back"));
        assert!(external(&files, "main.cpp", "sort"));
    }

    #[test]
    fn test_bash_builtin_and_function_with_the_name() {
        let run = "run() {\n  echo a\n  cd /tmp\n}\n";
        let util = "cd() {\n  builtin cd \"$@\"\n}\n";
        let files = [("run.sh", run), ("util.sh", util)];
        assert!(external(&files, "run.sh", "echo"));
        assert!(!external(&files, "run.sh", "cd"));
    }

    #[test]
    fn test_rust_trait_path_can_name_an_implementation_of_the_repository() {
        let main = "struct Config;\nimpl Default for Config {\n    fn default() -> Self { Config }\n}\nfn run() {\n    let _config: Config = Default::default();\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "default"));
    }

    #[test]
    fn test_rust_path_to_an_external_type_with_a_method_from_a_repository_trait() {
        let main = "use std::io;\ntrait Message { fn message(e: io::Error); }\nimpl Message for io::Error {\n    fn message(e: io::Error) {}\n}\nfn run(e: io::Error) {\n    io::Error::message(e);\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "message"));
    }

    #[test]
    fn test_rust_conversion_into_an_external_type_can_be_a_repository_impl() {
        let main = "use std::io;\nstruct Failure;\nimpl From<Failure> for io::Error {\n    fn from(_value: Failure) -> Self { todo!() }\n}\nfn run(failure: Failure) {\n    let _ = io::Error::from(failure);\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "from"));
    }

    #[test]
    fn test_rust_conversion_into_a_qualified_external_type_when_a_repository_type_has_the_name() {
        let main = "use std::io;\nstruct Error;\nstruct Failure;\nimpl From<Failure> for io::Error {\n    fn from(_value: Failure) -> Self { todo!() }\n}\nfn run(failure: Failure) {\n    let _ = io::Error::from(failure);\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "from"));
    }

    #[test]
    fn test_rust_pattern_can_hide_a_local_name_with_a_declared_type() {
        let main = "struct Seq;\nimpl Seq {\n    fn choose(self) -> Seq { self }\n}\nfn run(prev: Option<Seq>) {\n    if let Some(prev) = prev {\n        prev.choose();\n    }\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "choose"));
    }

    #[test]
    fn test_rust_value_chain_is_not_a_crate_path() {
        let main = "fn run() {\n    items.iter().for_each(|m| m.status.is_ready());\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "is_ready"));
    }

    #[test]
    fn test_ecma_object_pattern_parameter_is_a_local_name() {
        for (file, app) in [
            ("a.js", "function run({Math}) { return Math.max(1); }\n"),
            ("a.jsx", "function run({Math}) { return Math.max(1); }\n"),
            (
                "a.ts",
                "function run({Math}: {Math: any}) { return Math.max(1); }\n",
            ),
            (
                "a.tsx",
                "function run({Math}: {Math: any}) { return Math.max(1); }\n",
            ),
            ("b.js", "function run([Math]) { return Math.max(1); }\n"),
            ("c.js", "const run = ({value: Math}) => Math.max(1);\n"),
        ] {
            assert!(!external(&[(file, app)], file, "max"), "{file}");
        }
    }

    #[test]
    fn test_java_inferred_lambda_parameter_is_a_local_name() {
        let main = "class Main {\n    void run() {\n        java.util.function.Function<Object, Integer> f = Math -> Math.max();\n    }\n}\n";
        assert!(!external(&[("Main.java", main)], "Main.java", "max"));
    }

    #[test]
    fn test_csharp_implicit_lambda_parameter_is_a_local_name() {
        let program = "using System;\nclass Program {\n    static void Run() {\n        Func<string, int> f = Math => Math.Max();\n    }\n}\n";
        assert!(!external(&[("Program.cs", program)], "Program.cs", "Max"));
    }

    #[test]
    fn test_a_wildcard_import_of_the_repository_can_bind_the_name() {
        let util =
            "class Repo:\n    def append(self, value):\n        return value\n\nlist = Repo()\n";
        let app = "from util import *\n\ndef main():\n    list.append(1)\n    len([1])\n";
        let files = [("util.py", util), ("app.py", app)];
        assert!(!external(&files, "app.py", "append"));
        assert!(!external(&files, "app.py", "len"));
        let external_wildcard = [(
            "app.py",
            "from os.path import *\n\ndef main():\n    len([1])\n",
        )];
        assert!(external(&external_wildcard, "app.py", "len"));
    }

    #[test]
    fn test_java_wildcard_of_a_repository_package_can_bind_the_name() {
        let main = "import shop.*;\nclass Main {\n    void run() {\n        Integer.parseInt(\"1\");\n    }\n}\n";
        let shop = "package shop;\npublic class Integer {\n    public static int parseInt(String text) { return 1; }\n}\n";
        let files = [("Main.java", main), ("shop/Integer.java", shop)];
        assert!(!external(&files, "Main.java", "parseInt"));
    }

    #[test]
    fn test_a_receiver_without_a_binding_abstains_when_a_repository_method_has_the_call_name() {
        // `clamp` is not a standard member of `Math`, thus a declaration of
        // the repository can add it.
        let app =
            "function run() { return Math.clamp(1); }\nclass Repo { clamp() { return 1; } }\n";
        assert!(!external(&[("a.ts", app)], "a.ts", "clamp"));
    }

    #[test]
    fn test_rust_path_head_is_external_only_for_a_declared_dependency() {
        let main = "fn run() {\n    serde_json::from_str(\"1\");\n    std::fs::read(\"x\");\n}\n";
        let without = [("main.rs", main)];
        assert!(external(&without, "main.rs", "read"));
        assert!(!external(&without, "main.rs", "from_str"));
        let cargo = "[package]\nname = \"demo\"\n\n[dependencies]\nserde_json = \"1\"\n";
        let with = [("main.rs", main), ("Cargo.toml", cargo)];
        assert!(external(&with, "main.rs", "from_str"));
    }

    #[test]
    fn test_rust_dependency_tables() {
        let cargo = "[package]\nname = \"demo\"\n\n[dependencies]\nserde-json = \"1\"\njson = { package = \"simd-json\", version = \"0.1\" }\nshared.workspace = true\n\n[dev-dependencies]\ntempfile = \"3\"\n\n[build-dependencies]\ncc = \"1\"\n\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n\n[dependencies.rand]\nversion = \"0.8\"\n\n[workspace.dependencies]\nanyhow = \"1\"\n";
        let main = "fn run() {\n    serde_json::a();\n    json::b();\n    simd_json::c();\n    shared::d();\n    tempfile::e();\n    cc::f();\n    libc::g();\n    rand::h();\n    anyhow::i();\n    other::j();\n}\n";
        let files = [("main.rs", main), ("Cargo.toml", cargo)];
        for name in ["a", "b", "d", "e", "f", "g", "h", "i"] {
            assert!(external(&files, "main.rs", name), "{name}");
        }
        for name in ["c", "j"] {
            assert!(!external(&files, "main.rs", name), "{name}");
        }
    }

    #[test]
    fn test_rust_dependency_belongs_to_the_nearest_manifest() {
        let main = "fn run() {\n    serde_json::a();\n}\n";
        let files = [
            ("one/src/main.rs", main),
            ("one/Cargo.toml", "[dependencies]\nserde_json = \"1\"\n"),
            ("two/src/main.rs", main),
            ("two/Cargo.toml", "[dependencies]\nrand = \"1\"\n"),
        ];
        assert!(external(&files, "one/src/main.rs", "a"));
        assert!(!external(&files, "two/src/main.rs", "a"));
    }

    #[test]
    fn test_rust_dotted_path_dependency_is_not_external() {
        let main = "fn run() {\n    local_alias::work();\n}\n";
        let manifest = "[dependencies]\nlocal_alias.path = \"../local\"\nlocal_alias.package = \"local-pkg\"\n";
        let files = [("src/main.rs", main), ("Cargo.toml", manifest)];
        assert!(!external(&files, "src/main.rs", "work"));
    }

    #[test]
    fn test_rust_macro_import_can_shadow_a_declared_dependency() {
        let main = "mod local {\n    pub fn from_str(_: &str) {}\n}\n\nmacro_rules! bind_local {\n    () => { use crate::local as serde_json; }\n}\n\nbind_local!();\n\nfn run() {\n    serde_json::from_str(\"1\");\n}\n";
        let manifest = "[dependencies]\nserde_json = \"1\"\n";
        let files = [("src/main.rs", main), ("Cargo.toml", manifest)];
        assert!(!external(&files, "src/main.rs", "from_str"));
        let plain = "macro_rules! twice {\n    ($x:expr) => { $x + $x };\n}\n\nfn run() {\n    serde_json::from_str(\"1\");\n}\n";
        let files = [("src/main.rs", plain), ("Cargo.toml", manifest)];
        assert!(external(&files, "src/main.rs", "from_str"));
    }

    #[test]
    fn test_rust_macro_arguments_and_std_heads_can_be_rebound() {
        let manifest = "[dependencies]\nserde_json = \"1\"\n";
        let invoked = "mod local {\n    pub fn from_str(_: &str) {}\n}\n\nmacro_rules! bind {\n    ($item:item) => { $item };\n}\n\nbind!(use crate::local as serde_json;);\n\nfn run() {\n    serde_json::from_str(\"1\");\n}\n";
        let files = [("src/main.rs", invoked), ("Cargo.toml", manifest)];
        assert!(!external(&files, "src/main.rs", "from_str"));
        let std_head = "mod local {\n    pub fn swap() {}\n}\n\nmacro_rules! bind {\n    () => { use crate::local as std; }\n}\n\nbind!();\n\nfn run() {\n    std::swap();\n}\n";
        assert!(!external(
            &[("src/main.rs", std_head)],
            "src/main.rs",
            "swap"
        ));
        let quoted =
            "fn run() {\n    println!(\"use this\");\n    std::mem::swap(&mut 1, &mut 2);\n}\n";
        assert!(external(&[("src/main.rs", quoted)], "src/main.rs", "swap"));
    }

    #[test]
    fn test_rust_macro_can_bind_a_path_head() {
        let main = "mod local {\n    pub(crate) fn run() {}\n}\nmacro_rules! expose {\n    () => { use crate::local as generated; }\n}\nexpose!();\n\nfn main() {\n    generated::run();\n}\n";
        assert!(!external(&[("main.rs", main)], "main.rs", "run"));
    }

    #[test]
    fn test_c_macro_can_rebind_a_library_function() {
        let main = "#include <stdio.h>\nstatic int repo_printf(const char *s) { return 0; }\n#define printf repo_printf\n\nint main(void) {\n    return printf(\"x\");\n}\n";
        assert!(!external(&[("main.c", main)], "main.c", "printf"));
        let other = "#include <stdio.h>\nint main(void) {\n    return printf(\"x\");\n}\n";
        let header = "#define printf repo_printf\n";
        let files = [("main.c", other), ("rebind.h", header)];
        assert!(!external(&files, "main.c", "printf"));
    }

    #[test]
    fn test_cpp_macro_can_rebind_the_std_namespace() {
        let main = "namespace repo {\nstatic void sort() {}\n}\n#define std repo\n\nint main() {\n    std::sort();\n}\n";
        assert!(!external(&[("main.cpp", main)], "main.cpp", "sort"));
    }

    #[test]
    fn test_bash_alias_can_rebind_a_builtin() {
        let run = "shopt -s expand_aliases\nrepo_echo() { printf '%s\\n' \"$1\"; }\nalias echo=repo_echo\nrun() { echo hello; }\nrun\n";
        assert!(!external(&[("run.sh", run)], "run.sh", "echo"));
    }

    #[test]
    fn test_ecma_global_assignment_can_rebind_a_global_function() {
        for (file, object) in [
            ("a.js", "globalThis"),
            ("b.ts", "window"),
            ("c.jsx", "global"),
            ("d.tsx", "self"),
        ] {
            let app = format!(
                "function repoFetch() {{ return 1; }}\n{object}.fetch = repoFetch;\nfetch();\n"
            );
            assert!(!external(&[(file, app.as_str())], file, "fetch"), "{file}");
        }
    }

    #[test]
    fn test_php_property_and_constant_names_are_case_sensitive() {
        let app = "<?php\nclass Settings {\n    public string $STRLEN;\n    const STRPOS = 1;\n}\n\nfunction run(string $value): int {\n    return strlen($value) + strpos($value, 'a');\n}\n";
        assert!(external(&[("app.php", app)], "app.php", "strlen"));
        assert!(external(&[("app.php", app)], "app.php", "strpos"));
    }

    #[test]
    fn test_parenthesized_arrow_parameter_does_not_shadow_the_name_after_the_arrow() {
        for (file, source) in [
            ("a.js", "const f = (Math) => Math.max(1);\nMath.max(2);\n"),
            ("b.js", "consume((Math) => Math.max(1));\nMath.max(2);\n"),
            (
                "c.ts",
                "const f = (Math: Maths) => Math.max(1);\nMath.max(2);\n",
            ),
        ] {
            let found = statuses(&[(file, source)], file, "max");
            assert_eq!(found.last().map(String::as_str), Some("External"), "{file}");
        }
    }

    #[test]
    fn test_assignment_to_a_local_named_like_the_global_object_is_not_a_rebinding() {
        let parameter = "function install(globalThis) {\n  globalThis.fetch = localFetch;\n}\n\nfunction run() {\n  return fetch('/remote');\n}\n";
        let variable = "function install() {\n  const window = {};\n  window.fetch = localFetch;\n}\n\nfunction run() {\n  return fetch('/remote');\n}\n";
        let arrow = "const install = self => { self.fetch = localFetch; };\nfunction run() { return fetch('/remote'); }\n";
        for (file, source) in [("a.js", parameter), ("b.ts", variable), ("c.js", arrow)] {
            assert!(external(&[(file, source)], file, "fetch"), "{file}");
        }
    }

    #[test]
    fn test_single_lambda_parameter_does_not_shadow_the_name_outside_its_body() {
        let javascript = "const callback = fetch => fetch('/local');\nfunction run() { return fetch('/remote'); }\n";
        let csharp = "using System;\nclass Program {\n    void Callback() {\n        Action<string> action = Console => Console.WriteLine(\"a\");\n    }\n    void Run() {\n        Console.WriteLine(\"b\");\n    }\n}\n";
        let java = "import java.util.function.Function;\nclass Main {\n    void callback() {\n        Function<String, Integer> parse = Integer -> Integer.length();\n    }\n    void run() {\n        Integer.parseInt(\"2\");\n    }\n}\n";
        let mut failed = Vec::new();
        let parenthesized =
            "consume((fetch) => fetch('/local'));\nfunction run() { return fetch('/remote'); }\n";
        let typescript = "consume((fetch: string) => fetch);\nconsume(function (fetch) { return fetch('/x'); });\nfunction run() { return fetch('/remote'); }\n";
        let csharp_typed = "using System;\nclass Widget {}\nclass Program {\n    void Callback() {\n        Take((Widget Console) => Console.WriteLine(\"a\"));\n        Take(delegate (Widget Console) { Console.WriteLine(\"c\"); });\n        Console.WriteLine(\"b\");\n    }\n}\n";
        let java_typed = "class Widget {}\nclass Main {\n    void callback() {\n        take((Widget Integer) -> Integer.length());\n        Integer.parseInt(\"2\");\n    }\n}\n";
        for (file, source, name) in [
            ("b.js", parenthesized, "fetch"),
            ("c.ts", typescript, "fetch"),
            ("Typed.cs", csharp_typed, "WriteLine"),
            ("Typed.java", java_typed, "parseInt"),
            ("a.js", javascript, "fetch"),
            ("Program.cs", csharp, "WriteLine"),
            ("Main.java", java, "parseInt"),
        ] {
            let found = statuses(&[(file, source)], file, name);
            if found.last().map(String::as_str) != Some("External") {
                failed.push(file);
            }
        }
        assert!(failed.is_empty(), "shadowed outside the lambda: {failed:?}");
    }

    #[test]
    fn test_ruby_singleton_class_can_rebind_a_core_class() {
        let app = "class << File\n  def join(*parts)\n    'repository'\n  end\nend\n\nFile.join('a', 'b')\n";
        assert!(!external(&[("app.rb", app)], "app.rb", "join"));
    }

    #[test]
    fn test_python_module_assignment_can_rebind_a_builtin_name() {
        let app = "class Repo:\n    def append(self, value):\n        return value\n\nlist = Repo()\nlen = Repo()\n\ndef main():\n    list.append(1)\n    len.append(2)\n    return len(3)\n";
        let files = [("app.py", app)];
        assert!(!external(&files, "app.py", "append"));
        let other = "list = 1\n";
        let user = "def main():\n    return len([1])\n";
        assert!(external(&[("a.py", other), ("b.py", user)], "b.py", "len"));
    }

    #[test]
    fn test_php_function_names_ignore_case() {
        let app = "<?php\nnamespace {\n    function Str_Contains($text, $part) {\n        return strpos($text, $part) !== false;\n    }\n}\nnamespace App {\n    function main() {\n        return str_contains(\"ab\", \"a\");\n    }\n}\n";
        assert!(!external(&[("app.php", app)], "app.php", "str_contains"));
        let other = "<?php\nfunction run($x) {\n    return STRLEN($x);\n}\n";
        assert!(external(&[("other.php", other)], "other.php", "STRLEN"));
    }

    #[test]
    fn test_rust_path_dependency_is_a_crate_of_the_repository() {
        let cargo = "[dependencies]\nutil = { path = \"util\" }\ncore-lib.workspace = true\nserde_json = \"1\"\n\n[dependencies.local]\npath = \"local\"\n";
        let main = "fn run() {\n    util::a();\n    core_lib::b();\n    local::c();\n    serde_json::d();\n}\n";
        let files = [
            ("main.rs", main),
            ("Cargo.toml", cargo),
            ("util/Cargo.toml", "[package]\nname = \"util\"\n"),
            ("util/src/lib.rs", "pub fn a() {}\n"),
            ("core-lib/Cargo.toml", "[package]\nname = \"core-lib\"\n"),
            ("core-lib/src/lib.rs", "pub fn b() {}\n"),
        ];
        assert!(!external(&files, "main.rs", "a"));
        assert!(!external(&files, "main.rs", "b"));
        assert!(!external(&files, "main.rs", "c"));
        assert!(external(&files, "main.rs", "d"));
    }

    #[test]
    fn test_a_call_with_a_candidate_is_never_external() {
        let files = [(
            "app.py",
            "def len(a):\n    return a\n\ndef run():\n    len(1)\n",
        )];
        assert!(!external(&files, "app.py", "len"));
    }
}

#[cfg(test)]
mod test_runner_timing_tests {
    use crate::index::resolve::testing::rows;
    use crate::index::timing::{assert_scales_linearly, Stopwatch};

    /// One test file with `size` statements, each with a chain of calls on
    /// a test global. A chain of many calls is not a part of the case: the
    /// extraction of one chain takes time in proportion to the square of its
    /// length, before the resolution.
    fn matcher_chains_at(size: usize) -> std::time::Duration {
        let package = "{\"name\": \"app\", \"devDependencies\": {\"vitest\": \"1.0.0\"}}";
        let mut app = String::from("it('a', () => {\n");
        for position in 0..size {
            app.push_str(&format!(
                "  expect({position}).not.toBe({position});\n  vi.fn().mockReturnValue({position}).mockReturnValueOnce({position});\n"
            ));
        }
        app.push_str("});\n");
        let files = [("package.json", package), ("app.test.ts", app.as_str())];
        let started = Stopwatch::start();
        let found = rows(&files, "app.test.ts");
        let elapsed = started.elapsed();
        let last = 2 * size + 1;
        for call in [
            format!("{} ->(..).toBe External", last - 1),
            format!("{last} ->(..).mockReturnValueOnce External"),
        ] {
            assert!(
                found.iter().any(|row| row.starts_with(&call)),
                "{call} {:?}",
                found.last()
            );
        }
        elapsed
    }

    #[test]
    fn test_matcher_chains_are_external() {
        matcher_chains_at(20);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_matcher_chains_are_classified_in_linear_time() {
        assert_scales_linearly("matcher chains", 2_000, matcher_chains_at);
    }
}
