// nexus-table — Legacy event-log storage (SqliteStorage).
//
// Backward-compatible event-log persistence. Retained for migration
// scenarios. New code should use SqlBlackboard (blackboard.rs) instead.

use nexus_model::{Storage, StoredEvent};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

/// SQLite-backed FIH event store (legacy). Thread-safe via Mutex.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS fih_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT DEFAULT (datetime('now'))
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS fih_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl Storage for SqliteStorage {
    fn log_fih(&self, event_type: &str, payload: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO fih_events (event_type, payload) VALUES (?1, ?2)",
            params![event_type, payload],
        );
    }

    fn load_events(&self) -> Vec<StoredEvent> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT event_type, payload FROM fih_events ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(StoredEvent {
                    event_type: row.get(0)?,
                    payload: row.get(1)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }
}
