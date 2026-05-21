// nexus-storage-duckdb — DuckDB-backed cold storage for analytical queries.

use nexus_model::{
    BoardState, Fact, FihHash, FilterCapable, Hint, Intent, PartitionData, ScanCapable,
    StateFilter, StorageRead, TimeRangeCapable,
};
use std::ops::Range;
use std::sync::Mutex;

#[allow(dead_code)]
pub struct DuckDbStorage {
    conn: Mutex<duckdb::Connection>,
    base_path: String,
    facts_glob: String,
    intents_glob: String,
    hints_glob: String,
}

#[allow(missing_docs)]
impl DuckDbStorage {
    pub fn new(base_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let conn = duckdb::Connection::open_in_memory()?;
        conn.execute_batch("LOAD parquet;")?;

        let facts_glob = format!("{}/facts/**/*.parquet", base_path);
        let intents_glob = format!("{}/intents/**/*.parquet", base_path);
        let hints_glob = format!("{}/hints/**/*.parquet", base_path);

        let _ = conn.execute_batch(&format!(
            "CREATE VIEW IF NOT EXISTS facts_view AS
             SELECT * FROM read_parquet('{}', union_by_name=true);",
            facts_glob
        ));
        let _ = conn.execute_batch(&format!(
            "CREATE VIEW IF NOT EXISTS intents_view AS
             SELECT * FROM read_parquet('{}', union_by_name=true);",
            intents_glob
        ));
        let _ = conn.execute_batch(&format!(
            "CREATE VIEW IF NOT EXISTS hints_view AS
             SELECT * FROM read_parquet('{}', union_by_name=true);",
            hints_glob
        ));

        Ok(Self {
            conn: Mutex::new(conn),
            base_path: base_path.to_string(),
            facts_glob,
            intents_glob,
            hints_glob,
        })
    }

    fn read_facts(&self) -> Vec<Fact> {
        /* unchanged */
        self.exec_fact_query("SELECT fact_id, origin, content, creator, created_at FROM facts_view")
    }
    fn read_intents(&self) -> Vec<Intent> {
        /* unchanged */
        self.exec_intent_query("SELECT intent_id, from_facts, description, creator, worker, to_fact_id, last_heartbeat_at, created_at, concluded_at FROM intents_view")
    }
    fn read_hints(&self) -> Vec<Hint> {
        /* unchanged */
        self.exec_hint_query("SELECT hint_id, content, creator, created_at FROM hints_view")
    }

    fn build_where_clause(&self, filter: &StateFilter) -> String {
        let mut clauses: Vec<String> = Vec::new();
        if let Some(ids) = &filter.fact_ids {
            let list = ids
                .iter()
                .map(|s| format!("'{}'", s.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(",");
            clauses.push(format!("fact_id IN ({})", list));
        }
        if let Some(ids) = &filter.intent_ids {
            let list = ids
                .iter()
                .map(|s| format!("'{}'", s.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(",");
            clauses.push(format!("intent_id IN ({})", list));
        }
        if let Some(since) = &filter.since {
            clauses.push(format!("created_at >= '{}'", since.replace('\'', "''")));
        }
        if let Some(until) = &filter.until {
            clauses.push(format!("created_at <= '{}'", until.replace('\'', "''")));
        }
        if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        }
    }

    fn build_limit_offset(filter: &StateFilter) -> String {
        match (filter.limit, filter.offset) {
            (Some(limit), Some(offset)) => format!("LIMIT {} OFFSET {}", limit, offset),
            (Some(limit), None) => format!("LIMIT {}", limit),
            (None, Some(offset)) => format!("LIMIT -1 OFFSET {}", offset),
            (None, None) => String::new(),
        }
    }

    fn exec_fact_query(&self, sql: &str) -> Vec<Fact> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        match stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let origin: String = row.get(1)?;
            let content_str: String = row.get(2)?;
            let creator: String = row.get(3)?;
            Ok(Fact {
                id: FihHash(id),
                origin,
                content: serde_json::from_str(&content_str)
                    .unwrap_or(serde_json::Value::String(content_str)),
                creator,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn exec_intent_query(&self, sql: &str) -> Vec<Intent> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        match stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let from_facts_json: Option<String> = row.get(1).ok();
            let description: String = row.get(2)?;
            let creator: String = row.get(3)?;
            let worker: Option<String> = row.get(4).ok();
            let to_fact_id: Option<String> = row.get(5).ok();
            let last_hb: Option<String> = row.get(6).ok();
            let created_at: Option<String> = row.get(7).ok();
            let concluded_at: Option<String> = row.get(8).ok();
            let from_facts: Vec<String> = from_facts_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default();
            Ok(Intent {
                id: FihHash(id),
                from_facts,
                description,
                creator,
                worker,
                to_fact_id,
                last_heartbeat_at: last_hb,
                created_at,
                concluded_at,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn exec_hint_query(&self, sql: &str) -> Vec<Hint> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        match stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let creator: String = row.get(2)?;
            Ok(Hint {
                id: FihHash(id),
                content,
                creator,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl StorageRead for DuckDbStorage {
    fn project_id(&self) -> &str {
        "cold"
    }
    fn read_state(&self) -> BoardState {
        BoardState {
            facts: self.read_facts(),
            intents: self.read_intents(),
            hints: self.read_hints(),
        }
    }
}

impl FilterCapable for DuckDbStorage {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        let wc = self.build_where_clause(filter);
        let lo = Self::build_limit_offset(filter);
        let needs_filter = filter.fact_ids.is_some()
            || filter.intent_ids.is_some()
            || filter.since.is_some()
            || filter.until.is_some()
            || filter.limit.is_some()
            || filter.offset.is_some();

        let facts = if needs_filter || filter.fact_ids.is_some() {
            self.exec_fact_query(&format!(
                "SELECT fact_id, origin, content, creator, created_at FROM facts_view {} {}",
                wc, lo
            ))
        } else {
            self.read_facts()
        };

        let intents = if needs_filter || filter.intent_ids.is_some() {
            self.exec_intent_query(&format!("SELECT intent_id, from_facts, description, creator, worker, to_fact_id, last_heartbeat_at, created_at, concluded_at FROM intents_view {} {}", wc, lo))
        } else {
            self.read_intents()
        };

        BoardState {
            facts,
            intents,
            hints: self.read_hints(),
        }
    }
}

impl ScanCapable for DuckDbStorage {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let fg = format!(
            "{}/facts/partition={}/**/*.parquet",
            self.base_path, partition
        );
        let ig = format!(
            "{}/intents/partition={}/**/*.parquet",
            self.base_path, partition
        );
        let hg = format!(
            "{}/hints/partition={}/**/*.parquet",
            self.base_path, partition
        );
        Ok(PartitionData {
            partition: partition.to_string(),
            facts: self.exec_fact_query(&format!("SELECT fact_id, origin, content, creator, created_at FROM read_parquet('{}', union_by_name=true)", fg)),
            intents: self.exec_intent_query(&format!("SELECT intent_id, from_facts, description, creator, worker, to_fact_id, last_heartbeat_at, created_at, concluded_at FROM read_parquet('{}', union_by_name=true)", ig)),
            hints: self.exec_hint_query(&format!("SELECT hint_id, content, creator, created_at FROM read_parquet('{}', union_by_name=true)", hg)),
        })
    }
}

impl TimeRangeCapable for DuckDbStorage {
    fn time_range(&self) -> Option<Range<String>> {
        let conn = self.conn.lock().unwrap();
        let min: Option<String> = conn
            .prepare("SELECT created_at FROM facts_view ORDER BY created_at LIMIT 1")
            .ok()
            .and_then(|mut s| s.query_row([], |row| row.get(0)).ok());
        let max: Option<String> = conn
            .prepare("SELECT created_at FROM facts_view ORDER BY created_at DESC LIMIT 1")
            .ok()
            .and_then(|mut s| s.query_row([], |row| row.get(0)).ok());
        min.zip(max).map(|(lo, hi)| lo..hi)
    }
}
