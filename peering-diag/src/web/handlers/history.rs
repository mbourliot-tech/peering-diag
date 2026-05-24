//! Handlers pour l'API historique — lecture SQLite → JSON.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::web::handlers::diag::AppError;
use crate::web::server::AppState;
use crate::report::init_db;

// ─── Types de réponse ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RunJson {
    pub id:              i64,
    pub timestamp:       String,
    pub target:          String,
    pub verdict:         String,
    pub finding:         String,
    pub max_loss_aller:  f64,
    pub max_loss_retour: f64,
    pub avg_rtt_ms:      f64,
    pub dl_mbps:         f64,
}

#[derive(Debug, Serialize)]
pub struct HourStatJson {
    pub hour:        u8,
    pub total:       usize,
    pub bad:         usize,
    pub bad_pct:     f64,
    pub avg_loss:    f64,
    pub avg_rtt_ms:  f64,
    pub avg_dl_mbps: f64,
}

#[derive(Debug, Serialize)]
pub struct HopJson {
    pub timestamp: String,
    pub asn:       Option<i64>,
    pub as_name:   Option<String>,
    pub loss_pct:  f64,
    pub avg_ms:    f64,
    pub max_ms:    f64,
    pub stdev_ms:  f64,
}

#[derive(Debug, Serialize)]
pub struct RunDetailJson {
    pub id:        i64,
    pub timestamp: String,
    pub target:    String,
    pub aller:     Vec<HopDetailJson>,
    pub retour:    Vec<HopDetailJson>,
}

#[derive(Debug, Serialize)]
pub struct HopDetailJson {
    pub ttl:       i64,
    pub ip:        Option<String>,
    pub asn:       Option<i64>,
    pub as_name:   Option<String>,
    pub loss_pct:  Option<f64>,   // None = rate-limited / pas de réponse
    pub avg_ms:    f64,
    pub min_ms:    f64,
    pub max_ms:    f64,
    pub jitter_ms: f64,
    pub ratelimit: bool,
}

// ─── Query params ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub target: Option<String>,
    pub last:   Option<i64>,
    pub since:  Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HopQuery {
    pub target: Option<String>,
    pub last:   Option<i64>,
}

// ─── GET /api/history ─────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<RunJson>>, AppError> {
    let db_path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<RunJson>> {
        let conn  = init_db(&db_path)?;
        let limit = q.last.unwrap_or(50).clamp(1, 10_000);
        let sql = "
            SELECT
                r.id, r.timestamp, r.target, r.payload_json,
                COALESCE((
                    SELECT MAX(h.loss_pct) FROM hop_samples h
                    WHERE h.report_id = r.id AND h.suspected_ratelimit = 0 AND h.ip IS NOT NULL
                ), 0.0),
                COALESCE((
                    SELECT MAX(rh.loss_pct) FROM return_hop_samples rh
                    WHERE rh.report_id = r.id AND rh.ip IS NOT NULL
                ), 0.0),
                COALESCE((
                    SELECT AVG(h.avg_rtt_ms) FROM hop_samples h
                    WHERE h.report_id = r.id AND h.avg_rtt_ms IS NOT NULL
                ), 0.0),
                COALESCE((
                    SELECT MAX(s.download_mbps) FROM speedtest_samples s
                    WHERE s.report_id = r.id
                ), 0.0)
            FROM reports r
            WHERE (?1 IS NULL OR r.target = ?1)
              AND (?2 IS NULL OR r.timestamp >= ?2)
            ORDER BY r.timestamp DESC
            LIMIT ?3
        ";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(
            params![q.target, q.since, limit],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, f64>(7)?,
            )),
        )?;

        let mut result = Vec::new();
        for row in rows {
            let (id, ts, target, payload, loss_a, loss_r, rtt, dl) = row?;
            let (verdict, finding) = extract_verdict_finding(&payload);
            result.push(RunJson {
                id, timestamp: ts, target, verdict, finding,
                max_loss_aller: loss_a, max_loss_retour: loss_r,
                avg_rtt_ms: rtt, dl_mbps: dl,
            });
        }
        // Remettre en ordre chronologique pour les graphiques
        result.reverse();
        Ok(result)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(result))
}

// ─── GET /api/history/by-hour ─────────────────────────────────────────────────

pub async fn by_hour(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<HourStatJson>>, AppError> {
    let db_path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<HourStatJson>> {
        let conn = init_db(&db_path)?;
        // On réutilise la même requête que list mais sans LIMIT
        let sql = "
            SELECT r.timestamp, r.payload_json,
                COALESCE((
                    SELECT MAX(h.loss_pct) FROM hop_samples h
                    WHERE h.report_id = r.id AND h.suspected_ratelimit = 0 AND h.ip IS NOT NULL
                ), 0.0),
                COALESCE((
                    SELECT AVG(h.avg_rtt_ms) FROM hop_samples h
                    WHERE h.report_id = r.id AND h.avg_rtt_ms IS NOT NULL
                ), 0.0),
                COALESCE((
                    SELECT MAX(s.download_mbps) FROM speedtest_samples s WHERE s.report_id = r.id
                ), 0.0)
            FROM reports r
            WHERE (?1 IS NULL OR r.target = ?1)
            ORDER BY r.timestamp DESC
            LIMIT 100000
        ";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![q.target], |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, f64>(4)?,
        )))?;

        // Agrégation par heure
        struct HourAcc { total: usize, bad: usize, sum_loss: f64, sum_rtt: f64, sum_dl: f64 }
        let mut by_hour: Vec<HourAcc> = (0..24).map(|_| HourAcc { total:0, bad:0, sum_loss:0.0, sum_rtt:0.0, sum_dl:0.0 }).collect();

        for row in rows {
            let (ts, payload, loss, rtt, dl) = row?;
            let h = hour_from_ts(&ts) as usize;
            let verdict = extract_verdict_finding(&payload).0;
            let bad = verdict != "Healthy";
            by_hour[h].total    += 1;
            by_hour[h].sum_loss += loss;
            by_hour[h].sum_rtt  += rtt;
            by_hour[h].sum_dl   += dl;
            if bad { by_hour[h].bad += 1; }
        }

        let result = by_hour.into_iter().enumerate().filter(|(_, a)| a.total > 0).map(|(h, a)| {
            let n = a.total as f64;
            HourStatJson {
                hour: h as u8, total: a.total, bad: a.bad,
                bad_pct:     a.bad as f64 / n * 100.0,
                avg_loss:    a.sum_loss / n,
                avg_rtt_ms:  a.sum_rtt  / n,
                avg_dl_mbps: a.sum_dl   / n,
            }
        }).collect();
        Ok(result)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(result))
}

// ─── GET /api/history/run/:id ─────────────────────────────────────────────────

pub async fn run_detail(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<i64>,
) -> Result<Json<RunDetailJson>, AppError> {
    let db_path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<RunDetailJson> {
        let conn = init_db(&db_path)?;

        let (ts, target): (String, String) = conn.query_row(
            "SELECT timestamp, target FROM reports WHERE id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|_| anyhow::anyhow!("run #{} introuvable", run_id))?;

        // Hops aller
        let mut stmt = conn.prepare("
            SELECT ttl, ip, asn, as_name,
                   loss_pct, COALESCE(avg_rtt_ms,0.0),
                   COALESCE(min_rtt_ms,0.0), COALESCE(max_rtt_ms,0.0),
                   COALESCE(jitter_ms,0.0), suspected_ratelimit
            FROM hop_samples WHERE report_id = ?1 ORDER BY ttl
        ")?;
        let aller: Vec<HopDetailJson> = stmt.query_map(params![run_id], |row| Ok(HopDetailJson {
            ttl:       row.get(0)?,
            ip:        row.get(1)?,
            asn:       row.get(2)?,
            as_name:   row.get(3)?,
            loss_pct:  row.get(4)?,
            avg_ms:    row.get(5)?,
            min_ms:    row.get(6)?,
            max_ms:    row.get(7)?,
            jitter_ms: row.get(8)?,
            ratelimit: row.get::<_, i32>(9)? != 0,
        }))?.collect::<rusqlite::Result<_>>()?;

        // Hops retour
        let mut stmt2 = conn.prepare("
            SELECT ttl, ip, asn, as_name,
                   COALESCE(loss_pct,0.0), COALESCE(avg_ms,0.0),
                   COALESCE(min_ms,0.0), COALESCE(max_ms,0.0),
                   COALESCE(stdev_ms,0.0)
            FROM return_hop_samples WHERE report_id = ?1 ORDER BY ttl
        ")?;
        let retour: Vec<HopDetailJson> = stmt2.query_map(params![run_id], |row| Ok(HopDetailJson {
            ttl:       row.get(0)?,
            ip:        row.get(1)?,
            asn:       row.get(2)?,
            as_name:   row.get(3)?,
            loss_pct:  Some(row.get::<_, f64>(4)?),
            avg_ms:    row.get(5)?,
            min_ms:    row.get(6)?,
            max_ms:    row.get(7)?,
            jitter_ms: row.get(8)?,
            ratelimit: false,
        }))?.collect::<rusqlite::Result<_>>()?;

        Ok(RunDetailJson { id: run_id, timestamp: ts, target, aller, retour })
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(result))
}

// ─── GET /api/history/run/:id/map ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MapHopJson {
    pub ttl:       i64,
    pub ip:        Option<String>,
    pub asn:       Option<i64>,
    pub as_name:   Option<String>,
    pub lat:       Option<f64>,
    pub lon:       Option<f64>,
    pub city:      Option<String>,
    pub loss_pct:  Option<f64>,
    pub avg_ms:    f64,
    pub ratelimit: bool,
}

#[derive(Debug, Serialize)]
pub struct MapRunJson {
    pub id:        i64,
    pub timestamp: String,
    pub target:    String,
    pub aller:     Vec<MapHopJson>,
    pub retour:    Vec<MapHopJson>,
}

pub async fn run_map(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<i64>,
) -> Result<Json<MapRunJson>, AppError> {
    let db_path = state.db_path.clone();

    // ── Lecture des hops depuis la DB ─────────────────────────────────────────
    type AllerRow  = (i64, Option<String>, Option<i64>, Option<String>, Option<f64>, f64, bool);
    type RetourRow = (i64, Option<String>, Option<i64>, Option<String>, Option<f64>, f64);

    let (ts, target, aller_rows, retour_rows) = tokio::task::spawn_blocking(move || -> anyhow::Result<(String, String, Vec<AllerRow>, Vec<RetourRow>)> {
        let conn = init_db(&db_path)?;

        let (ts, target): (String, String) = conn.query_row(
            "SELECT timestamp, target FROM reports WHERE id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|_| anyhow::anyhow!("run #{} introuvable", run_id))?;

        let mut s1 = conn.prepare("
            SELECT ttl, ip, asn, as_name, loss_pct, COALESCE(avg_rtt_ms,0.0), suspected_ratelimit
            FROM hop_samples WHERE report_id = ?1 ORDER BY ttl
        ")?;
        let aller: Vec<AllerRow> = s1.query_map(params![run_id], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
            r.get(4)?, r.get(5)?, r.get::<_, i32>(6)? != 0,
        )))?.collect::<rusqlite::Result<_>>()?;

        let mut s2 = conn.prepare("
            SELECT ttl, ip, asn, as_name, loss_pct, COALESCE(avg_ms,0.0)
            FROM return_hop_samples WHERE report_id = ?1 ORDER BY ttl
        ")?;
        let retour: Vec<RetourRow> = s2.query_map(params![run_id], |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
            r.get(4)?, r.get(5)?,
        )))?.collect::<rusqlite::Result<_>>()?;

        Ok((ts, target, aller, retour))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    // ── Collecte des IPs publiques uniques ────────────────────────────────────
    let all_ips: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        aller_rows.iter()
            .filter_map(|(_, ip, ..)| ip.clone())
            .chain(retour_rows.iter().filter_map(|(_, ip, ..)| ip.clone()))
            .filter(|ip| seen.insert(ip.clone()))
            .collect()
    };

    // ── Géolocalisation ───────────────────────────────────────────────────────
    let geo = crate::web::geo::geolocate_batch(all_ips, state.db_path.clone()).await;

    // ── Construction de la réponse ────────────────────────────────────────────
    let aller = aller_rows.into_iter().map(|(ttl, ip, asn, as_name, loss_pct, avg_ms, ratelimit)| {
        let g = ip.as_ref().and_then(|i| geo.get(i));
        MapHopJson {
            ttl, ip, asn, as_name,
            lat:      g.map(|g| g.lat),
            lon:      g.map(|g| g.lon),
            city:     g.and_then(|g| g.city.clone()),
            loss_pct, avg_ms, ratelimit,
        }
    }).collect();

    let retour = retour_rows.into_iter().map(|(ttl, ip, asn, as_name, loss_pct, avg_ms)| {
        let g = ip.as_ref().and_then(|i| geo.get(i));
        MapHopJson {
            ttl, ip, asn, as_name,
            lat:      g.map(|g| g.lat),
            lon:      g.map(|g| g.lon),
            city:     g.and_then(|g| g.city.clone()),
            loss_pct, avg_ms, ratelimit: false,
        }
    }).collect();

    Ok(Json(MapRunJson { id: run_id, timestamp: ts, target, aller, retour }))
}

// ─── GET /api/history/targets ────────────────────────────────────────────────

pub async fn targets(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<String>>, AppError> {
    let db_path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
        let conn = init_db(&db_path)?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT target FROM reports ORDER BY target ASC"
        )?;
        let targets: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(targets)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(result))
}

// ─── GET /api/history/hop/:filter ─────────────────────────────────────────────

pub async fn hop(
    State(state): State<Arc<AppState>>,
    Path(filter): Path<String>,
    Query(q): Query<HopQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
        let conn  = init_db(&db_path)?;
        let limit = q.last.unwrap_or(50).clamp(1, 10_000);

        let upper = filter.to_uppercase();
        let filter_asn: Option<i64> = if let Some(s) = upper.strip_prefix("AS") {
            s.parse().ok()
        } else {
            filter.parse().ok()
        };
        let filter_ip: Option<&str> = if filter_asn.is_none() { Some(&filter) } else { None };

        let sql_aller = "
            SELECT r.timestamp, h.asn, h.as_name,
                   COALESCE(h.loss_pct,0.0), COALESCE(h.avg_rtt_ms,0.0),
                   COALESCE(h.max_rtt_ms,0.0), COALESCE(h.jitter_ms,0.0)
            FROM reports r
            INNER JOIN hop_samples h ON h.report_id = r.id
            WHERE (?1 IS NULL OR r.target = ?1)
              AND (?2 IS NULL OR h.asn = ?2)
              AND (?3 IS NULL OR h.ip  = ?3)
            ORDER BY r.timestamp DESC LIMIT ?4
        ";
        let mut stmt = conn.prepare(sql_aller)?;
        let aller: Vec<HopJson> = stmt.query_map(
            params![q.target, filter_asn, filter_ip, limit],
            hop_row,
        )?.collect::<rusqlite::Result<_>>()?;

        let sql_retour = "
            SELECT r.timestamp, rh.asn, rh.as_name,
                   COALESCE(rh.loss_pct,0.0), COALESCE(rh.avg_ms,0.0),
                   COALESCE(rh.max_ms,0.0), COALESCE(rh.stdev_ms,0.0)
            FROM reports r
            INNER JOIN return_hop_samples rh ON rh.report_id = r.id
            WHERE (?1 IS NULL OR r.target = ?1)
              AND (?2 IS NULL OR rh.asn = ?2)
              AND (?3 IS NULL OR rh.ip  = ?3)
            ORDER BY r.timestamp DESC LIMIT ?4
        ";
        let mut stmt2 = conn.prepare(sql_retour)?;
        let retour: Vec<HopJson> = stmt2.query_map(
            params![q.target, filter_asn, filter_ip, limit],
            hop_row,
        )?.collect::<rusqlite::Result<_>>()?;

        Ok(serde_json::json!({ "forward": aller, "return_path": retour }))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(result))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hop_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HopJson> {
    Ok(HopJson {
        timestamp: row.get(0)?,
        asn:       row.get(1)?,
        as_name:   row.get(2)?,
        loss_pct:  row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
        avg_ms:    row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
        max_ms:    row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
        stdev_ms:  row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
    })
}

fn extract_verdict_finding(payload: &str) -> (String, String) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return ("?".into(), "—".into());
    };
    let verdict = v["verdict"]["status"].as_str().unwrap_or("?").to_string();
    let finding = v["findings"].as_array()
        .and_then(|fs| fs.iter().find(|f| {
            let s = f["severity"].as_str().unwrap_or("");
            s == "Critical" || s == "Warning"
        }))
        .and_then(|f| f["description"].as_str())
        .unwrap_or("—")
        .to_string();
    (verdict, finding)
}

fn hour_from_ts(ts: &str) -> u8 {
    ts.find('T')
        .and_then(|i| ts.get(i+1..i+3))
        .and_then(|h| h.parse().ok())
        .unwrap_or(0)
}
