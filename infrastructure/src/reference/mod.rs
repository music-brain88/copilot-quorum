//! Reference resolution adapters.
//!
//! Provides infrastructure implementations for resolving resource references
//! (GitHub Issues, PRs) to their content.

mod github;

pub use github::GitHubReferenceResolver;
