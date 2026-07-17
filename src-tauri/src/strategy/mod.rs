pub mod bat_import;
pub mod fake_tls;
pub mod schema;
pub mod winws_mapper;

use rusqlite::{params, Connection};

use crate::error::AppResult;
use crate::strategy::schema::Strategy;

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
            (id, name, target, params_json, source, score, last_tested_at, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5, 0.0, NULL, 0)",
        params![
            strategy.id,
            strategy.name,
            strategy.target.as_db_str(),
            params_json,
            strategy.source.as_db_str(),
        ],
    )?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn syncing_twice_only_inserts_once() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE strategies (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, target TEXT NOT NULL,
                params_json TEXT NOT NULL, source TEXT NOT NULL, score REAL DEFAULT 0.0,
                last_tested_at INTEGER, is_active BOOLEAN DEFAULT 0
            );
            CREATE TABLE hydra_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();

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
