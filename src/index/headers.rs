//! The language of a header with the extension `.h`.
//!
//! `Language::detect` gives C for each `.h` file. Many C++ projects also use
//! this extension. The C grammar does not read a namespace or a class, so the
//! index then has no symbol for them.
//!
//! Only the index uses these rules. A `.h` file is C++ in these conditions:
//!
//! - A C++ file of the repository includes it, directly or through a header
//!   that is C++.
//! - The C grammar finds a syntax error in it, and the C++ grammar finds none.
//!
//! In each other condition, the file is C.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::build::ReadFile;
use super::extract::{extract_file, FileSymbols};
use super::model::{ImportKind, ImportTarget};
use crate::core::Language;
use crate::parser::{ParseResult, Parser};

fn is_dot_h(path: &str) -> bool {
    path.ends_with(".h")
}

/// Parses a file for the index. A `.h` file that is not correct C, and that is
/// correct C++, gets the C++ grammar.
pub fn parse(path: &Path, language: Language, content: &[u8]) -> Option<ParseResult> {
    let parsed = Parser::new().parse(content, language, path).ok()?;
    let header = language == Language::C && is_dot_h(&path.to_string_lossy());
    if !header || !parsed.tree.root_node().has_error() {
        return Some(parsed);
    }
    match Parser::new().parse(content, Language::Cpp, path) {
        Ok(as_cpp) if !as_cpp.tree.root_node().has_error() => Some(as_cpp),
        _ => Some(parsed),
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The path without `.` and `..` segments. `None` means that the path goes
/// above the root.
fn normalised(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    Some(segments.join("/"))
}

/// The C headers that an include of `includer` names.
///
/// The path is first relative to the directory of the includer. If the index
/// has a file there, that file is the only answer. If it has none, each C
/// header whose path ends with the include path is an answer: the include
/// directories of the compiler are not known.
fn included_headers(
    includer: &str,
    include_path: &str,
    positions: &BTreeMap<&str, usize>,
    facts: &[FileSymbols],
) -> Vec<usize> {
    let is_c_header = |position: &usize| {
        facts[*position].language == Language::C && is_dot_h(&facts[*position].file)
    };
    let directory = includer
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let beside = normalised(&format!("{directory}/{include_path}"));
    if let Some(position) = beside.and_then(|path| positions.get(path.as_str()).copied()) {
        return Some(position).filter(is_c_header).into_iter().collect();
    }
    let Some(tail) = normalised(include_path).filter(|_| !include_path.contains("..")) else {
        return Vec::new();
    };
    let suffix = format!("/{tail}");
    positions
        .iter()
        .filter(|(path, _)| **path == tail || path.ends_with(&suffix))
        .map(|(_, position)| *position)
        .filter(is_c_header)
        .collect()
}

/// Gives the C++ grammar to each `.h` file that a C++ file includes, directly
/// or through a header that is C++. `facts` has the files that `parse` read.
/// The result does not depend on the order of `files` or of `facts`.
pub fn reparse_headers_of_cpp_files(
    root: &Path,
    files: &[PathBuf],
    read: ReadFile<'_>,
    facts: &mut [FileSymbols],
) {
    let mut found: BTreeSet<usize> = BTreeSet::new();
    {
        let positions: BTreeMap<&str, usize> = facts
            .iter()
            .enumerate()
            .map(|(position, file)| (file.file.as_str(), position))
            .collect();
        let mut pending: Vec<usize> = positions
            .values()
            .copied()
            .filter(|position| facts[*position].language == Language::Cpp)
            .collect();
        while let Some(includer) = pending.pop() {
            for binding in &facts[includer].imports {
                // A system include has an external target from the start.
                let local = binding.kind == ImportKind::Load
                    && binding.target_file != ImportTarget::External;
                if !local {
                    continue;
                }
                let headers = included_headers(
                    &facts[includer].file,
                    &binding.module_path,
                    &positions,
                    facts,
                );
                for header in headers {
                    if found.insert(header) {
                        pending.push(header);
                    }
                }
            }
        }
    }
    if found.is_empty() {
        return;
    }
    let paths: BTreeMap<String, &PathBuf> = files
        .iter()
        .map(|path| (relative(root, path), path))
        .collect();
    for position in found {
        let name = facts[position].file.clone();
        let parsed = paths.get(&name).and_then(|path| {
            let content = read(path).ok()?;
            Parser::new().parse(&content, Language::Cpp, path).ok()
        });
        if let Some(parsed) = parsed {
            facts[position] = extract_file(&parsed, &name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::build::CodeIndex;
    use crate::index::model::SymbolKind;

    const CPP_HEADER: &str = "namespace shop {\n\nclass Circle {\npublic:\n    double area() const;\n};\n\n}  // namespace shop\n";
    const C_HEADER: &str =
        "struct Shape {\n    double side;\n};\n\ndouble area(struct Shape shape);\n";

    fn index(sources: &[(&str, &str)]) -> CodeIndex {
        let read = |path: &Path| {
            sources
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| crate::core::Error::git("no such file".to_string()))
        };
        let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };
        let files: Vec<PathBuf> = sources
            .iter()
            .map(|(name, _)| PathBuf::from(name))
            .collect();
        CodeIndex::build(Path::new("."), &files, &read, &list)
    }

    fn languages(index: &CodeIndex) -> Vec<String> {
        index
            .files
            .iter()
            .map(|file| format!("{} {:?}", file.file, file.language))
            .collect()
    }

    #[test]
    fn test_a_header_with_cpp_syntax_is_cpp() {
        let index = index(&[("shapes.h", CPP_HEADER)]);

        assert_eq!(languages(&index), ["shapes.h Cpp"]);
        let symbols: Vec<(SymbolKind, &str)> = index.files[0]
            .symbols
            .iter()
            .map(|symbol| (symbol.kind, symbol.name.as_str()))
            .collect();
        assert_eq!(
            symbols,
            [
                (SymbolKind::Module, "shop"),
                (SymbolKind::Class, "Circle"),
                (SymbolKind::Method, "area"),
            ]
        );
    }

    #[test]
    fn test_a_c_header_that_no_cpp_file_includes_is_c() {
        let index = index(&[
            (
                "main.c",
                "#include \"shapes.h\"\n\nint main(void) { return 0; }\n",
            ),
            ("shapes.h", C_HEADER),
        ]);

        assert_eq!(languages(&index), ["main.c C", "shapes.h C"]);
    }

    #[test]
    fn test_a_header_that_a_cpp_file_includes_is_cpp() {
        let index = index(&[
            (
                "main.cpp",
                "#include <vector>\n#include \"shapes.h\"\n\nint main() { return 0; }\n",
            ),
            ("shapes.h", C_HEADER),
        ]);

        assert_eq!(languages(&index), ["main.cpp Cpp", "shapes.h Cpp"]);
        let names: Vec<&str> = index.files[1]
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();
        assert!(
            names.contains(&"Shape") && names.contains(&"area"),
            "{names:?}"
        );
    }

    #[test]
    fn test_a_header_that_a_cpp_header_includes_is_cpp() {
        let index = index(&[
            ("inner.h", C_HEADER),
            ("main.cpp", "#include \"outer.h\"\n"),
            ("outer.h", "#include \"inner.h\"\n\nint outer(void);\n"),
        ]);

        assert_eq!(
            languages(&index),
            ["inner.h Cpp", "main.cpp Cpp", "outer.h Cpp"]
        );
    }

    #[test]
    fn test_a_mixed_repository_has_c_headers_and_cpp_headers() {
        let sources = [
            (
                "c/main.c",
                "#include \"util.h\"\n\nint main(void) { return helper(); }\n",
            ),
            ("c/util.h", "int helper(void);\n"),
            (
                "cpp/main.cpp",
                "#include \"util.h\"\n#include \"shared/shape.h\"\n",
            ),
            ("cpp/util.h", "int helper(void);\n"),
            ("include/shared/shape.h", C_HEADER),
            ("legacy/circle.h", CPP_HEADER),
            ("legacy/plain.h", C_HEADER),
        ];
        let expected = [
            "c/main.c C",
            // The include of `cpp/main.cpp` names the file in its directory.
            "c/util.h C",
            "cpp/main.cpp Cpp",
            "cpp/util.h Cpp",
            // No file is at `cpp/shared/shape.h`. The end of the path gives the header.
            "include/shared/shape.h Cpp",
            "legacy/circle.h Cpp",
            "legacy/plain.h C",
        ];

        assert_eq!(languages(&index(&sources)), expected);
        let mut reversed = sources;
        reversed.reverse();
        assert_eq!(languages(&index(&reversed)), expected);
    }

    #[test]
    fn test_a_header_with_an_error_in_the_two_grammars_stays_c() {
        let index = index(&[("broken.h", "int helper(void;\nstruct {\n")]);

        assert_eq!(languages(&index), ["broken.h C"]);
    }

    #[test]
    fn test_a_class_of_a_dot_h_header_is_a_receiver_type() {
        let index = index(&[
            ("circle.h", "class Circle {\npublic:\n    double area() const { return 1.0; }\n};\n"),
            (
                "main.cpp",
                "#include \"circle.h\"\n\ndouble measure(const Circle& circle) {\n    return circle.area();\n}\n",
            ),
        ]);

        let call = &index.files[1].references[0];
        assert_eq!(call.resolution.candidates, ["circle.h#Circle.area"]);
    }

    #[test]
    fn test_normalised_paths() {
        assert_eq!(normalised("a/./b/../c.h").as_deref(), Some("a/c.h"));
        assert_eq!(normalised("/c.h").as_deref(), Some("c.h"));
        assert_eq!(normalised("../c.h"), None);
    }
}
