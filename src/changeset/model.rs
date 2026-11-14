//! The data of a change set. The pull-request comment and the HTML report
//! use the same data.

use serde::{Deserialize, Serialize};

use crate::index::model::SymbolKind;

/// The state of a symbol, a call, or a file between the base and the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeState {
    Added,
    Removed,
    Changed,
    /// Context only: the item did not change.
    Unchanged,
}

impl ChangeState {
    /// The name in the output and the name of the style class.
    pub fn name(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
            Self::Unchanged => "unchanged",
        }
    }

    /// The prefix of a label. Colour alone is not accessible, thus each state
    /// that changed has a sign. An item that did not change has no sign.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Removed => "-",
            Self::Changed => "~",
            Self::Unchanged => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Complexity {
    pub cyclomatic: u32,
    pub cognitive: u32,
}

/// A direct caller of a changed symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerRef {
    pub key: String,
    pub file: String,
    pub line: u32,
    pub is_test: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolChange {
    pub key: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    /// The line at the head. For a removed symbol, the line at the base.
    pub line: u32,
    pub state: ChangeState,
    pub signature_before: Option<String>,
    pub signature_after: Option<String>,
    pub complexity_before: Option<Complexity>,
    pub complexity_after: Option<Complexity>,
    /// The callers at the head. For a removed symbol, the callers at the base.
    pub direct_callers: Vec<CallerRef>,
    pub transitive_callers: usize,
    /// The name of the weakest resolution basis of an edge to the symbol.
    pub weakest_basis: Option<String>,
}

impl SymbolChange {
    pub fn test_callers(&self) -> usize {
        self.direct_callers
            .iter()
            .filter(|caller| caller.is_test)
            .count()
    }

    pub fn signature_changed(&self) -> bool {
        matches!((&self.signature_before, &self.signature_after),
            (Some(before), Some(after)) if before != after)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub key: String,
    /// The name with the containers, for example `Circle.area`.
    pub label: String,
    pub file: String,
    /// The directory of the file.
    pub component: String,
    pub state: ChangeState,
}

/// A call from `from` to `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub state: ChangeState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub file: String,
    pub state: ChangeState,
    pub additions: u32,
    pub deletions: u32,
    pub symbols_changed: usize,
    pub max_cyclomatic: Option<u32>,
    /// The severity of the hotspot, if the file is a hotspot.
    pub hotspot: Option<String>,
    /// The authors of the file, in name order.
    pub owners: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Stub,
    Satd,
}

/// A stub or a debt comment that the head has and the base does not have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewFinding {
    pub kind: FindingKind,
    pub file: String,
    pub line: u32,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub files_changed: usize,
    pub lines_added: u32,
    pub lines_deleted: u32,
    pub symbols_added: usize,
    pub symbols_removed: usize,
    pub symbols_changed: usize,
    /// The callers of the changed symbols that did not change themselves.
    pub callers_affected: usize,
    pub cyclomatic_before: u32,
    pub cyclomatic_after: u32,
    pub new_stubs: usize,
    pub new_satd: usize,
    pub hotspots_touched: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// The name that the user gave for the base.
    pub target: String,
    pub base: String,
    pub head: String,
    pub summary: Summary,
    pub symbols: Vec<SymbolChange>,
    pub files: Vec<FileChange>,
    /// Facts about the limits of this change set that the reader must know.
    #[serde(default)]
    pub notes: Vec<String>,
    pub new_findings: Vec<NewFinding>,
    pub graph: ChangeGraph,
}
