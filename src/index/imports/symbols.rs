//! The symbol tables of the import resolution. See `crate::index::export_tables`.
//!
//! The table of a file gives each name the keys of its symbols. A name has
//! a symbol only when it has one key. A file that re-exports a name with
//! more than one key gets nothing from that file.

use std::collections::HashMap;

use super::{ImportKind, ImportTarget, Repository};
use crate::core::Language;
use crate::index::export_tables::{self, Exports, Member};
use crate::index::model::SymbolKind;

impl<'a> Repository<'a> {
    /// The key of the symbol that the name `name` of `file` gives to an import.
    pub(super) fn symbol_of(&self, file: &str, name: &str) -> Option<String> {
        let position = *self.by_path.get(file)?;
        let table = export_tables::table(self, &self.symbol_tables, position);
        match table.get(name)?.as_slice() {
            [only] => Some(only.to_string()),
            _ => None,
        }
    }

    /// The re-exports of the file at `position`: a wildcard re-export gives
    /// `None` as the local name.
    fn re_exports(
        &self,
        position: usize,
    ) -> impl Iterator<Item = (Option<(&'a str, &'a str)>, usize)> + '_ {
        let facts: &'a crate::index::extract::FileSymbols = &self.files[position];
        let python = facts.language == Language::Python;
        facts.imports.iter().filter_map(move |binding| {
            let ImportTarget::Resolved(target) = &binding.target_file else {
                return None;
            };
            let target = *self.by_path.get(target.as_str())?;
            let named = binding.kind == ImportKind::ReExportNamed
                || (python && binding.kind == ImportKind::Named);
            let wildcard = (binding.kind == ImportKind::ReExportWildcard
                || (python && binding.kind == ImportKind::Wildcard))
                && binding.local_name.is_none();
            if named {
                let local = binding.local_name.as_deref()?;
                let source = binding.imported_name.as_deref().unwrap_or(local);
                Some((Some((local, source)), target))
            } else if wildcard {
                Some((None, target))
            } else {
                None
            }
        })
    }
}

impl<'a> Exports<'a, &'a str> for Repository<'a> {
    fn targets(&self, file: usize) -> Vec<usize> {
        let mut targets: Vec<usize> = self.re_exports(file).map(|(_, target)| target).collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// A definition at the top level or an alias of an export decides the
    /// name, also with two symbols, which give no symbol. A module of the
    /// file decides the name only with one symbol.
    fn member(&self, file: usize) -> Member<'a, &'a str> {
        let facts: &'a crate::index::extract::FileSymbols = &self.files[file];
        let modules: std::collections::HashSet<&str> = facts
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Module)
            .map(|symbol| symbol.name.as_str())
            .collect();
        let mut defined: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        let mut aliased: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        let mut in_module: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        for (symbol, link) in facts.symbols.iter().zip(&facts.link_facts) {
            match &symbol.container {
                None => defined.entry(&symbol.name).or_default().push(&symbol.key),
                Some(container) if modules.contains(container.as_str()) => {
                    in_module.entry(&symbol.name).or_default().push(&symbol.key);
                }
                Some(_) => {}
            }
            for alias in &link.export_aliases {
                aliased.entry(alias).or_default().push(&symbol.key);
            }
        }
        let mut own: HashMap<&'a str, Vec<&'a str>> = in_module
            .into_iter()
            .filter(|(_, keys)| keys.len() == 1)
            .collect();
        own.extend(aliased);
        own.extend(defined);
        let mut wildcards = Vec::new();
        let mut named = Vec::new();
        for (names, target) in self.re_exports(file) {
            match names {
                Some((local, source)) => named.push((local, target, source)),
                None => wildcards.push(target),
            }
        }
        Member {
            own: own.into_iter().collect(),
            wildcards,
            named,
        }
    }

    fn contribution<'v>(&self, values: &'v [&'a str]) -> &'v [&'a str] {
        if values.len() == 1 {
            values
        } else {
            &[]
        }
    }
}
