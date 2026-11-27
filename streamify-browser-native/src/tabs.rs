// Streamify Browser v0.2.0 – Tab state management
// Ported from v0.1.3 state.rs. Single active WebView, metadata-only inactive tabs.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::constants::{BLANK_TAB_TITLE, BLANK_URL};
use crate::url_util;

// ── Tab data ───────────────────────────────────────────────────────

/// Unique tab identifier (monotonically increasing within session).
pub type TabId = u32;

/// Counter for generating unique tab IDs.
static NEXT_TAB_ID: AtomicU32 = AtomicU32::new(1);

fn next_id() -> TabId {
    NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed)
}

/// Metadata for a single tab. Only the active tab has a live WebView2 controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub is_active: bool,
}

impl TabInfo {
    /// Creates a new blank tab.
    pub fn new_blank() -> Self {
        Self {
            id: next_id(),
            url: BLANK_URL.to_string(),
            title: BLANK_TAB_TITLE.to_string(),
            is_active: false,
        }
    }

    /// Creates a new tab with a given URL.
    pub fn new_with_url(url: &str) -> Self {
        Self {
            id: next_id(),
            url: url.to_string(),
            title: String::new(),
            is_active: false,
        }
    }
}

// ── Session data for persistence ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub tabs: Vec<SessionTab>,
    #[serde(default)]
    pub active_tab_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
}

// ── Tab collection ─────────────────────────────────────────────────

/// Manages the list of tabs and the active tab index.
/// All access is from the main (UI) thread, so no Mutex needed —
/// the Win32 message loop is single-threaded.
pub struct TabState {
    tabs: Vec<TabInfo>,
    active_id: Option<TabId>,
}

impl TabState {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_id: None,
        }
    }

    /// Adds a tab and returns its ID.
    pub fn add_tab(&mut self, mut tab: TabInfo) -> TabId {
        let id = tab.id;
        // Deactivate all existing tabs
        for t in &mut self.tabs {
            t.is_active = false;
        }
        tab.is_active = true;
        self.active_id = Some(id);
        self.tabs.push(tab);
        id
    }

    /// Removes a tab by ID. Returns the removed tab, if found.
    pub fn remove_tab(&mut self, id: TabId) -> Option<TabInfo> {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            let removed = self.tabs.remove(pos);
            // If we removed the active tab, activate the next one
            if self.active_id == Some(id) {
                self.active_id = None;
                if !self.tabs.is_empty() {
                    // Activate the tab at the same position, or the last one
                    let new_pos = pos.min(self.tabs.len() - 1);
                    self.tabs[new_pos].is_active = true;
                    self.active_id = Some(self.tabs[new_pos].id);
                }
            }
            Some(removed)
        } else {
            None
        }
    }

    /// Sets the active tab by ID, deactivating all others.
    pub fn set_active(&mut self, id: TabId) -> bool {
        let found = self.tabs.iter().any(|t| t.id == id);
        if found {
            for t in &mut self.tabs {
                t.is_active = t.id == id;
            }
            self.active_id = Some(id);
        }
        found
    }

    /// Returns the active tab ID, if any.
    pub fn active_id(&self) -> Option<TabId> {
        self.active_id
    }

    /// Returns a reference to the active tab, if any.
    pub fn active_tab(&self) -> Option<&TabInfo> {
        self.active_id
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    /// Returns a mutable reference to the active tab.
    pub fn active_tab_mut(&mut self) -> Option<&mut TabInfo> {
        let id = self.active_id?;
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Returns all tabs as a slice.
    pub fn tabs(&self) -> &[TabInfo] {
        &self.tabs
    }

    /// Returns the number of tabs.
    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    /// Finds a tab by ID.
    pub fn get_tab(&self, id: TabId) -> Option<&TabInfo> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// Finds a mutable tab by ID.
    pub fn get_tab_mut(&mut self, id: TabId) -> Option<&mut TabInfo> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Builds session data for persistence.
    pub fn get_session_data(&self) -> SessionData {
        SessionData {
            tabs: self
                .tabs
                .iter()
                .map(|t| SessionTab {
                    url: t.url.clone(),
                    title: t.title.clone(),
                })
                .collect(),
            active_tab_index: self
                .active_id
                .and_then(|id| self.tabs.iter().position(|tab| tab.id == id)),
        }
    }

    pub fn restore_session(&mut self, data: &SessionData) {
        self.tabs.clear();
        self.active_id = None;
        self.tabs.extend(data.tabs.iter().map(|saved| {
            if url_util::is_http(&saved.url) {
                let mut tab = TabInfo::new_with_url(&saved.url);
                tab.title.clone_from(&saved.title);
                tab
            } else {
                TabInfo::new_blank()
            }
        }));
        if let Some(tab) = self
            .tabs
            .get(data.active_tab_index.unwrap_or(0))
            .or_else(|| self.tabs.first())
        {
            self.set_active(tab.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_the_selected_tab_with_new_ids() {
        let mut original = TabState::new();
        original.add_tab(TabInfo::new_with_url("https://open.spotify.com/"));
        let old_id = original.add_tab(TabInfo::new_with_url(
            "https://www.twitch.tv/popout/channel/chat",
        ));
        let mut restored = TabState::new();
        restored.restore_session(&original.get_session_data());
        assert_ne!(restored.active_id(), Some(old_id));
        assert_eq!(
            restored.active_tab().unwrap().url,
            "https://www.twitch.tv/popout/channel/chat"
        );
    }

    #[test]
    fn reads_legacy_sessions_and_rejects_non_web_urls() {
        let session: SessionData = serde_json::from_str(
            r#"{"tabs":[{"url":"file:///private","title":"Local"}],"active_tab_id":24}"#,
        )
        .unwrap();
        let mut restored = TabState::new();
        restored.restore_session(&session);
        assert_eq!(restored.active_tab().unwrap().url, BLANK_URL);
    }
}
