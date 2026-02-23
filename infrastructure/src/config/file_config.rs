//! Raw TOML configuration data types
//!
//! These structs represent the exact structure of the TOML config file.
//! They are deserialized directly and use domain types where appropriate.

use quorum_domain::ContextBudget;
use quorum_domain::agent::validation::{ConfigIssue, ConfigIssueCode, Severity};
use quorum_domain::{ConsensusLevel, HilMode, Model, OutputFormat, PhaseScope, QuorumRule};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export OutputFormat from domain for convenience
pub use quorum_domain::OutputFormat as FileOutputFormat;

/// Role-based model configuration from TOML
///
/// # Example
///
/// ```toml
/// [models]
/// exploration = "gpt-5.2-codex"           # Context gathering + low-risk tools
/// decision = "claude-sonnet-4.5"          # Planning + high-risk tools
/// review = ["claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
/// participants = ["claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
/// moderator = "claude-opus-4.5"           # Quorum Synthesis
/// ask = "claude-sonnet-4.5"               # Ask (Q&A) interaction
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileModelsConfig {
    /// Model for exploration: context gathering + low-risk tools
    pub exploration: Option<String>,
    /// Model for decisions: planning + high-risk tools
    pub decision: Option<String>,
    /// Models for review phases
    pub review: Option<Vec<String>>,
    /// Models for Quorum Discussion participants
    pub participants: Option<Vec<String>>,
    /// Model for Quorum Synthesis (moderator)
    pub moderator: Option<String>,
    /// Model for Ask (Q&A) interaction
    pub ask: Option<String>,
}

impl FileModelsConfig {
    /// Parse a single model string, collecting issues for empty names.
    fn parse_single_model(
        field: &str,
        value: Option<&String>,
    ) -> (Option<Model>, Vec<ConfigIssue>) {
        let mut issues = Vec::new();
        match value {
            None => (None, issues),
            Some(s) if s.trim().is_empty() => {
                issues.push(ConfigIssue {
                    severity: Severity::Error,
                    code: ConfigIssueCode::EmptyModelName {
                        field: field.to_string(),
                    },
                    message: format!("models.{}: model name cannot be empty", field),
                });
                (None, issues)
            }
            Some(s) => {
                // Model::from_str is infallible; unknown names become Custom(...)
                let model: Model = s.parse().unwrap();
                (Some(model), issues)
            }
        }
    }

    /// Parse a model list, collecting issues for empty names.
    fn parse_model_list(
        field: &str,
        values: Option<&Vec<String>>,
    ) -> (Option<Vec<Model>>, Vec<ConfigIssue>) {
        let mut issues = Vec::new();
        match values {
            None => (None, issues),
            Some(strings) => {
                let mut models = Vec::new();
                for s in strings {
                    if s.trim().is_empty() {
                        issues.push(ConfigIssue {
                            severity: Severity::Error,
                            code: ConfigIssueCode::EmptyModelName {
                                field: field.to_string(),
                            },
                            message: format!(
                                "models.{}: model name cannot be empty in list",
                                field
                            ),
                        });
                    } else {
                        let model: Model = s.parse().unwrap();
                        models.push(model);
                    }
                }
                (Some(models), issues)
            }
        }
    }

    /// Parse exploration model string into Model enum
    pub fn parse_exploration(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("exploration", self.exploration.as_ref())
    }

    /// Parse decision model string into Model enum
    pub fn parse_decision(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("decision", self.decision.as_ref())
    }

    /// Parse review model strings into `Vec<Model>`
    pub fn parse_review(&self) -> (Option<Vec<Model>>, Vec<ConfigIssue>) {
        Self::parse_model_list("review", self.review.as_ref())
    }

    /// Parse participants model strings into `Vec<Model>`
    pub fn parse_participants(&self) -> (Option<Vec<Model>>, Vec<ConfigIssue>) {
        Self::parse_model_list("participants", self.participants.as_ref())
    }

    /// Parse moderator model string into Model enum
    pub fn parse_moderator(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("moderator", self.moderator.as_ref())
    }

    /// Parse ask model string into Model enum
    pub fn parse_ask(&self) -> (Option<Model>, Vec<ConfigIssue>) {
        Self::parse_single_model("ask", self.ask.as_ref())
    }
}

/// Raw output configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileOutputConfig {
    /// Output format (uses domain type)
    pub format: Option<OutputFormat>,
    /// Enable colored terminal output
    pub color: bool,
}

impl Default for FileOutputConfig {
    fn default() -> Self {
        Self {
            format: None,
            color: true,
        }
    }
}

/// Raw REPL configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileReplConfig {
    /// Show progress indicators
    pub show_progress: bool,
    /// Path to history file
    pub history_file: Option<String>,
}

impl Default for FileReplConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            history_file: None,
        }
    }
}

/// Raw agent configuration from TOML
///
/// # Example
///
/// ```toml
/// [agent]
/// consensus_level = "solo"                 # "solo" or "ensemble"
/// phase_scope = "full"                     # "full", "fast", "plan-only"
/// strategy = "quorum"                      # "quorum" or "debate"
/// hil_mode = "interactive"                 # "interactive", "auto_reject", "auto_approve"
/// max_plan_revisions = 3
/// ```
///
/// Model settings are in `[models]` section, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileAgentConfig {
    /// Maximum plan revisions before human intervention
    pub max_plan_revisions: usize,
    /// Human-in-the-loop mode (interactive, auto_reject, auto_approve)
    pub hil_mode: String,
    /// Consensus level: "solo" or "ensemble"
    pub consensus_level: String,
    /// Phase scope: "full", "fast", "plan-only"
    pub phase_scope: String,
    /// Orchestration strategy: "quorum" or "debate"
    pub strategy: String,
}

impl Default for FileAgentConfig {
    fn default() -> Self {
        Self {
            max_plan_revisions: 3,
            hil_mode: "interactive".to_string(),
            consensus_level: "solo".to_string(),
            phase_scope: "full".to_string(),
            strategy: "quorum".to_string(),
        }
    }
}

impl FileAgentConfig {
    /// Parse hil_mode string into HilMode enum, returning warnings on failure.
    pub fn parse_hil_mode(&self) -> (HilMode, Vec<ConfigIssue>) {
        match self.hil_mode.parse::<HilMode>() {
            Ok(mode) => (mode, vec![]),
            Err(_) => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.hil_mode".to_string(),
                        value: self.hil_mode.clone(),
                        valid_values: vec![
                            "interactive".to_string(),
                            "auto_reject".to_string(),
                            "auto_approve".to_string(),
                        ],
                    },
                    message: format!(
                        "agent.hil_mode: unknown value '{}', falling back to 'interactive'",
                        self.hil_mode
                    ),
                };
                (HilMode::default(), vec![issue])
            }
        }
    }

    /// Parse consensus_level string into ConsensusLevel enum
    ///
    /// Accepts: "solo", "s", "ensemble", "ens", "e"
    pub fn parse_consensus_level(&self) -> (ConsensusLevel, Vec<ConfigIssue>) {
        match self.consensus_level.parse::<ConsensusLevel>() {
            Ok(level) => (level, vec![]),
            Err(_) => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.consensus_level".to_string(),
                        value: self.consensus_level.clone(),
                        valid_values: vec!["solo".to_string(), "ensemble".to_string()],
                    },
                    message: format!(
                        "agent.consensus_level: unknown value '{}', falling back to 'solo'",
                        self.consensus_level
                    ),
                };
                (ConsensusLevel::default(), vec![issue])
            }
        }
    }

    /// Parse phase_scope string into PhaseScope enum
    ///
    /// Accepts: "full", "fast", "plan-only", "plan"
    pub fn parse_phase_scope(&self) -> (PhaseScope, Vec<ConfigIssue>) {
        match self.phase_scope.parse::<PhaseScope>() {
            Ok(scope) => (scope, vec![]),
            Err(_) => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.phase_scope".to_string(),
                        value: self.phase_scope.clone(),
                        valid_values: vec![
                            "full".to_string(),
                            "fast".to_string(),
                            "plan-only".to_string(),
                        ],
                    },
                    message: format!(
                        "agent.phase_scope: unknown value '{}', falling back to 'full'",
                        self.phase_scope
                    ),
                };
                (PhaseScope::default(), vec![issue])
            }
        }
    }

    /// Parse strategy string into strategy name, returning warnings on failure.
    ///
    /// Returns "quorum" or "debate". Used by CLI to configure OrchestrationStrategy.
    pub fn parse_strategy(&self) -> (&str, Vec<ConfigIssue>) {
        match self.strategy.to_lowercase().as_str() {
            "quorum" => ("quorum", vec![]),
            "debate" => ("debate", vec![]),
            _ => {
                let issue = ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "agent.strategy".to_string(),
                        value: self.strategy.clone(),
                        valid_values: vec!["quorum".to_string(), "debate".to_string()],
                    },
                    message: format!(
                        "agent.strategy: unknown value '{}', falling back to 'quorum'",
                        self.strategy
                    ),
                };
                ("quorum", vec![issue])
            }
        }
    }
}

// ==================== Quorum Configuration ====================
//
// The quorum configuration controls how multi-model consensus works.
// This is the core of the Quorum system - inspired by distributed systems
// where consensus ensures reliability.
//
// Example configuration:
//
// ```toml
// [quorum]
// rule = "majority"
// min_models = 2
//
// [quorum.discussion]
// models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro"]
// moderator = "claude-opus-4.5"
// enable_peer_review = true
// ```

/// Quorum consensus configuration
///
/// Controls how Quorum Consensus (voting for approval) works.
///
/// # Example
///
/// ```toml
/// [quorum]
/// rule = "majority"           # or "unanimous", "atleast:2", "75%"
/// min_models = 2              # minimum models required for valid consensus
/// moderator = "claude-opus-4.5"
/// enable_peer_review = true
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileQuorumConfig {
    /// Consensus rule: "majority", "unanimous", "atleast:N", "N%"
    pub rule: String,
    /// Minimum number of models required for valid consensus
    pub min_models: usize,
    /// Moderator model for synthesis
    pub moderator: Option<String>,
    /// Enable peer review phase
    pub enable_peer_review: bool,
}

impl Default for FileQuorumConfig {
    fn default() -> Self {
        Self {
            rule: "majority".to_string(),
            min_models: 2,
            moderator: None,
            enable_peer_review: true,
        }
    }
}

impl FileQuorumConfig {
    /// Parse the rule string into QuorumRule enum
    pub fn parse_rule(&self) -> QuorumRule {
        self.rule.parse().unwrap_or_default()
    }

    /// Parse moderator into Model enum
    pub fn parse_moderator(&self) -> Option<Model> {
        self.moderator.as_ref().and_then(|s| s.parse().ok())
    }
}

// ==================== Tools Configuration ====================
//
// The tools configuration allows customizing how Quorum executes tools.
// Tools come from multiple providers (builtin, CLI, MCP, scripts) and
// can be configured to use enhanced CLI tools when available.
//
// Example configuration:
//
// ```toml
// [tools]
// providers = ["cli", "builtin"]
// suggest_enhanced_tools = true
//
// [tools.cli.aliases]
// grep_search = "rg"    # Use ripgrep
// glob_search = "fd"    # Use fd-find
// ```

/// Tool provider types that can be enabled
///
/// Providers are tried in order of their priority (highest first).
/// See [`crate::tools::ToolRegistry`] for details on priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolProviderType {
    /// Built-in tools (read_file, write_file, etc.) - always available
    Builtin,
    /// CLI tools (grep/rg, find/fd) - wraps system commands
    Cli,
    /// MCP server tools - connects to external servers
    Mcp,
    /// User scripts as tools
    Script,
}

/// Enhanced tool definition (for suggesting upgrades)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EnhancedToolConfig {
    /// CLI command to use
    pub command: String,
    /// Additional arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Description for user prompt
    #[serde(default)]
    pub description: String,
}

/// CLI tool provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileCliToolsConfig {
    /// Whether CLI tools are enabled
    pub enabled: bool,
    /// Tool aliases (tool_name -> CLI command)
    /// e.g., grep_search = "grep" or grep_search = "rg"
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    /// Enhanced tool definitions (for suggesting upgrades)
    #[serde(default)]
    pub enhanced: HashMap<String, EnhancedToolConfig>,
}

impl Default for FileCliToolsConfig {
    fn default() -> Self {
        let mut aliases = HashMap::new();
        // Default to standard tools (available everywhere)
        aliases.insert("grep_search".to_string(), "grep".to_string());
        aliases.insert("glob_search".to_string(), "find".to_string());
        aliases.insert("read_file".to_string(), "cat".to_string());

        let mut enhanced = HashMap::new();
        enhanced.insert(
            "grep_search".to_string(),
            EnhancedToolConfig {
                command: "rg".to_string(),
                args: vec![],
                description: "10x faster than grep".to_string(),
            },
        );
        enhanced.insert(
            "glob_search".to_string(),
            EnhancedToolConfig {
                command: "fd".to_string(),
                args: vec![],
                description: "5x faster than find".to_string(),
            },
        );
        enhanced.insert(
            "read_file".to_string(),
            EnhancedToolConfig {
                command: "bat".to_string(),
                args: vec!["--plain".to_string(), "--paging=never".to_string()],
                description: "syntax highlighting".to_string(),
            },
        );

        Self {
            enabled: true,
            aliases,
            enhanced,
        }
    }
}

/// MCP server configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileMcpServerConfig {
    /// Server name
    pub name: String,
    /// Command to start the server
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// MCP tools configuration
///
/// MCP is disabled by default as it requires explicit server setup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileMcpToolsConfig {
    /// Whether MCP tools are enabled (default: false, requires setup)
    pub enabled: bool,
    /// MCP servers to connect to
    #[serde(default)]
    pub servers: Vec<FileMcpServerConfig>,
}

/// Builtin tools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileBuiltinToolsConfig {
    /// Whether builtin tools are enabled
    pub enabled: bool,
}

impl Default for FileBuiltinToolsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Custom tool parameter definition
///
/// Defines a single parameter for a custom tool registered in `quorum.toml`.
///
/// # Example
///
/// ```toml
/// [tools.custom.gh_create_issue.parameters.title]
/// type = "string"
/// description = "Issue title"
/// required = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCustomToolParameter {
    /// Parameter type: "string", "number", "integer", "boolean"
    #[serde(rename = "type", default = "default_string_type")]
    pub param_type: String,
    /// Human-readable description of the parameter
    pub description: String,
    /// Whether this parameter is required (default: true)
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_string_type() -> String {
    "string".to_string()
}

fn default_true() -> bool {
    true
}

/// Custom tool definition from `quorum.toml`
///
/// Allows users to register external CLI commands as first-class tools.
/// The command template uses `{param_name}` placeholders that are replaced
/// with argument values at execution time.
///
/// # Security
///
/// All parameter values are shell-escaped before substitution to prevent
/// command injection.
///
/// # Example
///
/// ```toml
/// [tools.custom.gh_create_issue]
/// description = "Create a GitHub issue in the current repository"
/// command = "gh issue create --title {title} --body {body}"
/// risk_level = "high"
///
/// [tools.custom.gh_create_issue.parameters.title]
/// type = "string"
/// description = "Issue title"
/// required = true
///
/// [tools.custom.gh_create_issue.parameters.body]
/// type = "string"
/// description = "Issue body in markdown"
/// required = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCustomToolConfig {
    /// Human-readable description of what this tool does
    pub description: String,
    /// Command template with `{param_name}` placeholders
    pub command: String,
    /// Risk level: "low" or "high" (default: "high" — safe by default)
    #[serde(default = "default_high_risk")]
    pub risk_level: String,
    /// Parameter definitions
    #[serde(default)]
    pub parameters: HashMap<String, FileCustomToolParameter>,
}

fn default_high_risk() -> String {
    "high".to_string()
}

/// Complete tools configuration
///
/// Controls which tool providers are enabled and how they behave.
///
/// # Example
///
/// ```toml
/// [tools]
/// providers = ["mcp", "cli", "builtin"]
/// suggest_enhanced_tools = true
///
/// [tools.cli.aliases]
/// grep_search = "rg"
/// glob_search = "fd"
///
/// [tools.mcp]
/// enabled = true
///
/// [[tools.mcp.servers]]
/// name = "filesystem"
/// command = "npx"
/// args = ["-y", "@anthropic/mcp-server-filesystem"]
///
/// [tools.custom.gh_create_issue]
/// description = "Create a GitHub issue"
/// command = "gh issue create --title {title} --body {body}"
/// risk_level = "high"
///
/// [tools.custom.gh_create_issue.parameters.title]
/// type = "string"
/// description = "Issue title"
/// required = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileToolsConfig {
    /// Which providers to enable (order matters for priority)
    ///
    /// Providers are listed from highest to lowest priority.
    /// Default: `["cli", "builtin"]`
    #[serde(default)]
    pub providers: Vec<ToolProviderType>,
    /// Whether to suggest enhanced tools when detected
    ///
    /// When enabled, Quorum will check for tools like `rg`, `fd`, `bat`
    /// and offer to configure them if found.
    #[serde(default = "default_suggest_enhanced")]
    pub suggest_enhanced_tools: bool,
    /// Builtin tools settings
    #[serde(default)]
    pub builtin: FileBuiltinToolsConfig,
    /// CLI tools settings
    #[serde(default)]
    pub cli: FileCliToolsConfig,
    /// MCP tools settings
    #[serde(default)]
    pub mcp: FileMcpToolsConfig,
    /// Custom tools — user-defined CLI commands exposed as tools
    #[serde(default)]
    pub custom: HashMap<String, FileCustomToolConfig>,
}

fn default_suggest_enhanced() -> bool {
    true
}

impl Default for FileToolsConfig {
    fn default() -> Self {
        Self {
            providers: vec![ToolProviderType::Cli, ToolProviderType::Builtin],
            suggest_enhanced_tools: true,
            builtin: FileBuiltinToolsConfig::default(),
            cli: FileCliToolsConfig::default(),
            mcp: FileMcpToolsConfig::default(),
            custom: HashMap::new(),
        }
    }
}

// ==================== Context Budget Configuration ====================

/// Context budget configuration from TOML.
///
/// Controls how much task result context is retained between task executions.
///
/// # Example
///
/// ```toml
/// [context_budget]
/// max_entry_bytes = 20000
/// max_total_bytes = 60000
/// recent_full_count = 3
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileContextBudgetConfig {
    /// Maximum bytes for a single task result (head+tail truncated).
    pub max_entry_bytes: usize,
    /// Maximum bytes for the entire previous_results buffer.
    pub max_total_bytes: usize,
    /// Number of recent task results to keep in full (older are summarized).
    pub recent_full_count: usize,
}

impl Default for FileContextBudgetConfig {
    fn default() -> Self {
        let budget = ContextBudget::default();
        Self {
            max_entry_bytes: budget.max_entry_bytes(),
            max_total_bytes: budget.max_total_bytes(),
            recent_full_count: budget.recent_full_count(),
        }
    }
}

impl FileContextBudgetConfig {
    /// Convert to domain `ContextBudget`, returning validation issues.
    ///
    /// If the values violate constraints, falls back to `ContextBudget::default()`
    /// and returns warnings describing the issues.
    pub fn to_context_budget(&self) -> (ContextBudget, Vec<ConfigIssue>) {
        match ContextBudget::try_new(
            self.max_entry_bytes,
            self.max_total_bytes,
            self.recent_full_count,
        ) {
            Ok(budget) => (budget, vec![]),
            Err(errors) => {
                let issues = errors
                    .into_iter()
                    .map(|msg| ConfigIssue {
                        severity: Severity::Warning,
                        code: ConfigIssueCode::InvalidConstraint {
                            field: "context_budget".to_string(),
                        },
                        message: msg,
                    })
                    .collect();
                (ContextBudget::default(), issues)
            }
        }
    }
}

// ==================== TUI Configuration ====================

/// TUI input configuration
///
/// Controls keybindings and behavior of the modal input system.
///
/// # Example
///
/// ```toml
/// [tui.input]
/// submit_key = "enter"
/// newline_key = "alt+enter"
/// editor_key = "I"
/// editor_action = "return_to_insert"
/// max_height = 10
/// dynamic_height = true
/// context_header = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTuiInputConfig {
    /// Key to submit input (default: "enter")
    pub submit_key: String,
    /// Key to insert a newline in multiline mode (default: "alt+enter")
    pub newline_key: String,
    /// Key to launch $EDITOR from Normal mode (default: "I")
    pub editor_key: String,
    /// What happens after editor saves: "return_to_insert" or "submit"
    pub editor_action: String,
    /// Maximum height for the input area in lines (default: 10)
    pub max_height: u16,
    /// Whether input area grows dynamically with content (default: true)
    pub dynamic_height: bool,
    /// Whether to show context header in $EDITOR temp file (default: true)
    pub context_header: bool,
}

impl Default for FileTuiInputConfig {
    fn default() -> Self {
        Self {
            submit_key: "enter".to_string(),
            newline_key: "shift+enter".to_string(),
            editor_key: "I".to_string(),
            editor_action: "return_to_insert".to_string(),
            max_height: 10,
            dynamic_height: true,
            context_header: true,
        }
    }
}

/// TUI layout configuration from TOML
///
/// # Example
///
/// ```toml
/// [tui.layout]
/// preset = "default"
/// flex_threshold = 120
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTuiLayoutConfig {
    /// Layout preset: "default", "minimal", "wide", "stacked"
    pub preset: String,
    /// Terminal width threshold for responsive fallback to Minimal
    pub flex_threshold: u16,
}

impl Default for FileTuiLayoutConfig {
    fn default() -> Self {
        Self {
            preset: "default".to_string(),
            flex_threshold: 120,
        }
    }
}

/// TUI route customization from TOML
///
/// # Example
///
/// ```toml
/// [tui.routes]
/// tool_log = "sidebar"
/// notification = "flash"
/// hil_prompt = "overlay"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTuiRoutesConfig {
    /// Route target for tool_log: "sidebar", "float", "notification"
    pub tool_log: Option<String>,
    /// Route target for notification
    pub notification: Option<String>,
    /// Route target for hil_prompt
    pub hil_prompt: Option<String>,
}

/// Per-surface configuration from TOML
///
/// # Example
///
/// ```toml
/// [tui.surfaces.progress_pane]
/// position = "right"
/// width = "30%"
/// border = "rounded"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTuiSurfaceConfig {
    /// Surface position: "right", "left", "bottom"
    pub position: Option<String>,
    /// Width as percentage string, e.g. "30%"
    pub width: Option<String>,
    /// Border style: "rounded", "plain", "none", "double"
    pub border: Option<String>,
}

/// TUI surfaces configuration from TOML
///
/// # Example
///
/// ```toml
/// [tui.surfaces.progress_pane]
/// position = "right"
/// width = "30%"
/// border = "rounded"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTuiSurfacesConfig {
    /// Progress pane configuration
    pub progress_pane: Option<FileTuiSurfaceConfig>,
    /// Tool float configuration
    pub tool_float: Option<FileTuiSurfaceConfig>,
}

/// TUI configuration
///
/// Controls the terminal user interface behavior.
///
/// # Example
///
/// ```toml
/// [tui]
/// [tui.input]
/// max_height = 12
/// editor_action = "submit"
///
/// [tui.layout]
/// preset = "default"
/// flex_threshold = 120
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTuiConfig {
    /// Input area configuration
    pub input: FileTuiInputConfig,
    /// Layout configuration
    pub layout: FileTuiLayoutConfig,
    /// Route overrides
    pub routes: FileTuiRoutesConfig,
    /// Surface configuration
    pub surfaces: FileTuiSurfacesConfig,
}

/// Complete file configuration (raw TOML structure)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    /// Role-based model selection
    pub models: FileModelsConfig,
    /// Quorum consensus settings
    pub quorum: FileQuorumConfig,
    /// Output settings
    pub output: FileOutputConfig,
    /// REPL settings
    pub repl: FileReplConfig,
    /// Agent settings
    pub agent: FileAgentConfig,
    /// Tools settings
    pub tools: FileToolsConfig,
    /// TUI settings
    pub tui: FileTuiConfig,
    /// Context budget settings
    pub context_budget: FileContextBudgetConfig,
    /// Provider settings (e.g. Bedrock credentials)
    /// This is separate from the agent/model config to allow flexible provider routing.
    pub providers: FileProvidersConfig,
}

impl FileConfig {
    /// Validate the entire configuration, returning all detected issues.
    ///
    /// This is the single entry point for config validation. It checks:
    /// 1. Empty model names across all model fields
    /// 2. Enum parse failures for agent fields (hil_mode, consensus_level, etc.)
    /// 3. Dead sections that are not wired into the application
    pub fn validate(&self) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();

        // 1. Model parse validation (catches empty names)
        issues.extend(self.models.parse_exploration().1);
        issues.extend(self.models.parse_decision().1);
        issues.extend(self.models.parse_review().1);
        issues.extend(self.models.parse_participants().1);
        issues.extend(self.models.parse_moderator().1);
        issues.extend(self.models.parse_ask().1);

        // 2. Enum parse validation
        issues.extend(self.agent.parse_hil_mode().1);
        issues.extend(self.agent.parse_consensus_level().1);
        issues.extend(self.agent.parse_phase_scope().1);
        issues.extend(self.agent.parse_strategy().1);

        // 3. Context budget validation
        issues.extend(self.context_budget.to_context_budget().1);

        // 4. TUI layout preset validation
        {
            let valid = ["default", "minimal", "min", "wide", "stacked", "stack"];
            if !valid.contains(&self.tui.layout.preset.to_lowercase().as_str()) {
                issues.push(ConfigIssue {
                    severity: Severity::Warning,
                    code: ConfigIssueCode::InvalidEnumValue {
                        field: "tui.layout.preset".to_string(),
                        value: self.tui.layout.preset.clone(),
                        valid_values: vec![
                            "default".to_string(),
                            "minimal".to_string(),
                            "wide".to_string(),
                            "stacked".to_string(),
                        ],
                    },
                    message: format!(
                        "tui.layout.preset: unknown value '{}', falling back to 'default'",
                        self.tui.layout.preset
                    ),
                });
            }
        }

        // 5. Dead [quorum] section detection
        if self.quorum != FileQuorumConfig::default() {
            issues.push(ConfigIssue {
                severity: Severity::Warning,
                code: ConfigIssueCode::DeadSection {
                    section: "quorum".to_string(),
                },
                message: "[quorum] section is configured but not currently used by the application"
                    .to_string(),
            });
        }

        issues
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBedrockConfig {
    /// AWS region for Bedrock models (default: "us-east-1")
    pub region: String,
    /// AWS profile name for credentials (default: "default")
    pub profile: Option<String>,
    /// Max Tokens per response (default: 8192)
    pub max_tokens: u32,
}

impl Default for FileBedrockConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            profile: None,
            max_tokens: 8192,
        }
    }
}

/// Anthropic API provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileAnthropicConfig {
    /// Environment variable name for the API key (default: "ANTHROPIC_API_KEY").
    pub api_key_env: String,
    /// Direct API key (not recommended — use env var instead).
    pub api_key: Option<String>,
    /// Base URL for the Anthropic API.
    pub base_url: String,
    /// Default max tokens per response.
    pub max_tokens: u32,
    /// Anthropic API version header.
    pub api_version: String,
}

impl Default for FileAnthropicConfig {
    fn default() -> Self {
        Self {
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            api_key: None,
            base_url: "https://api.anthropic.com".to_string(),
            max_tokens: 8192,
            api_version: "2023-06-01".to_string(),
        }
    }
}

/// OpenAI API provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileOpenAiConfig {
    /// Environment variable name for the API key (default: "OPENAI_API_KEY").
    pub api_key_env: String,
    /// Direct API key (not recommended — use env var instead).
    pub api_key: Option<String>,
    /// Base URL for the OpenAI API (can be overridden for Azure OpenAI).
    pub base_url: String,
    /// Default max tokens per response.
    pub max_tokens: u32,
}

impl Default for FileOpenAiConfig {
    fn default() -> Self {
        Self {
            api_key_env: "OPENAI_API_KEY".to_string(),
            api_key: None,
            base_url: "https://api.openai.com".to_string(),
            max_tokens: 8192,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileProvidersConfig {
    /// Default provider: "copilot", "anthropic", "openai", "bedrock", "azure".
    pub default: Option<String>,
    /// Anthropic API settings.
    pub anthropic: FileAnthropicConfig,
    /// OpenAI API settings.
    pub openai: FileOpenAiConfig,
    /// AWS Bedrock settings.
    pub bedrock: FileBedrockConfig,
    /// Explicit model → provider routing overrides.
    pub routing: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_full_config() {
        let toml_str = r#"
[models]
exploration = "gpt-5.2-codex"
decision = "claude-sonnet-4.5"
review = ["claude-opus-4.5", "gpt-5.2-codex"]

[output]
format = "full"
color = false

[repl]
show_progress = false
history_file = "~/.local/share/quorum/history.txt"
"#;

        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.models.exploration, Some("gpt-5.2-codex".to_string()));
        assert_eq!(
            config.models.decision,
            Some("claude-sonnet-4.5".to_string())
        );
        assert_eq!(config.models.review.as_ref().unwrap().len(), 2);
        assert_eq!(config.output.format, Some(OutputFormat::Full));
        assert!(!config.output.color);
        assert!(!config.repl.show_progress);
    }

    #[test]
    fn test_deserialize_partial_config() {
        let toml_str = r#"
[models]
decision = "gpt-5.2-codex"
"#;

        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.models.parse_decision().0, Some(Model::Gpt52Codex));
        // Defaults should apply
        assert!(config.models.exploration.is_none());
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_default_config() {
        let config = FileConfig::default();
        assert!(config.models.exploration.is_none());
        assert!(config.models.decision.is_none());
        assert!(config.models.review.is_none());
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_validate_valid_config() {
        let config = FileConfig::default();
        assert!(config.validate().is_empty());
    }

    #[test]
    fn test_validate_empty_model_name() {
        let toml_str = r#"
[models]
review = ["gpt-5.2-codex", ""]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::EmptyModelName { field } if field == "review"
        )));
    }

    #[test]
    fn test_output_format_deserialize() {
        let toml_str = r#"
[output]
format = "json"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.output.format, Some(OutputFormat::Json));
    }

    #[test]
    fn test_tools_config_default() {
        let config = FileToolsConfig::default();
        assert!(config.suggest_enhanced_tools);
        assert!(config.builtin.enabled);
        assert!(config.cli.enabled);
        assert!(!config.mcp.enabled);
        assert_eq!(
            config.cli.aliases.get("grep_search"),
            Some(&"grep".to_string())
        );
        assert_eq!(
            config.cli.aliases.get("glob_search"),
            Some(&"find".to_string())
        );
    }

    #[test]
    fn test_tools_config_deserialize() {
        let toml_str = r#"
[tools]
providers = ["cli", "builtin"]
suggest_enhanced_tools = false

[tools.cli]
enabled = true

[tools.cli.aliases]
grep_search = "rg"
glob_search = "fd"

[tools.mcp]
enabled = true

[[tools.mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@anthropic/mcp-server-filesystem"]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.tools.suggest_enhanced_tools);
        assert!(config.tools.cli.enabled);
        assert_eq!(
            config.tools.cli.aliases.get("grep_search"),
            Some(&"rg".to_string())
        );
        assert!(config.tools.mcp.enabled);
        assert_eq!(config.tools.mcp.servers.len(), 1);
        assert_eq!(config.tools.mcp.servers[0].name, "filesystem");
    }

    #[test]
    fn test_tools_enhanced_config() {
        let config = FileCliToolsConfig::default();
        let rg = config.enhanced.get("grep_search").unwrap();
        assert_eq!(rg.command, "rg");
        assert!(rg.description.contains("faster"));
    }

    #[test]
    fn test_models_config_defaults() {
        let config = FileModelsConfig::default();
        assert!(config.exploration.is_none());
        assert!(config.decision.is_none());
        assert!(config.review.is_none());
        assert!(config.participants.is_none());
        assert!(config.moderator.is_none());
        assert!(config.ask.is_none());
    }

    #[test]
    fn test_models_config_deserialize() {
        let toml_str = r#"
[models]
exploration = "gpt-5.2-codex"
decision = "claude-sonnet-4.5"
review = ["claude-sonnet-4.5", "gpt-5.2-codex"]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.models.parse_exploration().0, Some(Model::Gpt52Codex));
        assert_eq!(
            config.models.parse_decision().0,
            Some(Model::ClaudeSonnet45)
        );
        let review = config.models.parse_review().0.unwrap();
        assert_eq!(review.len(), 2);
        assert!(review.contains(&Model::ClaudeSonnet45));
        assert!(review.contains(&Model::Gpt52Codex));
    }

    #[test]
    fn test_models_config_partial() {
        let toml_str = r#"
[models]
decision = "claude-opus-4.5"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert!(config.models.exploration.is_none());
        assert_eq!(config.models.parse_decision().0, Some(Model::ClaudeOpus45));
        assert!(config.models.review.is_none());
        assert!(config.models.participants.is_none());
        assert!(config.models.moderator.is_none());
        assert!(config.models.ask.is_none());
    }

    #[test]
    fn test_models_config_interaction_roles() {
        let toml_str = r#"
[models]
participants = ["claude-opus-4.5", "gpt-5.2-codex"]
moderator = "claude-opus-4.5"
ask = "claude-sonnet-4.5"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let participants = config.models.parse_participants().0.unwrap();
        assert_eq!(participants.len(), 2);
        assert!(participants.contains(&Model::ClaudeOpus45));
        assert!(participants.contains(&Model::Gpt52Codex));
        assert_eq!(config.models.parse_moderator().0, Some(Model::ClaudeOpus45));
        assert_eq!(config.models.parse_ask().0, Some(Model::ClaudeSonnet45));
    }

    #[test]
    fn test_validate_empty_participants_name() {
        let toml_str = r#"
[models]
participants = ["gpt-5.2-codex", ""]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(!issues.is_empty());
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::EmptyModelName { field } if field == "participants"
        )));
    }

    #[test]
    fn test_agent_config_deserialize() {
        let toml_str = r#"
[agent]
max_plan_revisions = 5
hil_mode = "auto_reject"
consensus_level = "ensemble"
phase_scope = "fast"
strategy = "debate"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.max_plan_revisions, 5);
        assert_eq!(config.agent.hil_mode, "auto_reject");
        assert_eq!(config.agent.consensus_level, "ensemble");
        assert_eq!(
            config.agent.parse_consensus_level().0,
            ConsensusLevel::Ensemble
        );
        assert_eq!(config.agent.phase_scope, "fast");
        assert_eq!(config.agent.parse_phase_scope().0, PhaseScope::Fast);
        assert_eq!(config.agent.parse_strategy().0, "debate");
    }

    #[test]
    fn test_agent_config_consensus_level_deserialize() {
        // Test "solo" (default)
        let toml_str = r#"
[agent]
consensus_level = "solo"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.consensus_level, "solo");
        assert_eq!(config.agent.parse_consensus_level().0, ConsensusLevel::Solo);

        // Test "ensemble"
        let toml_str = r#"
[agent]
consensus_level = "ensemble"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.consensus_level, "ensemble");
        assert_eq!(
            config.agent.parse_consensus_level().0,
            ConsensusLevel::Ensemble
        );

        // Test alias "ens" -> Ensemble
        let toml_str = r#"
[agent]
consensus_level = "ens"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.agent.parse_consensus_level().0,
            ConsensusLevel::Ensemble
        );
    }

    #[test]
    fn test_agent_config_phase_scope_deserialize() {
        let toml_str = r#"
[agent]
phase_scope = "plan-only"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.parse_phase_scope().0, PhaseScope::PlanOnly);
    }

    #[test]
    fn test_quorum_config_default() {
        let config = FileQuorumConfig::default();
        assert_eq!(config.rule, "majority");
        assert_eq!(config.min_models, 2);
        assert!(config.enable_peer_review);
        assert!(config.moderator.is_none());
    }

    #[test]
    fn test_quorum_config_deserialize() {
        let toml_str = r#"
[quorum]
rule = "unanimous"
min_models = 3
moderator = "claude-opus-4.5"
enable_peer_review = false
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.quorum.rule, "unanimous");
        assert_eq!(config.quorum.min_models, 3);
        assert!(!config.quorum.enable_peer_review);

        let moderator = config.quorum.parse_moderator().unwrap();
        assert_eq!(moderator, Model::ClaudeOpus45);
    }

    #[test]
    fn test_quorum_config_parse_rule() {
        let mut config = FileQuorumConfig::default();

        config.rule = "majority".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Majority);

        config.rule = "unanimous".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Unanimous);

        config.rule = "atleast:2".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::AtLeast(2));

        config.rule = "75%".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Percentage(75));
    }

    #[test]
    fn test_custom_tools_config_deserialize() {
        let toml_str = r#"
[tools.custom.gh_create_issue]
description = "Create a GitHub issue"
command = "gh issue create --title {title} --body {body}"
risk_level = "high"

[tools.custom.gh_create_issue.parameters.title]
type = "string"
description = "Issue title"
required = true

[tools.custom.gh_create_issue.parameters.body]
type = "string"
description = "Issue body in markdown"
required = true

[tools.custom.aws_s3_ls]
description = "List S3 objects"
command = "aws s3 ls {path}"
risk_level = "low"

[tools.custom.aws_s3_ls.parameters.path]
type = "string"
description = "S3 path (e.g. s3://my-bucket/)"
required = true
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tools.custom.len(), 2);

        let gh = config.tools.custom.get("gh_create_issue").unwrap();
        assert_eq!(gh.description, "Create a GitHub issue");
        assert_eq!(gh.command, "gh issue create --title {title} --body {body}");
        assert_eq!(gh.risk_level, "high");
        assert_eq!(gh.parameters.len(), 2);

        let title_param = gh.parameters.get("title").unwrap();
        assert_eq!(title_param.param_type, "string");
        assert!(title_param.required);

        let s3 = config.tools.custom.get("aws_s3_ls").unwrap();
        assert_eq!(s3.risk_level, "low");
    }

    #[test]
    fn test_custom_tools_config_defaults() {
        let toml_str = r#"
[tools.custom.simple_tool]
description = "A simple tool"
command = "echo {message}"

[tools.custom.simple_tool.parameters.message]
description = "Message to echo"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let tool = config.tools.custom.get("simple_tool").unwrap();

        // risk_level defaults to "high"
        assert_eq!(tool.risk_level, "high");

        let param = tool.parameters.get("message").unwrap();
        // param_type defaults to "string"
        assert_eq!(param.param_type, "string");
        // required defaults to true
        assert!(param.required);
    }

    #[test]
    fn test_custom_tools_empty_by_default() {
        let config = FileToolsConfig::default();
        assert!(config.custom.is_empty());
    }

    #[test]
    fn test_tui_config_default() {
        let config = FileTuiConfig::default();
        assert_eq!(config.input.submit_key, "enter");
        assert_eq!(config.input.newline_key, "shift+enter");
        assert_eq!(config.input.editor_key, "I");
        assert_eq!(config.input.editor_action, "return_to_insert");
        assert_eq!(config.input.max_height, 10);
        assert!(config.input.dynamic_height);
        assert!(config.input.context_header);
    }

    #[test]
    fn test_tui_config_deserialize() {
        let toml_str = r#"
[tui.input]
max_height = 15
editor_action = "submit"
context_header = false
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tui.input.max_height, 15);
        assert_eq!(config.tui.input.editor_action, "submit");
        assert!(!config.tui.input.context_header);
        // Defaults still apply for unset fields
        assert_eq!(config.tui.input.submit_key, "enter");
        assert_eq!(config.tui.input.newline_key, "shift+enter");
    }

    #[test]
    fn test_tui_config_partial() {
        let toml_str = r#"
[tui.input]
max_height = 20
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tui.input.max_height, 20);
        // All other fields use defaults
        assert_eq!(config.tui.input.submit_key, "enter");
        assert!(config.tui.input.dynamic_height);
    }

    // ==================== Validation Tests ====================

    #[test]
    fn test_validate_typo_hil_mode_warns() {
        let toml_str = r#"
[agent]
hil_mode = "typo"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "agent.hil_mode"
        )));
        // Typo should be a warning, not an error
        assert!(issues.iter().all(|i| i.severity == Severity::Warning));
    }

    #[test]
    fn test_validate_typo_consensus_level_warns() {
        let toml_str = r#"
[agent]
consensus_level = "typo"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "agent.consensus_level"
        )));
    }

    #[test]
    fn test_validate_typo_strategy_warns() {
        let toml_str = r#"
[agent]
strategy = "typo"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "agent.strategy"
        )));
    }

    #[test]
    fn test_validate_dead_quorum_section() {
        let toml_str = r#"
[quorum]
rule = "unanimous"
min_models = 3
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::DeadSection { section } if section == "quorum"
        )));
    }

    #[test]
    fn test_validate_default_quorum_no_dead_warning() {
        // Default [quorum] values should NOT trigger a dead section warning
        let config = FileConfig::default();
        let issues = config.validate();
        assert!(
            !issues
                .iter()
                .any(|i| matches!(&i.code, ConfigIssueCode::DeadSection { .. }))
        );
    }

    // ==================== Context Budget Tests ====================

    #[test]
    fn test_context_budget_config_default() {
        let config = FileContextBudgetConfig::default();
        assert_eq!(config.max_entry_bytes, 20_000);
        assert_eq!(config.max_total_bytes, 60_000);
        assert_eq!(config.recent_full_count, 3);
    }

    #[test]
    fn test_context_budget_config_deserialize() {
        let toml_str = r#"
[context_budget]
max_entry_bytes = 10000
max_total_bytes = 30000
recent_full_count = 2
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.context_budget.max_entry_bytes, 10_000);
        assert_eq!(config.context_budget.max_total_bytes, 30_000);
        assert_eq!(config.context_budget.recent_full_count, 2);
    }

    #[test]
    fn test_context_budget_config_to_domain() {
        let config = FileContextBudgetConfig::default();
        let (budget, issues) = config.to_context_budget();
        assert!(issues.is_empty());
        assert_eq!(budget.max_entry_bytes(), 20_000);
    }

    #[test]
    fn test_context_budget_config_validation_falls_back_to_default() {
        let config = FileContextBudgetConfig {
            max_entry_bytes: 50_000,
            max_total_bytes: 10_000, // Less than entry — invalid
            recent_full_count: 0,    // Less than 1 — invalid
        };
        let (budget, issues) = config.to_context_budget();
        assert_eq!(issues.len(), 2);
        assert!(
            issues
                .iter()
                .all(|i| matches!(&i.code, ConfigIssueCode::InvalidConstraint { .. }))
        );
        // Should fall back to default
        assert_eq!(budget, ContextBudget::default());
    }

    #[test]
    fn test_context_budget_missing_uses_defaults() {
        // Empty config should use defaults
        let config: FileConfig = toml::from_str("").unwrap();
        assert_eq!(config.context_budget.max_entry_bytes, 20_000);
    }

    // ==================== TUI Layout Tests ====================

    #[test]
    fn test_tui_layout_config_default() {
        let config = FileTuiLayoutConfig::default();
        assert_eq!(config.preset, "default");
        assert_eq!(config.flex_threshold, 120);
    }

    #[test]
    fn test_tui_layout_config_deserialize() {
        let toml_str = r#"
[tui.layout]
preset = "wide"
flex_threshold = 100
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tui.layout.preset, "wide");
        assert_eq!(config.tui.layout.flex_threshold, 100);
    }

    #[test]
    fn test_tui_routes_config_deserialize() {
        let toml_str = r#"
[tui.routes]
tool_log = "sidebar"
notification = "flash"
hil_prompt = "overlay"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tui.routes.tool_log, Some("sidebar".to_string()));
        assert_eq!(config.tui.routes.notification, Some("flash".to_string()));
        assert_eq!(config.tui.routes.hil_prompt, Some("overlay".to_string()));
    }

    #[test]
    fn test_tui_surfaces_config_deserialize() {
        let toml_str = r#"
[tui.surfaces.progress_pane]
position = "right"
width = "30%"
border = "rounded"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let progress = config.tui.surfaces.progress_pane.unwrap();
        assert_eq!(progress.position, Some("right".to_string()));
        assert_eq!(progress.width, Some("30%".to_string()));
        assert_eq!(progress.border, Some("rounded".to_string()));
    }

    #[test]
    fn test_validate_invalid_layout_preset() {
        let toml_str = r#"
[tui.layout]
preset = "invalid_preset"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "tui.layout.preset"
        )));
    }

    #[test]
    fn test_validate_default_layout_no_warning() {
        let config = FileConfig::default();
        let issues = config.validate();
        assert!(!issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "tui.layout.preset"
        )));
    }

    #[test]
    fn test_tui_layout_missing_uses_defaults() {
        let config: FileConfig = toml::from_str("").unwrap();
        assert_eq!(config.tui.layout.preset, "default");
        assert_eq!(config.tui.layout.flex_threshold, 120);
        assert!(config.tui.routes.tool_log.is_none());
        assert!(config.tui.surfaces.progress_pane.is_none());
    }
}
