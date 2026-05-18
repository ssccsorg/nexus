// nexus-table — SqliteStorage + SqlBlackboard: SQLite-backed FIH storage.
//
// Two independent implementations:
//   SqliteStorage — event-log only (legacy, backward compat)
//   SqlBlackboard — normalized Cairn-pattern tables, implements Blackboard directly

use nexus_model::{Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent, Storage, StoredEvent};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

// ── Legacy event-log storage ─────────────────────────────────────────────

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
        Ok(Self { conn: Mutex::new(conn) })
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
        Ok(Self { conn: Mutex::new(conn) })
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

// ── Normalized Cairn-pattern Blackboard ──────────────────────────────────

/// Normalized SQLite-backed FIH Blackboard.
///
/// Implements `Blackboard` trait directly against normalized tables
/// (facts, intents, hints, intent_sources). No event replay.
/// Write-through on every mutation.
pub struct SqlBlackboard {
    conn: Mutex<Connection>,
}

impl SqlBlackboard {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        apply_schema(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        apply_schema(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

fn apply_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;

         CREATE TABLE IF NOT EXISTS facts (
             id TEXT NOT NULL,
             project_id TEXT NOT NULL DEFAULT 'default',
             description TEXT NOT NULL,
             PRIMARY KEY (id, project_id)
         );

         CREATE TABLE IF NOT EXISTS intents (
             id TEXT NOT NULL,
             project_id TEXT NOT NULL DEFAULT 'default',
             to_fact_id TEXT,
             description TEXT NOT NULL,
             creator TEXT NOT NULL,
             worker TEXT,
             last_heartbeat_at TEXT,
             created_at TEXT NOT NULL,
             concluded_at TEXT,
             PRIMARY KEY (id, project_id)
         );

         CREATE TABLE IF NOT EXISTS intent_sources (
             intent_id TEXT NOT NULL,
             project_id TEXT NOT NULL,
             fact_id TEXT NOT NULL,
             PRIMARY KEY (intent_id, project_id, fact_id)
         );

         CREATE TABLE IF NOT EXISTS hints (
             id TEXT NOT NULL,
             project_id TEXT NOT NULL DEFAULT 'default',
             content TEXT NOT NULL,
             creator TEXT NOT NULL,
             created_at TEXT NOT NULL,
             PRIMARY KEY (id, project_id)
         );

         CREATE TABLE IF NOT EXISTS schema_version (
             version INTEGER NOT NULL
         );

         INSERT OR IGNORE INTO schema_version (version) VALUES (1);"
    )?;
    Ok(())
}

fn utc_now() -> String {
    chrono_now().unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

fn chrono_now() -> Option<String> {
    // Manual UTC timestamp to avoid chrono dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let secs = d.as_secs();
    // Simple UTC datetime formatting
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;

    // Days since epoch to date
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let months_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 0;
    for (i, &md) in months_days.iter().enumerate() {
        if remaining < md { mo = i + 1; break; }
        remaining -= md;
    }
    if mo == 0 { mo = 12; }
    let day = remaining + 1;

    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, mo, day, h, m, s
    ))
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn project_id() -> &'static str {
    "default"
}

impl Blackboard for SqlBlackboard {
    fn submit_fact(&mut self, fact: &Fact) -> FihHash {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO facts (id, project_id, description) VALUES (?1, ?2, ?3)",
            params![fact.id.0, pid, serde_json::to_string(&fact.content).unwrap_or_default()],
        );
        fact.id.clone()
    }

    fn submit_hint(&mut self, hint: &Hint) {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();
        let now = utc_now();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO hints (id, project_id, content, creator, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hint.id.0, pid, hint.content, hint.creator, now],
        );
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();

        // Validate source facts exist
        for fid in &intent.from_facts {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM facts WHERE id = ?1 AND project_id = ?2",
                    params![fid, pid],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !exists {
                return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
            }
        }

        let now = utc_now();
        let worker = intent.worker.as_deref();
        let heartbeat = if worker.is_some() { Some(&now) } else { None };
        conn.execute(
            "INSERT INTO intents (id, project_id, to_fact_id, description, creator, worker, last_heartbeat_at, created_at, concluded_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, NULL)",
            params![intent.id.0, pid, intent.description, intent.creator, worker, heartbeat, now],
        ).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        for fid in &intent.from_facts {
            conn.execute(
                "INSERT INTO intent_sources (intent_id, project_id, fact_id) VALUES (?1, ?2, ?3)",
                params![intent.id.0, pid, fid],
            ).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        }

        Ok(intent.id.clone())
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();
        let now = utc_now();

        let updated = conn.execute(
            "UPDATE intents SET worker = ?1, last_heartbeat_at = ?2
             WHERE id = ?3 AND project_id = ?4 AND to_fact_id IS NULL",
            params![agent, now, intent_id, pid],
        ).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        if updated == 0 {
            return Err(BlackboardError::NotFound(format!("Intent {intent_id} not found or already concluded")));
        }
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();
        let now = utc_now();

        let updated = conn.execute(
            "UPDATE intents SET worker = ?1, last_heartbeat_at = ?2
             WHERE id = ?3 AND project_id = ?4 AND to_fact_id IS NULL",
            params![agent, now, intent_id, pid],
        ).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        if updated == 0 {
            return Err(BlackboardError::NotFound(format!("Intent {intent_id} not found or already concluded")));
        }
        Ok(())
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();

        let row: Option<(Option<String>,)> = conn
            .query_row(
                "SELECT worker FROM intents WHERE id = ?1 AND project_id = ?2 AND to_fact_id IS NULL",
                params![intent_id, pid],
                |row| Ok((row.get(0)?,)),
            )
            .ok();

        match row {
            None => return Err(BlackboardError::NotFound(format!("Intent {intent_id} not found or already concluded"))),
            Some((Some(ref w),)) if w != agent => {
                return Err(BlackboardError::Forbidden(format!("Intent claimed by {w}")));
            }
            _ => {}
        }

        let _ = conn.execute(
            "UPDATE intents SET worker = NULL WHERE id = ?1 AND project_id = ?2",
            params![intent_id, pid],
        );
        Ok(())
    }

    fn conclude_intent(&mut self, intent_id: &str, result: &serde_json::Value) -> Result<Fact, BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();

        let worker: String = conn
            .query_row(
                "SELECT COALESCE(worker, 'unknown') FROM intents WHERE id = ?1 AND project_id = ?2 AND to_fact_id IS NULL",
                params![intent_id, pid],
                |row| row.get(0),
            )
            .map_err(|_| BlackboardError::NotFound(
                format!("Intent {intent_id} not found or already concluded")
            ))?;

        let now = utc_now();
        let result_str = serde_json::to_string(&result).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        // Create new fact from conclusion
        let new_fact_id = format!("f_concl_{}", intent_id);
        let new_fact = Fact {
            id: FihHash(new_fact_id.clone()),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: worker.clone(),
        };

        let _ = conn.execute(
            "INSERT OR IGNORE INTO facts (id, project_id, description) VALUES (?1, ?2, ?3)",
            params![new_fact_id, pid, result_str],
        );

        let _ = conn.execute(
            "UPDATE intents SET to_fact_id = ?1, worker = ?2, last_heartbeat_at = ?3, concluded_at = ?4
             WHERE id = ?5 AND project_id = ?6",
            params![new_fact_id, worker, now, now, intent_id, pid],
        );

        Ok(new_fact)
    }

    fn read_state(&self) -> BoardState {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();

        let facts: Vec<Fact> = {
            let mut stmt = conn.prepare("SELECT id, description FROM facts WHERE project_id = ?1 ORDER BY id").unwrap();
            stmt.query_map(params![pid], |row| {
                let id: String = row.get(0)?;
                let desc: String = row.get(1)?;
                Ok(Fact {
                    id: FihHash(id),
                    origin: String::new(),
                    content: serde_json::from_str(&desc).unwrap_or(serde_json::Value::String(desc)),
                    creator: String::new(),
                })
            }).unwrap().filter_map(|r| r.ok()).collect()
        };

        let intents: Vec<Intent> = {
            let mut stmt = conn.prepare(
                "SELECT i.id, i.description, i.creator, i.worker, i.created_at, i.concluded_at
                 FROM intents i WHERE i.project_id = ?1 ORDER BY i.created_at"
            ).unwrap();
            stmt.query_map(params![pid], |row| {
                let id: String = row.get(0)?;
                let desc: String = row.get(1)?;
                let creator: String = row.get(2)?;
                let worker: Option<String> = row.get(3)?;
                let concluded_at: Option<String> = row.get(5)?;
                Ok((id, desc, creator, worker, concluded_at))
            }).unwrap().filter_map(|r| r.ok()).map(|(id, desc, creator, worker, concluded)| {
                Intent {
                    id: FihHash(id),
                    from_facts: Vec::new(),
                    description: desc,
                    creator,
                    worker,
                    concluded_at: concluded,
                }
            }).collect()
        };

        let hints: Vec<Hint> = {
            let mut stmt = conn.prepare(
                "SELECT id, content, creator FROM hints WHERE project_id = ?1 ORDER BY created_at"
            ).unwrap();
            stmt.query_map(params![pid], |row| {
                let id: String = row.get(0)?;
                let content: String = row.get(1)?;
                let creator: String = row.get(2)?;
                Ok(Hint {
                    id: FihHash(id),
                    content,
                    creator,
                })
            }).unwrap().filter_map(|r| r.ok()).collect()
        };

        BoardState { facts, intents, hints }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fact(id: &str, content: &str) -> Fact {
        Fact {
            id: FihHash(id.into()),
            origin: "test".into(),
            content: serde_json::Value::String(content.into()),
            creator: "tester".into(),
        }
    }

    fn make_intent(id: &str, from: Vec<&str>, desc: &str) -> Intent {
        Intent {
            id: FihHash(id.into()),
            from_facts: from.into_iter().map(|s| s.to_string()).collect(),
            description: desc.into(),
            creator: "tester".into(),
            worker: None,
            concluded_at: None,
        }
    }

    #[test]
    fn test_sql_blackboard_submit_fact() {
        let mut bb = SqlBlackboard::memory().unwrap();
        let hash = bb.submit_fact(&make_fact("f001", "test fact"));
        assert_eq!(hash.0, "f001");

        let state = bb.read_state();
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].id.0, "f001");
    }

    #[test]
    fn test_sql_blackboard_submit_intent() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_fact(&make_fact("f001", "source fact"));

        let hash = bb.submit_intent(&make_intent("i001", vec!["f001"], "test intent")).unwrap();
        assert_eq!(hash.0, "i001");

        let state = bb.read_state();
        assert_eq!(state.intents.len(), 1);
    }

    #[test]
    fn test_sql_blackboard_intent_missing_fact() {
        let mut bb = SqlBlackboard::memory().unwrap();
        let result = bb.submit_intent(&make_intent("i001", vec!["f_nonexistent"], "test"));
        assert!(result.is_err());
    }

    #[test]
    fn test_sql_blackboard_heartbeat_and_conclude() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_fact(&make_fact("f001", "source"));
        bb.submit_intent(&make_intent("i001", vec!["f001"], "explore")).unwrap();

        bb.heartbeat("i001", "agent-a").unwrap();

        let result = serde_json::Value::String("discovery!".into());
        let fact = bb.conclude_intent("i001", &result).unwrap();
        assert_eq!(fact.content, "discovery!");

        let state = bb.read_state();
        assert!(state.facts.iter().any(|f| f.content == "discovery!"), "concluded fact exists");
    }

    #[test]
    fn test_sql_blackboard_release_intent() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_fact(&make_fact("f001", "source"));
        bb.submit_intent(&make_intent("i001", vec!["f001"], "explore")).unwrap();
        bb.heartbeat("i001", "agent-a").unwrap();
        bb.release_intent("i001", "agent-a").unwrap();

        let state = bb.read_state();
        let intent = state.intents.iter().find(|i| i.id.0 == "i001").unwrap();
        assert!(intent.worker.is_none(), "released intent has no worker");
    }

    #[test]
    fn test_sql_blackboard_concurrent_session() {
        let path = "test_sql_bb.db";
        let _ = std::fs::remove_file(path);

        // Session 1
        {
            let mut bb = SqlBlackboard::open(path).unwrap();
            bb.submit_fact(&make_fact("f001", "persistent fact"));
            bb.submit_fact(&make_fact("f002", "another fact"));
            bb.submit_intent(&make_intent("i001", vec!["f001"], "persistent intent")).unwrap();
            assert_eq!(bb.read_state().facts.len(), 2);
        }

        // Session 2: reload
        {
            let bb = SqlBlackboard::open(path).unwrap();
            let state = bb.read_state();
            assert_eq!(state.facts.len(), 2, "facts restored");
            assert_eq!(state.intents.len(), 1, "intents restored");
            assert!(state.facts.iter().any(|f| f.id.0 == "f001"));
            assert!(state.intents.iter().any(|i| i.id.0 == "i001"));
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_sql_blackboard_hint() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_hint(&Hint {
            id: FihHash("h001".into()),
            content: "check the web service first".into(),
            creator: "analyst".into(),
        });

        let state = bb.read_state();
        assert_eq!(state.hints.len(), 1);
        assert_eq!(state.hints[0].content, "check the web service first");
    }

    #[test]
    fn test_sqlite_storage_backward_compat() {
        let store = SqliteStorage::memory().unwrap();
        store.log_fih("test_event", "{\"key\": \"value\"}");
        let events = store.load_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "test_event");
    }
}
