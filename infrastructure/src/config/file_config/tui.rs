//! TUI configuration from TOML (`[tui]` section)

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::agent::validation::ConfigIssueCode;

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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tui.input.max_height, 20);
        // All other fields use defaults
        assert_eq!(config.tui.input.submit_key, "enter");
        assert!(config.tui.input.dynamic_height);
    }

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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        let issues = config.validate();
        assert!(issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "tui.layout.preset"
        )));
    }

    #[test]
    fn test_validate_default_layout_no_warning() {
        let config = super::super::FileConfig::default();
        let issues = config.validate();
        assert!(!issues.iter().any(|i| matches!(
            &i.code,
            ConfigIssueCode::InvalidEnumValue { field, .. } if field == "tui.layout.preset"
        )));
    }

    #[test]
    fn test_tui_layout_missing_uses_defaults() {
        let config: super::super::FileConfig = toml::from_str("").unwrap();
        assert_eq!(config.tui.layout.preset, "default");
        assert_eq!(config.tui.layout.flex_threshold, 120);
        assert!(config.tui.routes.tool_log.is_none());
        assert!(config.tui.surfaces.progress_pane.is_none());
    }
}
