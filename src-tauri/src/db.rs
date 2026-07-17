//! SQLite storage for Hydra: strategy scores and switch history. The
//! project has no prior DB or migration mechanism (config is a flat JSON
//! file, see `config.rs`), so this introduces a minimal sequential
//! migration runner alongside the one rusqlite dependency it needs.

use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use tauri::AppHandle;

use crate::error::AppResult;

pub struct Db {
    pub conn: Mutex<Connection>,
}

/// Applied in order, tracked in `schema_migrations`. Append, never edit, a
/// past entry — the same discipline as any other migration log.
const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE strategies (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        target TEXT NOT NULL,
        params_json TEXT NOT NULL,
        source TEXT NOT NULL,
        score REAL DEFAULT 0.0,
        last_tested_at INTEGER,
        is_active BOOLEAN DEFAULT 0
    );
    CREATE INDEX idx_strategies_target ON strategies(target);

    CREATE TABLE strategy_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        strategy_id TEXT NOT NULL,
        event TEXT NOT NULL,
        detail TEXT,
        occurred_at INTEGER NOT NULL
    );

    CREATE TABLE hydra_meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
"#, r#"
    ALTER TABLE strategies ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
"#];

pub fn open(app: &AppHandle) -> AppResult<Db> {
    let path = crate::config::data_dir(app)?.join("hydra.sqlite3");
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    run_migrations(&conn)?;
    Ok(Db {
        conn: Mutex::new(conn),
    })
}

pub(crate) fn run_migrations(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        )",
    )?;

    let applied: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;

    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = (index + 1) as i64;
        if version <= applied {
            continue;
        }
        conn.execute_batch(sql)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![version, now_secs()],
        )?;
    }
    Ok(())
}

pub fn meta_get(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM hydra_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?)
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO hydra_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent_and_create_expected_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).expect("first run");
        run_migrations(&conn).expect("second run must be a no-op, not an error");

        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN
                 ('strategies', 'strategy_history', 'hydra_meta')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 3);
    }

    #[test]
    fn migration_2_adds_created_at_column() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).expect("migrations");
        let has_column: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('strategies') WHERE name = 'created_at'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_column, 1);
    }

    #[test]
    fn meta_roundtrips_and_upserts() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        assert_eq!(meta_get(&conn, "k").unwrap(), None);
        meta_set(&conn, "k", "1").unwrap();
        assert_eq!(meta_get(&conn, "k").unwrap(), Some("1".into()));
        meta_set(&conn, "k", "2").unwrap();
        assert_eq!(meta_get(&conn, "k").unwrap(), Some("2".into()));
    }
}
