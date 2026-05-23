//! Exécution de speedtests via le CLI Ookla officiel.
//!
//! On wrappe `speedtest` (le CLI officiel Ookla, à installer via `winget install Ookla.Speedtest.CLI`
//! sur Windows ou `apt install speedtest` après le repo Ookla).
//!
//! Pourquoi pas réimplémenter le protocole ? Parce que Ookla a un protocole
//! propriétaire qui change régulièrement et qui gère intelligemment le nombre
//! de threads, le warmup, etc. Réinventer ça est un projet en soi. Et le CLI
//! officiel sort un JSON parfaitement parsable.

use crate::types::SpeedtestResult;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const SPEEDTEST_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize)]
struct OoklaOutput {
    ping: OoklaPing,
    download: OoklaBandwidth,
    upload: OoklaBandwidth,
    server: OoklaServer,
}

#[derive(Debug, Deserialize)]
struct OoklaPing {
    latency: f64,
    #[serde(default)]
    jitter: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OoklaBandwidth {
    /// Octets par seconde.
    bandwidth: u64,
}

#[derive(Debug, Deserialize)]
struct OoklaServer {
    id: u32,
    name: String,
}

/// Vérifie que le CLI Ookla est installé.
pub async fn check_speedtest_cli() -> Result<String> {
    let output = Command::new("speedtest")
        .arg("--version")
        .output()
        .await
        .context("speedtest CLI introuvable. Installer via : `winget install Ookla.Speedtest.CLI` (Windows) ou voir https://www.speedtest.net/apps/cli")?;

    if !output.status.success() {
        return Err(anyhow!("speedtest --version a échoué"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Lance un speedtest vers un serveur spécifique.
pub async fn run_speedtest(server_id: u32, sponsor_hint: &str) -> Result<SpeedtestResult> {
    let fut = async {
        let output = Command::new("speedtest")
            .args([
                "--server-id",
                &server_id.to_string(),
                "--format=json",
                "--accept-license",
                "--accept-gdpr",
                "--progress=no",
            ])
            .output()
            .await
            .context("exécution speedtest CLI")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "speedtest a échoué pour le serveur {} : {}",
                server_id,
                stderr
            ));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let parsed: OoklaOutput =
            serde_json::from_str(&stdout).context("parse JSON speedtest")?;

        Ok::<SpeedtestResult, anyhow::Error>(SpeedtestResult {
            timestamp: Utc::now(),
            server_id: parsed.server.id,
            server_name: parsed.server.name,
            sponsor: sponsor_hint.to_string(),
            asn: None,
            as_name: None,
            download_mbps: bytes_per_sec_to_mbps(parsed.download.bandwidth),
            upload_mbps: bytes_per_sec_to_mbps(parsed.upload.bandwidth),
            ping_ms: parsed.ping.latency,
            jitter_ms: parsed.ping.jitter,
            method: None,
            endpoint_label: None,
        })
    };

    timeout(SPEEDTEST_TIMEOUT, fut)
        .await
        .map_err(|_| anyhow!("speedtest timeout pour serveur {}", server_id))?
}

fn bytes_per_sec_to_mbps(bps: u64) -> f64 {
    (bps as f64 * 8.0) / 1_000_000.0
}

/// Délai recommandé entre deux speedtests pour ne pas fausser les mesures.
pub const COOLDOWN_BETWEEN_TESTS: Duration = Duration::from_secs(20);
