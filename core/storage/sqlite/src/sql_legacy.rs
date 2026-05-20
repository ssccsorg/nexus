// nexus-storage-sqlite — SqliteStorage: legacy event-log storage.
//
// Backward-compatible event-log persistence. Implements capability-based
// Storage traits (`StorageRead`, `FactCapable`, `IntentCapable`, `HintCapable`,
// `FilterCapable`, `FihPersistence`, `ColdStorage`) by serializing FIH
// operations as JSON events in the `fih_events` table. Retained for migration
// scenarios. New code should use `SqlNormalizedStorage`.

use nexus_model::{
    BlackboardError, BoardState, Fact, FactCapable, FihHash, FilterCapable, Hint, HintCapable,
    Intent, IntentCapable, StateFilter, StorageRead, StoredEvent,
};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

/// SQLite-backed FIH event store (legacy). Thread-safe via Mutex.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// Open or create a persistent database at `path`.
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

    /// Create an in-memory database (for testing).
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

    /// Log a raw event. Kept for backward compatibility.
    pub fn log_fih(&self, event_type: &str, payload: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO fih_events (event_type, payload) VALUES (?1, ?2)",
            params![event_type, payload],
        );
    }

    /// Load all events. Kept for backward compatibility.
    pub fn load_events(&self) -> Vec<StoredEvent> {
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

impl StorageRead for SqliteStorage {
    fn project_id(&self) -> &str {
        "default"
    }

    fn read_state(&self) -> BoardState {
        // Replay all events to reconstruct state.
        let mut facts: Vec<Fact> = Vec::new();
        let mut intents: Vec<Intent> = Vec::new();
        let mut hints: Vec<Hint> = Vec::new();

        for event in self.load_events() {
            match event.event_type.as_str() {
                "submit_fact" => {
                    if let Ok(f) = serde_json::from_str::<Fact>(&event.payload) {
                        facts.push(f);
                    }
                }
                "submit_hint" => {
                    if let Ok(h) = serde_json::from_str::<Hint>(&event.payload) {
                        hints.push(h);
                    }
                }
                "submit_intent" => {
                    if let Ok(i) = serde_json::from_str::<Intent>(&event.payload) {
                        intents.push(i);
                    }
                }
                "claim_intent" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                        let id = v["id"].as_str().unwrap_or("");
                        let agent = v["agent"].as_str().unwrap_or("");
                        if let Some(i) = intents.iter_mut().find(|i| i.id.0 == id) {
                            i.worker = Some(agent.to_string());
                            i.last_heartbeat_at = Some("now".into());
                        }
                    }
                }
                "heartbeat" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                        let id = v["id"].as_str().unwrap_or("");
                        let agent = v["agent"].as_str().unwrap_or("");
                        if let Some(i) = intents.iter_mut().find(|i| i.id.0 == id) {
                            i.worker = Some(agent.to_string());
                            i.last_heartbeat_at = Some("now".into());
                        }
                    }
                }
                "release_intent" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                        let id = v["id"].as_str().unwrap_or("");
                        if let Some(i) = intents.iter_mut().find(|i| i.id.0 == id) {
                            i.worker = None;
                        }
                    }
                }
                "conclude_intent" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                        let id = v["id"].as_str().unwrap_or("");
                        let result = v["result"].as_str().unwrap_or("");
                        if let Some(i) = intents.iter_mut().find(|i| i.id.0 == id) {
                            i.concluded_at = Some("now".into());
                            i.to_fact_id = Some(format!("f_concl_{}", id));
                        }
                        let new_fact = Fact {
                            id: FihHash(format!("f_concl_{}", id)),
                            origin: format!("conclusion:{}", id),
                            content: serde_json::Value::String(result.to_string()),
                            creator: String::new(),
                        };
                        facts.push(new_fact);
                    }
                }
                _ => {}
            }
        }

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

impl FactCapable for SqliteStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let payload =
            serde_json::to_string(fact).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.log_fih("submit_fact", &payload);
        Ok(fact.id.clone())
    }
}

impl IntentCapable for SqliteStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let payload =
            serde_json::to_string(intent).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.log_fih("submit_intent", &payload);
        Ok(intent.id.clone())
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let payload = serde_json::json!({"id": intent_id, "agent": agent}).to_string();
        self.log_fih("claim_intent", &payload);
        Ok(())
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let payload = serde_json::json!({"id": intent_id, "agent": agent}).to_string();
        self.log_fih("heartbeat", &payload);
        Ok(())
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let payload = serde_json::json!({"id": intent_id, "agent": agent}).to_string();
        self.log_fih("release_intent", &payload);
        Ok(())
    }

    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let result_str = result
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| result.to_string());
        let payload = serde_json::json!({"id": intent_id, "result": result_str}).to_string();
        self.log_fih("conclude_intent", &payload);
        let new_fact_id = format!("f_concl_{}", intent_id);
        Ok(Fact {
            id: FihHash(new_fact_id),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: String::new(),
        })
    }
}

impl HintCapable for SqliteStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let payload =
            serde_json::to_string(hint).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.log_fih("submit_hint", &payload);
        Ok(())
    }
}

impl FilterCapable for SqliteStorage {
    fn read_state_filtered(&self, _filter: &StateFilter) -> BoardState {
        // For now, returns the full state. SQL-level filtering to be added later.
        self.read_state()
    }
}
