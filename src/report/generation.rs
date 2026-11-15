//! The generation of a report data directory.
//!
//! `report generate` writes `metadata.json` last. It lists each data file of the run with the
//! SHA-256 digest of the file, and it names the files of earlier runs that it kept. A data
//! directory that a run left in the middle of a commit has a file that does not match its digest.
//! `report validate` and `report render` find such a directory with `check`.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};

/// The SHA-256 digest of some bytes, as lower-case hexadecimal.
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The identifier of a run: the time of the run and the digest of each file.
pub fn generation_id(generated_at: &str, files: &BTreeMap<String, String>) -> String {
    let mut text = generated_at.to_string();
    for (name, file_digest) in files {
        text.push_str(&format!("\n{name} {file_digest}"));
    }
    digest(text.as_bytes())[..16].to_string()
}

/// The problems of a data directory. An empty list means that each file is in the generation
/// of `metadata.json`. A directory whose metadata has no generation is from an older version,
/// and it has no problem.
///
/// `known` has the file names without the extension that `report generate` writes.
pub fn check(data_dir: &Path, known: &[&str]) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(data_dir.join("metadata.json")) else {
        return Vec::new();
    };
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let (Some(generation), Some(files)) = (
        metadata.get("generation").and_then(|value| value.as_str()),
        metadata.get("files").and_then(|value| value.as_object()),
    ) else {
        return Vec::new();
    };

    let mut problems = Vec::new();
    for (name, expected) in files {
        match std::fs::read(data_dir.join(name)) {
            Ok(bytes) if Some(digest(&bytes).as_str()) == expected.as_str() => {}
            Ok(_) => problems.push(format!(
                "{name}: the file is not from generation {generation} of metadata.json"
            )),
            Err(error) => problems.push(format!(
                "{name}: the file of generation {generation} cannot be read: {error}"
            )),
        }
    }
    for stem in known.iter().copied().chain(["diff"]) {
        let name = format!("{stem}.json");
        if stem != "metadata" && data_dir.join(&name).exists() && !files.contains_key(&name) {
            problems.push(format!(
                "{name}: the file is not part of generation {generation} of metadata.json"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_metadata(directory: &Path, generation: &str, files: &[(&str, &str)]) {
        let files: serde_json::Map<String, serde_json::Value> = files
            .iter()
            .map(|(name, content)| {
                (
                    (*name).to_string(),
                    serde_json::Value::String(digest(content.as_bytes())),
                )
            })
            .collect();
        let metadata = serde_json::json!({"generation": generation, "files": files});
        std::fs::write(directory.join("metadata.json"), metadata.to_string()).unwrap();
    }

    #[test]
    fn a_directory_without_a_generation_has_no_problem() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("metadata.json"), "{}").unwrap();
        std::fs::write(directory.path().join("score.json"), "{}").unwrap();
        assert!(check(directory.path(), &["score"]).is_empty());
    }

    #[test]
    fn a_file_that_matches_its_digest_has_no_problem() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("score.json"), "new").unwrap();
        write_metadata(directory.path(), "g1", &[("score.json", "new")]);
        assert!(check(directory.path(), &["score"]).is_empty());
    }

    #[test]
    fn a_file_of_another_run_is_a_problem() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("score.json"), "old").unwrap();
        write_metadata(directory.path(), "g1", &[("score.json", "new")]);
        let problems = check(directory.path(), &["score"]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("score.json"));
    }

    #[test]
    fn a_known_file_that_the_generation_does_not_list_is_a_problem() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("trend.json"), "old").unwrap();
        write_metadata(directory.path(), "g1", &[]);
        let problems = check(directory.path(), &["trend"]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("trend.json"));
    }

    #[test]
    fn the_id_depends_on_the_files() {
        let mut files = BTreeMap::new();
        files.insert("a.json".to_string(), digest(b"1"));
        let first = generation_id("2026-01-01", &files);
        files.insert("a.json".to_string(), digest(b"2"));
        assert_ne!(first, generation_id("2026-01-01", &files));
        assert_eq!(first.len(), 16);
    }
}
