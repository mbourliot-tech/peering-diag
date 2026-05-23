//! Handlers pour la maintenance de la base SQLite.

use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::report::{init_db, maintenance};
use crate::web::handlers::diag::AppError;
use crate::web::server::AppState;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DbStatsJson {
    pub run_count:          i64,
    pub hop_count:          i64,
    pub speedtest_count:    i64,
    pub watch_series_count: i64,
    pub oldest_run:         Option<String>,
    pub newest_run:         Option<String>,
    pub db_size_bytes:      u64,
    pub human_size:         String,
}

#[derive(Debug, Deserialize)]
pub struct PurgeRequest {
    pub older_than_days: Option<u32>,
    pub keep_last:       Option<usize>,
}

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_bytes_bytes() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
    }

    #[test]
    fn test_human_bytes_kilobytes() {
        assert_eq!(human_bytes(1_000), "1.0 KB");
        assert_eq!(human_bytes(1_500), "1.5 KB");
        assert_eq!(human_bytes(999_999), "1000.0 KB");
    }

    #[test]
    fn test_human_bytes_megabytes() {
        assert_eq!(human_bytes(1_000_000), "1.0 MB");
        assert_eq!(human_bytes(2_500_000), "2.5 MB");
    }
}

// ─── GET /api/db/stats ────────────────────────────────────────────────────────

pub async fn stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DbStatsJson>, AppError> {
    let db_path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<DbStatsJson> {
        let conn = init_db(&db_path)?;
        let s = maintenance::get_stats(&conn, &db_path)?;
        Ok(DbStatsJson {
            run_count:          s.run_count,
            hop_count:          s.hop_count,
            speedtest_count:    s.speedtest_count,
            watch_series_count: s.watch_series_count,
            oldest_run:         s.oldest_run,
            newest_run:         s.newest_run,
            human_size:         human_bytes(s.db_size_bytes),
            db_size_bytes:      s.db_size_bytes,
        })
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(result))
}

// ─── POST /api/db/vacuum ──────────────────────────────────────────────────────

pub async fn vacuum_db(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = init_db(&db_path)?;
        maintenance::vacuum(&conn)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "message": "VACUUM terminé" })))
}

// ─── POST /api/db/purge ───────────────────────────────────────────────────────

pub async fn purge(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PurgeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_path = state.db_path.clone();
    let deleted = tokio::task::spawn_blocking(move || -> anyhow::Result<i64> {
        let conn = init_db(&db_path)?;
        if let Some(days) = body.older_than_days {
            let days = days.clamp(1, 3_650);
            return maintenance::purge_older_than(&conn, days);
        }
        if let Some(keep) = body.keep_last {
            let keep = keep.clamp(1, 10_000);
            return maintenance::purge_keep_last(&conn, keep);
        }
        Ok(0)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
