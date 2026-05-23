//! Handlers pour les jobs (diag, aller, mtr, ecmp, retour, lg, check-env).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::web::{jobs::JobInfo, server::AppState, sse::job_stream};

// ─── Types de requête / réponse ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct StartJobRequest {
    pub command: String,
    /// Arguments spécifiques à la commande (target, rounds, etc.)
    #[serde(default)]
    pub args: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct StartJobResponse {
    pub job_id: Uuid,
}

// ─── POST /api/jobs ───────────────────────────────────────────────────────────

pub async fn start_job(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartJobRequest>,
) -> Result<Json<StartJobResponse>, AppError> {
    let args = build_args(&req.command, &req.args)?;

    // Seules ces commandes acceptent --db
    const DB_COMMANDS: &[&str] = &["diag", "aller", "mtr", "watch"];
    let db_path = if DB_COMMANDS.contains(&req.command.as_str()) {
        Some(state.db_path.clone())
    } else {
        None
    };

    let job_id = state
        .jobs
        .spawn(req.command.clone(), args, db_path)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(StartJobResponse { job_id }))
}

/// Construit la liste d'arguments CLI à partir de la commande et des args JSON.
fn build_args(
    command: &str,
    args: &HashMap<String, serde_json::Value>,
) -> Result<Vec<String>, AppError> {
    // Whitelist des commandes autorisées
    const VALID_COMMANDS: &[&str] = &[
        "diag", "aller", "mtr", "ecmp", "retour", "lg", "check-env", "watch",
    ];
    if !VALID_COMMANDS.contains(&command) {
        return Err(AppError::BadRequest(format!("Commande inconnue : {command}")));
    }

    let mut cli = vec![command.to_string()];

    // target (positionnel — toujours en premier si présent)
    if let Some(t) = args.get("target").and_then(|v| v.as_str()) {
        if t.starts_with('-') {
            return Err(AppError::BadRequest(
                "Cible invalide : ne peut pas commencer par '-'".into(),
            ));
        }
        if t.len() > 253 {
            return Err(AppError::BadRequest(
                "Cible trop longue (max 253 caractères)".into(),
            ));
        }
        cli.push(t.to_string());
    }

    // Flags booléens
    for flag in &["no_speedtest", "quiet", "by_hour"] {
        if args.get(*flag).and_then(|v| v.as_bool()).unwrap_or(false) {
            cli.push(format!("--{}", flag.replace('_', "-")));
        }
    }

    // Arguments avec valeur entière
    for key in &["rounds", "probes", "max_hops", "last", "interval", "count", "port", "flows", "ttl"] {
        if let Some(v) = args.get(*key).and_then(|v| v.as_i64()) {
            cli.push(format!("--{}", key.replace('_', "-")));
            cli.push(v.to_string());
        }
    }

    // Arguments avec valeur string
    for key in &["target_filter", "since", "hop", "my_ip"] {
        if let Some(v) = args.get(*key).and_then(|v| v.as_str()) {
            cli.push(format!("--{}", key.replace('_', "-")));
            cli.push(v.to_string());
        }
    }

    // check-env n'a pas de target
    if command == "check-env" {
        cli = vec!["check-env".to_string()];
    }

    Ok(cli)
}

// ─── GET /api/jobs ─────────────────────────────────────────────────────────────

pub async fn list_jobs(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<JobInfo>> {
    Json(state.jobs.list().await)
}

// ─── GET /api/jobs/:id ────────────────────────────────────────────────────────

pub async fn job_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<JobInfo>, AppError> {
    let job = state
        .jobs
        .get(id)
        .await
        .ok_or(AppError::NotFound)?;
    Ok(Json(job.info().await))
}

// ─── GET /api/jobs/:id/stream ─────────────────────────────────────────────────

pub async fn job_stream_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let job = state
        .jobs
        .get(id)
        .await
        .ok_or(AppError::NotFound)?;
    Ok(job_stream(job).into_response())
}

// ─── Erreurs ─────────────────────────────────────────────────────────────────

pub enum AppError {
    NotFound,
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AppError::NotFound       => (StatusCode::NOT_FOUND,            "Not found".to_string()),
            AppError::BadRequest(m)  => (StatusCode::BAD_REQUEST,           m),
            AppError::Internal(m)    => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}
