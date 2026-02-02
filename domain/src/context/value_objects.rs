//! Value objects for context management
//!
//! This module provides value objects for representing known context files
//! and their loaded content. These are immutable data structures that carry
//! context information without behavior dependencies.
//!
//! # Overview
//!
//! The context system recognizes several types of files that can provide
//! project context to the AI agent:
//!
//! - **Primary context files**: CLAUDE.md, .quorum/context.md
//! - **Documentation files**: README.md, docs/*.md
//! - **Project configuration**: Cargo.toml, package.json, pyproject.toml
//!
//! # Example
//!
//! ```
//! use quorum_domain::context::{KnownContextFile, LoadedContextFile};
//!
//! // Get all known file types
//! for file_type in KnownContextFile::all() {
//!     println!("{}: priority {}", file_type, file_type.priority());
//! }
//!
//! // Create a loaded context file
//! let loaded = LoadedContextFile::new(
//!     KnownContextFile::ClaudeMdLocal,
//!     "/project/CLAUDE.md",
//!     "# Project Instructions\nBuild with cargo.",
//! );
//!
//! assert!(loaded.is_primary());
//! ```

use std::path::Path;

/// Known context files that should be checked in a project.
///
/// This enum represents all file types that the context loader will attempt
/// to read when gathering project context. Files are prioritized, with
/// lower priority values indicating higher importance.
///
/// # Priority Order
///
/// 1. `.quorum/context.md` - Generated context (highest priority)
/// 2. `CLAUDE.md` - Local project instructions
/// 3. `.claude/CLAUDE.md` - Global Claude configuration
/// 4. `README.md` - Project readme
/// 5. `docs/**/*.md` - Documentation directory
/// 6. Build files (Cargo.toml, package.json, pyproject.toml)
///
/// # Primary vs Secondary Context
///
/// Primary context files (QuorumContext, ClaudeMdLocal, ClaudeMdGlobal)
/// are considered sufficient to skip the exploration phase. Secondary
/// files provide supplementary information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownContextFile {
    /// `.quorum/context.md` - Generated context file with highest priority.
    ///
    /// This file is created by the `/init` command and contains a
    /// synthesized project summary from multiple AI models.
    QuorumContext,

    /// `CLAUDE.md` in project root - Local project instructions.
    ///
    /// Contains project-specific instructions and guidelines for
    /// AI assistants working on this codebase.
    ClaudeMdLocal,

    /// `.claude/CLAUDE.md` - Global Claude configuration from home directory.
    ///
    /// Contains user-wide preferences and instructions that apply
    /// to all projects.
    ClaudeMdGlobal,

    /// `README.md` - Project readme file.
    ///
    /// Provides general project overview, setup instructions, and
    /// usage documentation.
    ReadmeMd,

    /// `docs/**/*.md` - Documentation directory.
    ///
    /// All markdown files under the docs/ directory are aggregated
    /// into a single context entry.
    DocsMarkdown,

    /// `Cargo.toml` - Rust project manifest.
    ///
    /// Indicates a Rust project and provides dependency information.
    CargoToml,

    /// `package.json` - Node.js project manifest.
    ///
    /// Indicates a Node.js/JavaScript project with npm dependencies.
    PackageJson,

    /// `pyproject.toml` - Python project manifest.
    ///
    /// Indicates a Python project using modern packaging standards.
    PyprojectToml,
}

impl KnownContextFile {
    /// Returns the relative path pattern for this file type.
    ///
    /// For most file types, this is a simple relative path. For `DocsMarkdown`,
    /// this returns a glob pattern.
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::KnownContextFile;
    ///
    /// assert_eq!(KnownContextFile::ClaudeMdLocal.relative_path(), "CLAUDE.md");
    /// assert_eq!(KnownContextFile::DocsMarkdown.relative_path(), "docs/**/*.md");
    /// ```
    pub fn relative_path(&self) -> &'static str {
        match self {
            KnownContextFile::QuorumContext => ".quorum/context.md",
            KnownContextFile::ClaudeMdLocal => "CLAUDE.md",
            KnownContextFile::ClaudeMdGlobal => ".claude/CLAUDE.md",
            KnownContextFile::ReadmeMd => "README.md",
            KnownContextFile::DocsMarkdown => "docs/**/*.md",
            KnownContextFile::CargoToml => "Cargo.toml",
            KnownContextFile::PackageJson => "package.json",
            KnownContextFile::PyprojectToml => "pyproject.toml",
        }
    }

    /// Returns the priority of this file type.
    ///
    /// Lower values indicate higher priority. When building project context,
    /// files are processed in priority order, and the first primary context
    /// file found is used.
    ///
    /// # Priority Values
    ///
    /// - 0: QuorumContext (highest)
    /// - 1: ClaudeMdLocal
    /// - 2: ClaudeMdGlobal
    /// - 3: ReadmeMd
    /// - 4: DocsMarkdown
    /// - 5: Build files (CargoToml, PackageJson, PyprojectToml)
    pub fn priority(&self) -> u8 {
        match self {
            KnownContextFile::QuorumContext => 0,
            KnownContextFile::ClaudeMdLocal => 1,
            KnownContextFile::ClaudeMdGlobal => 2,
            KnownContextFile::ReadmeMd => 3,
            KnownContextFile::DocsMarkdown => 4,
            KnownContextFile::CargoToml => 5,
            KnownContextFile::PackageJson => 5,
            KnownContextFile::PyprojectToml => 5,
        }
    }

    /// Checks if this file type is a primary context source.
    ///
    /// Primary context files provide sufficient information to skip
    /// the exploration phase during context gathering. These are files
    /// specifically designed to provide AI assistants with project context.
    ///
    /// # Returns
    ///
    /// `true` for QuorumContext, ClaudeMdLocal, and ClaudeMdGlobal.
    pub fn is_primary_context(&self) -> bool {
        matches!(
            self,
            KnownContextFile::QuorumContext
                | KnownContextFile::ClaudeMdLocal
                | KnownContextFile::ClaudeMdGlobal
        )
    }

    /// Checks if this file type provides project type information.
    ///
    /// Build configuration files can be used to automatically detect
    /// the project's programming language and ecosystem.
    pub fn provides_project_type(&self) -> bool {
        matches!(
            self,
            KnownContextFile::CargoToml
                | KnownContextFile::PackageJson
                | KnownContextFile::PyprojectToml
        )
    }

    /// Returns the project type indicated by this file, if any.
    ///
    /// # Returns
    ///
    /// - `Some("rust")` for CargoToml
    /// - `Some("nodejs")` for PackageJson
    /// - `Some("python")` for PyprojectToml
    /// - `None` for other file types
    pub fn project_type(&self) -> Option<&'static str> {
        match self {
            KnownContextFile::CargoToml => Some("rust"),
            KnownContextFile::PackageJson => Some("nodejs"),
            KnownContextFile::PyprojectToml => Some("python"),
            _ => None,
        }
    }

    /// Returns all known file types in priority order.
    ///
    /// This is useful for iterating over all file types when loading
    /// context from a project.
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::KnownContextFile;
    ///
    /// let all_types = KnownContextFile::all();
    /// assert_eq!(all_types[0], KnownContextFile::QuorumContext);
    /// assert_eq!(all_types.len(), 8);
    /// ```
    pub fn all() -> &'static [KnownContextFile] {
        &[
            KnownContextFile::QuorumContext,
            KnownContextFile::ClaudeMdLocal,
            KnownContextFile::ClaudeMdGlobal,
            KnownContextFile::ReadmeMd,
            KnownContextFile::DocsMarkdown,
            KnownContextFile::CargoToml,
            KnownContextFile::PackageJson,
            KnownContextFile::PyprojectToml,
        ]
    }
}

impl std::fmt::Display for KnownContextFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.relative_path())
    }
}

/// A loaded context file with its content.
///
/// This struct represents a context file that has been successfully
/// read from the file system. It contains the file type, full path,
/// and the file's content.
///
/// # Examples
///
/// ```
/// use quorum_domain::context::{KnownContextFile, LoadedContextFile};
///
/// let loaded = LoadedContextFile::new(
///     KnownContextFile::ReadmeMd,
///     "/project/README.md",
///     "# My Project\n\nA great project.",
/// );
///
/// assert_eq!(loaded.filename(), "README.md");
/// assert!(!loaded.is_primary());
/// ```
#[derive(Debug, Clone)]
pub struct LoadedContextFile {
    /// The type of file that was loaded.
    pub file_type: KnownContextFile,

    /// The full path to the file on disk.
    pub path: String,

    /// The content of the file.
    pub content: String,
}

impl LoadedContextFile {
    /// Creates a new loaded context file.
    ///
    /// # Arguments
    ///
    /// * `file_type` - The type of context file
    /// * `path` - The full path to the file
    /// * `content` - The file's content
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::{KnownContextFile, LoadedContextFile};
    ///
    /// let loaded = LoadedContextFile::new(
    ///     KnownContextFile::CargoToml,
    ///     "/project/Cargo.toml",
    ///     "[package]\nname = \"my-crate\"",
    /// );
    /// ```
    pub fn new(file_type: KnownContextFile, path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            file_type,
            path: path.into(),
            content: content.into(),
        }
    }

    /// Checks if this file is a primary context source.
    ///
    /// Primary context sources are sufficient to skip the exploration
    /// phase during context gathering.
    ///
    /// # Returns
    ///
    /// `true` if the file type is QuorumContext, ClaudeMdLocal, or ClaudeMdGlobal.
    pub fn is_primary(&self) -> bool {
        self.file_type.is_primary_context()
    }

    /// Returns the project type if this file indicates one.
    ///
    /// # Returns
    ///
    /// The project type string if this is a build configuration file,
    /// or `None` otherwise.
    pub fn project_type(&self) -> Option<&'static str> {
        self.file_type.project_type()
    }

    /// Extracts the filename from the path.
    ///
    /// # Returns
    ///
    /// The filename portion of the path, or the full path if extraction fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::{KnownContextFile, LoadedContextFile};
    ///
    /// let loaded = LoadedContextFile::new(
    ///     KnownContextFile::ReadmeMd,
    ///     "/path/to/project/README.md",
    ///     "content",
    /// );
    ///
    /// assert_eq!(loaded.filename(), "README.md");
    /// ```
    pub fn filename(&self) -> &str {
        Path::new(&self.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_context_file_priority() {
        assert!(KnownContextFile::QuorumContext.priority() < KnownContextFile::ClaudeMdLocal.priority());
        assert!(KnownContextFile::ClaudeMdLocal.priority() < KnownContextFile::ReadmeMd.priority());
    }

    #[test]
    fn test_is_primary_context() {
        assert!(KnownContextFile::QuorumContext.is_primary_context());
        assert!(KnownContextFile::ClaudeMdLocal.is_primary_context());
        assert!(!KnownContextFile::ReadmeMd.is_primary_context());
        assert!(!KnownContextFile::CargoToml.is_primary_context());
    }

    #[test]
    fn test_project_type() {
        assert_eq!(KnownContextFile::CargoToml.project_type(), Some("rust"));
        assert_eq!(KnownContextFile::PackageJson.project_type(), Some("nodejs"));
        assert_eq!(KnownContextFile::PyprojectToml.project_type(), Some("python"));
        assert_eq!(KnownContextFile::ReadmeMd.project_type(), None);
    }

    #[test]
    fn test_loaded_context_file() {
        let loaded = LoadedContextFile::new(
            KnownContextFile::ClaudeMdLocal,
            "/project/CLAUDE.md",
            "# Project Instructions",
        );

        assert!(loaded.is_primary());
        assert_eq!(loaded.filename(), "CLAUDE.md");
        assert!(loaded.project_type().is_none());
    }
}
