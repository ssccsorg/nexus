// nexus-table — Database schema (Cairn-pattern normalized tables).

use rusqlite::Connection;

pub fn apply_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;

         CREATE TABLE IF NOT EXISTS projects (
             id TEXT PRIMARY KEY,
             title TEXT NOT NULL,
             status TEXT NOT NULL DEFAULT 'active',
             created_at TEXT NOT NULL DEFAULT (datetime('now')),
             reason_worker TEXT,
             reason_trigger TEXT,
             reason_started_at TEXT,
             reason_last_heartbeat_at TEXT
         );

         CREATE TABLE IF NOT EXISTS facts (
             id TEXT NOT NULL,
             project_id TEXT NOT NULL,
             description TEXT NOT NULL,
             creator TEXT,
             origin TEXT,
             PRIMARY KEY (id, project_id),
             FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS intents (
             id TEXT NOT NULL,
             project_id TEXT NOT NULL,
             to_fact_id TEXT,
             description TEXT NOT NULL,
             creator TEXT NOT NULL,
             worker TEXT,
             last_heartbeat_at TEXT,
             created_at TEXT NOT NULL DEFAULT (datetime('now')),
             concluded_at TEXT,
             PRIMARY KEY (id, project_id),
             FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
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
             project_id TEXT NOT NULL,
             content TEXT NOT NULL,
             creator TEXT NOT NULL,
             created_at TEXT NOT NULL DEFAULT (datetime('now')),
             PRIMARY KEY (id, project_id),
             FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS schema_version (
             version INTEGER NOT NULL
         );

         INSERT OR IGNORE INTO schema_version (version) VALUES (1);",
    )?;
    Ok(())
}

/// Placeholder for future schema migrations.
/// Call after apply_schema to ensure the schema version is current.
pub fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    let version: i32 = conn.query_row(
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    if version < 1 {
        // Initial schema is applied by apply_schema(). No migrations yet.
    }

    Ok(())
}
