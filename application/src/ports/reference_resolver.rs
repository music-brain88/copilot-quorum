//! Reference resolver port.
//!
//! Defines the interface for resolving resource references (GitHub Issues, PRs)
//! to their content. Infrastructure adapters implement this to provide actual
//! resolution (e.g., via `gh` CLI).

use async_trait::async_trait;
use quorum_domain::ResourceReference;

/// A resolved reference with its content.
#[derive(Debug, Clone)]
pub struct ResolvedReference {
    /// The original reference that was resolved
    pub reference: ResourceReference,
    /// Title of the resource (e.g., issue title)
    pub title: String,
    /// Content/body of the resource
    pub content: String,
}

/// Errors that can occur during reference resolution.
#[derive(Debug)]
pub enum ReferenceError {
    /// The reference type is not supported (e.g., Discussions)
    Unsupported(String),
    /// The resolver is not available (e.g., `gh` CLI not authenticated)
    NotAvailable(String),
    /// Resolution failed for some other reason
    ResolutionFailed(String),
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReferenceError::Unsupported(msg) => write!(f, "Unsupported: {}", msg),
            ReferenceError::NotAvailable(msg) => write!(f, "Not available: {}", msg),
            ReferenceError::ResolutionFailed(msg) => write!(f, "Resolution failed: {}", msg),
        }
    }
}

/// Port for resolving resource references to their content.
///
/// Infrastructure adapters implement this trait to provide actual resolution
/// (e.g., GitHub Issues via `gh` CLI).
#[async_trait]
pub trait ReferenceResolverPort: Send + Sync {
    /// Resolve a single reference to its content.
    async fn resolve(
        &self,
        reference: &ResourceReference,
    ) -> Result<ResolvedReference, ReferenceError>;

    /// Resolve multiple references concurrently, skipping errors.
    ///
    /// Default implementation uses `futures::future::join_all` for parallel resolution.
    async fn resolve_all(&self, references: &[ResourceReference]) -> Vec<ResolvedReference> {
        use futures::future::join_all;

        let futures: Vec<_> = references.iter().map(|r| self.resolve(r)).collect();
        let results = join_all(futures).await;

        results
            .into_iter()
            .zip(references.iter())
            .filter_map(|(result, reference)| match result {
                Ok(resolved) => Some(resolved),
                Err(e) => {
                    tracing::debug!("Skipping {}: {}", reference, e);
                    None
                }
            })
            .collect()
    }
}
