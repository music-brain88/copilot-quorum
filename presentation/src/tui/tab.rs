//! Tab + Pane architecture — Vim-inspired buffer/window/tab model.
//!
//! Maps to Vim's three-layer model:
//! - Buffer → `Interaction` (domain, existing)
//! - Window → `Pane` (presentation, this module)
//! - Tab Page → `Tab` (presentation, this module)
//!
//! Phase 1: each Tab contains exactly one Pane (no splits).

use super::content::{ConversationContent, ProgressContent};
use super::state::DisplayMessage;
use quorum_domain::core::string::truncate;
use quorum_domain::interaction::{InteractionForm, InteractionId};

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
    Interaction(InteractionForm, Option<InteractionId>),
}

impl PaneKind {
    /// Short label for display in the tab bar.
    pub fn label(&self) -> &'static str {
        match self {
            PaneKind::Interaction(InteractionForm::Agent, _) => "Agent",
            PaneKind::Interaction(InteractionForm::Ask, _) => "Ask",
            PaneKind::Interaction(InteractionForm::Discuss, _) => "Discuss",
        }
    }
}

/// A single pane — the minimal rendering unit, owning its own buffers.
pub struct Pane {
    pub id: PaneId,
    pub kind: PaneKind,

    // -- Tab title (auto-generated from first user message) --
    pub title: Option<String>,

    // -- Conversation (messages + streaming + scroll) --
    pub conversation: ConversationContent,

    // -- Input buffer (per-pane, preserves drafts across tab switches) --
    pub input: String,
    pub cursor_pos: usize,

    // -- Progress (per-pane, ready for future parallel execution) --
    pub progress: ProgressContent,
}

impl Pane {
    pub fn new(id: PaneId, kind: PaneKind) -> Self {
        Self {
            id,
            kind,
            title: None,
            conversation: ConversationContent::default(),
            input: String::new(),
            cursor_pos: 0,
            progress: ProgressContent::default(),
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
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Agent, None));
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

    /// Find pane by interaction id (mutable).
    pub fn pane_for_interaction_mut(&mut self, interaction_id: InteractionId) -> Option<&mut Pane> {
        self.find_tab_index_by_interaction(interaction_id)
            .map(move |index| &mut self.tabs[index].pane)
    }

    /// Find tab index by interaction id.
    pub fn find_tab_index_by_interaction(&self, interaction_id: InteractionId) -> Option<usize> {
        self.tabs.iter().position(|tab| match tab.pane.kind {
            PaneKind::Interaction(_, Some(id)) => id == interaction_id,
            PaneKind::Interaction(_, None) => false,
        })
    }

    /// Bind an interaction id to a placeholder tab (one with matching form and no id).
    ///
    /// Used by Fix A (immediate tab creation): `handle_tab_command` creates a tab
    /// with `PaneKind::Interaction(form, None)`, and when `InteractionSpawned`
    /// arrives later, this method associates the real `InteractionId`.
    ///
    /// Returns `true` if a placeholder was found and bound; `false` if a new tab
    /// should be created instead (fallback for programmatic spawns without a
    /// preceding placeholder).
    pub fn bind_interaction_id(
        &mut self,
        form: InteractionForm,
        interaction_id: InteractionId,
    ) -> bool {
        // Find the oldest placeholder tab matching the form
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.pane.kind == PaneKind::Interaction(form, None))
        {
            self.tabs[index].pane.kind = PaneKind::Interaction(form, Some(interaction_id));
            true
        } else {
            false
        }
    }

    /// Push a message to the pane that owns the given interaction id.
    pub fn push_message_to_interaction(
        &mut self,
        interaction_id: InteractionId,
        msg: DisplayMessage,
    ) -> bool {
        if let Some(index) = self.find_tab_index_by_interaction(interaction_id) {
            let pane = &mut self.tabs[index].pane;
            pane.conversation.messages.push(msg);
            if pane.conversation.auto_scroll {
                pane.conversation.scroll_offset = 0;
            }
            true
        } else {
            false
        }
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
            PaneKind::Interaction(InteractionForm::Agent, None)
        );
    }

    #[test]
    fn test_create_tab_switches_to_new() {
        let mut mgr = TabManager::new();
        let id = mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));
        assert_eq!(mgr.len(), 2);
        assert_eq!(mgr.active_index(), 1);
        assert_eq!(mgr.active_tab().id, id);
        assert_eq!(
            mgr.active_pane().kind,
            PaneKind::Interaction(InteractionForm::Ask, None)
        );
    }

    #[test]
    fn test_close_active_not_last() {
        let mut mgr = TabManager::new();
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));
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
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Discuss, None));
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

        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));
        assert!(mgr.active_pane().input.is_empty());

        mgr.prev_tab();
        assert_eq!(mgr.active_pane().input, "draft text");
        assert_eq!(mgr.active_pane().cursor_pos, 10);
    }

    #[test]
    fn test_tab_list_summary() {
        let mut mgr = TabManager::new();
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));

        let summary = mgr.tab_list_summary();
        assert_eq!(summary.len(), 2);
        assert!(summary[0].contains("Agent"));
        assert!(summary[1].starts_with(">"));
        assert!(summary[1].contains("Ask"));
    }

    #[test]
    fn test_find_tab_index_by_interaction() {
        let mut mgr = TabManager::new();
        let ask_id = InteractionId(1);
        let discuss_id = InteractionId(2);
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, Some(ask_id)));
        mgr.create_tab(PaneKind::Interaction(
            InteractionForm::Discuss,
            Some(discuss_id),
        ));

        assert_eq!(mgr.find_tab_index_by_interaction(ask_id), Some(1));
        assert_eq!(mgr.find_tab_index_by_interaction(discuss_id), Some(2));
        assert_eq!(mgr.find_tab_index_by_interaction(InteractionId(999)), None);
    }

    #[test]
    fn test_pane_for_interaction_mut() {
        let mut mgr = TabManager::new();
        let id = InteractionId(7);
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, Some(id)));

        if let Some(pane) = mgr.pane_for_interaction_mut(id) {
            pane.title = Some("Target".into());
        } else {
            panic!("Expected pane for interaction");
        }

        assert_eq!(mgr.tabs()[1].pane.title.as_deref(), Some("Target"));
        assert!(mgr.pane_for_interaction_mut(InteractionId(999)).is_none());
    }

    #[test]
    fn test_push_message_to_interaction() {
        let mut mgr = TabManager::new();
        let interaction_id = InteractionId(42);
        mgr.create_tab(PaneKind::Interaction(
            InteractionForm::Ask,
            Some(interaction_id),
        ));

        let pushed =
            mgr.push_message_to_interaction(interaction_id, DisplayMessage::system("hello"));
        assert!(pushed);
        assert_eq!(mgr.tabs()[1].pane.conversation.messages.len(), 1);
        assert_eq!(mgr.tabs()[1].pane.conversation.messages[0].content, "hello");

        let missing =
            mgr.push_message_to_interaction(InteractionId(999), DisplayMessage::system("nope"));
        assert!(!missing);
    }

    #[test]
    fn test_close_adjusts_active_index() {
        let mut mgr = TabManager::new();
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Discuss, None));
        // Active: index 2 (Discuss)

        mgr.close_active();
        // After closing last, active should be the new last
        assert_eq!(mgr.active_index(), 1);
        assert_eq!(
            mgr.active_pane().kind,
            PaneKind::Interaction(InteractionForm::Ask, None)
        );
    }

    #[test]
    fn test_pane_kind_label() {
        assert_eq!(
            PaneKind::Interaction(InteractionForm::Agent, None).label(),
            "Agent"
        );
        assert_eq!(
            PaneKind::Interaction(InteractionForm::Ask, None).label(),
            "Ask"
        );
        assert_eq!(
            PaneKind::Interaction(InteractionForm::Discuss, None).label(),
            "Discuss"
        );
    }

    #[test]
    fn test_pane_display_title_default() {
        let pane = Pane::new(
            PaneId(0),
            PaneKind::Interaction(InteractionForm::Agent, None),
        );
        assert_eq!(pane.display_title(), "Agent");
    }

    #[test]
    fn test_pane_set_title_from_message() {
        let mut pane = Pane::new(
            PaneId(0),
            PaneKind::Interaction(InteractionForm::Agent, None),
        );
        pane.set_title_if_empty("Fix the auth bug");
        assert_eq!(pane.display_title(), "Fix the auth bug");
    }

    #[test]
    fn test_pane_title_only_set_once() {
        let mut pane = Pane::new(
            PaneId(0),
            PaneKind::Interaction(InteractionForm::Agent, None),
        );
        pane.set_title_if_empty("First message");
        pane.set_title_if_empty("Second message");
        assert_eq!(pane.display_title(), "First message");
    }

    #[test]
    fn test_pane_title_multiline_uses_first_line() {
        let mut pane = Pane::new(
            PaneId(0),
            PaneKind::Interaction(InteractionForm::Agent, None),
        );
        pane.set_title_if_empty("First line\nSecond line\nThird line");
        assert_eq!(pane.display_title(), "First line");
    }

    #[test]
    fn test_pane_title_empty_ignored() {
        let mut pane = Pane::new(
            PaneId(0),
            PaneKind::Interaction(InteractionForm::Agent, None),
        );
        pane.set_title_if_empty("");
        assert_eq!(pane.display_title(), "Agent");
        assert!(pane.title.is_none());
    }

    #[test]
    fn test_pane_title_whitespace_ignored() {
        let mut pane = Pane::new(
            PaneId(0),
            PaneKind::Interaction(InteractionForm::Agent, None),
        );
        pane.set_title_if_empty("   \n  \n  ");
        assert_eq!(pane.display_title(), "Agent");
        assert!(pane.title.is_none());
    }

    #[test]
    fn test_bind_interaction_id_to_placeholder() {
        let mut mgr = TabManager::new();
        // Create placeholder (no interaction id)
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Discuss, None));
        assert_eq!(mgr.len(), 2);

        // Bind interaction id
        let bound = mgr.bind_interaction_id(InteractionForm::Discuss, InteractionId(42));
        assert!(bound);
        assert_eq!(
            mgr.tabs()[1].pane.kind,
            PaneKind::Interaction(InteractionForm::Discuss, Some(InteractionId(42)))
        );
        // Should now be findable
        assert_eq!(
            mgr.find_tab_index_by_interaction(InteractionId(42)),
            Some(1)
        );
    }

    #[test]
    fn test_bind_interaction_id_no_placeholder() {
        let mut mgr = TabManager::new();
        // No placeholder for Discuss — bind should return false
        let bound = mgr.bind_interaction_id(InteractionForm::Discuss, InteractionId(1));
        assert!(!bound);
    }

    #[test]
    fn test_bind_interaction_id_wrong_form() {
        let mut mgr = TabManager::new();
        // Placeholder is Ask, but trying to bind Discuss
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));
        let bound = mgr.bind_interaction_id(InteractionForm::Discuss, InteractionId(1));
        assert!(!bound);
        // Ask placeholder should remain unbound
        assert_eq!(
            mgr.tabs()[1].pane.kind,
            PaneKind::Interaction(InteractionForm::Ask, None)
        );
    }

    #[test]
    fn test_bind_interaction_id_multiple_placeholders_binds_oldest() {
        let mut mgr = TabManager::new();
        // Two Discuss placeholders
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Discuss, None));
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Discuss, None));

        // First bind goes to the oldest placeholder (index 1)
        let bound = mgr.bind_interaction_id(InteractionForm::Discuss, InteractionId(10));
        assert!(bound);
        assert_eq!(
            mgr.tabs()[1].pane.kind,
            PaneKind::Interaction(InteractionForm::Discuss, Some(InteractionId(10)))
        );
        // Second placeholder still unbound
        assert_eq!(
            mgr.tabs()[2].pane.kind,
            PaneKind::Interaction(InteractionForm::Discuss, None)
        );

        // Second bind goes to the remaining placeholder (index 2)
        let bound = mgr.bind_interaction_id(InteractionForm::Discuss, InteractionId(11));
        assert!(bound);
        assert_eq!(
            mgr.tabs()[2].pane.kind,
            PaneKind::Interaction(InteractionForm::Discuss, Some(InteractionId(11)))
        );
    }

    #[test]
    fn test_tab_list_summary_with_title() {
        let mut mgr = TabManager::new();
        mgr.active_pane_mut().set_title_if_empty("Fix the auth bug");
        mgr.create_tab(PaneKind::Interaction(InteractionForm::Ask, None));

        let summary = mgr.tab_list_summary();
        assert_eq!(summary.len(), 2);
        assert!(summary[0].contains("Fix the auth bug"));
        assert!(!summary[0].contains("Agent"));
        assert!(summary[1].contains("Ask"));
    }
}
