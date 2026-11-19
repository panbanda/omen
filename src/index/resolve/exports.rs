//! The export tables of the call resolution. See `crate::index::export_tables`.

use std::collections::HashMap;

use super::{Family, Id, ImportBinding, ImportKind, Resolver, SymbolKind};
use crate::index::export_tables::{self, Exports, Member, Table};

/// How a binding gives names of its target again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReExport {
    Named,
    Wildcard,
}

impl<'a> Resolver<'a> {
    fn re_export(&self, file: usize, binding: &ImportBinding) -> Option<ReExport> {
        let python = self.family(file) == Family::Python;
        if binding.kind == ImportKind::ReExportNamed
            || (python && binding.kind == ImportKind::Named)
        {
            Some(ReExport::Named)
        } else if binding.kind == ImportKind::ReExportWildcard
            || (python && binding.kind == ImportKind::Wildcard)
        {
            Some(ReExport::Wildcard)
        } else {
            None
        }
    }

    /// The table of the names that `file` exports.
    pub(super) fn export_table(&self, file: usize) -> Table<'a, Id> {
        export_tables::table(self, &self.export_tables, file)
    }
}

impl<'a> Exports<'a, Id> for Resolver<'a> {
    fn targets(&self, file: usize) -> Vec<usize> {
        let mut targets: Vec<usize> = self.files[file]
            .imports
            .iter()
            .filter(|binding| self.re_export(file, binding).is_some())
            .flat_map(|binding| self.target_files(binding))
            .collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// The rules of the own definitions are the rules of a name of one
    /// file: a default export, a top-level definition, an alias of an
    /// export, and the one definition of the name in a module of the file.
    fn member(&self, file: usize) -> Member<'a, Id> {
        let facts = &self.files[file];
        let mut defaults: Vec<Id> = Vec::new();
        let mut top_level: HashMap<&'a str, Vec<Id>> = HashMap::new();
        let mut aliased: HashMap<&'a str, Vec<Id>> = HashMap::new();
        let mut in_module: HashMap<&'a str, Vec<Id>> = HashMap::new();
        for (position, symbol) in facts.symbols.iter().enumerate() {
            let id = (file, position);
            let link = &facts.link_facts[position];
            if link.default_export {
                defaults.push(id);
            }
            if self.is_top_level(id) {
                top_level.entry(symbol.name.as_str()).or_default().push(id);
            }
            for alias in &link.export_aliases {
                aliased.entry(alias.as_str()).or_default().push(id);
            }
            let in_a_module = link
                .parent
                .is_some_and(|parent| facts.symbols[parent].kind == SymbolKind::Module);
            if in_a_module {
                in_module.entry(symbol.name.as_str()).or_default().push(id);
            }
        }
        let mut own: HashMap<&'a str, Vec<Id>> = HashMap::new();
        for (name, found) in in_module {
            if let [only] = found.as_slice() {
                own.insert(name, vec![*only]);
            }
        }
        own.extend(aliased);
        own.extend(top_level);
        if !defaults.is_empty() {
            own.insert("default", defaults);
        }
        let mut wildcards = Vec::new();
        let mut named = Vec::new();
        for binding in &facts.imports {
            let Some(kind) = self.re_export(file, binding) else {
                continue;
            };
            for target in self.target_files(binding) {
                match kind {
                    ReExport::Wildcard => wildcards.push(target),
                    ReExport::Named => {
                        if let Some(local) = binding.local_name.as_deref() {
                            let source = binding.imported_name.as_deref().unwrap_or(local);
                            named.push((local, target, source));
                        }
                    }
                }
            }
        }
        Member {
            own: own.into_iter().collect(),
            wildcards,
            named,
        }
    }

    fn contribution<'v>(&self, values: &'v [Id]) -> &'v [Id] {
        values
    }
}

#[cfg(test)]
mod timing_tests {
    use crate::index::resolve::testing::rows;
    use crate::index::timing::{assert_scales_linearly, Stopwatch};

    /// `size` modules with one function each, a barrel that re-exports each
    /// module with a wildcard, and a file that imports each function from
    /// the barrel and calls it.
    fn wildcard_barrel_at(size: usize) -> std::time::Duration {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut barrel = String::new();
        let mut names = Vec::new();
        let mut calls = String::new();
        for position in 0..size {
            files.push((
                format!("m{position}.ts"),
                format!("export function f{position}() {{}}\n"),
            ));
            barrel.push_str(&format!("export * from './m{position}';\n"));
            names.push(format!("f{position}"));
            calls.push_str(&format!("\tf{position}();\n"));
        }
        files.push(("index.ts".to_string(), barrel));
        files.push((
            "view.ts".to_string(),
            format!(
                "import {{ {} }} from './index';\n\nexport function view() {{\n{calls}}}\n",
                names.join(", ")
            ),
        ));
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = Stopwatch::start();
        let found = rows(&files, "view.ts");
        let elapsed = started.elapsed();
        assert_eq!(found.len(), size);
        let last = size - 1;
        assert_eq!(
            found[last],
            format!(
                "{} view>f{last} UniqueCandidate ImportedName [m{last}.ts#f{last}]",
                4 + last
            )
        );
        elapsed
    }

    /// A chain of `size` barrels. Each barrel defines one function and
    /// re-exports the barrel before it with a wildcard. A file imports each
    /// function from the last barrel and calls it. The case gives the time
    /// and the number of the nodes of the tables.
    fn barrel_chain_at(size: usize) -> (std::time::Duration, usize) {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut names = Vec::new();
        let mut calls = String::new();
        for position in 0..size {
            let previous = if position == 0 {
                String::new()
            } else {
                format!("export * from './b{}';\n", position - 1)
            };
            files.push((
                format!("b{position}.ts"),
                format!("{previous}export function f{position}() {{}}\n"),
            ));
            names.push(format!("f{position}"));
            calls.push_str(&format!("\tf{position}();\n"));
        }
        files.push((
            "view.ts".to_string(),
            format!(
                "import {{ {} }} from './b{}';\n\nexport function view() {{\n{calls}}}\n",
                names.join(", "),
                size - 1
            ),
        ));
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let nodes = crate::index::export_tables::NODES.with(std::cell::Cell::get);
        let started = Stopwatch::start();
        let found = rows(&files, "view.ts");
        let elapsed = started.elapsed();
        let nodes = crate::index::export_tables::NODES.with(std::cell::Cell::get) - nodes;
        assert_eq!(found.len(), size);
        assert_eq!(
            found[0],
            "4 view>f0 UniqueCandidate ImportedName [b0.ts#f0]".to_string()
        );
        (elapsed, nodes)
    }

    /// `size` files in one cycle of wildcard re-exports. Each file defines
    /// one function, and a file imports each function from the first file.
    fn barrel_cycle_at(size: usize) -> std::time::Duration {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut names = Vec::new();
        let mut calls = String::new();
        for position in 0..size {
            files.push((
                format!("c{position}.ts"),
                format!(
                    "export * from './c{}';\nexport function f{position}() {{}}\n",
                    (position + 1) % size
                ),
            ));
            names.push(format!("f{position}"));
            calls.push_str(&format!("\tf{position}();\n"));
        }
        files.push((
            "view.ts".to_string(),
            format!(
                "import {{ {} }} from './c0';\n\nexport function view() {{\n{calls}}}\n",
                names.join(", ")
            ),
        ));
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let started = Stopwatch::start();
        let found = rows(&files, "view.ts");
        let elapsed = started.elapsed();
        assert_eq!(found.len(), size);
        let last = size - 1;
        assert_eq!(
            found[last],
            format!(
                "{} view>f{last} UniqueCandidate ImportedName [c{last}.ts#f{last}]",
                4 + last
            )
        );
        elapsed
    }

    #[test]
    fn test_timing_cases_resolve_each_call() {
        wildcard_barrel_at(3);
        barrel_chain_at(3);
        barrel_cycle_at(3);
    }

    /// Each file of a cycle gives the definitions of each file of the cycle,
    /// and its own definition comes first. Two definitions of the cycle give
    /// a name that is ambiguous.
    #[test]
    fn test_names_of_a_cycle_of_re_exports() {
        let files = [
            ("a.ts", "export * from './b';\nexport function x() {}\nexport function own() {}\n"),
            ("b.ts", "export * from './c';\nexport function own() {}\n"),
            ("c.ts", "export * from './a';\nexport function x() {}\n"),
            (
                "view.ts",
                "import { x, own } from './b';\nimport { own as mine } from './a';\n\nexport function view() {\n\tx();\n\town();\n\tmine();\n}\n",
            ),
        ];
        assert_eq!(
            rows(&files, "view.ts"),
            [
                "5 view>x Ambiguous ImportedName [a.ts#x, c.ts#x]",
                "6 view>own UniqueCandidate ImportedName [b.ts#own]",
                "7 view>mine UniqueCandidate ImportedAlias [a.ts#own]",
            ]
        );
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_calls_through_a_chain_of_barrels_resolve_in_linear_time() {
        assert_scales_linearly("chain of barrels", 8_000, |size| barrel_chain_at(size).0);
        // A table that copied each name below it would have four times the nodes.
        let (_, small) = crate::index::timing::on_one_thread(|| barrel_chain_at(8_000));
        let (_, large) = crate::index::timing::on_one_thread(|| barrel_chain_at(16_000));
        assert!(
            large * 10 < small * 25,
            "nodes of the tables: {small} at 8000, {large} at 16000"
        );
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_calls_through_a_cycle_of_barrels_resolve_in_linear_time() {
        assert_scales_linearly("cycle of barrels", 8_000, barrel_cycle_at);
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_calls_through_a_wildcard_barrel_resolve_in_linear_time() {
        assert_scales_linearly("wildcard barrel", 4_000, wildcard_barrel_at);
    }
}
