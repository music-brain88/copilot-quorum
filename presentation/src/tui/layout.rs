//! Layout configuration — preset-based TUI layout customization.
//!
//! Provides `LayoutPreset` (Default, Minimal, Wide, Stacked) and supporting
//! types for TOML-driven layout configuration.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use quorum_application::CustomPresetConfig;
use ratatui::layout::Direction;

use super::content::ContentSlot;
use super::surface::SurfaceId;

/// Layout preset — predefined layout configurations.
///
/// | Preset   | Description                                      |
/// |----------|--------------------------------------------------|
/// | Default  | 70/30 horizontal split (conversation + sidebar)  |
/// | Minimal  | Full-width conversation, no sidebar               |
/// | Wide     | 60/20/20 three-pane horizontal split              |
/// | Stacked  | 70/30 vertical split (conversation top, progress bottom) |
/// | Custom   | Lua-registered preset with custom splits and direction |
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum LayoutPreset {
    #[default]
    Default,
    Minimal,
    Wide,
    Stacked,
    /// Custom layout preset registered from Lua scripting.
    Custom(String),
}

impl LayoutPreset {
    /// Default percentage splits for a given number of content panes.
    ///
    /// Returns a `Vec<u16>` of percentages that sum to 100.
    /// For `Custom` presets, returns equal splits (the caller should use
    /// [`TuiLayoutConfig::resolve_splits`] for the real values).
    pub fn default_splits(&self, pane_count: usize) -> Vec<u16> {
        match (self, pane_count) {
            (_, 0) => vec![],
            (_, 1) => vec![100],
            (Self::Default, 2) => vec![70, 30],
            (Self::Wide, 3) => vec![60, 20, 20],
            (Self::Stacked, 2) => vec![70, 30],
            _ => {
                let per = 100 / pane_count as u16;
                let remainder = 100 - per * pane_count as u16;
                let mut splits = vec![per; pane_count];
                // Give the remainder to the first pane
                if remainder > 0 {
                    splits[0] += remainder;
                }
                splits
            }
        }
    }

    /// Split direction for this preset.
    ///
    /// Stacked uses vertical split; Custom falls back to horizontal
    /// (the caller should use [`TuiLayoutConfig::resolve_direction`]
    /// for the real direction).
    pub fn split_direction(&self) -> Direction {
        match self {
            Self::Stacked => Direction::Vertical,
            _ => Direction::Horizontal,
        }
    }

    /// Whether this is a built-in preset (not Custom).
    pub fn is_builtin(&self) -> bool {
        !matches!(self, Self::Custom(_))
    }
}

impl FromStr for LayoutPreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "minimal" | "min" => Ok(Self::Minimal),
            "wide" => Ok(Self::Wide),
            "stacked" | "stack" => Ok(Self::Stacked),
            _ => Err(format!(
                "unknown layout preset '{}', valid: default, minimal, wide, stacked",
                s
            )),
        }
    }
}

impl fmt::Display for LayoutPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Minimal => write!(f, "minimal"),
            Self::Wide => write!(f, "wide"),
            Self::Stacked => write!(f, "stacked"),
            Self::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Surface position — where a surface is placed relative to the main pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SurfacePosition {
    #[default]
    Right,
    Left,
    Bottom,
}

impl FromStr for SurfacePosition {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "right" => Ok(Self::Right),
            "left" => Ok(Self::Left),
            "bottom" => Ok(Self::Bottom),
            _ => Err(format!(
                "unknown surface position '{}', valid: right, left, bottom",
                s
            )),
        }
    }
}

impl fmt::Display for SurfacePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Right => write!(f, "right"),
            Self::Left => write!(f, "left"),
            Self::Bottom => write!(f, "bottom"),
        }
    }
}

/// Border style for surface widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BorderStyle {
    #[default]
    Rounded,
    Plain,
    None,
    Double,
}

impl FromStr for BorderStyle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "rounded" => Ok(Self::Rounded),
            "plain" => Ok(Self::Plain),
            "none" => Ok(Self::None),
            "double" => Ok(Self::Double),
            _ => Err(format!(
                "unknown border style '{}', valid: rounded, plain, none, double",
                s
            )),
        }
    }
}

impl fmt::Display for BorderStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rounded => write!(f, "rounded"),
            Self::Plain => write!(f, "plain"),
            Self::None => write!(f, "none"),
            Self::Double => write!(f, "double"),
        }
    }
}

/// Per-surface configuration (position, size, border).
#[derive(Debug, Clone)]
pub struct SurfaceConfig {
    pub position: SurfacePosition,
    pub width_percent: u16,
    pub border: BorderStyle,
}

impl Default for SurfaceConfig {
    fn default() -> Self {
        Self {
            position: SurfacePosition::Right,
            width_percent: 30,
            border: BorderStyle::Rounded,
        }
    }
}

/// Route target — maps a string name to a `SurfaceId`.
///
/// Used to parse TOML route overrides like `tool_log = "sidebar"`.
pub fn parse_route_target(name: &str) -> Option<SurfaceId> {
    match name.to_lowercase().as_str() {
        "sidebar" | "progress" => Some(SurfaceId::Sidebar),
        "main" | "mainpane" | "main_pane" => Some(SurfaceId::MainPane),
        "overlay" => Some(SurfaceId::Overlay),
        "float" | "tool_float" | "toolfloat" => Some(SurfaceId::ToolFloat),
        "notification" | "status" | "statusbar" | "status_bar" => Some(SurfaceId::StatusBar),
        "tool_pane" | "toolpane" => Some(SurfaceId::ToolPane),
        _ => None,
    }
}

/// Parse a surface name from Lua API into a `SurfaceId`.
///
/// Supports all static names plus `"dynamic_pane:<name>"` for dynamic panes.
pub fn parse_surface_id(name: &str) -> Option<SurfaceId> {
    match name {
        "main_pane" => Some(SurfaceId::MainPane),
        "sidebar" => Some(SurfaceId::Sidebar),
        "overlay" => Some(SurfaceId::Overlay),
        "header" => Some(SurfaceId::Header),
        "input" => Some(SurfaceId::Input),
        "status_bar" => Some(SurfaceId::StatusBar),
        "tab_bar" => Some(SurfaceId::TabBar),
        "tool_pane" => Some(SurfaceId::ToolPane),
        "tool_float" => Some(SurfaceId::ToolFloat),
        s if s.starts_with("dynamic_pane:") => Some(SurfaceId::DynamicPane(
            s["dynamic_pane:".len()..].to_string(),
        )),
        _ => None,
    }
}

/// Convert a `SurfaceId` to its string name (inverse of [`parse_surface_id`]).
pub fn surface_id_to_string(id: &SurfaceId) -> String {
    match id {
        SurfaceId::MainPane => "main_pane".to_string(),
        SurfaceId::Sidebar => "sidebar".to_string(),
        SurfaceId::Overlay => "overlay".to_string(),
        SurfaceId::Header => "header".to_string(),
        SurfaceId::Input => "input".to_string(),
        SurfaceId::StatusBar => "status_bar".to_string(),
        SurfaceId::TabBar => "tab_bar".to_string(),
        SurfaceId::ToolPane => "tool_pane".to_string(),
        SurfaceId::ToolFloat => "tool_float".to_string(),
        SurfaceId::DynamicPane(name) => format!("dynamic_pane:{}", name),
    }
}

/// Parse a content slot name from Lua API into a `ContentSlot`.
///
/// Supports all static names plus `"model_stream:<name>"` and `"lua:<name>"`.
pub fn parse_content_slot(name: &str) -> Option<ContentSlot> {
    match name {
        "conversation" => Some(ContentSlot::Conversation),
        "progress" => Some(ContentSlot::Progress),
        "notification" => Some(ContentSlot::Notification),
        "hil_prompt" => Some(ContentSlot::HilPrompt),
        "help" => Some(ContentSlot::Help),
        "tool_log" => Some(ContentSlot::ToolLog),
        s if s.starts_with("model_stream:") => Some(ContentSlot::ModelStream(
            s["model_stream:".len()..].to_string(),
        )),
        s if s.starts_with("lua:") => Some(ContentSlot::LuaSlot(s["lua:".len()..].to_string())),
        _ => None,
    }
}

/// Convert a `ContentSlot` to its string name (inverse of [`parse_content_slot`]).
pub fn content_slot_to_string(slot: &ContentSlot) -> String {
    match slot {
        ContentSlot::Conversation => "conversation".to_string(),
        ContentSlot::Progress => "progress".to_string(),
        ContentSlot::Notification => "notification".to_string(),
        ContentSlot::HilPrompt => "hil_prompt".to_string(),
        ContentSlot::Help => "help".to_string(),
        ContentSlot::ToolLog => "tool_log".to_string(),
        ContentSlot::ModelStream(name) => format!("model_stream:{}", name),
        ContentSlot::LuaSlot(name) => format!("lua:{}", name),
    }
}

/// Route override entry — parsed from TOML `[tui.routes]`.
#[derive(Debug, Clone)]
pub struct RouteOverride {
    pub content: ContentSlot,
    pub surface: SurfaceId,
}

/// Complete TUI layout configuration.
///
/// Assembled from `[tui.layout]` TOML section and used by the render loop.
///
/// # Strategy-based preset overrides
///
/// The `strategy_presets` map allows per-strategy layout overrides:
/// ```toml
/// [tui.layout.strategy]
/// quorum = "stacked"
/// ensemble = "wide"
/// ```
///
/// When an orchestration strategy activates, the TUI switches to the
/// corresponding preset. If no override is configured, `preset` is used.
#[derive(Debug, Clone)]
pub struct TuiLayoutConfig {
    pub preset: LayoutPreset,
    pub flex_threshold: u16,
    pub surface_config: SurfaceConfig,
    pub route_overrides: Vec<RouteOverride>,
    /// Per-strategy layout preset overrides (e.g., "quorum" → Stacked).
    pub strategy_presets: HashMap<String, LayoutPreset>,
    /// Custom layout presets registered from Lua scripting.
    pub custom_presets: HashMap<String, CustomPresetConfig>,
}

impl TuiLayoutConfig {
    /// Get the effective preset for a given strategy, falling back to the base preset.
    pub fn preset_for_strategy(&self, strategy: &str) -> LayoutPreset {
        self.strategy_presets
            .get(strategy)
            .cloned()
            .unwrap_or_else(|| self.preset.clone())
    }

    /// Resolve splits for the current preset (handles both built-in and custom).
    pub fn resolve_splits(&self, pane_count: usize) -> Vec<u16> {
        if let LayoutPreset::Custom(name) = &self.preset {
            if let Some(config) = self.custom_presets.get(name) {
                return config.splits.clone();
            }
        }
        self.preset.default_splits(pane_count)
    }

    /// Resolve split direction for the current preset.
    pub fn resolve_direction(&self) -> Direction {
        if let LayoutPreset::Custom(name) = &self.preset {
            if let Some(config) = self.custom_presets.get(name) {
                return match config.direction.as_str() {
                    "vertical" => Direction::Vertical,
                    _ => Direction::Horizontal,
                };
            }
        }
        self.preset.split_direction()
    }
}

impl Default for TuiLayoutConfig {
    fn default() -> Self {
        Self {
            preset: LayoutPreset::Default,
            flex_threshold: 120,
            surface_config: SurfaceConfig::default(),
            route_overrides: Vec::new(),
            strategy_presets: HashMap::new(),
            custom_presets: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_preset_from_str() {
        assert_eq!(
            "default".parse::<LayoutPreset>().unwrap(),
            LayoutPreset::Default
        );
        assert_eq!(
            "minimal".parse::<LayoutPreset>().unwrap(),
            LayoutPreset::Minimal
        );
        assert_eq!(
            "min".parse::<LayoutPreset>().unwrap(),
            LayoutPreset::Minimal
        );
        assert_eq!("wide".parse::<LayoutPreset>().unwrap(), LayoutPreset::Wide);
        assert_eq!(
            "stacked".parse::<LayoutPreset>().unwrap(),
            LayoutPreset::Stacked
        );
        assert_eq!(
            "stack".parse::<LayoutPreset>().unwrap(),
            LayoutPreset::Stacked
        );
        assert!("unknown".parse::<LayoutPreset>().is_err());
    }

    #[test]
    fn test_layout_preset_display() {
        assert_eq!(LayoutPreset::Default.to_string(), "default");
        assert_eq!(LayoutPreset::Minimal.to_string(), "minimal");
        assert_eq!(LayoutPreset::Wide.to_string(), "wide");
        assert_eq!(LayoutPreset::Stacked.to_string(), "stacked");
    }

    #[test]
    fn test_layout_preset_round_trip() {
        for preset in [
            LayoutPreset::Default,
            LayoutPreset::Minimal,
            LayoutPreset::Wide,
            LayoutPreset::Stacked,
        ] {
            let s = preset.to_string();
            assert_eq!(s.parse::<LayoutPreset>().unwrap(), preset);
        }
    }

    #[test]
    fn test_surface_position_from_str() {
        assert_eq!(
            "right".parse::<SurfacePosition>().unwrap(),
            SurfacePosition::Right
        );
        assert_eq!(
            "left".parse::<SurfacePosition>().unwrap(),
            SurfacePosition::Left
        );
        assert_eq!(
            "bottom".parse::<SurfacePosition>().unwrap(),
            SurfacePosition::Bottom
        );
        assert!("top".parse::<SurfacePosition>().is_err());
    }

    #[test]
    fn test_border_style_from_str() {
        assert_eq!(
            "rounded".parse::<BorderStyle>().unwrap(),
            BorderStyle::Rounded
        );
        assert_eq!("plain".parse::<BorderStyle>().unwrap(), BorderStyle::Plain);
        assert_eq!("none".parse::<BorderStyle>().unwrap(), BorderStyle::None);
        assert_eq!(
            "double".parse::<BorderStyle>().unwrap(),
            BorderStyle::Double
        );
        assert!("dashed".parse::<BorderStyle>().is_err());
    }

    #[test]
    fn test_parse_route_target() {
        assert_eq!(parse_route_target("sidebar"), Some(SurfaceId::Sidebar));
        assert_eq!(parse_route_target("float"), Some(SurfaceId::ToolFloat));
        assert_eq!(
            parse_route_target("notification"),
            Some(SurfaceId::StatusBar)
        );
        assert_eq!(parse_route_target("overlay"), Some(SurfaceId::Overlay));
        assert_eq!(parse_route_target("tool_pane"), Some(SurfaceId::ToolPane));
        assert_eq!(parse_route_target("unknown"), None);
    }

    #[test]
    fn test_tui_layout_config_default() {
        let config = TuiLayoutConfig::default();
        assert_eq!(config.preset, LayoutPreset::Default);
        assert_eq!(config.flex_threshold, 120);
        assert_eq!(config.surface_config.position, SurfacePosition::Right);
        assert_eq!(config.surface_config.width_percent, 30);
        assert_eq!(config.surface_config.border, BorderStyle::Rounded);
        assert!(config.route_overrides.is_empty());
        assert!(config.strategy_presets.is_empty());
    }

    #[test]
    fn test_preset_for_strategy() {
        let mut config = TuiLayoutConfig::default();
        config
            .strategy_presets
            .insert("quorum".to_string(), LayoutPreset::Stacked);
        config
            .strategy_presets
            .insert("ensemble".to_string(), LayoutPreset::Wide);

        assert_eq!(config.preset_for_strategy("quorum"), LayoutPreset::Stacked);
        assert_eq!(config.preset_for_strategy("ensemble"), LayoutPreset::Wide);
        // Unknown strategy falls back to base preset
        assert_eq!(config.preset_for_strategy("debate"), LayoutPreset::Default);
    }

    #[test]
    fn test_default_splits() {
        assert_eq!(LayoutPreset::Default.default_splits(2), vec![70, 30]);
        assert_eq!(LayoutPreset::Wide.default_splits(3), vec![60, 20, 20]);
        assert_eq!(LayoutPreset::Stacked.default_splits(2), vec![70, 30]);
        assert_eq!(LayoutPreset::Minimal.default_splits(1), vec![100]);
        assert_eq!(LayoutPreset::Default.default_splits(0), Vec::<u16>::new());
        // Fallback: equal split with remainder to first pane
        assert_eq!(LayoutPreset::Default.default_splits(3), vec![34, 33, 33]);
    }

    #[test]
    fn test_split_direction() {
        use ratatui::layout::Direction;
        assert_eq!(
            LayoutPreset::Default.split_direction(),
            Direction::Horizontal
        );
        assert_eq!(
            LayoutPreset::Minimal.split_direction(),
            Direction::Horizontal
        );
        assert_eq!(LayoutPreset::Wide.split_direction(), Direction::Horizontal);
        assert_eq!(LayoutPreset::Stacked.split_direction(), Direction::Vertical);
    }
}
