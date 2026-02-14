//! GitHub reference resolver using the `gh` CLI.
//!
//! Resolves GitHub Issue and Pull Request references by invoking
//! `gh issue view` (which handles both Issues and PRs).

use async_trait::async_trait;
use quorum_application::ports::reference_resolver::{
    ReferenceError, ReferenceResolverPort, ResolvedReference,
};
use quorum_domain::ResourceReference;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Resolves GitHub references using the `gh` CLI.
///
/// Created via `try_new()` which validates that `gh` is installed and authenticated.
/// If either check fails, `try_new()` returns `None` for graceful degradation.
pub struct GitHubReferenceResolver {
    working_dir: Option<String>,
}

impl GitHubReferenceResolver {
    /// Try to create a new resolver.
    ///
    /// Returns `None` if `gh` CLI is not installed or not authenticated,
    /// allowing graceful degradation (no reference resolution).
    pub async fn try_new(working_dir: Option<String>) -> Option<Self> {
        // Check if `gh` CLI exists
        if which::which("gh").is_err() {
            debug!("gh CLI not found, reference resolution disabled");
            return None;
        }

        // Check if `gh` is authenticated
        let mut cmd = std::process::Command::new("gh");
        cmd.arg("auth").arg("status");
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        if let Some(ref dir) = working_dir {
            cmd.current_dir(dir);
        }
        match cmd.status() {
            Ok(status) if status.success() => {
                info!("GitHub reference resolver initialized");
                Some(Self { working_dir })
            }
            _ => {
                debug!("gh CLI not authenticated, reference resolution disabled");
                None
            }
        }
    }

    /// Build and execute `gh issue view` command.
    ///
    /// `gh issue view` works for both Issues and PRs — GitHub's API treats
    /// them as the same entity at the issue endpoint level.
    async fn gh_issue_view(
        &self,
        number: u64,
        repo: Option<&str>,
    ) -> Result<(String, String), ReferenceError> {
        let mut cmd = Command::new("gh");
        cmd.arg("issue")
            .arg("view")
            .arg(number.to_string())
            .arg("--json")
            .arg("title,body");

        if let Some(repo) = repo {
            cmd.arg("--repo").arg(repo);
        }

        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        let output = cmd.output().await.map_err(|e| {
            ReferenceError::ResolutionFailed(format!("Failed to execute gh: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ReferenceError::ResolutionFailed(format!(
                "gh issue view failed: {}",
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| {
            ReferenceError::ResolutionFailed(format!("Failed to parse gh output: {}", e))
        })?;

        let title = json["title"]
            .as_str()
            .unwrap_or("(no title)")
            .to_string();
        let body = json["body"].as_str().unwrap_or("").to_string();

        Ok((title, body))
    }
}

#[async_trait]
impl ReferenceResolverPort for GitHubReferenceResolver {
    async fn resolve(
        &self,
        reference: &ResourceReference,
    ) -> Result<ResolvedReference, ReferenceError> {
        let (repo, number) = match reference {
            ResourceReference::GitHubIssue { repo, number } => (repo.as_deref(), *number),
            ResourceReference::GitHubPullRequest { repo, number } => (repo.as_deref(), *number),
        };

        debug!("Resolving {} (number={}, repo={:?})", reference, number, repo);

        let (title, body) = self.gh_issue_view(number, repo).await?;

        Ok(ResolvedReference {
            reference: reference.clone(),
            title,
            content: body,
        })
    }

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
                    warn!("Skipping {}: {}", reference, e);
                    None
                }
            })
            .collect()
    }
}
