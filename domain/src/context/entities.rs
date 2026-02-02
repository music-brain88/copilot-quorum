//! Entities for context management
//!
//! Uses [`crate::core::string::truncate`] for UTF-8 safe truncation.
//!
//! This module provides the `ProjectContext` entity which aggregates
//! context information from multiple sources into a unified view.
//!
//! # Overview
//!
//! The `ProjectContext` entity is built from loaded context files and
//! provides:
//!
//! - Primary context (from CLAUDE.md or .quorum/context.md)
//! - README content
//! - Detected project type
//! - Aggregated documentation
//!
//! # Example
//!
//! ```
//! use quorum_domain::context::{KnownContextFile, LoadedContextFile, ProjectContext};
//!
//! let files = vec![
//!     LoadedContextFile::new(
//!         KnownContextFile::ClaudeMdLocal,
//!         "/project/CLAUDE.md",
//!         "# Instructions",
//!     ),
//!     LoadedContextFile::new(
//!         KnownContextFile::CargoToml,
//!         "/project/Cargo.toml",
//!         "[package]\nname = \"my-crate\"",
//!     ),
//! ];
//!
//! let ctx = ProjectContext::from_files(files);
//! assert!(ctx.has_sufficient_context());
//! assert_eq!(ctx.project_type, Some("rust".to_string()));
//! ```

use super::value_objects::{KnownContextFile, LoadedContextFile};
use crate::core::string::truncate;

/// Aggregated project context from multiple sources.
///
/// This entity combines information from various context files into a
/// single, unified view of the project. It's used during agent execution
/// to provide the AI with relevant project information.
///
/// # Building Context
///
/// Context is typically built using [`ProjectContext::from_files`], which
/// processes loaded files in priority order and extracts relevant information.
///
/// # Sufficiency Check
///
/// The [`has_sufficient_context`](ProjectContext::has_sufficient_context) method
/// determines whether the context is sufficient to skip the exploration phase.
/// This is true when a primary context file (CLAUDE.md or .quorum/context.md)
/// is available.
#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    /// Primary context content (from CLAUDE.md or .quorum/context.md).
    ///
    /// This is the main context that provides project-specific instructions
    /// and information for the AI assistant.
    pub primary_context: Option<String>,

    /// The source of the primary context.
    ///
    /// Indicates which file provided the primary context, useful for
    /// logging and debugging.
    pub context_source: Option<KnownContextFile>,

    /// README content.
    ///
    /// The project's README.md content, providing general project overview.
    pub readme: Option<String>,

    /// Detected project type (rust, nodejs, python, etc.).
    ///
    /// Automatically detected from build configuration files.
    pub project_type: Option<String>,

    /// Documentation content (aggregated from docs/).
    ///
    /// Combined content from all markdown files in the docs/ directory.
    pub documentation: Option<String>,

    /// All loaded files.
    ///
    /// Keeps track of which files were loaded to build this context.
    loaded_files: Vec<LoadedContextFile>,
}

impl ProjectContext {
    /// Creates a new empty project context.
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::ProjectContext;
    ///
    /// let ctx = ProjectContext::new();
    /// assert!(ctx.is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds project context from loaded files.
    ///
    /// This method processes the provided files in priority order and
    /// extracts:
    ///
    /// - Primary context from the highest-priority primary file
    /// - README content
    /// - Project type from build configuration files
    /// - Aggregated documentation from docs/ files
    ///
    /// # Arguments
    ///
    /// * `files` - List of loaded context files to process
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::{KnownContextFile, LoadedContextFile, ProjectContext};
    ///
    /// let files = vec![
    ///     LoadedContextFile::new(
    ///         KnownContextFile::ReadmeMd,
    ///         "/project/README.md",
    ///         "# My Project",
    ///     ),
    ///     LoadedContextFile::new(
    ///         KnownContextFile::CargoToml,
    ///         "/project/Cargo.toml",
    ///         "[package]\nname = \"test\"",
    ///     ),
    /// ];
    ///
    /// let ctx = ProjectContext::from_files(files);
    /// assert_eq!(ctx.project_type, Some("rust".to_string()));
    /// ```
    pub fn from_files(files: Vec<LoadedContextFile>) -> Self {
        let mut ctx = Self::new();
        ctx.loaded_files = files.clone();

        // Sort by priority
        let mut sorted_files = files;
        sorted_files.sort_by_key(|f| f.file_type.priority());

        for file in &sorted_files {
            // Set primary context from highest priority file
            if file.is_primary() && ctx.primary_context.is_none() {
                ctx.primary_context = Some(file.content.clone());
                ctx.context_source = Some(file.file_type);
            }

            // Set README
            if file.file_type == KnownContextFile::ReadmeMd && ctx.readme.is_none() {
                ctx.readme = Some(file.content.clone());
            }

            // Set project type
            if let Some(pt) = file.project_type()
                && ctx.project_type.is_none()
            {
                ctx.project_type = Some(pt.to_string());
            }

            // Aggregate documentation
            if file.file_type == KnownContextFile::DocsMarkdown {
                let docs = ctx.documentation.get_or_insert_with(String::new);
                if !docs.is_empty() {
                    docs.push_str("\n\n---\n\n");
                }
                docs.push_str(&format!("## {}\n\n{}", file.filename(), file.content));
            }
        }

        ctx
    }

    /// Checks if the context has sufficient information to proceed without exploration.
    ///
    /// This returns `true` when a primary context file (CLAUDE.md or
    /// .quorum/context.md) has been loaded. These files are considered
    /// sufficient to skip the exploration phase during agent execution.
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::{KnownContextFile, LoadedContextFile, ProjectContext};
    ///
    /// // Without primary context
    /// let ctx = ProjectContext::new();
    /// assert!(!ctx.has_sufficient_context());
    ///
    /// // With primary context
    /// let files = vec![LoadedContextFile::new(
    ///     KnownContextFile::ClaudeMdLocal,
    ///     "/CLAUDE.md",
    ///     "# Instructions",
    /// )];
    /// let ctx = ProjectContext::from_files(files);
    /// assert!(ctx.has_sufficient_context());
    /// ```
    pub fn has_sufficient_context(&self) -> bool {
        self.primary_context.is_some()
    }

    /// Checks if the context is completely empty.
    ///
    /// Returns `true` if no context information has been gathered from
    /// any source.
    pub fn is_empty(&self) -> bool {
        self.primary_context.is_none()
            && self.readme.is_none()
            && self.project_type.is_none()
            && self.documentation.is_none()
    }

    /// Gets a description of the context source for logging.
    ///
    /// # Returns
    ///
    /// The relative path of the primary context source, or "none" if
    /// no primary context is available.
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::context::{KnownContextFile, LoadedContextFile, ProjectContext};
    ///
    /// let files = vec![LoadedContextFile::new(
    ///     KnownContextFile::ClaudeMdLocal,
    ///     "/CLAUDE.md",
    ///     "# Instructions",
    /// )];
    /// let ctx = ProjectContext::from_files(files);
    /// assert_eq!(ctx.source_description(), "CLAUDE.md");
    /// ```
    pub fn source_description(&self) -> String {
        match &self.context_source {
            Some(src) => src.to_string(),
            None => "none".to_string(),
        }
    }

    /// Gets all loaded files.
    ///
    /// Returns a reference to the list of files that were used to
    /// build this context.
    pub fn loaded_files(&self) -> &[LoadedContextFile] {
        &self.loaded_files
    }

    /// Converts the context to a summary string suitable for prompts.
    ///
    /// This method formats the context information into a human-readable
    /// string that can be included in prompts to the AI.
    ///
    /// # Content Order
    ///
    /// 1. Project type (if detected)
    /// 2. Primary context (truncated to 2000 chars)
    /// 3. README (truncated to 1000 chars)
    /// 4. Documentation (truncated to 1000 chars)
    ///
    /// # Returns
    ///
    /// A formatted string summarizing the context, or "No context available."
    /// if the context is empty.
    pub fn to_summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(pt) = &self.project_type {
            parts.push(format!("Project Type: {}", pt));
        }

        if let Some(ctx) = &self.primary_context {
            parts.push(format!("Primary Context:\n{}", truncate(ctx, 2000)));
        }

        if let Some(readme) = &self.readme {
            parts.push(format!("README:\n{}", truncate(readme, 1000)));
        }

        if let Some(docs) = &self.documentation {
            parts.push(format!("Documentation:\n{}", truncate(docs, 1000)));
        }

        if parts.is_empty() {
            "No context available.".to_string()
        } else {
            parts.join("\n\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_context_from_files() {
        let files = vec![
            LoadedContextFile::new(
                KnownContextFile::ClaudeMdLocal,
                "/project/CLAUDE.md",
                "# Project Instructions\nThis is a Rust project.",
            ),
            LoadedContextFile::new(
                KnownContextFile::CargoToml,
                "/project/Cargo.toml",
                "[package]\nname = \"test\"",
            ),
            LoadedContextFile::new(
                KnownContextFile::ReadmeMd,
                "/project/README.md",
                "# My Project\nA great project.",
            ),
        ];

        let ctx = ProjectContext::from_files(files);

        assert!(ctx.has_sufficient_context());
        assert!(ctx.primary_context.is_some());
        assert_eq!(ctx.context_source, Some(KnownContextFile::ClaudeMdLocal));
        assert_eq!(ctx.project_type, Some("rust".to_string()));
        assert!(ctx.readme.is_some());
    }

    #[test]
    fn test_project_context_empty() {
        let ctx = ProjectContext::new();
        assert!(!ctx.has_sufficient_context());
        assert!(ctx.is_empty());
    }

    #[test]
    fn test_project_context_priority() {
        // QuorumContext should have higher priority than ClaudeMdLocal
        let files = vec![
            LoadedContextFile::new(
                KnownContextFile::ClaudeMdLocal,
                "/project/CLAUDE.md",
                "Local context",
            ),
            LoadedContextFile::new(
                KnownContextFile::QuorumContext,
                "/project/.quorum/context.md",
                "Quorum context",
            ),
        ];

        let ctx = ProjectContext::from_files(files);

        assert_eq!(ctx.primary_context, Some("Quorum context".to_string()));
        assert_eq!(ctx.context_source, Some(KnownContextFile::QuorumContext));
    }
}
