//! Context module for project context management
//!
//! This module provides types and utilities for gathering and managing
//! project context from known files like CLAUDE.md, README.md, and build
//! configuration files.
//!
//! # Overview
//!
//! The context system is designed to help AI agents understand a project
//! by loading and aggregating information from various sources:
//!
//! - **Primary context**: CLAUDE.md, .quorum/context.md
//! - **Documentation**: README.md, docs/**/*.md
//! - **Project metadata**: Cargo.toml, package.json, pyproject.toml
//!
//! # Key Types
//!
//! - [`KnownContextFile`] - Enum of recognized context file types
//! - [`LoadedContextFile`] - A file that has been loaded with its content
//! - [`ProjectContext`] - Aggregated context from multiple sources
//!
//! # Context Priority
//!
//! Files are prioritized to determine which content to use as the
//! primary context:
//!
//! 1. `.quorum/context.md` - Highest priority (generated)
//! 2. `CLAUDE.md` - Local project instructions
//! 3. `~/.claude/CLAUDE.md` - Global configuration
//! 4. Other files provide supplementary information
//!
//! # Example
//!
//! ```
//! use quorum_domain::context::{KnownContextFile, LoadedContextFile, ProjectContext};
//!
//! // Load some context files
//! let files = vec![
//!     LoadedContextFile::new(
//!         KnownContextFile::ClaudeMdLocal,
//!         "/project/CLAUDE.md",
//!         "# Instructions\nThis is a Rust project.",
//!     ),
//!     LoadedContextFile::new(
//!         KnownContextFile::CargoToml,
//!         "/project/Cargo.toml",
//!         "[package]\nname = \"my-crate\"",
//!     ),
//! ];
//!
//! // Build aggregated context
//! let ctx = ProjectContext::from_files(files);
//!
//! assert!(ctx.has_sufficient_context());
//! assert_eq!(ctx.project_type, Some("rust".to_string()));
//! assert_eq!(ctx.source_description(), "CLAUDE.md");
//! ```

pub mod entities;
pub mod value_objects;

pub use entities::ProjectContext;
pub use value_objects::{KnownContextFile, LoadedContextFile};
