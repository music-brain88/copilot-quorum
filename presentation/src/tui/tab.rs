//! Tab + Pane architecture — Vim-inspired buffer/window/tab model.
//!
//! Maps to Vim's three-layer model:
//! - Buffer → `Interaction` (domain, existing)
//! - Window → `Pane` (presentation, this module)
//! - Tab Page → `Tab` (presentation, this module)
//!
//! Phase 1: each Tab contains exactly one Pane (no splits).

use super::state::{DisplayMessage, ProgressState};
use quorum_domain::core::string::truncate;
use quorum_domain::interaction::InteractionForm;

/// Unique identifier for a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub usize);

/// Unique identifier for a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub usize);

/// What kind of content a pane displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// An interaction pane — Agent, Ask, or Discuss.
    Interaction(InteractionForm),
}

impl PaneKind {
    /// Short label for display in the tab bar.
    pub fn label(&self) -> &'static str {
        match self {
            PaneKind::Interaction(InteractionForm::Agent) => "Agent",
            PaneKind::Interaction(InteractionForm::Ask) => "Ask",
            PaneKind::Interaction(InteractionForm::Discuss) => "Discuss",
        }
    }
}

/// A single pane — the minimal rendering unit, owning its own buffers.
pub struct Pane {
    pub id: PaneId,
    pub kind: PaneKind,

    // -- Tab title (auto-generated from first user message) --
    pub title: Option<String>,

    // -- Conversation --
    pub messages: Vec<DisplayMessage>,
    pub streaming_text: String,
    pub scroll_offset: usize,
    pub auto_scroll: bool,

    // -- Input buffer (per-pane, preserves drafts across tab switches) --
    pub input: String,
    pub cursor_pos: usize,

    // -- Progress (per-pane, ready for future parallel execution) --
    pub progress: ProgressState,
}

impl Pane {
    pub fn new(id: PaneId, kind: PaneKind) -> Self {
        Self {
            id,
            kind,
            title: None,
            messages: Vec::new(),
            streaming_text: String::new(),
            scroll_offset: 0,
            auto_scroll: true,
            input: String::new(),
            cursor_pos: 0,
            progress: ProgressState::default(),
        }
    }

    /// Display title: custom title if set, otherwise the PaneKind label.
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or(self.kind.label())
    }

    /// Auto-set title from the first user message (idempotent: no-op after first call).
    pub fn set_title_if_empty(&mut self, message: &str) {
        if self.title.is_none() {
            let first_line = message.lines().next().unwrap_or(message).trim();
            if !first_line.is_empty() {
                self.title = Some(truncate(first_line, 30));
            }
        }
    }
}

/// A tab page — contains exactly one pane (Phase 1).
pub struct Tab {
    pub id: TabId,
    pub pane: Pane,
}

/// Manages all open tabs and tracks the active one.
pub struct TabManager {
    tabs: Vec<Tab>,
    active_index: usize,
    next_tab_id: usize,
    next_pane_id: usize,
}

impl TabManager {
    /// Create a new TabManager with a single default Agent tab.
    pub fn new() -> Self {
        let mut mgr = Self {
            tabs: Vec::new(),
            active_index: 0,
            next_tab_id: 0,
            next_pane_id: 0,
        };
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Agent));
        mgr
    }

    /// Create a new tab with the given pane kind. Returns the new TabId.
    /// The new tab becomes active.
    pub fn create_tab(&mut self, kind: PaneKind) -> TabId {
        let tab_id = TabId(self.next_tab_id);
        self.next_tab_id += 1;

        let pane_id = PaneId(self.next_pane_id);
        self.next_pane_id += 1;

        let tab = Tab {
            id: tab_id,
            pane: Pane::new(pane_id, kind),
        };

        self.tabs.push(tab);
        self.active_index = self.tabs.len() - 1;
        tab_id
    }

    /// Close the active tab. Returns false if it's the last tab (won't close).
    pub fn close_active(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        self.tabs.remove(self.active_index);
        // Adjust active index if we removed the last tab
        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }
        true
    }

    /// Switch to the next tab (wrap-around).
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_index = (self.active_index + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab (wrap-around).
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_index = if self.active_index == 0 {
                self.tabs.len() - 1
            } else {
                self.active_index - 1
            };
        }
    }

    /// Get the active tab (immutable).
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_index]
    }

    /// Get the active pane (immutable).
    pub fn active_pane(&self) -> &Pane {
        &self.tabs[self.active_index].pane
    }

    /// Get the active pane (mutable).
    pub fn active_pane_mut(&mut self) -> &mut Pane {
        &mut self.tabs[self.active_index].pane
    }

    /// All tabs (immutable slice).
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Current active tab index (0-based).
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Number of open tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether there are no tabs (always false in practice — at least one exists).
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Summary list for `:tabs` command.
    pub fn tab_list_summary(&self) -> Vec<String> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let marker = if i == self.active_index { ">" } else { " " };
                format!("{} {}: [{}]", marker, i + 1, tab.pane.display_title())
            })
            .collect()
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_default_agent_tab() {
        let mgr = TabManager::new();
        assert_eq!(mgr.len(), 1);
        assert_eq!(mgr.active_index(), 0);
        assert_eq!(
            mgr.active_pane().kind,
            PaneKind::Interaction(InteractionForm::Agent)
        );
    }

    #[test]
    fn test_create_tab_switches_to_new() {
        let mut mgr = TabManager::new();
        let id = mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask));
        assert_eq!(mgr.len(), 2);
        assert_eq!(mgr.active_index(), 1);
        assert_eq!(mgr.active_tab().id, id);
        assert_eq!(
            mgr.active_pane().kind,
            PaneKind::Interaction(InteractionForm::Ask)
        );
    }

    #[test]
    fn test_close_active_not_last() {
        let mut mgr = TabManager::new();
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask));
        assert_eq!(mgr.len(), 2);

        let closed = mgr.close_active();
        assert!(closed);
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn test_close_active_last_tab_refused() {
        let mut mgr = TabManager::new();
        let closed = mgr.close_active();
        assert!(!closed);
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn test_next_prev_tab_wrap() {
        let mut mgr = TabManager::new();
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask));
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Discuss));
        // Active is index 2 (Discuss)

        mgr.next_tab(); // wraps to 0
        assert_eq!(mgr.active_index(), 0);

        mgr.prev_tab(); // wraps to 2
        assert_eq!(mgr.active_index(), 2);

        mgr.prev_tab(); // 1
        assert_eq!(mgr.active_index(), 1);
    }

    #[test]
    fn test_pane_input_preserved() {
        let mut mgr = TabManager::new();
        mgr.active_pane_mut().input = "draft text".into();
        mgr.active_pane_mut().cursor_pos = 10;

        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask));
        assert!(mgr.active_pane().input.is_empty());

        mgr.prev_tab();
        assert_eq!(mgr.active_pane().input, "draft text");
        assert_eq!(mgr.active_pane().cursor_pos, 10);
    }

    #[test]
    fn test_tab_list_summary() {
        let mut mgr = TabManager::new();
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask));

        let summary = mgr.tab_list_summary();
        assert_eq!(summary.len(), 2);
        assert!(summary[0].contains("Agent"));
        assert!(summary[1].starts_with(">"));
        assert!(summary[1].contains("Ask"));
    }

    #[test]
    fn test_close_adjusts_active_index() {
        let mut mgr = TabManager::new();
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask));
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Discuss));
        // Active: index 2 (Discuss)

        mgr.close_active();
        // After closing last, active should be the new last
        assert_eq!(mgr.active_index(), 1);
        assert_eq!(
            mgr.active_pane().kind,
            PaneKind::Interaction(InteractionForm::Ask)
        );
    }

    #[test]
    fn test_pane_kind_label() {
        assert_eq!(
            PaneKind::Interaction(InteractionForm::Agent).label(),
            "Agent"
        );
        assert_eq!(PaneKind::Interaction(InteractionForm::Ask).label(), "Ask");
        assert_eq!(
            PaneKind::Interaction(InteractionForm::Discuss).label(),
            "Discuss"
        );
    }

    #[test]
    fn test_pane_display_title_default() {
        let pane = Pane::new(PaneId(0), PaneKind::Interaction(InteractionForm::Agent));
        assert_eq!(pane.display_title(), "Agent");
    }

    #[test]
    fn test_pane_set_title_from_message() {
        let mut pane = Pane::new(PaneId(0), PaneKind::Interaction(InteractionForm::Agent));
        pane.set_title_if_empty("Fix the auth bug");
        assert_eq!(pane.display_title(), "Fix the auth bug");
    }

    #[test]
    fn test_pane_title_only_set_once() {
        let mut pane = Pane::new(PaneId(0), PaneKind::Interaction(InteractionForm::Agent));
        pane.set_title_if_empty("First message");
        pane.set_title_if_empty("Second message");
        assert_eq!(pane.display_title(), "First message");
    }

    #[test]
    fn test_pane_title_multiline_uses_first_line() {
        let mut pane = Pane::new(PaneId(0), PaneKind::Interaction(InteractionForm::Agent));
        pane.set_title_if_empty("First line\nSecond line\nThird line");
        assert_eq!(pane.display_title(), "First line");
    }

    #[test]
    fn test_pane_title_empty_ignored() {
        let mut pane = Pane::new(PaneId(0), PaneKind::Interaction(InteractionForm::Agent));
        pane.set_title_if_empty("");
        assert_eq!(pane.display_title(), "Agent");
        assert!(pane.title.is_none());
    }

    #[test]
    fn test_pane_title_whitespace_ignored() {
        let mut pane = Pane::new(PaneId(0), PaneKind::Interaction(InteractionForm::Agent));
        pane.set_title_if_empty("   \n  \n  ");
        assert_eq!(pane.display_title(), "Agent");
        assert!(pane.title.is_none());
    }

    #[test]
    fn test_tab_list_summary_with_title() {
        let mut mgr = TabManager::new();
        mgr.active_pane_mut().set_title_if_empty("Fix the auth bug");
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask));

        let summary = mgr.tab_list_summary();
        assert_eq!(summary.len(), 2);
        assert!(summary[0].contains("Fix the auth bug"));
        assert!(!summary[0].contains("Agent"));
        assert!(summary[1].contains("Ask"));
    }
}
