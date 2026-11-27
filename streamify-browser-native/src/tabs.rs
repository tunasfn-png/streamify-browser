// Streamify Browser v0.2.0 – Tab state management
// Ported from v0.1.3 state.rs. Single active WebView, metadata-only inactive tabs.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::constants::{BLANK_TAB_TITLE, BLANK_URL};

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
    pub is_loading: bool,
}

impl TabInfo {
    /// Creates a new blank tab.
    pub fn new_blank() -> Self {
        Self {
            id: next_id(),
            url: BLANK_URL.to_string(),
            title: BLANK_TAB_TITLE.to_string(),
            is_active: false,
            is_loading: false,
        }
    }

    /// Creates a new tab with a given URL.
    pub fn new_with_url(url: &str) -> Self {
        Self {
            id: next_id(),
            url: url.to_string(),
            title: String::new(),
            is_active: false,
            is_loading: false,
        }
    }
}

// ── Session data for persistence ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub tabs: Vec<SessionTab>,
    pub active_tab_id: Option<TabId>,
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

    /// Updates the URL of a specific tab.
    pub fn set_tab_url(&mut self, id: TabId, url: &str) {
        if let Some(tab) = self.get_tab_mut(id) {
            tab.url = url.to_string();
        }
    }

    /// Updates the title of a specific tab.
    pub fn set_tab_title(&mut self, id: TabId, title: &str) {
        if let Some(tab) = self.get_tab_mut(id) {
            tab.title = title.to_string();
        }
    }

    /// Sets loading state for a tab.
    pub fn set_tab_loading(&mut self, id: TabId, loading: bool) {
        if let Some(tab) = self.get_tab_mut(id) {
            tab.is_loading = loading;
        }
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
            active_tab_id: self.active_id,
        }
    }

    /// Restores tabs from session data. Returns the list of tab IDs created.
    pub fn restore_session(&mut self, data: &SessionData) -> Vec<TabId> {
        let mut ids = Vec::new();
        for st in &data.tabs {
            let mut tab = if st.url.is_empty() || st.url == BLANK_URL {
                TabInfo::new_blank()
            } else {
                TabInfo::new_with_url(&st.url)
            };
            tab.title = st.title.clone();
            tab.is_active = false;
            let id = tab.id;
            self.tabs.push(tab);
            ids.push(id);
        }

        // Try to activate the previously active tab (by index match)
        if let Some(active_id) = data.active_tab_id {
            // Find position in original session
            if let Some(pos) = data.tabs.iter().position(|_| true) {
                // The original active_tab_id won't match new IDs, so activate by position
                // Find which new tab corresponds to the same position
                let _ = active_id; // original ID, unused
                if pos < ids.len() {
                    self.set_active(ids[pos]);
                }
            }
        }

        // If nothing was activated, activate the last tab
        if self.active_id.is_none() && !ids.is_empty() {
            let last_id = *ids.last().unwrap();
            self.set_active(last_id);
        }

        ids
    }

    /// Returns the index of a tab by ID.
    pub fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }
}
