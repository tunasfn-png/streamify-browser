// Streamify Browser v0.2.0 – SQLite database layer
// Ported from v0.1.3 database.rs. Schema: history, bookmarks, sessions, settings.
// SQLite pragmas: WAL, NORMAL sync, 512 KiB page cache, MEMORY temp_store.

use rusqlite::{
    params, Connection, OptionalExtension, Result as SqlResult, Transaction, TransactionBehavior,
};
use std::path::Path;

use crate::constants::MAX_SESSIONS;

pub struct Bookmark {
    pub url: String,
    pub title: String,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens (or creates) the database at the given path and applies pragmas + schema.
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;

        // Performance pragmas (safe, no data-loss risk)
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -512;
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

        Ok(Database { conn })
    }

    // ── History ────────────────────────────────────────────────────

    /// Adds a URL to history. If the URL exists, increments visit_count and updates timestamp.
    pub fn add_history(&self, url: &str, title: &str) -> SqlResult<()> {
        let conn = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;

        // Check if URL already exists
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM history WHERE url = ?1 ORDER BY id LIMIT 1",
                params![url],
                |row| row.get(0),
            )
            .optional()?;

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
        conn.commit()
    }

    // ── Bookmarks ──────────────────────────────────────────────────

    /// Adds a bookmark.
    pub fn add_bookmark(&self, url: &str, title: &str, folder: &str) -> SqlResult<()> {
        let conn = &self.conn;
        conn.execute(
            "INSERT INTO bookmarks (url, title, folder) VALUES (?1, ?2, ?3)",
            params![url, title, folder],
        )?;
        Ok(())
    }

    /// Removes a bookmark by URL.
    pub fn remove_bookmark(&self, url: &str) -> SqlResult<()> {
        let conn = &self.conn;
        conn.execute("DELETE FROM bookmarks WHERE url = ?1", params![url])?;
        Ok(())
    }

    pub fn get_bookmarks(&self, limit: u32) -> SqlResult<Vec<Bookmark>> {
        let mut stmt = self
            .conn
            .prepare("SELECT url, title FROM bookmarks ORDER BY id DESC LIMIT ?1")?;
        let entries = stmt.query_map([limit], |row| {
            Ok(Bookmark {
                url: row.get(0)?,
                title: row.get(1)?,
            })
        })?;
        entries.collect()
    }

    /// Checks if a URL is bookmarked.
    pub fn is_bookmarked(&self, url: &str) -> bool {
        let conn = &self.conn;
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
        let conn = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        conn.execute("INSERT INTO sessions (data) VALUES (?1)", [json_data])?;
        conn.execute(
            "DELETE FROM sessions WHERE id NOT IN
             (SELECT id FROM sessions ORDER BY id DESC LIMIT ?1)",
            [MAX_SESSIONS as i64],
        )?;
        conn.commit()
    }

    pub fn get_last_session(&self) -> SqlResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT data FROM sessions ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_session_wins_when_timestamps_match() {
        let db = Database::open(Path::new(":memory:")).unwrap();
        for value in 0..=MAX_SESSIONS {
            db.save_session(&value.to_string()).unwrap();
        }
        assert_eq!(
            db.get_last_session().unwrap(),
            Some(MAX_SESSIONS.to_string())
        );
        let count: usize = db
            .conn
            .query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, MAX_SESSIONS);
    }
}
