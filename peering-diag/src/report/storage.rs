//! Persistance du rapport : JSON pour archivage, SQLite pour historisation.

use crate::lg::globalping::MtrHop;
use crate::types::DiagnosticReport;
use anyhow::{Context, Result};
use chrono::Local;
use rusqlite::{params, Connection};
use std::path::Path;

/// Exporte le rapport au format JSON.
pub fn export_json(report: &DiagnosticReport, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json).context("écriture JSON")?;
    Ok(())
}

/// Initialise la base SQLite (idempotent).
pub fn init_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS reports (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            target TEXT NOT NULL,
            target_ip TEXT NOT NULL,
            target_asn INTEGER,
            target_as_name TEXT,
            payload_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_reports_target    ON reports(target);
        CREATE INDEX IF NOT EXISTS idx_reports_timestamp ON reports(timestamp);

        CREATE TABLE IF NOT EXISTS hop_samples (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id         INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
            ttl               INTEGER NOT NULL,
            ip                TEXT,
            asn               INTEGER,
            as_name           TEXT,
            loss_pct          REAL,
            avg_rtt_ms        REAL,
            min_rtt_ms        REAL,
            max_rtt_ms        REAL,
            jitter_ms         REAL,
            suspected_ratelimit INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_hop_report ON hop_samples(report_id);

        CREATE TABLE IF NOT EXISTS speedtest_samples (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id    INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
            timestamp    TEXT NOT NULL,
            asn          INTEGER,
            as_name      TEXT,
            server_name  TEXT,
            download_mbps REAL,
            upload_mbps  REAL,
            ping_ms      REAL,
            jitter_ms    REAL
        );
        CREATE INDEX IF NOT EXISTS idx_speedtest_report ON speedtest_samples(report_id);

        -- ── Extensions Phase 1 (tests temporels) ───────────────────────────

        -- Versionnage des migrations (idempotent)
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );

        -- Hops du chemin retour mesurés par Globalping (MTR-style agrégé)
        CREATE TABLE IF NOT EXISTS return_hop_samples (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            report_id INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
            ttl       INTEGER NOT NULL,
            host      TEXT,
            ip        TEXT,
            asn       INTEGER,
            as_name   TEXT,
            loss_pct  REAL,
            snt       INTEGER,
            last_ms   REAL,
            avg_ms    REAL,
            min_ms    REAL,
            max_ms    REAL,
            stdev_ms  REAL
        );
        CREATE INDEX IF NOT EXISTS idx_return_hop_report ON return_hop_samples(report_id);

        -- Métadonnées de chaque série de tests périodiques (commande watch)
        CREATE TABLE IF NOT EXISTS watch_series (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at TEXT NOT NULL,
            target     TEXT NOT NULL,
            interval_s INTEGER NOT NULL
        );

        -- Cache de géolocalisation IP (ip-api.com, TTL 30 jours)
        CREATE TABLE IF NOT EXISTS geo_cache (
            ip         TEXT PRIMARY KEY,
            lat        REAL NOT NULL,
            lon        REAL NOT NULL,
            city       TEXT,
            country    TEXT,
            cached_at  TEXT NOT NULL
        );
        "#,
    )?;
    migrate_db(&conn)?;
    Ok(conn)
}

/// Applique les migrations de schéma incrémentales (idempotent).
///
/// Version 1 : ajoute `watch_series_id` dans `reports` (nullable).
fn migrate_db(conn: &Connection) -> Result<()> {
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if version < 1 {
        // ALTER TABLE échoue silencieusement si la colonne existe déjà
        // (cas d'une DB créée avec le nouveau schéma avant la migration).
        let _ = conn.execute(
            "ALTER TABLE reports ADD COLUMN watch_series_id INTEGER REFERENCES watch_series(id)",
            [],
        );
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
    }
    Ok(())
}

/// Persiste un rapport en base. Retourne l'ID de la ligne créée.
///
/// `watch_series_id` : si `Some(id)`, associe le rapport à une série watch.
pub fn store_report(
    conn: &mut Connection,
    report: &DiagnosticReport,
    watch_series_id: Option<i64>,
) -> Result<i64> {
    let tx = conn.transaction()?;

    let payload = serde_json::to_string(report)?;
    tx.execute(
        "INSERT INTO reports (timestamp, target, target_ip, target_asn, target_as_name,
         payload_json, watch_series_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            report.timestamp.with_timezone(&Local).to_rfc3339(),
            report.target,
            report.target_ip.to_string(),
            report.target_as.as_ref().map(|a| a.asn),
            report.target_as.as_ref().map(|a| a.name.clone()),
            payload,
            watch_series_id,
        ],
    )?;
    let report_id = tx.last_insert_rowid();

    for hop in &report.hops {
        tx.execute(
            "INSERT INTO hop_samples (report_id, ttl, ip, asn, as_name, loss_pct,
             avg_rtt_ms, min_rtt_ms, max_rtt_ms, jitter_ms, suspected_ratelimit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                report_id,
                hop.ttl,
                hop.primary_ip.map(|ip| ip.to_string()),
                hop.as_info.as_ref().map(|a| a.asn),
                hop.as_info.as_ref().map(|a| a.name.clone()),
                // NULL pour les hops sans réponse (rate-limit ou trou) : la perte
                // brute n'a pas de sens — ces hops ne transmettent pas de TTL Exceeded.
                if hop.suspected_icmp_ratelimit || hop.primary_ip.is_none() {
                    None
                } else {
                    Some(hop.loss_pct())
                },
                hop.avg_rtt_ms(),
                hop.min_rtt_ms(),
                hop.max_rtt_ms(),
                hop.jitter_ms(),
                hop.suspected_icmp_ratelimit as i32,
            ],
        )?;
    }

    for st in &report.speedtests {
        tx.execute(
            "INSERT INTO speedtest_samples (report_id, timestamp, asn, as_name,
             server_name, download_mbps, upload_mbps, ping_ms, jitter_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                report_id,
                st.timestamp.with_timezone(&Local).to_rfc3339(),
                st.asn,
                st.as_name,
                st.server_name,
                st.download_mbps,
                st.upload_mbps,
                st.ping_ms,
                st.jitter_ms,
            ],
        )?;
    }

    tx.commit()?;
    Ok(report_id)
}

/// Crée une nouvelle série de tests périodiques (commande `watch`).
/// Retourne l'ID de la série créée.
pub fn store_watch_series(
    conn: &Connection,
    started_at: &str,
    target: &str,
    interval_s: i64,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO watch_series (started_at, target, interval_s) VALUES (?1, ?2, ?3)",
        params![started_at, target, interval_s],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Persiste les hops du chemin retour (Globalping) pour un rapport existant.
///
/// À appeler après `store_report` avec le `report_id` retourné.
pub fn store_return_hops(conn: &Connection, report_id: i64, hops: &[MtrHop]) -> Result<()> {
    for hop in hops {
        conn.execute(
            "INSERT INTO return_hop_samples
             (report_id, ttl, host, ip, asn, as_name, loss_pct, snt,
              last_ms, avg_ms, min_ms, max_ms, stdev_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                report_id,
                hop.ttl as i64,
                hop.host,
                hop.ip.map(|ip| ip.to_string()),
                hop.as_info.as_ref().map(|a| a.asn),
                hop.as_info.as_ref().map(|a| a.name.clone()),
                hop.loss_pct,
                hop.snt,
                hop.last_ms,
                hop.avg_ms,
                hop.min_ms,
                hop.max_ms,
                hop.stdev_ms,
            ],
        )?;
    }
    Ok(())
}
