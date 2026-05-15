// nexus-sql — SqlBlackboard: SQLite-backed FIH storage implementation.
//
// Implements the Storage trait from nexus-graph for persistent FIH event logs.

use nexus_graph::{Blackboard, Fact, FihHash, GraphBlackboard, Intent, Storage, StoredEvent};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

/// SQLite-backed FIH event store. Thread-safe via Mutex.
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
            );"
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS fih_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL
            );"
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

impl Storage for SqliteStorage {
    fn log_fih(&self, event_type: &str, payload: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO fih_events (event_type, payload) VALUES (?1, ?2)",
            rusqlite::params![event_type, payload],
        );
    }

    fn load_events(&self) -> Vec<StoredEvent> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT event_type, payload FROM fih_events ORDER BY id")
            .unwrap();
        let rows = stmt.query_map([], |row| {
            Ok(StoredEvent {
                event_type: row.get(0)?,
                payload: row.get(1)?,
            })
        }).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }
}

/// Convenience: create a persistent GraphBlackboard backed by SQLite.
pub fn blackboard_with_sqlite(path: &str) -> Result<GraphBlackboard, String> {
    let store = SqliteStorage::open(path).map_err(|e| e.to_string())?;
    Ok(GraphBlackboard::new().with_storage(Box::new(store)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_persistence() {
        let path = "test_persist_sql.db";
        let _ = std::fs::remove_file(path);

        // Session 1: write
        {
            let mut bb = blackboard_with_sqlite(path).unwrap();
            bb.submit_fact(&Fact {
                id: FihHash("f_sql_1".into()),
                origin: "sql_test".into(),
                content: "SQLite persistence test".into(),
                creator: "agent-x".into(),
            });
            assert_eq!(bb.read_state().facts.len(), 1, "1 fact after submit");

            bb.submit_intent(&Intent {
                id: FihHash("i_sql_1".into()),
                from_facts: vec!["f_sql_1".into()],
                description: "SQL hypothesis".into(),
                creator: "agent-x".into(),
                worker: None,
                concluded_at: None,
            }).unwrap();
            assert_eq!(bb.read_state().intents.len(), 1, "1 intent");

            bb.claim_intent("i_sql_1", "agent-x").unwrap();
            let (new_fact, follow_ups) = bb.conclude_intent("i_sql_1", "SQLite works").unwrap();
            assert_eq!(new_fact.content, "SQLite works");

            // Submit follow-up intent so it's persisted too
            for fu in &follow_ups {
                bb.submit_intent(fu).unwrap();
            }

            let state = bb.read_state();
            assert_eq!(state.facts.len(), 2, "1 submitted + 1 concluded = 2 facts");
            assert_eq!(state.intents.len(), 2, "1 original + 1 follow-up = 2 intents");
        }

        // Session 2: reload and verify
        {
            let mut bb = blackboard_with_sqlite(path).unwrap();
            let state = bb.read_state();
            assert_eq!(state.facts.len(), 2, "facts restored");
            assert_eq!(state.intents.len(), 2, "intents restored");
            assert!(state.facts.iter().any(|f| f.id.0 == "f_sql_1"), "f_sql_1 restored");
            assert!(state.intents.iter().any(|i| i.id.0 == "i_sql_1"), "i_sql_1 restored");

            // Concluded intent should have no worker
            let concluded = state.intents.iter().find(|i| i.id.0 == "i_sql_1").unwrap();
            assert!(concluded.worker.is_none(), "concluded intent has no worker");
            assert!(concluded.concluded_at.is_some(), "concluded intent has timestamp");

            // Continue working after reload
            bb.submit_fact(&Fact {
                id: FihHash("f_sql_2".into()),
                origin: "sql_test".into(),
                content: "After reload".into(),
                creator: "agent-x".into(),
            });
            assert_eq!(bb.read_state().facts.len(), 3, "3 facts after adding one more");
        }

        let _ = std::fs::remove_file(path);
        println!("  ✓ SqlBlackboard: write → reload → verify → continue");
    }
}
