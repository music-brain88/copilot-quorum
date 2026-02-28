//! TUI route/layout/content access port.
//!
//! Provides a string-based interface for Lua scripts to manipulate TUI
//! routes, layout presets, and content renderers without depending on
//! presentation-layer types (ContentSlot, SurfaceId, LayoutPreset).
//!
//! The presentation layer consumes pending changes each frame via
//! [`TuiAccessorPort::take_pending_changes`].

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from TUI access operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiAccessError {
    /// The content slot name is not recognized.
    UnknownContent { name: String },
    /// The surface name is not recognized.
    UnknownSurface { name: String },
    /// The layout preset name is not recognized.
    UnknownPreset { name: String },
    /// A custom preset with this name already exists.
    DuplicatePreset { name: String },
    /// A content slot with this name is already registered.
    DuplicateSlot { name: String },
    /// The configuration is invalid.
    InvalidConfig { message: String },
}

impl std::fmt::Display for TuiAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownContent { name } => write!(f, "unknown content slot: '{}'", name),
            Self::UnknownSurface { name } => write!(f, "unknown surface: '{}'", name),
            Self::UnknownPreset { name } => write!(f, "unknown layout preset: '{}'", name),
            Self::DuplicatePreset { name } => {
                write!(f, "layout preset '{}' already exists", name)
            }
            Self::DuplicateSlot { name } => {
                write!(f, "content slot '{}' already registered", name)
            }
            Self::InvalidConfig { message } => write!(f, "invalid config: {}", message),
        }
    }
}

impl std::error::Error for TuiAccessError {}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Configuration for a custom layout preset registered from Lua.
#[derive(Debug, Clone)]
pub struct CustomPresetConfig {
    /// Percentage split for each pane (must sum to ~100).
    pub splits: Vec<u16>,
    /// Split direction: `"horizontal"` or `"vertical"`.
    pub direction: String,
}

/// Pending changes accumulated by the port for the TUI to consume.
///
/// The presentation layer calls [`TuiAccessorPort::take_pending_changes`]
/// each frame and applies these atomically to `TuiState`.
#[derive(Debug, Clone, Default)]
pub struct TuiPendingChanges {
    /// Route overrides: `(content_name, surface_name)`.
    pub route_changes: Vec<(String, String)>,
    /// Preset to switch to (name).
    pub preset_switch: Option<String>,
    /// Newly registered custom presets.
    pub new_presets: Vec<(String, CustomPresetConfig)>,
    /// Newly registered Lua content slot names.
    pub new_content_slots: Vec<String>,
    /// Text updates for Lua content slots: `(slot_name, text)`.
    pub content_text_updates: Vec<(String, String)>,
}

impl TuiPendingChanges {
    pub fn is_empty(&self) -> bool {
        self.route_changes.is_empty()
            && self.preset_switch.is_none()
            && self.new_presets.is_empty()
            && self.new_content_slots.is_empty()
            && self.content_text_updates.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Known names (validation)
// ---------------------------------------------------------------------------

/// Built-in content slot names recognized by the port.
pub const KNOWN_CONTENT_SLOTS: &[&str] = &[
    "conversation",
    "progress",
    "notification",
    "hil_prompt",
    "help",
    "tool_log",
    // "model_stream:<name>" and "lua:<name>" are dynamic — validated by prefix
];

/// Built-in surface names recognized by the port.
pub const KNOWN_SURFACES: &[&str] = &[
    "main_pane",
    "sidebar",
    "overlay",
    "header",
    "input",
    "status_bar",
    "tab_bar",
    "tool_pane",
    "tool_float",
    // "dynamic_pane:<name>" is dynamic — validated by prefix
];

/// Built-in layout preset names.
pub const KNOWN_PRESETS: &[&str] = &["default", "minimal", "wide", "stacked"];

/// Returns true if the content name is a recognized static or dynamic slot.
pub fn is_valid_content_name(name: &str) -> bool {
    KNOWN_CONTENT_SLOTS.contains(&name)
        || name.starts_with("model_stream:")
        || name.starts_with("lua:")
}

/// Returns true if the surface name is a recognized static or dynamic surface.
pub fn is_valid_surface_name(name: &str) -> bool {
    KNOWN_SURFACES.contains(&name) || name.starts_with("dynamic_pane:")
}

// ---------------------------------------------------------------------------
// Port trait
// ---------------------------------------------------------------------------

/// Port for TUI route/layout/content access from scripting.
///
/// String-based interface to avoid presentation-layer type dependencies.
/// Content names: `"conversation"`, `"progress"`, `"tool_log"`, `"model_stream:<name>"`, `"lua:<name>"`.
/// Surface names: `"main_pane"`, `"sidebar"`, `"overlay"`, `"dynamic_pane:<name>"`.
/// Preset names: `"default"`, `"minimal"`, `"wide"`, `"stacked"`, or custom.
pub trait TuiAccessorPort: Send + Sync {
    // -- Routes --

    /// Set a route: map a content slot to a surface.
    fn route_set(&mut self, content: &str, surface: &str) -> Result<(), TuiAccessError>;

    /// Get the surface name currently mapped to a content slot, if any.
    fn route_get(&self, content: &str) -> Option<String>;

    /// List all current routes as `(content_name, surface_name)` pairs.
    fn route_entries(&self) -> Vec<(String, String)>;

    // -- Layout --

    /// Get the name of the current layout preset.
    fn layout_current_preset(&self) -> String;

    /// Switch to a layout preset (built-in or custom).
    fn layout_switch_preset(&mut self, name: &str) -> Result<(), TuiAccessError>;

    /// Register a custom layout preset.
    fn layout_register_preset(
        &mut self,
        name: &str,
        config: CustomPresetConfig,
    ) -> Result<(), TuiAccessError>;

    /// List all available preset names (built-in + custom).
    fn layout_presets(&self) -> Vec<String>;

    // -- Content --

    /// Register a new Lua text-based content slot.
    fn content_register(&mut self, slot_name: &str) -> Result<(), TuiAccessError>;

    /// Update the text content for a Lua-registered slot.
    fn content_set_text(&mut self, slot_name: &str, text: &str) -> Result<(), TuiAccessError>;

    /// List all registered Lua content slot names.
    fn content_slots(&self) -> Vec<String>;

    // -- Change tracking --

    /// Drain all pending changes for the presentation layer to consume.
    fn take_pending_changes(&mut self) -> TuiPendingChanges;
}
