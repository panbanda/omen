//! Session-local content-addressed syntax cache. No persisted stale indexes.
use crate::analyzers::repomap::{build_index_with_parser, CallGraphIndex};
use crate::core::{Error, Language, Result};
use crate::parser::{ParseResult, Parser};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct Snapshot {
    pub index: Arc<CallGraphIndex>,
    pub sources: BTreeMap<PathBuf, Arc<ParseResult>>,
    pub fingerprint: String,
    pub parsed_files: usize,
    pub reused_files: usize,
    pub graph_rebuilt: bool,
}

#[derive(Default)]
pub struct IndexCache {
    root: PathBuf,
    hashes: BTreeMap<PathBuf, blake3::Hash>,
    last: Option<Snapshot>,
}

impl IndexCache {
    pub fn load(&mut self, root: &Path, files: &[PathBuf]) -> Result<Snapshot> {
        let root = root.canonicalize()?;
        let mut hashes = BTreeMap::new();
        let mut contents = BTreeMap::new();
        let mut bytes = 0usize;
        for path in files {
            let path = path.canonicalize()?;
            if !path.starts_with(&root) {
                return Err(Error::analysis("Index file escapes repository"));
            }
            if Language::detect(&path).is_none() {
                continue;
            }
            let source = std::fs::read(&path)?;
            bytes = bytes.saturating_add(source.len());
            hashes.insert(path.clone(), blake3::hash(&source));
            contents.insert(path, source);
        }
        if self.root == root && hashes == self.hashes {
            if let Some(last) = &self.last {
                let mut hit = last.clone();
                hit.parsed_files = 0;
                hit.reused_files = hashes.len();
                hit.graph_rebuilt = false;
                return Ok(hit);
            }
        }
        let mut hasher = blake3::Hasher::new();
        let mut sources = BTreeMap::new();
        let mut parsed_files = 0;
        for (path, source) in contents {
            let relative = path
                .strip_prefix(&root)
                .map_err(|e| Error::analysis(e.to_string()))?;
            let name = relative.to_string_lossy();
            hasher.update(&(name.len() as u64).to_le_bytes());
            hasher.update(name.as_bytes());
            hasher.update(hashes[&path].as_bytes());
            let previous = (self.root == root && self.hashes.get(&path) == hashes.get(&path))
                .then(|| self.last.as_ref().and_then(|s| s.sources.get(&path)))
                .flatten();
            let parsed = match previous {
                Some(p) => p.clone(),
                None => {
                    parsed_files += 1;
                    let language = Language::detect(&path)
                        .ok_or_else(|| Error::analysis("Unsupported source"))?;
                    Arc::new(Parser::new().parse(&source, language, &path)?)
                }
            };
            sources.insert(path, parsed);
        }
        let paths: Vec<_> = sources.keys().cloned().collect();
        let index = build_index_with_parser(&root, &paths, &|path| {
            sources
                .get(path)
                .map(|p| p.as_ref().clone())
                .ok_or_else(|| Error::analysis("Missing snapshot source"))
        })?;
        let snapshot = Snapshot {
            index: Arc::new(index),
            fingerprint: hasher.finalize().to_hex().to_string(),
            parsed_files,
            reused_files: paths.len() - parsed_files,
            graph_rebuilt: true,
            sources,
        };
        // Bound retention by input size/file count, not an unbounded global map.
        // This is not a hard bound on expanded AST or graph memory.
        if bytes <= 32 * 1024 * 1024 && paths.len() <= 10_000 {
            self.root = root;
            self.hashes = hashes;
            self.last = Some(snapshot.clone());
        } else {
            *self = Self::default();
        }
        Ok(snapshot)
    }
}
