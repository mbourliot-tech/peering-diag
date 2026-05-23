//! Handlers pour la gestion des sessions watch.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::report::init_db;
use crate::web::handlers::diag::AppError;
use crate::web::server::AppState;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct StartWatchRequest {
    pub target:       String,
    pub interval:     Option<u64>,   // minutes (défaut : 15)
    pub no_speedtest: Option<bool>,
    pub my_ip:        Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WatchSeriesJson {
    pub id:          i64,
    pub started_at:  String,
    pub target:      String,
    pub interval_s:  i64,
    pub run_count:   i64,
    pub last_verdict: Option<String>,
    pub job_id:      Option<uuid::Uuid>,
}

// ─── POST /api/watch ─────────────────────────────────────────────────────────

pub async fn start(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartWatchRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let interval = req.interval.unwrap_or(15);

    let mut args = vec![
        "watch".to_string(),
        req.target.clone(),
        "--interval".to_string(),
        interval.to_string(),
        "--quiet".to_string(),
    ];
    if req.no_speedtest.unwrap_or(false) {
        args.push("--no-speedtest".to_string());
    }
    if let Some(ip) = req.my_ip {
        args.push("--my-ip".to_string());
        args.push(ip);
    }

    let job_id = state
        .jobs
        .spawn(format!("watch:{}", req.target), args, Some(state.db_path.clone()))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "job_id": job_id })))
}

// ─── GET /api/watch ──────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<WatchSeriesJson>>, AppError> {
    let db_path = state.db_path.clone();
    let mut result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<WatchSeriesJson>> {
        let conn = init_db(&db_path)?;
        let sql = "
            SELECT ws.id, ws.started_at, ws.target, ws.interval_s,
                   COUNT(r.id),
                   (SELECT payload_json FROM reports WHERE watch_series_id = ws.id
                    ORDER BY timestamp DESC LIMIT 1)
            FROM watch_series ws
            LEFT JOIN reports r ON r.watch_series_id = ws.id
            GROUP BY ws.id
            ORDER BY ws.started_at DESC
            LIMIT 50
        ";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
        )))?;

        let mut result = Vec::new();
        for row in rows {
            let (id, started_at, target, interval_s, run_count, last_payload) = row?;
            let last_verdict = last_payload.as_deref().and_then(|p| {
                serde_json::from_str::<serde_json::Value>(p).ok()
                    .and_then(|v| v["verdict"]["status"].as_str().map(|s| s.to_string()))
            });
            result.push(WatchSeriesJson {
                id, started_at, target, interval_s, run_count,
                last_verdict,
                job_id: None, // enrichi plus bas si actif
            });
        }
        Ok(result)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::Internal(e.to_string()))?;

    // Corréler les sessions DB avec les jobs watch actifs (en mémoire).
    // Un job actif a le label "watch:<target>" ; on l'associe à la série la plus
    // récente du même target qui n'a pas encore de job_id (result est trié DESC).
    let active = state.jobs.list().await;
    for job in active.iter().filter(|j| {
        j.status == crate::web::jobs::JobStatus::Running && j.command.starts_with("watch:")
    }) {
        let target = job.command.strip_prefix("watch:").unwrap_or("");
        if let Some(s) = result
            .iter_mut()
            .find(|s| s.target == target && s.job_id.is_none())
        {
            s.job_id = Some(job.id);
        }
    }

    Ok(Json(result))
}

// ─── DELETE /api/watch/:id ────────────────────────────────────────────────────

/// Stoppe un job watch actif identifié par son job_id (UUID du job, pas series_id).
pub async fn stop(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let job = state.jobs.get(job_id).await.ok_or(AppError::NotFound)?;
    // Tue réellement le subprocess (abort de la task → kill_on_drop) puis retire le job.
    job.kill().await;
    state.jobs.remove(job_id).await;
    Ok(Json(serde_json::json!({ "stopped": job_id })))
}
