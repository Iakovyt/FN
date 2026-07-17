pub mod bat_import;
pub mod benchmark;
pub mod fake_tls;
pub mod schema;
pub mod winws_mapper;

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::now_secs;
use crate::error::AppResult;
use crate::strategy::schema::{BypassParams, Strategy, StrategySource, TargetProtocol};

/// Bump whenever `assets/builtin_strategies.json` changes so existing
/// installs pick up the new/changed rows on next launch. See
/// `bat_import::generator::generate_builtin_strategies_json` for how the
/// file itself is produced.
pub const BUILTIN_STRATEGIES_VERSION: u32 = 1;

const BUILTIN_STRATEGIES_JSON: &str =
    include_str!("../../assets/builtin_strategies.json");
const BUILTIN_VERSION_META_KEY: &str = "builtin_strategies_version";

pub fn load_builtin_strategies() -> Vec<Strategy> {
    serde_json::from_str(BUILTIN_STRATEGIES_JSON)
        .expect("bundled assets/builtin_strategies.json must parse")
}

/// Insert any builtin strategies missing from the DB and bump the stored
/// version marker, but only when `BUILTIN_STRATEGIES_VERSION` moved past
/// what's recorded — makes this cheap to call on every startup. Existing
/// rows (including ones the user disabled) are left untouched; strategy
/// ids are content-addressed (see `schema::compute_id`), so re-syncing
/// never duplicates a strategy whose params haven't changed.
pub fn sync_builtin_strategies(conn: &Connection) -> AppResult<usize> {
    let stored_version: u32 = crate::db::meta_get(conn, BUILTIN_VERSION_META_KEY)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if stored_version >= BUILTIN_STRATEGIES_VERSION {
        return Ok(0);
    }

    let mut inserted = 0;
    for strategy in load_builtin_strategies() {
        if insert_strategy_if_new(conn, &strategy)? {
            inserted += 1;
        }
    }
    crate::db::meta_set(
        conn,
        BUILTIN_VERSION_META_KEY,
        &BUILTIN_STRATEGIES_VERSION.to_string(),
    )?;
    Ok(inserted)
}

fn insert_strategy_if_new(conn: &Connection, strategy: &Strategy) -> AppResult<bool> {
    let params_json = serde_json::to_string(&strategy.params)?;
    let changed = conn.execute(
        "INSERT OR IGNORE INTO strategies
            (id, name, target, params_json, source, score, last_tested_at, is_active, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0.0, NULL, 0, ?6)",
        params![
            strategy.id,
            strategy.name,
            strategy.target.as_db_str(),
            params_json,
            strategy.source.as_db_str(),
            strategy.created_at,
        ],
    )?;
    Ok(changed > 0)
}

/// Append-only log of what happened to a strategy (benchmark result,
/// activation, degradation, ...) — surfaced later as Hydra's history feed.
pub fn record_history(
    conn: &Connection,
    strategy_id: &str,
    event: &str,
    detail: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO strategy_history (strategy_id, event, detail, occurred_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![strategy_id, event, detail, now_secs()],
    )?;
    Ok(())
}

pub fn update_score(conn: &Connection, strategy_id: &str, score: f64) -> AppResult<()> {
    conn.execute(
        "UPDATE strategies SET score = ?1, last_tested_at = ?2 WHERE id = ?3",
        params![score, now_secs(), strategy_id],
    )?;
    Ok(())
}

/// Marks `strategy_id` as the sole active row. A plain two-statement swap
/// (not wrapped in an explicit transaction) is fine here: this runs on the
/// single benchmark/health-check task, never concurrently with itself.
pub fn activate_strategy(conn: &Connection, strategy_id: &str) -> AppResult<()> {
    conn.execute("UPDATE strategies SET is_active = 0", [])?;
    let changed = conn.execute(
        "UPDATE strategies SET is_active = 1 WHERE id = ?1",
        params![strategy_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Msg(format!(
            "activate_strategy: unknown strategy id {strategy_id}"
        )));
    }
    Ok(())
}

pub fn has_active_strategy(conn: &Connection) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM strategies WHERE is_active = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn get_active_strategy(conn: &Connection) -> AppResult<Option<Strategy>> {
    conn.query_row(
        "SELECT id, name, target, params_json, source, created_at
         FROM strategies WHERE is_active = 1 LIMIT 1",
        [],
        row_to_strategy,
    )
    .optional()
    .map_err(Into::into)
}

/// All strategies in the DB, optionally filtered to one [`StrategySource`]
/// (e.g. just the builtin pool for a first-run benchmark).
pub fn load_pool(conn: &Connection, source: Option<StrategySource>) -> AppResult<Vec<Strategy>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, target, params_json, source, created_at FROM strategies
         WHERE ?1 IS NULL OR source = ?1",
    )?;
    let source_filter = source.map(|s| s.as_db_str().to_string());
    let rows = stmt.query_map(params![source_filter], row_to_strategy)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn row_to_strategy(row: &rusqlite::Row) -> rusqlite::Result<Strategy> {
    let params_json: String = row.get(3)?;
    let params: BypassParams = serde_json::from_str(&params_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let target: String = row.get(2)?;
    let source: String = row.get(4)?;
    Ok(Strategy {
        id: row.get(0)?,
        name: row.get(1)?,
        target: TargetProtocol::from_db_str(&target),
        params,
        source: StrategySource::from_db_str(&source),
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn migrated_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn syncing_twice_only_inserts_once() {
        let conn = migrated_conn();

        let first = sync_builtin_strategies(&conn).unwrap();
        let second = sync_builtin_strategies(&conn).unwrap();

        assert_eq!(second, 0, "second sync should be a no-op");
        let stored_version = db::meta_get(&conn, BUILTIN_VERSION_META_KEY)
            .unwrap()
            .unwrap();
        assert_eq!(stored_version, BUILTIN_STRATEGIES_VERSION.to_string());

        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM strategies", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, first as i64);
    }

    #[test]
    fn bundled_builtin_strategies_parse() {
        // Once the generator has been run this should be non-empty; until
        // then the placeholder `[]` still parses, so this only guards
        // against malformed JSON, not emptiness.
        let _ = load_builtin_strategies();
    }

    #[test]
    fn activate_strategy_is_exclusive() {
        let conn = migrated_conn();
        sync_builtin_strategies(&conn).unwrap();
        let pool = load_pool(&conn, Some(StrategySource::Builtin)).unwrap();
        assert!(pool.len() >= 2, "need at least two builtin strategies for this test");

        assert!(!has_active_strategy(&conn).unwrap());

        activate_strategy(&conn, &pool[0].id).unwrap();
        assert!(has_active_strategy(&conn).unwrap());
        assert_eq!(get_active_strategy(&conn).unwrap().unwrap().id, pool[0].id);

        activate_strategy(&conn, &pool[1].id).unwrap();
        assert_eq!(get_active_strategy(&conn).unwrap().unwrap().id, pool[1].id);
    }

    #[test]
    fn activate_unknown_strategy_errors() {
        let conn = migrated_conn();
        assert!(activate_strategy(&conn, "does-not-exist").is_err());
    }

    #[test]
    fn record_history_and_update_score_roundtrip() {
        let conn = migrated_conn();
        sync_builtin_strategies(&conn).unwrap();
        let pool = load_pool(&conn, None).unwrap();
        let id = &pool[0].id;

        update_score(&conn, id, 0.83).unwrap();
        record_history(&conn, id, "benchmark_result", Some("score=0.83")).unwrap();

        let stored_score: f64 = conn
            .query_row("SELECT score FROM strategies WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((stored_score - 0.83).abs() < f64::EPSILON);

        let history_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM strategy_history WHERE strategy_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(history_count, 1);
    }

    #[test]
    fn load_pool_round_trips_params_through_json() {
        let conn = migrated_conn();
        sync_builtin_strategies(&conn).unwrap();
        let pool = load_pool(&conn, Some(StrategySource::Builtin)).unwrap();
        let original = load_builtin_strategies();
        assert_eq!(pool.len(), original.len());
        let reloaded = pool.iter().find(|s| s.id == original[0].id).unwrap();
        assert_eq!(reloaded.params, original[0].params);
    }
}

/// Stage 1 acceptance test: take one builtin strategy through
/// `winws_mapper::build_winws_args` and confirm it carries the same flag
/// values as the original `general (ALT).bat` — cross-checked against the
/// existing, already-tested `strategies::build_batch_args`, which resolves
/// the same file through the pre-Hydra path.
#[cfg(test)]
mod fidelity {
    use super::*;
    use crate::strategies::{self, ZapretPaths};
    use crate::strategy::schema::{encode_hex, StrategySource, TargetProtocol};
    use crate::strategy::winws_mapper::build_winws_args;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// `flag -> values`, normalizing bare flags (`--foo`) and `--foo=1` to
    /// the same shape so mapper output (which emits boolean flags bare) is
    /// comparable to the original bat's `=1` form.
    fn flag_values(args: &[String]) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for arg in args {
            let (flag, value) = arg
                .split_once('=')
                .map(|(f, v)| (f.to_string(), v.to_string()))
                .unwrap_or_else(|| (arg.clone(), "1".to_string()));
            map.entry(flag).or_default().push(value);
        }
        map
    }

    fn value_set(map: &HashMap<String, Vec<String>>, flag: &str) -> std::collections::HashSet<String> {
        map.get(flag).cloned().unwrap_or_default().into_iter().collect()
    }

    #[test]
    fn build_winws_args_matches_original_general_alt_bat() {
        let zapret_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("zapret");
        let paths = ZapretPaths {
            root: zapret_root.clone(),
            bin: zapret_root.join("bin"),
            lists: zapret_root.join("lists"),
        };

        let original_args = strategies::build_batch_args("bat:general (ALT).bat", &paths)
            .expect("parse original bat via the existing, pre-Hydra path");
        let original = flag_values(&original_args);

        let strategy = load_builtin_strategies()
            .into_iter()
            .find(|s| s.name == "general (ALT)")
            .expect("general (ALT) imported into builtin_strategies.json");
        assert_eq!(strategy.source, StrategySource::Builtin);
        assert_eq!(strategy.target, TargetProtocol::Generic("general".into()));

        let hostlist_path = paths.lists.join("hydra-target.txt");
        let mapped_args = build_winws_args(&strategy.params, &[], &hostlist_path);
        let mapped = flag_values(&mapped_args);

        for flag in [
            "--dpi-desync",
            "--dpi-desync-repeats",
            "--dpi-desync-fooling",
            "--dpi-desync-cutoff",
            "--dpi-desync-any-protocol",
            "--ip-id",
            "--hostlist-domains",
            "--dpi-desync-fakedsplit-pattern",
        ] {
            assert_eq!(
                value_set(&original, flag),
                value_set(&mapped, flag),
                "flag {flag} differs between original bat and mapped args"
            );
        }

        // The original bat references the fake-TLS blob by file path;
        // Hydra stores its content hex-encoded instead. Verify the mapped
        // output carries the exact same bytes rather than just "a value".
        let real_bytes = std::fs::read(paths.bin.join("tls_clienthello_www_google_com.bin"))
            .expect("bundled fake-tls sample exists");
        let expected = format!("--dpi-desync-fake-tls=0x{}", encode_hex(&real_bytes));
        assert!(
            mapped_args.contains(&expected),
            "mapped args missing byte-identical fake-tls blob"
        );
    }
}
