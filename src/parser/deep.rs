//! Source with very deep nesting, and a thread with a small stack, for the
//! tests of the walks over a syntax tree.

use std::path::Path;
use std::sync::OnceLock;

use rayon::prelude::*;

use crate::core::Language;

use super::{ParseResult, Parser};

/// The nesting depth of each source of the debug tests. Each stack frame keeps at least a
/// return address and a frame pointer, 16 bytes, so a walk with one frame for each level
/// needs more than 80,000 bytes and overflows the 64 KiB stack of `on_small_stack`. The
/// depth is small, so that each source parses in milliseconds.
pub const DEPTH: usize = 5_000;

/// The nesting depth of the release tests that run the whole analysis on a thread with the
/// default stack.
pub const LARGE_DEPTH: usize = 100_000;

/// A source file with deep nesting.
pub struct DeepSource {
    pub language: Language,
    pub file: String,
    pub source: String,
    /// Code outside the walks takes time in proportion to the depth for each
    /// level of this source: `Node::parent` or `Node::prev_sibling` for each
    /// nested scope, block, or declarator, or the text of each call of a chain. The release
    /// tests at `LARGE_DEPTH` leave out these sources.
    pub slow: bool,
}

impl DeepSource {
    pub fn parse(&self) -> ParseResult {
        Parser::new()
            .parse(self.source.as_bytes(), self.language, Path::new(&self.file))
            .expect("parse")
    }
}

/// The size of the stack of `on_small_stack`.
pub const SMALL_STACK: usize = 64 * 1024;

/// Run `work` on a thread with a stack of `SMALL_STACK` bytes.
pub fn on_small_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(work)
        .expect("spawn")
        .join()
        .expect("no stack overflow")
}

/// `open` `DEPTH` times, then `middle`, then `close` `DEPTH` times.
pub fn nest(open: &str, middle: &str, close: &str) -> String {
    nest_to(DEPTH, open, middle, close)
}

/// `open` `depth` times, then `middle`, then `close` `depth` times.
pub fn nest_to(depth: usize, open: &str, middle: &str, close: &str) -> String {
    let mut text = open.repeat(depth);
    text.push_str(middle);
    text.push_str(&close.repeat(depth));
    text
}

/// `depth` copies of `item` with `separator` between them.
fn chain_to(depth: usize, item: &str, separator: &str) -> String {
    vec![item; depth].join(separator)
}

fn deep(language: Language, file: &str, source: String) -> DeepSource {
    DeepSource {
        language,
        file: file.to_string(),
        source,
        slow: false,
    }
}

fn slow(language: Language, file: &str, source: String) -> DeepSource {
    DeepSource {
        slow: true,
        ..deep(language, file, source)
    }
}

/// The deep sources of `language` at `depth`: parentheses, blocks, nested functions,
/// literals, and long chains of operators and calls, as the grammar permits.
pub fn deep_sources_at(language: Language, depth: usize) -> Vec<DeepSource> {
    let nest = |open: &str, middle: &str, close: &str| nest_to(depth, open, middle, close);
    let chain = |item: &str, separator: &str| chain_to(depth, item, separator);
    let parentheses = nest("(", "1", ")");
    let sum = chain("1", "+");
    let calls = format!("a{}", ".b()".repeat(depth));
    let members = format!("a{}()", ".b".repeat(depth));
    let paths = format!("a{}()", "::b".repeat(depth));
    let l = language;
    match language {
        Language::Go => vec![
            deep(
                l,
                "parens.go",
                format!("package p\nfunc f() int {{ return {parentheses} }}\n"),
            ),
            deep(
                l,
                "blocks.go",
                format!(
                    "package p\nfunc f(x bool) {{ {} }}\n",
                    nest("if x {", "", "}")
                ),
            ),
            slow(
                l,
                "closures.go",
                format!(
                    "package p\nfunc f() {{ {} }}\n",
                    nest("func() {", "", "}()")
                ),
            ),
            deep(
                l,
                "sum.go",
                format!("package p\nfunc f() int {{ return {sum} }}\n"),
            ),
            slow(
                l,
                "calls.go",
                format!("package p\nfunc f() {{ {calls} }}\n"),
            ),
            deep(
                l,
                "members.go",
                format!("package p\nfunc f() {{ {members} }}\n"),
            ),
            deep(
                l,
                "literals.go",
                format!("package p\nvar x = {}\n", nest("[]any{", "", "}")),
            ),
        ],
        Language::Rust => vec![
            deep(
                l,
                "parens.rs",
                format!("fn f() -> i32 {{ {parentheses} }}\n"),
            ),
            deep(
                l,
                "blocks.rs",
                format!("fn f(x: bool) {{ {} }}\n", nest("if x {", "", "}")),
            ),
            slow(
                l,
                "closures.rs",
                format!("fn f() {{ let g = {}; }}\n", nest("|| ", "1", "")),
            ),
            deep(l, "sum.rs", format!("fn f() -> i32 {{ {sum} }}\n")),
            slow(l, "calls.rs", format!("fn f() {{ {calls}; }}\n")),
            deep(l, "members.rs", format!("fn f() {{ {members}; }}\n")),
            deep(l, "paths.rs", format!("fn f() {{ {paths}; }}\n")),
            deep(l, "uses.rs", format!("use {};\n", nest("a::{", "b", "}"))),
            deep(
                l,
                "arrays.rs",
                format!("fn f() {{ let x = {}; }}\n", nest("[", "1", "]")),
            ),
            slow(l, "modules.rs", nest("mod m {", "fn f() {}", "}")),
            deep(
                l,
                "types.rs",
                format!("fn f(x: {}) {{}}\n", nest("Vec<", "i32", ">")),
            ),
        ],
        Language::Python => vec![
            deep(
                l,
                "parens.py",
                format!("def f():\n    return {parentheses}\n"),
            ),
            deep(
                l,
                "lists.py",
                format!("def f():\n    return {}\n", nest("[", "1", "]")),
            ),
            deep(l, "sum.py", format!("def f():\n    return {sum}\n")),
            slow(l, "calls.py", format!("def f():\n    {calls}\n")),
            deep(l, "members.py", format!("def f():\n    {members}\n")),
            slow(
                l,
                "lambdas.py",
                format!("def f():\n    return {}\n", nest("lambda: ", "1", "")),
            ),
            deep(
                l,
                "arguments.py",
                format!("def f():\n    return {}\n", nest("g(", "1", ")")),
            ),
        ],
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            let extension = match language {
                Language::TypeScript => "ts",
                Language::Tsx => "tsx",
                Language::JavaScript => "js",
                _ => "jsx",
            };
            let file = |stem: &str| format!("{stem}.{extension}");
            let mut sources = vec![
                deep(
                    l,
                    &file("parens"),
                    format!("function f() {{ return {parentheses}; }}\n"),
                ),
                deep(
                    l,
                    &file("blocks"),
                    format!("function f(x) {{ {} }}\n", nest("if (x) {", "", "}")),
                ),
                slow(
                    l,
                    &file("arrows"),
                    format!("const f = {};\n", nest("() => ", "1", "")),
                ),
                slow(l, &file("functions"), nest("function f() {", "", "}")),
                deep(
                    l,
                    &file("sum"),
                    format!("function f() {{ return {sum}; }}\n"),
                ),
                slow(l, &file("calls"), format!("function f() {{ {calls}; }}\n")),
                deep(
                    l,
                    &file("members"),
                    format!("function f() {{ {members}; }}\n"),
                ),
                deep(
                    l,
                    &file("arrays"),
                    format!("const x = {};\n", nest("[", "1", "]")),
                ),
                deep(
                    l,
                    &file("objects"),
                    format!("const x = {};\n", nest("{a: ", "1", "}")),
                ),
                deep(
                    l,
                    &file("patterns"),
                    format!("function f() {{ const {} = y; }}\n", nest("[", "x", "]")),
                ),
            ];
            if matches!(language, Language::Tsx | Language::Jsx) {
                sources.push(deep(
                    l,
                    &file("elements"),
                    format!("const x = {};\n", nest("<a>", "", "</a>")),
                ));
            }
            sources
        }
        Language::Java => vec![
            deep(
                l,
                "Parens.java",
                format!("class A {{ int f() {{ return {parentheses}; }} }}\n"),
            ),
            deep(
                l,
                "Blocks.java",
                format!(
                    "class A {{ void f(boolean x) {{ {} }} }}\n",
                    nest("if (x) {", "", "}")
                ),
            ),
            slow(
                l,
                "Lambdas.java",
                format!(
                    "class A {{ Object f() {{ return {}; }} }}\n",
                    nest("() -> ", "1", "")
                ),
            ),
            deep(
                l,
                "Sum.java",
                format!("class A {{ int f() {{ return {sum}; }} }}\n"),
            ),
            slow(
                l,
                "Calls.java",
                format!("class A {{ void f() {{ {calls}; }} }}\n"),
            ),
            deep(
                l,
                "Members.java",
                format!("class A {{ void f() {{ {members}; }} }}\n"),
            ),
            slow(l, "Classes.java", nest("class A {", "void f() {}", "}")),
        ],
        Language::C => vec![
            deep(
                l,
                "parens.c",
                format!("int f(void) {{ return {parentheses}; }}\n"),
            ),
            slow(
                l,
                "blocks.c",
                format!("void f(int x) {{ {} }}\n", nest("if (x) {", "", "}")),
            ),
            deep(l, "sum.c", format!("int f(void) {{ return {sum}; }}\n")),
            slow(
                l,
                "pointers.c",
                format!("int {}f(void) {{ return 0; }}\n", "*".repeat(depth)),
            ),
            deep(
                l,
                "declarators.c",
                format!("int {}(void) {{ return 0; }}\n", nest("(", "f", ")")),
            ),
            deep(
                l,
                "initializers.c",
                format!("int x[] = {};\n", nest("{", "1", "}")),
            ),
            deep(
                l,
                "fields.c",
                format!("void f(void) {{ a{}; }}\n", "->b".repeat(depth)),
            ),
            deep(l, "members.c", format!("void f(void) {{ {members}; }}\n")),
        ],
        Language::Cpp => vec![
            deep(
                l,
                "parens.cpp",
                format!("int f() {{ return {parentheses}; }}\n"),
            ),
            deep(
                l,
                "blocks.cpp",
                format!("void f(bool x) {{ {} }}\n", nest("if (x) {", "", "}")),
            ),
            slow(
                l,
                "lambdas.cpp",
                format!(
                    "auto f() {{ return {}; }}\n",
                    nest("[]{ return ", "1", "; }")
                ),
            ),
            slow(
                l,
                "namespaces.cpp",
                nest("namespace a {", "void f() {}", "}"),
            ),
            slow(l, "classes.cpp", nest("struct A {", "void f() {}", "};")),
            deep(l, "sum.cpp", format!("int f() {{ return {sum}; }}\n")),
            deep(l, "members.cpp", format!("void f() {{ {members}; }}\n")),
            deep(l, "paths.cpp", format!("void f() {{ {paths}; }}\n")),
            slow(
                l,
                "pointers.cpp",
                format!("int {}f() {{ return 0; }}\n", "*".repeat(depth)),
            ),
            deep(
                l,
                "templates.cpp",
                format!("{} x;\n", nest("std::vector<", "int", ">")),
            ),
        ],
        Language::CSharp => vec![
            deep(
                l,
                "Parens.cs",
                format!("class A {{ int F() {{ return {parentheses}; }} }}\n"),
            ),
            deep(
                l,
                "Blocks.cs",
                format!(
                    "class A {{ void F(bool x) {{ {} }} }}\n",
                    nest("if (x) {", "", "}")
                ),
            ),
            slow(
                l,
                "Lambdas.cs",
                format!(
                    "class A {{ object F() {{ return {}; }} }}\n",
                    nest("() => ", "1", "")
                ),
            ),
            slow(l, "Classes.cs", nest("class A {", "void F() {}", "}")),
            slow(l, "Namespaces.cs", nest("namespace A {", "class B {}", "}")),
            deep(
                l,
                "Sum.cs",
                format!("class A {{ int F() {{ return {sum}; }} }}\n"),
            ),
            slow(
                l,
                "Calls.cs",
                format!("class A {{ void F() {{ {calls}; }} }}\n"),
            ),
            deep(
                l,
                "Members.cs",
                format!("class A {{ void F() {{ {members}; }} }}\n"),
            ),
        ],
        Language::Ruby => vec![
            deep(l, "parens.rb", format!("def f\n  {parentheses}\nend\n")),
            deep(
                l,
                "arrays.rb",
                format!("def f\n  {}\nend\n", nest("[", "1", "]")),
            ),
            slow(l, "modules.rb", nest("module A\n", "def f\nend\n", "end\n")),
            deep(
                l,
                "blocks.rb",
                format!("def f\n{}end\n", nest("if x\n", "", "end\n")),
            ),
            slow(
                l,
                "lambdas.rb",
                format!("def f\n  {}\nend\n", nest("-> { ", "1", " }")),
            ),
            deep(l, "sum.rb", format!("def f\n  {sum}\nend\n")),
            slow(l, "calls.rb", format!("def f\n  {calls}\nend\n")),
            slow(
                l,
                "paths.rb",
                format!("def f\n  A{}\nend\n", "::B".repeat(depth)),
            ),
            deep(l, "hashes.rb", format!("X = {}\n", nest("{a: ", "1", "}"))),
        ],
        Language::Php => vec![
            deep(
                l,
                "parens.php",
                format!("<?php\nfunction f() {{ return {parentheses}; }}\n"),
            ),
            deep(
                l,
                "blocks.php",
                format!(
                    "<?php\nfunction f($x) {{ {} }}\n",
                    nest("if ($x) {", "", "}")
                ),
            ),
            slow(
                l,
                "arrows.php",
                format!("<?php\n$f = {};\n", nest("fn() => ", "1", "")),
            ),
            deep(
                l,
                "arrays.php",
                format!("<?php\n$x = {};\n", nest("[", "1", "]")),
            ),
            deep(
                l,
                "sum.php",
                format!("<?php\nfunction f() {{ return {sum}; }}\n"),
            ),
            slow(
                l,
                "calls.php",
                format!("<?php\nfunction f() {{ $a{}; }}\n", "->b()".repeat(depth)),
            ),
            deep(
                l,
                "members.php",
                format!("<?php\nfunction f() {{ $a{}(); }}\n", "->b".repeat(depth)),
            ),
        ],
        Language::Bash => vec![
            deep(
                l,
                "subshells.sh",
                format!("f() {{\n{}\n}}\n", nest("( ", "true", " )")),
            ),
            deep(
                l,
                "arithmetic.sh",
                format!("f() {{\n  echo $(({parentheses}))\n}}\n"),
            ),
            deep(
                l,
                "ifs.sh",
                format!("f() {{\n{}}}\n", nest("if true; then\n", "", "fi\n")),
            ),
            deep(
                l,
                "lists.sh",
                format!("f() {{\n  {}\n}}\n", chain("true", " && ")),
            ),
            deep(
                l,
                "pipes.sh",
                format!("f() {{\n  {}\n}}\n", chain("cat", " | ")),
            ),
        ],
    }
}

/// The deep sources of every language with their syntax trees. The trees are parsed one time
/// for all tests.
pub fn parsed_deep_sources() -> &'static [(DeepSource, ParseResult)] {
    static PARSED: OnceLock<Vec<(DeepSource, ParseResult)>> = OnceLock::new();
    PARSED.get_or_init(|| {
        all_deep_sources()
            .into_par_iter()
            .map(|source| {
                let parsed = source.parse();
                (source, parsed)
            })
            .collect()
    })
}

/// The parsed deep sources of `language`.
pub fn parsed_deep_sources_of(
    language: Language,
) -> impl Iterator<Item = &'static (DeepSource, ParseResult)> {
    parsed_deep_sources()
        .iter()
        .filter(move |(source, _)| source.language == language)
}

/// The deep sources of `language` at `DEPTH`.
pub fn deep_sources(language: Language) -> Vec<DeepSource> {
    deep_sources_at(language, DEPTH)
}

/// The languages of the deep sources.
pub const LANGUAGES: [Language; 14] = [
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

/// The deep sources of every language.
pub fn all_deep_sources() -> Vec<DeepSource> {
    LANGUAGES.into_iter().flat_map(deep_sources).collect()
}
