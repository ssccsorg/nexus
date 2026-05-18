// nexus-table — SqliteStorage + SqlBlackboard: SQLite-backed FIH storage.
//
// Two independent implementations:
//   SqliteStorage — event-log only (legacy, backward compat)
//   SqlBlackboard — normalized Cairn-pattern tables, implements Blackboard directly

use nexus_model::{
    Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent, Storage, StoredEvent,
};
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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        apply_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
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
             creator TEXT,
             origin TEXT,
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
             PRIMARY KEY (intent_id, project_id, fact_id),
             FOREIGN KEY (intent_id, project_id) REFERENCES intents(id, project_id) ON DELETE CASCADE
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

         INSERT OR IGNORE INTO schema_version (version) VALUES (1);",
    )?;
    Ok(())
}

fn utc_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();

    // Seconds since epoch to broken-down time
    let secs_per_day: i64 = 86400;
    let mut days = (secs / secs_per_day as u64) as i64;
    let day_secs = (secs % secs_per_day as u64) as i64;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    // Civil date from days since epoch
    let mut y = 1970i64;
    loop {
        let days_in_year = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 1u32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        mo += 1;
    }
    let day = days + 1;

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, day, h, m, s)
}

fn project_id() -> &'static str {
    "default"
}

impl SqlBlackboard {
    /// Claim or heartbeat an open intent. Shared by `claim_intent` and `heartbeat`.
    fn set_worker(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();
        let now = utc_now();

        let updated = conn
            .execute(
                "UPDATE intents SET worker = ?1, last_heartbeat_at = ?2
             WHERE id = ?3 AND project_id = ?4 AND to_fact_id IS NULL",
                params![agent, now, intent_id, pid],
            )
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        if updated == 0 {
            return Err(BlackboardError::NotFound(format!(
                "Intent {intent_id} not found or already concluded"
            )));
        }
        Ok(())
    }
}

impl Blackboard for SqlBlackboard {
    fn submit_fact(&mut self, fact: &Fact) -> FihHash {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO facts (id, project_id, description, creator, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![fact.id.0, pid, serde_json::to_string(&fact.content).unwrap_or_default(), fact.creator, fact.origin],
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
            )
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        }

        Ok(intent.id.clone())
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.set_worker(intent_id, agent)
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.set_worker(intent_id, agent)
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
            None => {
                return Err(BlackboardError::NotFound(format!(
                    "Intent {intent_id} not found or already concluded"
                )));
            }
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

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let mut conn = self.conn.lock().unwrap();
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
        let result_str =
            serde_json::to_string(&result).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        // Create new fact from conclusion
        let new_fact_id = format!("f_concl_{}", intent_id);
        let new_fact = Fact {
            id: FihHash(new_fact_id.clone()),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: worker.clone(),
        };

        // Atomic: insert conclusion fact + update intent in one transaction
        let tx = conn
            .transaction()
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        tx.execute(
            "INSERT OR IGNORE INTO facts (id, project_id, description, creator, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_fact_id, pid, result_str, &worker, &new_fact.origin],
        ).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        tx.execute(
            "UPDATE intents SET to_fact_id = ?1, worker = ?2, last_heartbeat_at = ?3, concluded_at = ?4
             WHERE id = ?5 AND project_id = ?6",
            params![new_fact_id, &worker, &now, &now, intent_id, pid],
        ).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        tx.commit()
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        Ok(new_fact)
    }

    fn read_state(&self) -> BoardState {
        let conn = self.conn.lock().unwrap();
        let pid = project_id();

        let facts: Vec<Fact> = {
            let mut stmt = conn.prepare("SELECT id, description, creator, origin FROM facts WHERE project_id = ?1 ORDER BY id").unwrap();
            stmt.query_map(params![pid], |row| {
                let id: String = row.get(0)?;
                let desc: String = row.get(1)?;
                let creator: String = row.get(2).unwrap_or_default();
                let origin: String = row.get(3).unwrap_or_default();
                Ok(Fact {
                    id: FihHash(id),
                    origin,
                    content: serde_json::from_str(&desc).unwrap_or(serde_json::Value::String(desc)),
                    creator,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };

        // Load all intent_source mappings for the project in one pass
        let source_map: std::collections::HashMap<String, Vec<String>> = {
            let mut stmt = conn.prepare(
                "SELECT intent_id, fact_id FROM intent_sources WHERE project_id = ?1 ORDER BY rowid"
            ).unwrap();
            let mut map: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for row in stmt
                .query_map(params![pid], |row| {
                    let iid: String = row.get(0)?;
                    let fid: String = row.get(1)?;
                    Ok((iid, fid))
                })
                .unwrap()
                .filter_map(|r| r.ok())
            {
                map.entry(row.0).or_default().push(row.1);
            }
            map
        };

        let intents: Vec<Intent> = {
            let mut stmt = conn
                .prepare(
                    "SELECT i.id, i.description, i.creator, i.worker,
                            i.to_fact_id, i.last_heartbeat_at, i.created_at, i.concluded_at
                 FROM intents i WHERE i.project_id = ?1 ORDER BY i.created_at",
                )
                .unwrap();
            stmt.query_map(params![pid], |row| {
                let id: String = row.get(0)?;
                let desc: String = row.get(1)?;
                let creator: String = row.get(2)?;
                let worker: Option<String> = row.get(3)?;
                let to_fact_id: Option<String> = row.get(4)?;
                let last_heartbeat_at: Option<String> = row.get(5)?;
                let created_at: String = row.get(6)?;
                let concluded_at: Option<String> = row.get(7)?;
                Ok((
                    id,
                    desc,
                    creator,
                    worker,
                    to_fact_id,
                    last_heartbeat_at,
                    created_at,
                    concluded_at,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .map(
                |(
                    id,
                    desc,
                    creator,
                    worker,
                    to_fact_id,
                    last_heartbeat_at,
                    created_at,
                    concluded,
                )| Intent {
                    id: FihHash(id.clone()),
                    from_facts: source_map.get(&id).cloned().unwrap_or_default(),
                    description: desc,
                    creator,
                    worker,
                    to_fact_id,
                    last_heartbeat_at,
                    created_at: Some(created_at),
                    concluded_at: concluded,
                },
            )
            .collect()
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
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };

        BoardState {
            facts,
            intents,
            hints,
        }
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
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
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

        let hash = bb
            .submit_intent(&make_intent("i001", vec!["f001"], "test intent"))
            .unwrap();
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
        bb.submit_intent(&make_intent("i001", vec!["f001"], "explore"))
            .unwrap();

        bb.heartbeat("i001", "agent-a").unwrap();

        let result = serde_json::Value::String("discovery!".into());
        let fact = bb.conclude_intent("i001", &result).unwrap();
        assert_eq!(fact.content, "discovery!");

        let state = bb.read_state();
        assert!(
            state.facts.iter().any(|f| f.content == "discovery!"),
            "concluded fact exists"
        );
    }

    #[test]
    fn test_sql_blackboard_release_intent() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_fact(&make_fact("f001", "source"));
        bb.submit_intent(&make_intent("i001", vec!["f001"], "explore"))
            .unwrap();
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
            bb.submit_intent(&make_intent("i001", vec!["f001"], "persistent intent"))
                .unwrap();
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

    // ── from_facts populated via intent_sources join ──────────────────────
    #[test]
    fn test_intent_sources_join_on_read() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_fact(&make_fact("f001", "observation"));
        bb.submit_fact(&make_fact("f002", "inference"));
        bb.submit_fact(&make_fact("f003", "conclusion"));

        // Hyperedge: one intent grounded in multiple facts
        let intent = Intent {
            id: FihHash("i_hyper_001".into()),
            from_facts: vec!["f001".into(), "f002".into(), "f003".into()],
            description: "multi-source analysis".into(),
            creator: "researcher".into(),
            worker: Some("researcher".into()),
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        };
        bb.submit_intent(&intent).unwrap();

        let state = bb.read_state();
        let i = state
            .intents
            .iter()
            .find(|i| i.id.0 == "i_hyper_001")
            .unwrap();
        assert_eq!(i.from_facts.len(), 3, "all 3 source facts restored");
        assert!(i.from_facts.contains(&"f001".to_string()));
        assert!(i.from_facts.contains(&"f002".to_string()));
        assert!(i.from_facts.contains(&"f003".to_string()));
    }

    // ── Playbook scenario: full SRE agent lifecycle ────────────────────────
    #[test]
    fn test_playbook_sre_lifecycle() {
        let mut bb = SqlBlackboard::memory().unwrap();

        // CI bot submits deploy fact with structured JSON
        bb.submit_fact(&Fact {
            id: FihHash("f_deploy_001".into()),
            origin: "ci-bot".into(),
            content: serde_json::json!({
                "event": "deploy_complete",
                "service": "api-gateway",
                "version": "v2.4.1",
                "duration_ms": 3420,
                "status": "success"
            }),
            creator: "ci-bot".into(),
        });

        // SRE agent creates intent
        bb.submit_intent(&Intent {
            id: FihHash("i_sre_001".into()),
            from_facts: vec!["f_deploy_001".into()],
            description: "Investigate deploy duration regression (3.4s vs 1.2s baseline)".into(),
            creator: "sre-agent".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        })
        .unwrap();

        bb.heartbeat("i_sre_001", "sre-agent").unwrap();

        let finding = serde_json::json!({
            "finding": "healthcheck timeout regression",
            "fix": "Reduce timeout to 5s",
            "effort_hours": 0.5
        });
        let concluded = bb.conclude_intent("i_sre_001", &finding).unwrap();
        assert_eq!(
            concluded.content["finding"],
            "healthcheck timeout regression"
        );

        let state = bb.read_state();
        assert!(state.facts.iter().any(|f| f.creator == "sre-agent"));
    }

    // ── Multi-blackboard: isolated research contexts ───────────────────────
    #[test]
    fn test_multi_blackboard_isolation() {
        let mut bb_sensor = SqlBlackboard::memory().unwrap();
        let mut bb_knowledge = SqlBlackboard::memory().unwrap();

        bb_sensor.submit_fact(&make_fact("f_s1", "sensor reading"));
        bb_knowledge.submit_fact(&make_fact("f_k1", "knowledge graph node"));

        bb_sensor
            .submit_intent(&make_intent("i_s1", vec!["f_s1"], "analyze sensor"))
            .unwrap();
        bb_knowledge
            .submit_intent(&make_intent("i_k1", vec!["f_k1"], "query knowledge"))
            .unwrap();

        assert_eq!(
            bb_sensor.read_state().facts.len(),
            1,
            "sensor bb has 1 fact"
        );
        assert_eq!(
            bb_knowledge.read_state().facts.len(),
            1,
            "knowledge bb has 1 fact"
        );
        assert_eq!(bb_sensor.read_state().facts[0].id.0, "f_s1");
        assert_eq!(bb_knowledge.read_state().facts[0].id.0, "f_k1");
    }

    // ── Multi-agent handoff: two agents, one intent ────────────────────────
    #[test]
    fn test_multi_agent_handoff() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_fact(&make_fact("f001", "discovery"));
        bb.submit_intent(&make_intent("i001", vec!["f001"], "explore anomaly"))
            .unwrap();

        // Agent A claims, works, then releases
        bb.heartbeat("i001", "agent-a").unwrap();
        bb.release_intent("i001", "agent-a").unwrap();

        // Agent B sees unclaimed intent, claims and concludes
        bb.heartbeat("i001", "agent-b").unwrap();
        bb.conclude_intent("i001", &"resolved by agent-b".into())
            .unwrap();

        let state = bb.read_state();
        let i = state.intents.iter().find(|i| i.id.0 == "i001").unwrap();
        assert!(i.concluded_at.is_some(), "intent concluded after handoff");
    }

    // ── Protocol enforcement: error cases ──────────────────────────────────
    #[test]
    fn test_protocol_enforcement() {
        let mut bb = SqlBlackboard::memory().unwrap();
        bb.submit_fact(&make_fact("f001", "data"));
        bb.submit_intent(&make_intent("i001", vec!["f001"], "critical task"))
            .unwrap();
        bb.heartbeat("i001", "agent-a").unwrap();

        // Wrong agent cannot release
        let err = bb.release_intent("i001", "intruder").unwrap_err();
        assert!(
            matches!(err, BlackboardError::Forbidden(_)),
            "wrong agent rejected"
        );

        // Correct agent releases, then wrong agent cannot conclude
        bb.release_intent("i001", "agent-a").unwrap();
        bb.heartbeat("i001", "agent-b").unwrap();
        let _concluded = bb.conclude_intent("i001", &"done".into()).unwrap();

        // Cannot double-conclude
        let err2 = bb.conclude_intent("i001", &"again".into()).unwrap_err();
        assert!(
            matches!(err2, BlackboardError::NotFound(_)),
            "double conclude rejected"
        );

        // Cannot claim nonexistent intent
        let err3 = bb.claim_intent("i_nonexistent", "agent-c").unwrap_err();
        assert!(matches!(err3, BlackboardError::NotFound(_)));
    }

    // ── Full persist: all tables survive session restart ───────────────────
    #[test]
    fn test_full_persistence_across_sessions() {
        let path = "test_full_persist.db";
        let _ = std::fs::remove_file(path);

        {
            let mut bb = SqlBlackboard::open(path).unwrap();
            bb.submit_fact(&make_fact("f001", "alpha"));
            bb.submit_fact(&make_fact("f002", "beta"));
            bb.submit_intent(&make_intent("i001", vec!["f001"], "first"))
                .unwrap();
            bb.submit_intent(&make_intent("i002", vec!["f002"], "second"))
                .unwrap();
            bb.heartbeat("i001", "worker-x").unwrap();
            bb.conclude_intent("i001", &"result-a".into()).unwrap();
            bb.submit_hint(&Hint {
                id: FihHash("h001".into()),
                content: "strategic hint".into(),
                creator: "planner".into(),
            });
        }

        {
            let bb = SqlBlackboard::open(path).unwrap();
            let state = bb.read_state();
            assert_eq!(state.facts.len(), 3, "2 submitted + 1 concluded");
            assert_eq!(state.intents.len(), 2, "both intents restored");
            assert_eq!(state.hints.len(), 1, "hint restored");

            let i1 = state.intents.iter().find(|i| i.id.0 == "i001").unwrap();
            assert!(i1.concluded_at.is_some(), "concluded intent marked");

            let i2 = state.intents.iter().find(|i| i.id.0 == "i002").unwrap();
            assert!(i2.concluded_at.is_none(), "open intent stays open");
            assert_eq!(i2.from_facts.len(), 1, "intent_sources preserved");
        }

        let _ = std::fs::remove_file(path);
    }

    // ── Structured JSON content round-trip ────────────────────────────────
    #[test]
    fn test_structured_json_content() {
        let mut bb = SqlBlackboard::memory().unwrap();
        let complex_content = serde_json::json!({
            "nested": {
                "array": [1, 2, 3],
                "object": {"key": "value"},
                "null": null,
                "bool": true,
                "number": 42.5
            }
        });

        bb.submit_fact(&Fact {
            id: FihHash("f_complex".into()),
            origin: "json-test".into(),
            content: complex_content.clone(),
            creator: "tester".into(),
        });

        let state = bb.read_state();
        let fact = state.facts.iter().find(|f| f.id.0 == "f_complex").unwrap();
        assert_eq!(
            fact.content["nested"]["array"],
            serde_json::json!([1, 2, 3])
        );
        assert_eq!(fact.content["nested"]["object"]["key"], "value");
        assert!(fact.content["nested"]["null"].is_null());
        assert_eq!(fact.content["nested"]["bool"], true);
        assert_eq!(fact.content["nested"]["number"], 42.5);
    }

    // ═══════════════════════════════════════════════════════════════════
    //  Autonomous Research Scenarios
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn test_research_cross_document_entity_linking() {
        let mut bb = SqlBlackboard::memory().unwrap();

        // Document A: whitepaper mentions "homeomorphic verification"
        bb.submit_fact(&Fact {
            id: FihHash("f_doc_a_001".into()),
            origin: "whitepaper.llms.md".into(),
            content: serde_json::json!({
                "concept": "homeomorphic verification",
                "context": "A continuous bijection preserving topological structure",
                "source": "whitepaper §3.4",
                "tags": ["ulhm", "homeomorphism", "verification"]
            }),
            creator: "doc-ingest-agent".into(),
        });

        // Document B: nexus README mentions "boundaryless extension"
        bb.submit_fact(&Fact {
            id: FihHash("f_doc_b_001".into()),
            origin: "nexus-readme.llms.md".into(),
            content: serde_json::json!({
                "concept": "boundaryless extension",
                "context": "Extension from document-code to physical-digital",
                "source": "README.md §Strategic Alignment",
                "tags": ["boundaryless", "extension", "physical-digital"]
            }),
            creator: "doc-ingest-agent".into(),
        });

        bb.submit_intent(&Intent {
            id: FihHash("i_research_001".into()),
            from_facts: vec!["f_doc_a_001".into(), "f_doc_b_001".into()],
            description: "Determine if 'homeomorphic verification' and 'boundaryless extension' describe the same mechanism".into(),
            creator: "cross-ref-agent".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        }).unwrap();

        bb.heartbeat("i_research_001", "review-agent").unwrap();
        let _conclusion = bb.conclude_intent("i_research_001", &serde_json::json!({
            "finding": "homeomorphic verification IS the mathematical foundation of boundaryless extension",
            "confidence": 0.92,
            "evidence": [
                "Both reference ULHM framework",
                "Same three loss terms (continuity, trust, Wasserstein)",
                "whitepaper §3.4 explicitly adopted into neXus architecture"
            ]
        })).unwrap();

        let state = bb.read_state();
        let bridge_intent = state
            .intents
            .iter()
            .find(|i| i.id.0 == "i_research_001")
            .unwrap();
        assert!(bridge_intent.concluded_at.is_some());
        assert!(
            bridge_intent
                .from_facts
                .contains(&"f_doc_a_001".to_string())
        );
        assert!(
            bridge_intent
                .from_facts
                .contains(&"f_doc_b_001".to_string())
        );

        let bridge_fact = state
            .facts
            .iter()
            .find(|f| f.creator == "review-agent")
            .unwrap();
        assert_eq!(bridge_fact.content["confidence"], 0.92);
        assert_eq!(bridge_fact.content["evidence"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_research_contradiction_detection() {
        let mut bb = SqlBlackboard::memory().unwrap();

        bb.submit_fact(&Fact {
            id: FihHash("f_claim_a".into()),
            origin: "whitepaper.llms.md".into(),
            content: serde_json::json!({
                "claim": "Observation-centric hardware eliminates von Neumann bottleneck",
                "confidence": "high",
                "section": "§2.1"
            }),
            creator: "doc-ingest".into(),
        });

        bb.submit_fact(&Fact {
            id: FihHash("f_claim_b".into()),
            origin: "riscv-space.llms.md".into(),
            content: serde_json::json!({
                "claim": "Memory wall persists in current Observation-centric prototypes",
                "confidence": "empirical",
                "section": "§4.2"
            }),
            creator: "doc-ingest".into(),
        });

        bb.submit_intent(&Intent {
            id: FihHash("i_contradiction_001".into()),
            from_facts: vec!["f_claim_a".into(), "f_claim_b".into()],
            description: "CONTRADICTION: Whitepaper claims von Neumann bottleneck eliminated, but RISC-V survey shows persistence".into(),
            creator: "gap-detector".into(),
            worker: Some("gap-detector".into()),
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        }).unwrap();

        let state = bb.read_state();
        let gap = state
            .intents
            .iter()
            .find(|i| i.id.0 == "i_contradiction_001")
            .unwrap();
        assert!(gap.concluded_at.is_none(), "contradiction remains open");
        assert!(gap.worker.is_some(), "claimed by gap-detector");
        assert!(gap.from_facts.iter().any(|f| f == "f_claim_a"));
        assert!(gap.from_facts.iter().any(|f| f == "f_claim_b"));
        assert_eq!(state.facts.len(), 2, "no conclusion fact yet");
    }

    #[test]
    fn test_research_concept_drift_across_sources() {
        let mut bb = SqlBlackboard::memory().unwrap();

        bb.submit_fact(&Fact {
            id: FihHash("f_v1_evolving_memory".into()),
            origin: "impl_init.llms.md".into(),
            content: serde_json::json!({
                "concept": "Evolving Memory",
                "type": "append-only JSONL trajectories",
                "scope": "Planner-Executor-Verifier only"
            }),
            creator: "doc-ingest".into(),
        });

        bb.submit_fact(&Fact {
            id: FihHash("f_v2_ekg".into()),
            origin: "nexus-readme.llms.md".into(),
            content: serde_json::json!({
                "concept": "Episodic Knowledge Graph (eKG)",
                "type": "temporal symbolic memory",
                "scope": "multimodal, cross-reality, multi-agent"
            }),
            creator: "doc-ingest".into(),
        });

        bb.submit_intent(&Intent {
            id: FihHash("i_drift_001".into()),
            from_facts: vec!["f_v1_evolving_memory".into(), "f_v2_ekg".into()],
            description: "Track: Evolving Memory → eKG — concept evolution".into(),
            creator: "concept-tracker".into(),
            worker: Some("concept-tracker".into()),
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        })
        .unwrap();

        bb.submit_hint(&Hint {
            id: FihHash("h_drift_001".into()),
            content:
                "eKG is the successor of Evolving Memory — check if backward compat is maintained"
                    .into(),
            creator: "human-reviewer".into(),
        });

        let state = bb.read_state();
        assert_eq!(state.facts.len(), 2);
        assert_eq!(state.hints.len(), 1);
        assert_eq!(state.intents.len(), 1);
        assert_eq!(
            state.hints[0].content,
            "eKG is the successor of Evolving Memory — check if backward compat is maintained"
        );
    }

    #[test]
    fn test_research_gap_unexplored_territory() {
        let mut bb = SqlBlackboard::memory().unwrap();

        bb.submit_fact(&Fact {
            id: FihHash("f_gap_a".into()),
            origin: "whitepaper.llms.md".into(),
            content: serde_json::json!({
                "topics": ["homeomorphic", "latent manifold", "verification"]
            }),
            creator: "topic-extractor".into(),
        });

        bb.submit_fact(&Fact {
            id: FihHash("f_gap_b".into()),
            origin: "riscv-space.llms.md".into(),
            content: serde_json::json!({
                "topics": ["experimental validation", "RISC-V", "emulation"]
            }),
            creator: "topic-extractor".into(),
        });

        bb.submit_fact(&Fact {
            id: FihHash("f_gap_c".into()),
            origin: "nexus-index.llms.md".into(),
            content: serde_json::json!({
                "topics": ["latent space", "cross-domain", "representation"]
            }),
            creator: "topic-extractor".into(),
        });

        bb.submit_intent(&Intent {
            id: FihHash("i_gap_001".into()),
            from_facts: vec!["f_gap_a".into(), "f_gap_b".into(), "f_gap_c".into()],
            description: "GAP: homeomorphic verification never applied to experimental validation — potential research direction".into(),
            creator: "gap-detector".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        }).unwrap();

        let state = bb.read_state();
        let gap = state
            .intents
            .iter()
            .find(|i| i.id.0 == "i_gap_001")
            .unwrap();
        assert_eq!(gap.from_facts.len(), 3, "all three source facts linked");
        assert!(
            gap.concluded_at.is_none(),
            "gap remains open for researcher"
        );
    }

    #[test]
    fn test_research_memory_across_sessions() {
        let path = "test_research_memory.db";
        let _ = std::fs::remove_file(path);

        // Session 1: ingest phase — docs arrive from R2 sync
        {
            let mut bb = SqlBlackboard::open(path).unwrap();
            let docs = vec![
                (
                    "whitepaper.llms.md",
                    "whitepaper",
                    "Observation-centric hardware",
                ),
                (
                    "riscv.llms.md",
                    "research-riscv",
                    "RISC-V emulation pipeline",
                ),
                (
                    "nexus-index.llms.md",
                    "projects/nexus",
                    "neXus architecture",
                ),
                (
                    "manifesto.llms.md",
                    "manifesto",
                    "Field transition manifesto",
                ),
            ];
            for (i, (origin, source, desc)) in docs.iter().enumerate() {
                bb.submit_fact(&Fact {
                    id: FihHash(format!("f_doc_{:03}", i)),
                    origin: origin.to_string(),
                    content: serde_json::json!({
                        "source": source,
                        "description": desc,
                        "synced_at": "2026-05-18"
                    }),
                    creator: "sync-agent".into(),
                });
            }
            assert_eq!(bb.read_state().facts.len(), 4);
        }

        // Session 2: research phase — agents link and resolve
        {
            let mut bb = SqlBlackboard::open(path).unwrap();
            bb.submit_intent(&Intent {
                id: FihHash("i_link_001".into()),
                from_facts: vec!["f_doc_000".into(), "f_doc_001".into()],
                description: "Link: whitepaper hardware model validated via RISC-V emulation"
                    .into(),
                creator: "linker-agent".into(),
                worker: Some("linker-agent".into()),
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                concluded_at: None,
            })
            .unwrap();
            bb.submit_intent(&Intent {
                id: FihHash("i_link_002".into()),
                from_facts: vec!["f_doc_002".into(), "f_doc_003".into()],
                description: "Link: neXus implements Field transition vision".into(),
                creator: "linker-agent".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                concluded_at: None,
            })
            .unwrap();
            assert_eq!(bb.read_state().facts.len(), 4, "facts preserved");
            assert_eq!(bb.read_state().intents.len(), 2, "intents added");
        }

        // Session 3: conclusion phase — one intent resolved
        {
            let mut bb = SqlBlackboard::open(path).unwrap();
            bb.heartbeat("i_link_001", "reviewer").unwrap();
            let c = bb.conclude_intent("i_link_001", &serde_json::json!({
                "finding": "Confirmed: whitepaper's Observation-centric model directly maps to RISC-V emulation pipeline",
                "evidence": ["Both use Field primitives", "RISC-V emulation validates hardware claims"]
            })).unwrap();
            assert_eq!(c.content["evidence"].as_array().unwrap().len(), 2);

            let state = bb.read_state();
            assert_eq!(state.facts.len(), 5, "4 original + 1 conclusion fact");
            let resolved = state
                .intents
                .iter()
                .find(|i| i.id.0 == "i_link_001")
                .unwrap();
            assert!(resolved.concluded_at.is_some(), "intent resolved");
            let pending = state
                .intents
                .iter()
                .find(|i| i.id.0 == "i_link_002")
                .unwrap();
            assert!(pending.concluded_at.is_none(), "intent still open");
        }

        let _ = std::fs::remove_file(path);
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
