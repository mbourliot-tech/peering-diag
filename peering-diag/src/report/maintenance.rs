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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE reports (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp    TEXT NOT NULL,
                target       TEXT NOT NULL,
                target_ip    TEXT NOT NULL,
                target_asn   INTEGER,
                as_name      TEXT,
                payload_json TEXT NOT NULL
            );
            CREATE TABLE hop_samples (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                report_id INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE
            );
            CREATE TABLE speedtest_samples (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                report_id INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
                timestamp TEXT NOT NULL
            );
            CREATE TABLE watch_series (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL,
                target     TEXT NOT NULL,
                interval_s INTEGER NOT NULL
            );
            CREATE TABLE return_hop_samples (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                report_id INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
                ttl       INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    fn insert_report(conn: &Connection, timestamp: &str) {
        conn.execute(
            "INSERT INTO reports (timestamp, target, target_ip, payload_json)
             VALUES (?1, 'test.example.com', '1.2.3.4', '{}')",
            rusqlite::params![timestamp],
        )
        .unwrap();
    }

    fn count_reports(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn test_purge_keep_last_removes_excess() {
        let conn = setup_db();
        for i in 1..=5 {
            insert_report(&conn, &format!("2024-01-{:02}T00:00:00Z", i));
        }
        let deleted = purge_keep_last(&conn, 2).unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(count_reports(&conn), 2);
    }

    #[test]
    fn test_purge_keep_last_noop_when_under_limit() {
        let conn = setup_db();
        insert_report(&conn, "2024-01-01T00:00:00Z");
        insert_report(&conn, "2024-01-02T00:00:00Z");
        let deleted = purge_keep_last(&conn, 5).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(count_reports(&conn), 2);
    }

    #[test]
    fn test_purge_older_than_removes_old_records() {
        let conn = setup_db();
        insert_report(&conn, "2020-01-01T00:00:00Z");
        insert_report(&conn, "2020-06-01T00:00:00Z");
        let deleted = purge_older_than(&conn, 1).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(count_reports(&conn), 0);
    }

    #[test]
    fn test_purge_older_than_keeps_recent_records() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO reports (timestamp, target, target_ip, payload_json)
             VALUES (datetime('now'), 'test.example.com', '1.2.3.4', '{}')",
            [],
        )
        .unwrap();
        let deleted = purge_older_than(&conn, 30).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(count_reports(&conn), 1);
    }

    #[test]
    fn test_get_stats_empty_db() {
        let conn = setup_db();
        let stats = get_stats(&conn, Path::new(":memory:")).unwrap();
        assert_eq!(stats.run_count, 0);
        assert_eq!(stats.hop_count, 0);
        assert_eq!(stats.speedtest_count, 0);
        assert_eq!(stats.watch_series_count, 0);
        assert!(stats.oldest_run.is_none());
        assert!(stats.newest_run.is_none());
    }

    #[test]
    fn test_get_stats_counts_correctly() {
        let conn = setup_db();
        insert_report(&conn, "2024-01-01T10:00:00Z");
        insert_report(&conn, "2024-01-02T10:00:00Z");
        let stats = get_stats(&conn, Path::new(":memory:")).unwrap();
        assert_eq!(stats.run_count, 2);
        assert_eq!(stats.oldest_run.as_deref(), Some("2024-01-01T10:00:00Z"));
        assert_eq!(stats.newest_run.as_deref(), Some("2024-01-02T10:00:00Z"));
    }
}
