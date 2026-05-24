//! Serveur Axum — router principal + AppState.

use anyhow::Result;
use axum::{
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use axum::http::{header, HeaderValue, Method};
use tower_http::{cors::CorsLayer, services::{ServeDir, ServeFile}};

use crate::web::{
    handlers::{db, diag, history, watch},
    jobs::JobManager,
};

// ─── AppState ─────────────────────────────────────────────────────────────────

pub struct AppState {
    pub db_path: PathBuf,
    pub jobs:    Arc<JobManager>,
}

// ─── Démarrage du serveur ─────────────────────────────────────────────────────

pub async fn run_serve(port: u16, db_path: PathBuf) -> Result<()> {
    let state = Arc::new(AppState {
        db_path,
        jobs: Arc::new(JobManager::new()),
    });

    // Chemin vers le frontend compilé (relatif au binaire)
    let frontend_dir = {
        let bin_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        // En dev : web/dist/ depuis la racine du projet
        // En prod : dist/ à côté du binaire
        let dev_path  = PathBuf::from("web/dist");
        let prod_path = bin_dir.join("dist");
        if dev_path.exists() { dev_path } else { prod_path }
    };

    let app = Router::new()
        // ── Jobs (commandes longues) ─────────────────────────────────────────
        .route("/api/jobs",              post(diag::start_job).get(diag::list_jobs))
        .route("/api/jobs/:id",          get(diag::job_status))
        .route("/api/jobs/:id/stream",   get(diag::job_stream_handler))
        // ── Historique ────────────────────────────────────────────────────────
        .route("/api/history",           get(history::list))
        .route("/api/history/by-hour",   get(history::by_hour))
        .route("/api/history/run/:id",     get(history::run_detail))
        .route("/api/history/run/:id/map", get(history::run_map))
        .route("/api/history/hop/:filter", get(history::hop))
        // ── Watch ─────────────────────────────────────────────────────────────
        .route("/api/watch",             post(watch::start).get(watch::list))
        .route("/api/watch/:id",         delete(watch::stop))
        // ── Maintenance DB ────────────────────────────────────────────────────
        .route("/api/db/stats",          get(db::stats))
        .route("/api/db/vacuum",         post(db::vacuum_db))
        .route("/api/db/purge",          post(db::purge))
        // ── Frontend statique (fallback index.html pour React Router) ────────
        .fallback_service(
            ServeDir::new(&frontend_dir)
                .fallback(ServeFile::new(frontend_dir.join("index.html")))
        )
        .with_state(state.clone())
        .layer(
            CorsLayer::new()
                .allow_origin([
                    format!("http://localhost:{port}").parse::<HeaderValue>().expect("valid origin"),
                    format!("http://127.0.0.1:{port}").parse::<HeaderValue>().expect("valid origin"),
                ])
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers([header::CONTENT_TYPE]),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    println!();
    println!("  🌐  Interface web : http://localhost:{}/", port);
    if frontend_dir.exists() {
        println!("  📁  Frontend     : {}", frontend_dir.display());
    } else {
        println!("  ⚠   Frontend non trouvé — lancez : cd web && npm run build");
    }
    println!("  🗄   Base de données : {}", state.db_path.display());
    println!();
    println!("  Ctrl+C pour arrêter.");
    println!();

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
