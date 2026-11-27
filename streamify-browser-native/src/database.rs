// Streamify Browser v0.2.0 – SQLite database layer
// Ported from v0.1.3 database.rs. Schema: history, bookmarks, sessions, settings.
// SQLite pragmas: WAL, NORMAL sync, 2000 cache, MEMORY temp_store.

use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

use crate::constants::MAX_SESSIONS;

// ── Data types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub visited_at: String,
    pub visit_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub folder: String,
    pub created_at: String,
}

// ── Database wrapper ───────────────────────────────────────────────

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Opens (or creates) the database at the given path and applies pragmas + schema.
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;

        // Performance pragmas (safe, no data-loss risk)
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = 2000;
             PRAGMA temp_store = MEMORY;
             PRAGMA foreign_keys = ON;",
        )?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                url         TEXT NOT NULL,
                title       TEXT NOT NULL DEFAULT '',
                visited_at  TEXT NOT NULL DEFAULT (datetime('now')),
                visit_count INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS bookmarks (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                url        TEXT NOT NULL,
                title      TEXT NOT NULL DEFAULT '',
                folder     TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id       INTEGER PRIMARY KEY AUTOINCREMENT,
                data     TEXT NOT NULL,
                saved_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_history_url ON history(url);
            CREATE INDEX IF NOT EXISTS idx_history_visited ON history(visited_at);
            CREATE INDEX IF NOT EXISTS idx_bookmarks_url ON bookmarks(url);",
        )?;

        Ok(Database {
            conn: Mutex::new(conn),
        })
    }

    // ── History ────────────────────────────────────────────────────

    /// Adds a URL to history. If the URL exists, increments visit_count and updates timestamp.
    pub fn add_history(&self, url: &str, title: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();

        // Check if URL already exists
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM history WHERE url = ?1",
                params![url],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            conn.execute(
                "UPDATE history SET title = ?1, visited_at = datetime('now'),
                 visit_count = visit_count + 1 WHERE id = ?2",
                params![title, id],
            )?;
        } else {
            conn.execute(
                "INSERT INTO history (url, title) VALUES (?1, ?2)",
                params![url, title],
            )?;
        }
        Ok(())
    }

    /// Returns recent history entries, most recent first.
    pub fn get_history(&self, limit: usize) -> SqlResult<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, title, visited_at, visit_count
             FROM history ORDER BY visited_at DESC LIMIT ?1",
        )?;
        let entries = stmt
            .query_map(params![limit as i64], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    visited_at: row.get(3)?,
                    visit_count: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    /// Searches history by URL or title substring.
    pub fn search_history(&self, query: &str, limit: usize) -> SqlResult<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, url, title, visited_at, visit_count
             FROM history WHERE url LIKE ?1 OR title LIKE ?1
             ORDER BY visited_at DESC LIMIT ?2",
        )?;
        let entries = stmt
            .query_map(params![pattern, limit as i64], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    visited_at: row.get(3)?,
                    visit_count: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    /// Clears all history.
    pub fn clear_history(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM history", [])?;
        Ok(())
    }

    // ── Bookmarks ──────────────────────────────────────────────────

    /// Adds a bookmark.
    pub fn add_bookmark(&self, url: &str, title: &str, folder: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO bookmarks (url, title, folder) VALUES (?1, ?2, ?3)",
            params![url, title, folder],
        )?;
        Ok(())
    }

    /// Removes a bookmark by URL.
    pub fn remove_bookmark(&self, url: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM bookmarks WHERE url = ?1", params![url])?;
        Ok(())
    }

    /// Returns all bookmarks.
    pub fn get_bookmarks(&self) -> SqlResult<Vec<Bookmark>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, title, folder, created_at
             FROM bookmarks ORDER BY created_at DESC",
        )?;
        let entries = stmt
            .query_map([], |row| {
                Ok(Bookmark {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    folder: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    /// Checks if a URL is bookmarked.
    pub fn is_bookmarked(&self, url: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM bookmarks WHERE url = ?1",
            params![url],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
            > 0
    }

    // ── Sessions ───────────────────────────────────────────────────

    /// Saves session data as JSON. Keeps only the last MAX_SESSIONS entries.
    pub fn save_session(&self, json_data: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (data) VALUES (?1)",
            params![json_data],
        )?;

        // Keep only the last N sessions
        conn.execute(
            "DELETE FROM sessions WHERE id NOT IN
             (SELECT id FROM sessions ORDER BY saved_at DESC LIMIT ?1)",
            params![MAX_SESSIONS as i64],
        )?;
        Ok(())
    }

    /// Returns the most recent session JSON, if any.
    pub fn get_last_session(&self) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                "SELECT data FROM sessions ORDER BY saved_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok();
        Ok(result)
    }

    // ── Settings ───────────────────────────────────────────────────

    /// Gets a setting value by key.
    pub fn get_setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    /// Sets a setting value (upsert).
    pub fn set_setting(&self, key: &str, value: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
