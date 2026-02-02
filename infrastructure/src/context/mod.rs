//! Context loading infrastructure
//!
//! This module provides implementations for loading project context
//! from the file system. It implements the [`ContextLoaderPort`] trait
//! defined in the application layer.
//!
//! # Components
//!
//! - [`LocalContextLoader`] - Reads context files from the local file system
//!
//! # Usage
//!
//! ```
//! use quorum_infrastructure::LocalContextLoader;
//! use quorum_application::ContextLoaderPort;
//! use std::path::Path;
//!
//! let loader = LocalContextLoader::new();
//!
//! // The loader can be used to check for existing context
//! let project_root = Path::new(".");
//! if loader.context_file_exists(project_root) {
//!     println!(".quorum/context.md exists!");
//! }
//! ```
//!
//! [`ContextLoaderPort`]: quorum_application::ContextLoaderPort

mod loader;

pub use loader::LocalContextLoader;
