//! Mesure de débit via téléchargement HTTP.
//!
//! Télécharge un fichier test depuis un endpoint public et mesure le débit
//! réel obtenu. Utile comme fallback quand Speedtest et iperf3 ne sont pas
//! disponibles pour un AS donné.

use anyhow::{anyhow, Result};
use std::time::{Duration, Instant};
use tokio::time::timeout;

const HTTP_TIMEOUT: Duration = Duration::from_secs(60);
/// Taille max à télécharger pour la mesure (on n'a pas besoin du fichier entier)
const MAX_BYTES: usize = 50 * 1024 * 1024; // 50 MB suffisent pour une mesure fiable

#[derive(Debug, Clone)]
pub struct HttpMeasureResult {
    pub url: String,
    pub description: String,
    pub download_mbps: f64,
    pub bytes_received: usize,
    pub duration_secs: f64,
}

/// Mesure le débit de téléchargement depuis une URL.
pub async fn measure_http_download(url: &str, description: &str) -> Result<HttpMeasureResult> {
    let client = reqwest::Client::builder()
        .user_agent("peering-diag/0.1")
        .timeout(HTTP_TIMEOUT)
        .build()?;

    let fut = async {
        let response = client.get(url).send().await
            .map_err(|e| anyhow!("connexion HTTP échouée : {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("HTTP {} pour {}", response.status(), url));
        }

        let start = Instant::now();
        let body = response.bytes().await
            .map_err(|e| anyhow!("erreur lecture body : {}", e))?;

        let bytes_received = body.len().min(MAX_BYTES);
        let elapsed = start.elapsed();

        if elapsed.as_secs_f64() < 0.5 {
            return Err(anyhow!("test trop court pour être fiable ({:.1}s)", elapsed.as_secs_f64()));
        }

        let mbps = (bytes_received as f64 * 8.0) / elapsed.as_secs_f64() / 1_000_000.0;

        Ok(HttpMeasureResult {
            url: url.to_string(),
            description: description.to_string(),
            download_mbps: mbps,
            bytes_received,
            duration_secs: elapsed.as_secs_f64(),
        })
    };

    timeout(HTTP_TIMEOUT, fut)
        .await
        .map_err(|_| anyhow!("HTTP download timeout pour {}", url))?
}
