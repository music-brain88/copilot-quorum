//! Local file system context loader
//!
//! This module provides the [`LocalContextLoader`] implementation of
//! [`ContextLoaderPort`] that reads context files from the local file system.
//!
//! # Overview
//!
//! The local context loader handles:
//!
//! - Reading known context files from project directories
//! - Loading global Claude configuration from the home directory
//! - Recursively loading documentation from the docs/ directory
//! - Writing generated context files to `.quorum/context.md`
//!
//! # File Detection
//!
//! The loader checks for files in this priority order:
//!
//! 1. `.quorum/context.md` - Generated quorum context
//! 2. `CLAUDE.md` - Local project instructions
//! 3. `~/.claude/CLAUDE.md` - Global Claude configuration
//! 4. `README.md` - Project readme
//! 5. `docs/**/*.md` - All markdown in docs/ directory
//! 6. `Cargo.toml`, `package.json`, `pyproject.toml` - Build configs
//!
//! # Example
//!
//! ```ignore
//! use quorum_infrastructure::LocalContextLoader;
//! use quorum_application::ContextLoaderPort;
//! use std::path::Path;
//!
//! let loader = LocalContextLoader::new();
//! let project_root = Path::new("/path/to/project");
//!
//! // Load all known files
//! let files = loader.load_known_files(project_root);
//! println!("Loaded {} context files", files.len());
//!
//! // Build project context
//! let context = loader.build_project_context(files);
//! if context.has_sufficient_context() {
//!     println!("Primary context from: {}", context.source_description());
//! }
//! ```

use quorum_application::ContextLoaderPort;
use quorum_domain::{KnownContextFile, LoadedContextFile};
use std::fs;
use std::path::Path;
use tracing::{debug, warn};

/// Context loader that reads from the local file system.
///
/// This struct implements [`ContextLoaderPort`] by reading files from
/// the local file system. It handles various context file types and
/// supports both project-local and global configuration files.
///
/// # Thread Safety
///
/// `LocalContextLoader` is `Send + Sync` and can be safely shared
/// across threads and used in async contexts.
///
/// # Examples
///
/// ```
/// use quorum_infrastructure::LocalContextLoader;
///
/// let loader = LocalContextLoader::new();
/// ```
#[derive(Debug, Clone, Default)]
pub struct LocalContextLoader;

impl LocalContextLoader {
    /// Creates a new local context loader.
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_infrastructure::LocalContextLoader;
    ///
    /// let loader = LocalContextLoader::new();
    /// ```
    pub fn new() -> Self {
        Self
    }

    /// Attempts to load a single known file type.
    ///
    /// Handles special cases like global CLAUDE.md and docs/ directory,
    /// then delegates to the appropriate loading method.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The project root directory
    /// * `file_type` - The type of file to load
    ///
    /// # Returns
    ///
    /// `Some(LoadedContextFile)` if the file exists and was loaded,
    /// `None` otherwise.
    fn try_load_file(
        &self,
        project_root: &Path,
        file_type: KnownContextFile,
    ) -> Option<LoadedContextFile> {
        match file_type {
            KnownContextFile::DocsMarkdown => self.load_docs_markdown(project_root),
            KnownContextFile::ClaudeMdGlobal => self.load_global_claude_md(),
            _ => {
                let path = project_root.join(file_type.relative_path());
                self.load_single_file(file_type, &path)
            }
        }
    }

    /// Loads a single file from a path.
    ///
    /// Reads the file content and creates a `LoadedContextFile` if
    /// the file exists and is non-empty.
    ///
    /// # Arguments
    ///
    /// * `file_type` - The type of file being loaded
    /// * `path` - The full path to the file
    ///
    /// # Returns
    ///
    /// `Some(LoadedContextFile)` if successful, `None` if:
    /// - The file doesn't exist
    /// - The file is not a regular file
    /// - The file is empty
    /// - Reading the file fails
    fn load_single_file(
        &self,
        file_type: KnownContextFile,
        path: &Path,
    ) -> Option<LoadedContextFile> {
        if path.exists() && path.is_file() {
            match fs::read_to_string(path) {
                Ok(content) => {
                    if content.trim().is_empty() {
                        debug!("Skipping empty file: {:?}", path);
                        None
                    } else {
                        debug!("Loaded context file: {:?}", path);
                        Some(LoadedContextFile::new(
                            file_type,
                            path.to_string_lossy(),
                            content,
                        ))
                    }
                }
                Err(e) => {
                    warn!("Failed to read file {:?}: {}", path, e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Loads the global CLAUDE.md from the user's home directory.
    ///
    /// Looks for `~/.claude/CLAUDE.md` and loads it if present.
    ///
    /// # Returns
    ///
    /// `Some(LoadedContextFile)` if the global config exists and was loaded,
    /// `None` otherwise.
    fn load_global_claude_md(&self) -> Option<LoadedContextFile> {
        let home = dirs::home_dir()?;
        let path = home.join(".claude/CLAUDE.md");
        self.load_single_file(KnownContextFile::ClaudeMdGlobal, &path)
    }

    /// Loads all markdown files from the docs/ directory.
    ///
    /// Recursively walks the `docs/` directory and aggregates all
    /// markdown files into a single `LoadedContextFile`. Each file's
    /// content is prefixed with its relative path.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The project root directory
    ///
    /// # Returns
    ///
    /// `Some(LoadedContextFile)` containing all documentation if any
    /// markdown files were found, `None` otherwise.
    fn load_docs_markdown(&self, project_root: &Path) -> Option<LoadedContextFile> {
        let docs_dir = project_root.join("docs");
        if !docs_dir.exists() || !docs_dir.is_dir() {
            return None;
        }

        let mut all_content = String::new();
        let mut loaded_count = 0;

        // Walk the docs directory recursively
        if let Ok(entries) = walkdir(docs_dir) {
            for entry in entries {
                if let Some(ext) = entry.extension() {
                    if ext == "md" {
                        if let Ok(content) = fs::read_to_string(&entry) {
                            if !content.trim().is_empty() {
                                if !all_content.is_empty() {
                                    all_content.push_str("\n\n---\n\n");
                                }
                                let relative = entry.strip_prefix(project_root).unwrap_or(&entry);
                                all_content.push_str(&format!(
                                    "## {}\n\n{}",
                                    relative.display(),
                                    content
                                ));
                                loaded_count += 1;
                            }
                        }
                    }
                }
            }
        }

        if loaded_count > 0 {
            debug!("Loaded {} docs markdown files", loaded_count);
            Some(LoadedContextFile::new(
                KnownContextFile::DocsMarkdown,
                format!("{}/docs/**/*.md", project_root.display()),
                all_content,
            ))
        } else {
            None
        }
    }
}

/// Recursively walks a directory and returns all file paths.
///
/// # Arguments
///
/// * `dir` - The directory to walk
///
/// # Returns
///
/// A list of all file paths found, or an error if the directory
/// can't be read.
fn walkdir(dir: std::path::PathBuf) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    walkdir_recursive(&dir, &mut files)?;
    Ok(files)
}

/// Recursive helper for directory walking.
///
/// # Arguments
///
/// * `dir` - Current directory being walked
/// * `files` - Accumulator for found files
fn walkdir_recursive(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walkdir_recursive(&path, files)?;
            } else {
                files.push(path);
            }
        }
    }
    Ok(())
}

impl ContextLoaderPort for LocalContextLoader {
    /// Loads all known context files from the project root.
    ///
    /// Iterates through all [`KnownContextFile`] variants and attempts
    /// to load each one. Successfully loaded files are returned in
    /// priority order.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The root directory of the project
    ///
    /// # Returns
    ///
    /// A list of loaded context files, sorted by priority (highest first).
    fn load_known_files(&self, project_root: &Path) -> Vec<LoadedContextFile> {
        let mut files = Vec::new();

        for file_type in KnownContextFile::all() {
            if let Some(loaded) = self.try_load_file(project_root, *file_type) {
                files.push(loaded);
            }
        }

        // Sort by priority
        files.sort_by_key(|f| f.file_type.priority());

        debug!(
            "Loaded {} context files from {:?}",
            files.len(),
            project_root
        );
        files
    }

    /// Checks if the quorum context file exists.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The root directory of the project
    ///
    /// # Returns
    ///
    /// `true` if `.quorum/context.md` exists and is a file.
    fn context_file_exists(&self, project_root: &Path) -> bool {
        let path = self.context_file_path(project_root);
        path.exists() && path.is_file()
    }

    /// Writes the generated context file.
    ///
    /// Creates the `.quorum` directory if it doesn't exist, then
    /// writes the content to `context.md`.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The root directory of the project
    /// * `content` - The content to write
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an I/O error on failure.
    fn write_context_file(&self, project_root: &Path, content: &str) -> std::io::Result<()> {
        let path = self.context_file_path(project_root);

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&path, content)?;
        debug!("Wrote context file: {:?}", path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_load_known_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create some test files
        fs::write(root.join("CLAUDE.md"), "# Test Project").unwrap();
        fs::write(root.join("README.md"), "# README").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let loader = LocalContextLoader::new();
        let files = loader.load_known_files(root);

        assert!(files.len() >= 3);
        assert!(files
            .iter()
            .any(|f| f.file_type == KnownContextFile::ClaudeMdLocal));
        assert!(files
            .iter()
            .any(|f| f.file_type == KnownContextFile::ReadmeMd));
        assert!(files
            .iter()
            .any(|f| f.file_type == KnownContextFile::CargoToml));
    }

    #[test]
    fn test_context_file_operations() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let loader = LocalContextLoader::new();

        // Initially doesn't exist
        assert!(!loader.context_file_exists(root));

        // Write context file
        loader
            .write_context_file(root, "# Generated Context")
            .unwrap();

        // Now exists
        assert!(loader.context_file_exists(root));

        // Can be loaded
        let files = loader.load_known_files(root);
        assert!(files
            .iter()
            .any(|f| f.file_type == KnownContextFile::QuorumContext));
    }

    #[test]
    fn test_empty_file_skipped() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create an empty file
        fs::write(root.join("CLAUDE.md"), "").unwrap();

        let loader = LocalContextLoader::new();
        let files = loader.load_known_files(root);

        // Empty file should be skipped
        assert!(files
            .iter()
            .all(|f| f.file_type != KnownContextFile::ClaudeMdLocal));
    }

    #[test]
    fn test_build_project_context() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("CLAUDE.md"), "# Instructions").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]").unwrap();

        let loader = LocalContextLoader::new();
        let files = loader.load_known_files(root);
        let ctx = loader.build_project_context(files);

        assert!(ctx.has_sufficient_context());
        assert_eq!(ctx.project_type, Some("rust".to_string()));
    }
}
