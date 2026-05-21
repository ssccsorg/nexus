// nexus-storage-sqlite — SqlNormalizedStorage: normalized Cairn-pattern FIH Storage.
//
// Implements capability-based Storage traits (`StorageRead`, `FactCapable`,
// `IntentCapable`, `HintCapable`, `FilterCapable`, `FihPersistence`,
// `ColdStorage`) directly against normalized SQLite tables (facts, intents,
// hints, intent_sources). No event replay. Write-through on every mutation.
// Project-scoped via project_id.

use nexus_model::{
    BlackboardError, BoardState, Fact, FactCapable, FihHash, FilterCapable, Hint, HintCapable,
    Intent, IntentCapable, StateFilter, StorageRead,
};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

use crate::schema::apply_schema;
use crate::util::ProjectMeta;

/// Normalized SQLite-backed cold storage.
pub struct SqlNormalizedStorage {
    conn: Mutex<Connection>,
    project_id: String,
}

impl SqlNormalizedStorage {
    /// Open or create a SQLite database at `path`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rusqlite::Error> {
        Self::open_with_project(path, "default")
    }

    /// Open with a specific project ID.
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

    /// Create an in-memory database (for testing).
    pub fn memory() -> Result<Self, rusqlite::Error> {
        Self::memory_with_project("default")
    }

    /// Create an in-memory database with a specific project ID.
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

    /// Set the project status (active, stopped, completed).
    pub fn set_project_status(&self, status: &str) -> Result<(), rusqlite::Error> {
        if !["active", "stopped", "completed"].contains(&status) {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("invalid project status: {status}"),
                ),
            )));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE projects SET status = ?1 WHERE id = ?2",
            params![status, self.project_id],
        )?;
        Ok(())
    }

    /// Get project metadata.
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

    /// Return the project_id this storage is scoped to.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl StorageRead for SqlNormalizedStorage {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn read_state(&self) -> BoardState {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(poisoned) => {
                eprintln!("read_state: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let pid = &self.project_id;

        let facts = conn
            .prepare("SELECT id, description, creator, origin FROM facts WHERE project_id = ?1 ORDER BY id")
            .map(|mut stmt| {
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
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            })
            .unwrap_or_default();

        let source_map: std::collections::HashMap<String, Vec<String>> = conn
            .prepare("SELECT intent_id, fact_id FROM intent_sources WHERE project_id = ?1 ORDER BY rowid")
            .map(|mut stmt| {
                let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
                if let Ok(rows) = stmt.query_map(params![pid], |row| {
                    let iid: String = row.get(0)?;
                    let fid: String = row.get(1)?;
                    Ok((iid, fid))
                }) {
                    for row in rows.flatten() {
                        map.entry(row.0).or_default().push(row.1);
                    }
                }
                map
            })
            .unwrap_or_default();

        let intents = conn
            .prepare(
                "SELECT i.id, i.description, i.creator, i.worker,
                        i.to_fact_id, i.last_heartbeat_at, i.created_at, i.concluded_at
                 FROM intents i WHERE i.project_id = ?1 ORDER BY i.created_at",
            )
            .map(|mut stmt| {
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
                .map(|rows| {
                    rows.filter_map(|r| r.ok())
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
                })
                .unwrap_or_default()
            })
            .unwrap_or_default();

        let hints = conn
            .prepare(
                "SELECT id, content, creator FROM hints WHERE project_id = ?1 ORDER BY created_at",
            )
            .map(|mut stmt| {
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
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            })
            .unwrap_or_default();

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

impl FactCapable for SqlNormalizedStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;
        let desc = serde_json::to_string(&fact.content)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO facts (id, project_id, description, creator, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![fact.id.0, pid, &desc, &fact.creator, &fact.origin],
        )
        .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        Ok(fact.id.clone())
    }
}

impl IntentCapable for SqlNormalizedStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
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
        let worker = intent.worker.as_deref();
        tx.execute(
            "INSERT INTO intents (id, project_id, to_fact_id, description, creator, worker, last_heartbeat_at, concluded_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5,
               CASE WHEN ?5 IS NOT NULL THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE NULL END,
               NULL)",
            params![intent.id.0, pid, intent.description, intent.creator, worker],
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

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;
        let updated = conn
            .execute(
                "UPDATE intents SET worker = ?1, last_heartbeat_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?2 AND project_id = ?3 AND to_fact_id IS NULL
               AND worker IS NULL",
                params![agent, intent_id, pid],
            )
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        if updated == 0 {
            return Err(BlackboardError::Conflict(format!(
                "Intent {intent_id} already claimed"
            )));
        }
        Ok(())
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;
        let updated = conn
            .execute(
                "UPDATE intents SET worker = ?1, last_heartbeat_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?2 AND project_id = ?3 AND to_fact_id IS NULL
               AND (worker IS NULL OR worker = ?4)",
                params![agent, intent_id, pid, agent],
            )
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        if updated == 0 {
            return Err(BlackboardError::Conflict(format!(
                "Intent {intent_id} is claimed by another agent"
            )));
        }
        Ok(())
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;

        let updated = conn
            .execute(
                "UPDATE intents SET worker = NULL
             WHERE id = ?1 AND project_id = ?2 AND to_fact_id IS NULL
               AND (worker IS NULL OR worker = ?3)",
                params![intent_id, pid, agent],
            )
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        if updated == 0 {
            let row: Result<(Option<String>, Option<String>), _> = conn.query_row(
                "SELECT to_fact_id, worker FROM intents WHERE id = ?1 AND project_id = ?2",
                params![intent_id, pid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            );
            return match row {
                Err(_) => Err(BlackboardError::NotFound(format!(
                    "Intent {intent_id} not found"
                ))),
                Ok((Some(_), _)) => Err(BlackboardError::NotFound(format!(
                    "Intent {intent_id} already concluded"
                ))),
                Ok((None, Some(ref w))) if w != agent => {
                    Err(BlackboardError::Forbidden(format!("Intent claimed by {w}")))
                }
                _ => Err(BlackboardError::NotFound(format!(
                    "Intent {intent_id} cannot be released"
                ))),
            };
        }
        Ok(())
    }

    fn conclude_intent(
        &self,
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
            "INSERT INTO facts (id, project_id, description, creator, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_fact_id, pid, result_str, &worker, &new_fact.origin],
        ).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        tx.execute(
            "UPDATE intents SET to_fact_id = ?1, worker = ?2,
                    last_heartbeat_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    concluded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?3 AND project_id = ?4",
            params![new_fact_id, &worker, intent_id, pid],
        )
        .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        tx.commit()
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        Ok(new_fact)
    }
}

impl HintCapable for SqlNormalizedStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let conn = self.conn.lock().unwrap();
        let pid = &self.project_id;
        conn.execute(
            "INSERT INTO hints (id, project_id, content, creator) VALUES (?1, ?2, ?3, ?4)",
            params![hint.id.0, pid, &hint.content, &hint.creator],
        )
        .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        Ok(())
    }
}

impl FilterCapable for SqlNormalizedStorage {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(poisoned) => {
                eprintln!("read_state_filtered: mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        let pid = &self.project_id;

        // Build WHERE clause fragments for each table type.
        // Facts and hints use `id`; intents use `id`; all three have `created_at`.
        let fact_where = build_fact_where(filter, pid);
        let intent_where = build_intent_where(filter, pid);
        let hint_where = build_hint_where(filter, pid);

        let limit_off = build_limit_offset(filter);

        let facts = conn
            .prepare(&format!(
                "SELECT id, description, creator, origin FROM facts {} ORDER BY id {}",
                fact_where, limit_off
            ))
            .map(|mut stmt| {
                stmt.query_map([], |row| {
                    let id: String = row.get(0)?;
                    let desc: String = row.get(1)?;
                    let creator: String = row.get(2).unwrap_or_default();
                    let origin: String = row.get(3).unwrap_or_default();
                    Ok(Fact {
                        id: FihHash(id),
                        origin,
                        content: serde_json::from_str(&desc)
                            .unwrap_or(serde_json::Value::String(desc)),
                        creator,
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            })
            .unwrap_or_default();

        let intents = conn
            .prepare(&format!(
                "SELECT i.id, i.description, i.creator, i.worker,
                        i.to_fact_id, i.last_heartbeat_at, i.created_at, i.concluded_at
                 FROM intents i {} ORDER BY i.created_at {}",
                intent_where, limit_off
            ))
            .map(|mut stmt| {
                stmt.query_map([], |row| {
                    let id: String = row.get(0)?;
                    let desc: String = row.get(1)?;
                    let creator: String = row.get(2)?;
                    let worker: Option<String> = row.get(3)?;
                    let to_fact_id: Option<String> = row.get(4)?;
                    let last_heartbeat_at: Option<String> = row.get(5)?;
                    let created_at: String = row.get(6)?;
                    let concluded_at: Option<String> = row.get(7)?;
                    Ok(Intent {
                        id: FihHash(id.clone()),
                        from_facts: Vec::new(), // source_map join omitted for filtered reads
                        description: desc,
                        creator,
                        worker,
                        to_fact_id,
                        last_heartbeat_at,
                        created_at: Some(created_at),
                        concluded_at,
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            })
            .unwrap_or_default();

        let hints = conn
            .prepare(&format!(
                "SELECT id, content, creator FROM hints {} ORDER BY created_at {}",
                hint_where, limit_off
            ))
            .map(|mut stmt| {
                stmt.query_map([], |row| {
                    let id: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let creator: String = row.get(2)?;
                    Ok(Hint {
                        id: FihHash(id),
                        content,
                        creator,
                    })
                })
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            })
            .unwrap_or_default();

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

// ── Helper: build SQL WHERE clauses from StateFilter ────────────────────

/// Build WHERE clause for the facts table.
fn build_fact_where(filter: &StateFilter, project_id: &str) -> String {
    let mut clauses: Vec<String> =
        vec![format!("project_id = '{}'", project_id.replace('\'', "''"))];
    if let Some(ids) = &filter.fact_ids {
        let list: Vec<String> = ids
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect();
        clauses.push(format!("id IN ({})", list.join(",")));
    }
    if let Some(since) = &filter.since {
        clauses.push(format!("created_at >= '{}'", since.replace('\'', "''")));
    }
    if let Some(until) = &filter.until {
        clauses.push(format!("created_at <= '{}'", until.replace('\'', "''")));
    }
    format!("WHERE {}", clauses.join(" AND "))
}

/// Build WHERE clause for the intents table.
fn build_intent_where(filter: &StateFilter, project_id: &str) -> String {
    let mut clauses: Vec<String> = vec![format!(
        "i.project_id = '{}'",
        project_id.replace('\'', "''")
    )];
    if let Some(ids) = &filter.intent_ids {
        let list: Vec<String> = ids
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect();
        clauses.push(format!("i.id IN ({})", list.join(",")));
    }
    if let Some(since) = &filter.since {
        clauses.push(format!("i.created_at >= '{}'", since.replace('\'', "''")));
    }
    if let Some(until) = &filter.until {
        clauses.push(format!("i.created_at <= '{}'", until.replace('\'', "''")));
    }
    format!("WHERE {}", clauses.join(" AND "))
}

/// Build WHERE clause for the hints table.
fn build_hint_where(filter: &StateFilter, project_id: &str) -> String {
    let mut clauses: Vec<String> =
        vec![format!("project_id = '{}'", project_id.replace('\'', "''"))];
    if let Some(ids) = &filter.hint_ids {
        let list: Vec<String> = ids
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect();
        clauses.push(format!("id IN ({})", list.join(",")));
    }
    if let Some(since) = &filter.since {
        clauses.push(format!("created_at >= '{}'", since.replace('\'', "''")));
    }
    if let Some(until) = &filter.until {
        clauses.push(format!("created_at <= '{}'", until.replace('\'', "''")));
    }
    format!("WHERE {}", clauses.join(" AND "))
}

/// Build LIMIT/OFFSET suffix.
fn build_limit_offset(filter: &StateFilter) -> String {
    match (filter.limit, filter.offset) {
        (Some(limit), Some(offset)) => format!("LIMIT {} OFFSET {}", limit, offset),
        (Some(limit), None) => format!("LIMIT {}", limit),
        (None, Some(offset)) => format!("LIMIT -1 OFFSET {}", offset),
        (None, None) => String::new(),
    }
}
