//! Maintenance de la base SQLite : stats, purge, vacuum.

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct DbStats {
    pub run_count:          i64,
    pub hop_count:          i64,
    pub speedtest_count:    i64,
    pub watch_series_count: i64,
    pub oldest_run:         Option<String>,
    pub newest_run:         Option<String>,
    pub db_size_bytes:      u64,
}

/// Retourne les statistiques de la base.
pub fn get_stats(conn: &Connection, db_path: &Path) -> Result<DbStats> {
    let run_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reports", [], |r| r.get(0))?;

    let hop_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM hop_samples", [], |r| r.get(0))?;

    let speedtest_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM speedtest_samples", [], |r| r.get(0))?;

    let watch_series_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM watch_series", [], |r| r.get(0))?;

    let oldest_run: Option<String> = conn.query_row(
        "SELECT MIN(timestamp) FROM reports", [], |r| r.get(0))?;

    let newest_run: Option<String> = conn.query_row(
        "SELECT MAX(timestamp) FROM reports", [], |r| r.get(0))?;

    let db_size_bytes = std::fs::metadata(db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(DbStats {
        run_count,
        hop_count,
        speedtest_count,
        watch_series_count,
        oldest_run,
        newest_run,
        db_size_bytes,
    })
}

/// Compacte la base (SQLite VACUUM).
pub fn vacuum(conn: &Connection) -> Result<()> {
    conn.execute_batch("VACUUM;")?;
    Ok(())
}

/// Supprime tous les runs plus anciens que `days` jours (et leurs hops en CASCADE).
/// Retourne le nombre de runs supprimés.
pub fn purge_older_than(conn: &Connection, days: u32) -> Result<i64> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let param = format!("-{days} days");
    let deleted = conn.execute(
        "DELETE FROM reports WHERE timestamp < datetime('now', ?1)",
        rusqlite::params![param],
    )? as i64;
    Ok(deleted)
}

/// Garde uniquement les `keep` derniers runs (supprime le reste en CASCADE).
/// Retourne le nombre de runs supprimés.
pub fn purge_keep_last(conn: &Connection, keep: usize) -> Result<i64> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    let deleted = conn.execute(
        "DELETE FROM reports WHERE id NOT IN \
         (SELECT id FROM reports ORDER BY timestamp DESC LIMIT ?1)",
        rusqlite::params![keep as i64],
    )? as i64;
    Ok(deleted)
}
