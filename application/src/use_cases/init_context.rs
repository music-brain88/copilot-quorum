//! Initialize context use case
//!
//! Uses [`quorum_domain::core::string::truncate`] for UTF-8 safe truncation.
//!
//! This module provides the [`InitContextUseCase`] for generating a project
//! context file using quorum-based analysis. Multiple AI models analyze the
//! project in parallel, and a moderator synthesizes their analyses into a
//! comprehensive context document.
//!
//! # Overview
//!
//! The context initialization process:
//!
//! 1. **Load project files** - Read known files (README, Cargo.toml, etc.)
//! 2. **Parallel analysis** - Query multiple models to analyze the project
//! 3. **Synthesis** - Moderator combines analyses into a unified document
//! 4. **Write output** - Save to `.quorum/context.md`
//!
//! # Usage
//!
//! This use case is typically invoked via the `/init` command in agent mode:
//!
//! ```text
//! agent> /init
//! ```
//!
//! Or programmatically:
//!
//! ```ignore
//! use quorum_application::{InitContextInput, InitContextUseCase};
//!
//! let input = InitContextInput::new("/path/to/project", models)
//!     .with_moderator(moderator_model);
//!
//! let result = use_case.execute(input).await?;
//! println!("Created: {}", result.path);
//! ```
//!
//! # Generated File
//!
//! The generated `.quorum/context.md` file contains:
//!
//! - Project overview
//! - Tech stack information
//! - Architecture description
//! - Key directories
//! - Important concepts
//! - Generation metadata

use crate::ports::context_loader::ContextLoaderPort;
use crate::ports::llm_gateway::{GatewayError, LlmGateway};
use quorum_domain::core::string::truncate;
use quorum_domain::{AgentPromptTemplate, Model};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::{info, warn};

/// Errors that can occur during context initialization.
///
/// These errors represent the various failure modes of the context
/// initialization process.
#[derive(Error, Debug)]
pub enum InitContextError {
    /// The context file already exists and `--force` was not specified.
    ///
    /// The path to the existing file is included in the error message.
    #[error("Context file already exists at {0}")]
    AlreadyExists(String),

    /// No project files were found to analyze.
    ///
    /// This occurs when the project root contains none of the known
    /// context files (README.md, Cargo.toml, etc.).
    #[error("No project files found to analyze")]
    NoFilesFound,

    /// All models failed to analyze the project.
    ///
    /// This occurs when every model in the quorum fails to respond
    /// or returns an error.
    #[error("All models failed to analyze the project")]
    AllModelsFailed,

    /// The synthesis phase failed.
    ///
    /// This occurs when the moderator model fails to combine the
    /// individual analyses into a unified document.
    #[error("Synthesis failed: {0}")]
    SynthesisFailed(String),

    /// Failed to write the context file to disk.
    #[error("Failed to write context file: {0}")]
    WriteError(#[from] std::io::Error),

    /// An error occurred communicating with the LLM gateway.
    #[error("Gateway error: {0}")]
    GatewayError(#[from] GatewayError),
}

/// Input for the InitContext use case.
///
/// Configures the context initialization process, including which
/// models to use and whether to overwrite existing files.
///
/// # Examples
///
/// ```
/// use quorum_domain::Model;
/// use quorum_application::InitContextInput;
///
/// let input = InitContextInput::new("/project", vec![Model::default()])
///     .with_force(true);
/// ```
#[derive(Debug, Clone)]
pub struct InitContextInput {
    /// Project root directory to analyze.
    pub project_root: String,

    /// Models to use for analysis.
    ///
    /// All models will analyze the project in parallel.
    pub models: Vec<Model>,

    /// Model to use for synthesis (moderator).
    ///
    /// This model combines the individual analyses into a unified document.
    pub moderator: Model,

    /// Force overwrite if context file exists.
    ///
    /// When `true`, an existing `.quorum/context.md` will be overwritten.
    pub force: bool,
}

impl InitContextInput {
    /// Creates a new input with the given project root and models.
    ///
    /// The first model is used as the default moderator.
    ///
    /// # Arguments
    ///
    /// * `project_root` - Path to the project root directory
    /// * `models` - List of models to use for analysis
    ///
    /// # Examples
    ///
    /// ```
    /// use quorum_domain::Model;
    /// use quorum_application::InitContextInput;
    ///
    /// let input = InitContextInput::new("/project", Model::default_models());
    /// ```
    pub fn new(project_root: impl Into<String>, models: Vec<Model>) -> Self {
        let moderator = models.first().cloned().unwrap_or_default();
        Self {
            project_root: project_root.into(),
            models,
            moderator,
            force: false,
        }
    }

    /// Sets the moderator model for synthesis.
    ///
    /// # Arguments
    ///
    /// * `moderator` - The model to use for combining analyses
    pub fn with_moderator(mut self, moderator: Model) -> Self {
        self.moderator = moderator;
        self
    }

    /// Enables or disables force overwrite.
    ///
    /// # Arguments
    ///
    /// * `force` - When `true`, existing context files will be overwritten
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
}

/// Output from the InitContext use case.
///
/// Contains the results of a successful context initialization,
/// including the path to the created file and its content.
#[derive(Debug, Clone)]
pub struct InitContextOutput {
    /// Path to the created context file.
    pub path: String,

    /// Generated content of the context file.
    pub content: String,

    /// Names of models that contributed to the analysis.
    ///
    /// Only includes models that successfully completed their analysis.
    pub contributing_models: Vec<String>,
}

/// Progress notifier for context initialization.
///
/// Implement this trait to receive callbacks during the initialization
/// process. This is useful for displaying progress in a UI.
///
/// # Default Implementation
///
/// All methods have empty default implementations, so you only need
/// to implement the callbacks you're interested in.
pub trait InitContextProgressNotifier: Send + Sync {
    /// Called when starting to load project files.
    fn on_loading_files(&self) {}

    /// Called when starting the analysis phase.
    ///
    /// # Arguments
    ///
    /// * `model_count` - Number of models that will analyze the project
    fn on_analysis_start(&self, _model_count: usize) {}

    /// Called when a model completes its analysis.
    ///
    /// # Arguments
    ///
    /// * `model` - The model that completed
    fn on_model_complete(&self, _model: &Model) {}

    /// Called when starting the synthesis phase.
    fn on_synthesis_start(&self) {}

    /// Called when the context file has been created.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the created file
    fn on_complete(&self, _path: &str) {}
}

/// No-op implementation of progress notifier.
///
/// Used when progress notifications are not needed.
pub struct NoInitContextProgress;
impl InitContextProgressNotifier for NoInitContextProgress {}

/// Use case for initializing project context using quorum.
///
/// This use case orchestrates the context initialization process:
///
/// 1. Verifies the context file doesn't already exist (unless `force` is set)
/// 2. Loads all known project files
/// 3. Queries multiple models in parallel for project analysis
/// 4. Has a moderator model synthesize the analyses
/// 5. Writes the result to `.quorum/context.md`
///
/// # Type Parameters
///
/// * `G` - The LLM gateway type
/// * `C` - The context loader type
///
/// # Examples
///
/// ```ignore
/// use quorum_application::{InitContextInput, InitContextUseCase};
/// use std::sync::Arc;
///
/// let use_case = InitContextUseCase::new(gateway, context_loader);
/// let input = InitContextInput::new("/project", models);
///
/// let output = use_case.execute(input).await?;
/// println!("Generated context file at: {}", output.path);
/// ```
pub struct InitContextUseCase {
    gateway: Arc<dyn LlmGateway>,
    context_loader: Arc<dyn ContextLoaderPort>,
}

impl InitContextUseCase {
    /// Creates a new InitContextUseCase.
    ///
    /// # Arguments
    ///
    /// * `gateway` - LLM gateway for model communication
    /// * `context_loader` - Context loader for file operations
    pub fn new(gateway: Arc<dyn LlmGateway>, context_loader: Arc<dyn ContextLoaderPort>) -> Self {
        Self {
            gateway,
            context_loader,
        }
    }

    /// Executes the context initialization without progress reporting.
    ///
    /// This is a convenience method that uses [`NoInitContextProgress`].
    ///
    /// # Arguments
    ///
    /// * `input` - Configuration for the initialization
    ///
    /// # Returns
    ///
    /// The initialization output on success, or an error.
    ///
    /// # Errors
    ///
    /// See [`InitContextError`] for possible error conditions.
    pub async fn execute(
        &self,
        input: InitContextInput,
    ) -> Result<InitContextOutput, InitContextError> {
        self.execute_with_progress(input, &NoInitContextProgress)
            .await
    }

    /// Executes the context initialization with progress notifications.
    ///
    /// # Arguments
    ///
    /// * `input` - Configuration for the initialization
    /// * `progress` - Progress notifier for callbacks
    ///
    /// # Returns
    ///
    /// The initialization output on success, or an error.
    ///
    /// # Errors
    ///
    /// - [`InitContextError::AlreadyExists`] - Context file exists and `force` is false
    /// - [`InitContextError::NoFilesFound`] - No project files to analyze
    /// - [`InitContextError::AllModelsFailed`] - All models failed to respond
    /// - [`InitContextError::SynthesisFailed`] - Moderator failed to synthesize
    /// - [`InitContextError::WriteError`] - Could not write context file
    /// - [`InitContextError::GatewayError`] - LLM communication error
    pub async fn execute_with_progress(
        &self,
        input: InitContextInput,
        progress: &dyn InitContextProgressNotifier,
    ) -> Result<InitContextOutput, InitContextError> {
        let project_root = Path::new(&input.project_root);

        // Check if context file already exists
        if !input.force && self.context_loader.context_file_exists(project_root) {
            let path = self.context_loader.context_file_path(project_root);
            return Err(InitContextError::AlreadyExists(
                path.to_string_lossy().to_string(),
            ));
        }

        // Load project files
        progress.on_loading_files();
        let files = self.context_loader.load_known_files(project_root);

        if files.is_empty() {
            return Err(InitContextError::NoFilesFound);
        }

        // Build the project files summary for analysis
        let project_files_text = files
            .iter()
            .map(|f| {
                format!(
                    "### {}\n```\n{}\n```",
                    f.filename(),
                    truncate(&f.content, 2000)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Query all models in parallel for analysis
        info!(
            "Starting project analysis with {} models",
            input.models.len()
        );
        progress.on_analysis_start(input.models.len());

        let analysis_prompt = AgentPromptTemplate::context_analysis(&project_files_text);
        let mut join_set = JoinSet::new();

        for model in input.models.iter() {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let prompt = analysis_prompt.clone();

            join_set.spawn(async move {
                let result = Self::query_model(gateway.as_ref(), &model, &prompt).await;
                (model, result)
            });
        }

        // Collect analyses
        let mut analyses = Vec::new();
        let mut contributing_models = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((model, Ok(analysis))) => {
                    info!("Model {} completed analysis", model);
                    progress.on_model_complete(&model);
                    contributing_models.push(model.to_string());
                    analyses.push((model.to_string(), analysis));
                }
                Ok((model, Err(e))) => {
                    warn!("Model {} failed to analyze: {}", model, e);
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        if analyses.is_empty() {
            return Err(InitContextError::AllModelsFailed);
        }

        // Synthesize the analyses using the moderator
        progress.on_synthesis_start();
        info!(
            "Synthesizing {} analyses with moderator {}",
            analyses.len(),
            input.moderator
        );

        let date = chrono_lite_date();
        let synthesis_prompt = AgentPromptTemplate::context_synthesis(&analyses, &date);
        let system_prompt = AgentPromptTemplate::context_synthesis_system();

        let session = self
            .gateway
            .create_session_with_system_prompt(&input.moderator, system_prompt)
            .await?;

        let content = session
            .send(&synthesis_prompt)
            .await
            .map_err(|e| InitContextError::SynthesisFailed(e.to_string()))?;

        // Write the context file
        self.context_loader
            .write_context_file(project_root, &content)?;

        let path = self
            .context_loader
            .context_file_path(project_root)
            .to_string_lossy()
            .to_string();

        progress.on_complete(&path);

        Ok(InitContextOutput {
            path,
            content,
            contributing_models,
        })
    }

    /// Queries a single model for project analysis.
    ///
    /// # Arguments
    ///
    /// * `gateway` - The LLM gateway
    /// * `model` - The model to query
    /// * `prompt` - The analysis prompt
    ///
    /// # Returns
    ///
    /// The model's analysis response, or an error.
    async fn query_model(
        gateway: &dyn LlmGateway,
        model: &Model,
        prompt: &str,
    ) -> Result<String, GatewayError> {
        let session = gateway.create_session(model).await?;
        session.send(prompt).await
    }
}

/// Gets the current date as a string (YYYY-MM-DD format).
///
/// Uses a simplified calculation that's accurate enough for display purposes.
fn chrono_lite_date() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Simple date calculation (approximate, good enough for display)
    let days = secs / 86400;
    let years = days / 365;
    let remaining_days = days % 365;
    let months = remaining_days / 30;
    let day = remaining_days % 30 + 1;

    format!("{}-{:02}-{:02}", 1970 + years, months + 1, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_context_input() {
        let input = InitContextInput::new("/project", vec![Model::default()])
            .with_moderator(Model::default())
            .with_force(true);

        assert_eq!(input.project_root, "/project");
        assert!(input.force);
    }
}
