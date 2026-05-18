// nexus-table — SqlBlackboard: normalized Cairn-pattern FIH Blackboard.
//
// Implements `Blackboard` trait directly against normalized SQLite tables
// (facts, intents, hints, intent_sources). No event replay.
// Write-through on every mutation. Project-scoped via project_id.

use nexus_model::{Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

use crate::schema::apply_schema;
use crate::util::{ProjectMeta, utc_now};

/// Normalized SQLite-backed FIH Blackboard.
pub struct SqlBlackboard {
    conn: Mutex<Connection>,
    project_id: String,
}

impl SqlBlackboard {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rusqlite::Error> {
        Self::open_with_project(path, "default")
    }

    pub fn open_with_project<P: AsRef<Path>>(
        path: P,
        project_id: &str,
    ) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        apply_schema(&conn)?;
        let bb = Self {
            conn: Mutex::new(conn),
            project_id: project_id.to_string(),
        };
        bb.ensure_project()?;
        Ok(bb)
    }

    pub fn memory() -> Result<Self, rusqlite::Error> {
        Self::memory_with_project("default")
    }

    pub fn memory_with_project(project_id: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        apply_schema(&conn)?;
        let bb = Self {
            conn: Mutex::new(conn),
            project_id: project_id.to_string(),
        };
        bb.ensure_project()?;
        Ok(bb)
    }

    fn ensure_project(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO projects (id, title, status) VALUES (?1, ?2, 'active')",
            params![self.project_id, self.project_id],
        )?;
        Ok(())
    }

    /// Claim or heartbeat an open intent. Shared by `claim_intent` and `heartbeat`.
    fn set_worker(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;
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

    pub fn set_project_status(&self, status: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET status = ?1 WHERE id = ?2",
            params![status, self.project_id],
        )?;
        Ok(())
    }

    pub fn get_project(&self) -> Result<ProjectMeta, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, title, status, created_at, reason_worker, reason_trigger, reason_started_at, reason_last_heartbeat_at
             FROM projects WHERE id = ?1",
            params![self.project_id],
            |row| {
                Ok(ProjectMeta {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    reason_worker: row.get(4)?,
                    reason_trigger: row.get(5)?,
                    reason_started_at: row.get(6)?,
                    reason_last_heartbeat_at: row.get(7)?,
                })
            },
        )
    }
}

impl Blackboard for SqlBlackboard {
    fn submit_fact(&mut self, fact: &Fact) -> FihHash {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;
        let _ = conn.execute(
            "INSERT OR IGNORE INTO facts (id, project_id, description, creator, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![fact.id.0, pid, serde_json::to_string(&fact.content).unwrap_or_default(), fact.creator, fact.origin],
        );
        fact.id.clone()
    }

    fn submit_hint(&mut self, hint: &Hint) {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;
        let _ = conn.execute(
            "INSERT OR IGNORE INTO hints (id, project_id, content, creator) VALUES (?1, ?2, ?3, ?4)",
            params![hint.id.0, pid, hint.content, hint.creator],
        );
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let mut conn = self.conn.lock().unwrap();
        let pid = &self.project_id;

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

        // Atomic: insert intent + source links in one transaction
        let tx = conn
            .transaction()
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let now = utc_now();
        let worker = intent.worker.as_deref();
        let heartbeat = if worker.is_some() { Some(&now) } else { None };
        tx.execute(
            "INSERT INTO intents (id, project_id, to_fact_id, description, creator, worker, last_heartbeat_at, concluded_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, NULL)",
            params![intent.id.0, pid, intent.description, intent.creator, worker, heartbeat],
        ).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        for fid in &intent.from_facts {
            tx.execute(
                "INSERT INTO intent_sources (intent_id, project_id, fact_id) VALUES (?1, ?2, ?3)",
                params![intent.id.0, pid, fid],
            )
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        Ok(intent.id.clone())
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.set_worker(intent_id, agent)
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.set_worker(intent_id, agent)
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut conn = self.conn.lock().unwrap();
        let pid = &self.project_id;

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

        let tx = conn
            .transaction()
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        tx.execute(
            "UPDATE intents SET worker = NULL WHERE id = ?1 AND project_id = ?2",
            params![intent_id, pid],
        )
        .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        tx.commit()
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        Ok(())
    }

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let mut conn = self.conn.lock().unwrap();
        let pid = &self.project_id;

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

        let new_fact_id = format!("f_concl_{}", intent_id);
        let new_fact = Fact {
            id: FihHash(new_fact_id.clone()),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: worker.clone(),
        };

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
        let pid = &self.project_id;

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
