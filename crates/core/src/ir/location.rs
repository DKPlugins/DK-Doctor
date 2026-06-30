//! Engine-independent "breadcrumb" path to the location of a finding.
//!
//! [`Location`] points to a project file and a logical path within it
//! (`Map003/EV005/page2/cmd14`). These are not byte offsets into the source,
//! but a structural address of an entity — the user does not edit such files by hand.

use camino::Utf8PathBuf;

/// Location of a finding: a project file + a logical path within it.
///
/// `file` is stored relative to the project root, for example `data/Map003.json`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Location {
    /// Path to the file relative to the project root (`data/Map003.json`).
    pub file: Utf8PathBuf,
    /// Logical path within the file.
    pub path: LocationPath,
}

impl Location {
    /// Creates a [`Location`] from a file path and the segments of a logical path.
    pub fn new(file: impl Into<Utf8PathBuf>, segments: Vec<PathSeg>) -> Self {
        Self {
            file: file.into(),
            path: LocationPath(segments),
        }
    }

    /// A [`Location`] pointing at the file itself, without an internal path.
    pub fn file_only(file: impl Into<Utf8PathBuf>) -> Self {
        Self {
            file: file.into(),
            path: LocationPath(Vec::new()),
        }
    }
}

/// A sequence of path segments within a file.
///
/// Rendered via `Display` as `Map003/EV005/page2/cmd14`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct LocationPath(pub Vec<PathSeg>);

impl std::fmt::Display for LocationPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("/")?;
            }
            write!(f, "{seg}")?;
        }
        Ok(())
    }
}

/// A single segment of a logical path.
///
/// `Plugin`/`Line` are reserved for the future JS parsing layer (AST).
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSeg {
    /// Map by its id.
    Map(u32),
    /// Event on a map by its id.
    Event(u32),
    /// Event page (1-based).
    Page(u32),
    /// Command in a page's command list (0-based index).
    Command(u32),
    /// Common event by id.
    CommonEvent(u32),
    /// Troop (enemy group) by id.
    Troop(u32),
    /// A database record in a specific file.
    DbRecord {
        /// Name of the DB file, for example `"Items"`.
        file: &'static str,
        /// Record id (== index in the array).
        id: u32,
    },
    /// Plugin by name (reserved for the AST layer).
    Plugin(String),
    /// Plugin parameter by name (value from `plugins.js`).
    Param(String),
    /// Line in the plugin source (reserved for the AST layer).
    Line(u32),
}

impl std::fmt::Display for PathSeg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathSeg::Map(id) => write!(f, "Map{id:03}"),
            PathSeg::Event(id) => write!(f, "EV{id:03}"),
            PathSeg::Page(id) => write!(f, "page{id}"),
            PathSeg::Command(i) => write!(f, "cmd{i}"),
            PathSeg::CommonEvent(id) => write!(f, "CE{id:03}"),
            PathSeg::Troop(id) => write!(f, "Troop{id:03}"),
            PathSeg::DbRecord { file, id } => write!(f, "{file}#{id}"),
            PathSeg::Plugin(name) => write!(f, "plugin:{name}"),
            PathSeg::Param(name) => write!(f, "param:{name}"),
            PathSeg::Line(n) => write!(f, "L{n}"),
        }
    }
}
