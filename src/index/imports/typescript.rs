//! The facts of the project files of TypeScript and JavaScript:
//! `tsconfig.json`, `tsconfig.*.json`, and `jsconfig.json`. The index reads
//! them as JSON with comments and trailing commas. It does not run `tsc`.
//!
//! The index uses these options:
//!
//! - `extends`, as one path or as a list. A later config replaces the options
//!   of an earlier config, and the options of the file replace both.
//! - `baseUrl` and `paths`, which map a module name to a file.
//! - `outDir` and `rootDir`, which map a build output to its source file.
//!
//! A path in an option is relative to the config that has the option.
//! `${configDir}` is the directory of the config that the tool reads first.

use std::collections::BTreeMap;

use super::{directory, file_name, join, NodePackage};

/// The extensions of the source files of a module, in the order of the search.
pub const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "d.ts", "js", "jsx", "mjs", "cjs"];

/// The extensions of the build output of a source file. The longest comes first.
const OUTPUT_EXTENSIONS: &[&str] = &[".d.mts", ".d.cts", ".d.ts", ".mjs", ".cjs", ".jsx", ".js"];

/// The extensions of a source file that can give a build output.
const BUILD_SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// Each pattern of `paths` and its substitutions, sorted by the pattern.
pub type PathPatterns = Vec<(String, Vec<String>)>;

/// The options of one project file, as the file gives them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    pub extends: Vec<String>,
    pub base_url: Option<String>,
    pub paths: Option<PathPatterns>,
    pub out_dir: Option<String>,
    pub root_dir: Option<String>,
}

/// The options of a project file after `extends`. Each path is relative to
/// the root of the repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectOptions {
    pub base_url: Option<String>,
    /// The patterns of `paths`, and the directory of the config that has them.
    paths: Option<(PathPatterns, String)>,
    pub out_dir: Option<String>,
    pub root_dir: Option<String>,
}

impl ProjectOptions {
    /// The directory to which the substitutions of `paths` are relative:
    /// `baseUrl`, else the directory of the config that has `paths`.
    fn paths_base(&self) -> Option<&str> {
        let (_, directory) = self.paths.as_ref()?;
        Some(self.base_url.as_deref().unwrap_or(directory))
    }

    /// The paths that `paths` gives for a module name, relative to the root
    /// of the repository, in the order in which TypeScript tries them. An
    /// exact pattern comes before a pattern with `*`. Of the patterns with
    /// `*`, the pattern with the longest text before `*` applies.
    ///
    /// The second value tells if the pattern that applies is `*` alone, which
    /// matches each module name, also the name of a package.
    pub fn mapped(&self, module: &str) -> (Vec<String>, bool) {
        let (Some((patterns, _)), Some(base)) = (&self.paths, self.paths_base()) else {
            return (Vec::new(), false);
        };
        let exact = patterns
            .iter()
            .find(|(pattern, _)| pattern == module)
            .map(|(_, substitutions)| (substitutions, ""));
        let wildcard = || {
            patterns
                .iter()
                .filter_map(|(pattern, substitutions)| {
                    let (prefix, suffix) = pattern.split_once('*')?;
                    let matched = module.strip_prefix(prefix)?.strip_suffix(suffix)?;
                    Some((prefix.len(), substitutions, matched, pattern == "*"))
                })
                // The first pattern wins a tie.
                .rev()
                .max_by_key(|(prefix, _, _, _)| *prefix)
                .map(|(_, substitutions, matched, catch_all)| (substitutions, matched, catch_all))
        };
        let exact = exact.map(|(substitutions, matched)| (substitutions, matched, false));
        let Some((substitutions, matched, catch_all)) = exact.or_else(wildcard) else {
            return (Vec::new(), false);
        };
        let paths = substitutions
            .iter()
            .filter_map(|substitution| join(base, &substitution.replace('*', matched)))
            .collect();
        (paths, catch_all)
    }
}

/// Tells if a file with this name is a project file of TypeScript or JavaScript.
pub fn is_project_file(name: &str) -> bool {
    name == "jsconfig.json"
        || name == "tsconfig.json"
        || (name.starts_with("tsconfig.") && name.ends_with(".json"))
}

/// Removes the comments and the trailing commas of JSON with comments. A
/// comment becomes a space, thus the result is valid JSON when the input is
/// valid JSON with comments.
pub fn strip_comments(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut without_comments = String::with_capacity(text.len());
    let mut position = 0;
    let mut in_string = false;
    while position < characters.len() {
        let character = characters[position];
        let next = characters.get(position + 1).copied();
        if in_string {
            without_comments.push(character);
            if character == '\\' {
                if let Some(escaped) = next {
                    without_comments.push(escaped);
                    position += 1;
                }
            } else if character == '"' {
                in_string = false;
            }
            position += 1;
            continue;
        }
        match (character, next) {
            ('"', _) => {
                in_string = true;
                without_comments.push(character);
                position += 1;
            }
            ('/', Some('/')) => {
                while position < characters.len() && characters[position] != '\n' {
                    position += 1;
                }
                without_comments.push(' ');
            }
            ('/', Some('*')) => {
                position += 2;
                while position < characters.len()
                    && !(characters[position] == '*' && characters.get(position + 1) == Some(&'/'))
                {
                    position += 1;
                }
                position += 2;
                without_comments.push(' ');
            }
            _ => {
                without_comments.push(character);
                position += 1;
            }
        }
    }
    // A comma before `}` or `]` is a trailing comma.
    let characters: Vec<char> = without_comments.chars().collect();
    let mut result = String::with_capacity(characters.len());
    let mut in_string = false;
    let mut position = 0;
    while position < characters.len() {
        let character = characters[position];
        if in_string {
            if character == '\\' {
                result.push(character);
                position += 1;
                if let Some(escaped) = characters.get(position) {
                    result.push(*escaped);
                }
            } else {
                in_string = character != '"';
                result.push(character);
            }
            position += 1;
            continue;
        }
        if character == '"' {
            in_string = true;
        }
        if character == ',' {
            let following = characters[position + 1..]
                .iter()
                .find(|character| !character.is_whitespace());
            if matches!(following, Some('}' | ']')) {
                position += 1;
                continue;
            }
        }
        result.push(character);
        position += 1;
    }
    result
}

/// Reads a project file. The result is `None` when the text is not JSON with comments.
pub fn project_config(text: &str) -> Option<ProjectConfig> {
    let value: serde_json::Value = serde_json::from_str(&strip_comments(text)).ok()?;
    let extends = match value.get("extends") {
        Some(serde_json::Value::String(path)) => vec![path.clone()],
        Some(serde_json::Value::Array(paths)) => paths
            .iter()
            .filter_map(|path| path.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    let options = value.get("compilerOptions");
    let text_option = |name: &str| {
        options
            .and_then(|options| options.get(name))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    let paths = options
        .and_then(|options| options.get("paths"))
        .and_then(|paths| paths.as_object())
        .map(|paths| {
            paths
                .iter()
                .map(|(pattern, substitutions)| {
                    let substitutions = substitutions
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|substitution| substitution.as_str().map(str::to_string))
                        .collect();
                    (pattern.clone(), substitutions)
                })
                .collect()
        });
    Some(ProjectConfig {
        extends,
        base_url: text_option("baseUrl"),
        paths,
        out_dir: text_option("outDir"),
        root_dir: text_option("rootDir"),
    })
}

/// The project files of the repository, by their path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectConfigs {
    pub configs: BTreeMap<String, ProjectConfig>,
}

impl ProjectConfigs {
    /// The path of the config that `extends` names in the config at `from`.
    /// A name that is not a relative path names a file of a package. The
    /// result is `None` when the repository does not have the file.
    pub fn extended(&self, from: &str, name: &str, packages: &[NodePackage]) -> Option<String> {
        let candidates: Vec<String> = if name.starts_with('.') {
            join(directory(from), name).into_iter().collect()
        } else {
            packages
                .iter()
                .filter_map(|package| {
                    let rest = name.strip_prefix(package.name.as_str())?;
                    match rest {
                        "" => join(&package.directory, "tsconfig.json"),
                        rest => join(&package.directory, rest.strip_prefix('/')?),
                    }
                })
                .collect()
        };
        let found: Vec<String> = candidates
            .into_iter()
            .flat_map(|path| [format!("{path}.json"), path])
            .filter(|path| self.configs.contains_key(path))
            .collect();
        match found.as_slice() {
            [only] => Some(only.clone()),
            // `name` and `name.json` can both exist. TypeScript reads `name`.
            [with_extension, exact] if *with_extension == format!("{exact}.json") => {
                Some(exact.clone())
            }
            _ => None,
        }
    }

    /// The paths that `extends` of the config at `path` names and that the
    /// repository can have, but that the index did not read.
    pub fn unread(&self, packages: &[NodePackage]) -> Vec<String> {
        let mut unread = Vec::new();
        for (path, config) in &self.configs {
            for name in &config.extends {
                if self.extended(path, name, packages).is_some() {
                    continue;
                }
                if name.starts_with('.') {
                    if let Some(target) = join(directory(path), name) {
                        unread.push(format!("{target}.json"));
                        unread.push(target);
                    }
                    continue;
                }
                for package in packages {
                    let Some(rest) = name.strip_prefix(package.name.as_str()) else {
                        continue;
                    };
                    let target = match rest {
                        "" => join(&package.directory, "tsconfig.json"),
                        rest => rest
                            .strip_prefix('/')
                            .and_then(|rest| join(&package.directory, rest)),
                    };
                    if let Some(target) = target {
                        unread.push(format!("{target}.json"));
                        unread.push(target);
                    }
                }
            }
        }
        unread.sort();
        unread.dedup();
        unread
    }

    /// The names of the packages that `extends` names and that the index does not know.
    pub fn unknown_packages(&self, packages: &[NodePackage]) -> Vec<String> {
        let mut names: Vec<String> = self
            .configs
            .values()
            .flat_map(|config| &config.extends)
            .filter(|name| !name.starts_with('.'))
            .filter_map(|name| {
                let mut parts = name.split('/');
                let first = parts.next()?;
                let package = if first.starts_with('@') {
                    format!("{first}/{}", parts.next()?)
                } else {
                    first.to_string()
                };
                let known = packages.iter().any(|known| known.name == package);
                (!known).then_some(package)
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The options of the config at `path`, after `extends`.
    pub fn options(&self, path: &str, packages: &[NodePackage]) -> ProjectOptions {
        let leaf = directory(path).to_string();
        self.options_of(path, &leaf, packages, &mut Vec::new())
    }

    fn options_of(
        &self,
        path: &str,
        leaf: &str,
        packages: &[NodePackage],
        seen: &mut Vec<String>,
    ) -> ProjectOptions {
        let Some(config) = self.configs.get(path) else {
            return ProjectOptions::default();
        };
        if seen.iter().any(|earlier| earlier == path) {
            return ProjectOptions::default();
        }
        seen.push(path.to_string());
        let mut options = ProjectOptions::default();
        for name in &config.extends {
            let Some(parent) = self.extended(path, name, packages) else {
                continue;
            };
            let inherited = self.options_of(&parent, leaf, packages, seen);
            options.base_url = inherited.base_url.or(options.base_url);
            options.paths = inherited.paths.or(options.paths);
            options.out_dir = inherited.out_dir.or(options.out_dir);
            options.root_dir = inherited.root_dir.or(options.root_dir);
        }
        seen.pop();
        let here = directory(path);
        let to_leaf = relative_to(here, leaf);
        let located = |text: &Option<String>| -> Option<String> {
            let text = text.as_deref()?.replace("${configDir}", &to_leaf);
            if text.starts_with('/') {
                return None;
            }
            join(here, &text)
        };
        if config.base_url.is_some() {
            options.base_url = located(&config.base_url);
        }
        if let Some(paths) = &config.paths {
            let paths = paths
                .iter()
                .map(|(pattern, substitutions)| {
                    let substitutions = substitutions
                        .iter()
                        .map(|substitution| substitution.replace("${configDir}", &to_leaf))
                        .collect();
                    (pattern.clone(), substitutions)
                })
                .collect();
            options.paths = Some((paths, here.to_string()));
        }
        if config.out_dir.is_some() {
            options.out_dir = located(&config.out_dir);
        }
        if config.root_dir.is_some() {
            options.root_dir = located(&config.root_dir);
        }
        options
    }

    /// The configs in the directory `directory`.
    pub fn in_directory<'a>(&'a self, directory: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.configs
            .keys()
            .filter(move |path| super::directory(path) == directory)
            .map(String::as_str)
    }

    /// The config that applies to `file`: the nearest `tsconfig.json`, else
    /// the nearest `jsconfig.json`, in the directory of the file or above it.
    pub fn nearest(&self, file: &str) -> Option<&str> {
        let mut here = directory(file);
        loop {
            for name in ["tsconfig.json", "jsconfig.json"] {
                let path = if here.is_empty() {
                    name.to_string()
                } else {
                    format!("{here}/{name}")
                };
                if let Some((found, _)) = self.configs.get_key_value(&path) {
                    return Some(found.as_str());
                }
            }
            if here.is_empty() {
                return None;
            }
            here = directory(here);
        }
    }
}

/// The relative path from `from` to `to`. Both are directories relative to
/// the root of the repository.
fn relative_to(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|part| !part.is_empty()).collect();
    let to: Vec<&str> = to.split('/').filter(|part| !part.is_empty()).collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// The path of a build output without its extension, and tells if the path
/// had an extension. A path without an extension, as `dist/index`, can name
/// a directory with an index file.
pub fn output_stem(path: &str) -> (&str, bool) {
    let name = file_name(path);
    for extension in OUTPUT_EXTENSIONS {
        if name.len() > extension.len() {
            if let Some(stem) = path.strip_suffix(extension) {
                return (stem, true);
            }
        }
    }
    (path, false)
}

/// The paths of the source files that can give the build output with this
/// stem, in the order of the search.
pub fn source_candidates(stem: &str, had_extension: bool) -> Vec<String> {
    let mut candidates: Vec<String> = BUILD_SOURCE_EXTENSIONS
        .iter()
        .map(|extension| format!("{stem}.{extension}"))
        .collect();
    if !had_extension {
        candidates.extend(
            BUILD_SOURCE_EXTENSIONS
                .iter()
                .map(|extension| format!("{stem}/index.{extension}")),
        );
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_comments_keeps_strings() {
        let text =
            "{\n  // a\n  \"a\": \"//x/*y*/\", /* b */\n  \"b\": [1, 2,],\n  \"c\": \"\\\"\",\n}\n";
        let value: serde_json::Value = serde_json::from_str(&strip_comments(text)).unwrap();
        assert_eq!(value["a"], "//x/*y*/");
        assert_eq!(value["b"], serde_json::json!([1, 2]));
        assert_eq!(value["c"], "\"");
    }

    #[test]
    fn test_project_file_names() {
        assert!(is_project_file("tsconfig.json"));
        assert!(is_project_file("tsconfig.build.esm.json"));
        assert!(is_project_file("jsconfig.json"));
        assert!(!is_project_file("package.json"));
        assert!(!is_project_file("tsconfig.jsonc"));
    }

    #[test]
    fn test_relative_to() {
        assert_eq!(relative_to("a/b", "a/c"), "../c");
        assert_eq!(relative_to("a", "a"), ".");
        assert_eq!(relative_to("", "a/b"), "a/b");
    }

    #[test]
    fn test_output_stem() {
        assert_eq!(output_stem("dist/index.d.ts"), ("dist/index", true));
        assert_eq!(output_stem("dist/a.d.mts"), ("dist/a", true));
        assert_eq!(output_stem("dist/a.mjs"), ("dist/a", true));
        assert_eq!(output_stem("dist/index"), ("dist/index", false));
        assert_eq!(output_stem("dist/.js"), ("dist/.js", false));
    }
}
