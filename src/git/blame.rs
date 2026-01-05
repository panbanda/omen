//! Git blame operations.

use std::collections::HashMap;
use std::path::Path;

use gix::Repository;
use serde::{Deserialize, Serialize};

use crate::core::Result;

/// Blame information for a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameInfo {
    /// File path.
    pub path: String,
    /// Lines with their authors.
    pub lines: Vec<BlameLine>,
    /// Aggregated author statistics.
    pub authors: HashMap<String, AuthorStats>,
}

/// A single line with blame information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameLine {
    /// Line number (1-indexed).
    pub line: u32,
    /// Author name.
    pub author: String,
    /// Commit SHA.
    pub sha: String,
    /// Commit timestamp.
    pub timestamp: i64,
}

/// Statistics for an author.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthorStats {
    /// Number of lines owned.
    pub lines: u32,
    /// Percentage of file owned.
    pub percentage: f64,
    /// First contribution timestamp.
    pub first_commit: i64,
    /// Last contribution timestamp.
    pub last_commit: i64,
}

/// Get blame information for a file.
pub fn get_blame(_repo: &Repository, _root: &Path, path: &Path) -> Result<BlameInfo> {
    // TODO: Implement using gix
    // This is a placeholder that returns empty results
    // Full implementation would use repo.blame_file()

    Ok(BlameInfo {
        path: path.to_string_lossy().to_string(),
        lines: Vec::new(),
        authors: HashMap::new(),
    })
}

impl BlameInfo {
    /// Calculate the bus factor (number of significant contributors).
    pub fn bus_factor(&self) -> usize {
        self.authors
            .values()
            .filter(|stats| stats.percentage > 5.0)
            .count()
    }

    /// Get the primary owner (author with most lines).
    pub fn primary_owner(&self) -> Option<(&str, f64)> {
        self.authors
            .iter()
            .max_by(|a, b| a.1.lines.cmp(&b.1.lines))
            .map(|(name, stats)| (name.as_str(), stats.percentage))
    }

    /// Calculate ownership concentration.
    pub fn ownership_ratio(&self) -> f64 {
        self.primary_owner()
            .map(|(_, pct)| pct / 100.0)
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_factor_empty() {
        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: Vec::new(),
            authors: HashMap::new(),
        };
        assert_eq!(info.bus_factor(), 0);
    }

    #[test]
    fn test_bus_factor_with_authors() {
        let mut authors = HashMap::new();
        authors.insert(
            "Alice".to_string(),
            AuthorStats {
                lines: 80,
                percentage: 80.0,
                first_commit: 0,
                last_commit: 0,
            },
        );
        authors.insert(
            "Bob".to_string(),
            AuthorStats {
                lines: 15,
                percentage: 15.0,
                first_commit: 0,
                last_commit: 0,
            },
        );
        authors.insert(
            "Carol".to_string(),
            AuthorStats {
                lines: 5,
                percentage: 5.0,
                first_commit: 0,
                last_commit: 0,
            },
        );

        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: Vec::new(),
            authors,
        };

        assert_eq!(info.bus_factor(), 2); // Alice and Bob have >5%
    }
}
