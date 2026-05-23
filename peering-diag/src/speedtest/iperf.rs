//! Mesure de débit via iperf3.
//!
//! On wrappe le binaire `iperf3` s'il est disponible, sinon on skip proprement.
//! iperf3 donne une mesure de débit TCP brut, indépendante du protocole Ookla.
//! Utile pour les AS de transit qui n'ont pas de serveur Speedtest.

use anyhow::{anyhow, Result};
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use serde::Deserialize;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const IPERF3_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct Iperf3Result {
    pub server: String,
    pub location: String,
    pub download_mbps: f64,
    pub upload_mbps: f64,
}

#[derive(Debug, Deserialize)]
struct Iperf3Output {
    end: Iperf3End,
}

#[derive(Debug, Deserialize)]
struct Iperf3End {
    sum_received: Iperf3Sum,
    sum_sent: Iperf3Sum,
}

#[derive(Debug, Deserialize)]
struct Iperf3Sum {
    bits_per_second: f64,
}

/// Vérifie que iperf3 est installé.
pub async fn check_iperf3() -> bool {
    Command::new("iperf3")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Résout un hostname via Cloudflare pour contourner le DNS système (Livebox).
/// Utilisé avant de lancer iperf3 car le sous-processus utilise le DNS système.
async fn resolve_host(host: &str) -> String {
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(3);
    opts.attempts = 1;
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::cloudflare(), opts);
    resolver
        .lookup_ip(host)
        .await
        .ok()
        .and_then(|r| r.iter().next())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| host.to_string())
}

/// Lance un test iperf3 vers un serveur "host:port".
/// Fait un test download (reverse) puis upload.
pub async fn run_iperf3(host_port: &str, location: &str) -> Result<Iperf3Result> {
    let parts: Vec<&str> = host_port.splitn(2, ':').collect();
    let host = parts[0];
    let port = parts.get(1).copied().unwrap_or("5201");

    // Pré-résolution via Cloudflare : iperf3 (sous-processus) utilise le DNS
    // système qui peut être flaky — on passe l'IP directement.
    let resolved = resolve_host(host).await;

    // Test download (--reverse = serveur envoie, client reçoit)
    let dl = run_iperf3_direction(&resolved, port, true).await?;
    // Pause courte entre les deux tests
    tokio::time::sleep(Duration::from_secs(3)).await;
    // Test upload
    let ul = run_iperf3_direction(&resolved, port, false).await?;

    Ok(Iperf3Result {
        server: host_port.to_string(), // on garde le nom original pour l'affichage
        location: location.to_string(),
        download_mbps: dl,
        upload_mbps: ul,
    })
}

async fn run_iperf3_direction(host: &str, port: &str, reverse: bool) -> Result<f64> {
    let mut args = vec![
        "--client", host,
        "--port", port,
        "--json",
        "--time", "8",          // 8 secondes par test
        "--parallel", "4",      // 4 flux parallèles (compense le BDP sur longue distance)
        "--omit", "2",          // ignore les 2 premières secondes (warmup)
    ];
    if reverse {
        args.push("--reverse");
    }

    let fut = async {
        let output = Command::new("iperf3")
            .args(&args)
            .output()
            .await?;

        if !output.status.success() {
            // En mode --json, iperf3 écrit les erreurs sur stdout ({"error":"…"})
            // plutôt que sur stderr — on cherche d'abord dans le JSON stdout.
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = serde_json::from_str::<serde_json::Value>(&stdout)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_string))
                .or_else(|| {
                    let s = stderr.trim().to_string();
                    if s.is_empty() { None } else { Some(s) }
                })
                .unwrap_or_else(|| "raison inconnue".to_string());
            return Err(anyhow!("iperf3 a échoué : {}", reason));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let parsed: Iperf3Output = serde_json::from_str(&stdout)
            .map_err(|e| anyhow!("parse JSON iperf3 : {}", e))?;

        let bps = if reverse {
            parsed.end.sum_received.bits_per_second
        } else {
            parsed.end.sum_sent.bits_per_second
        };

        Ok(bps / 1_000_000.0)
    };

    timeout(IPERF3_TIMEOUT, fut)
        .await
        .map_err(|_| anyhow!("iperf3 timeout vers {}", host))?
}
