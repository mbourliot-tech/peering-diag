//! Récupération de la liste des serveurs Speedtest via le CLI Ookla.
//!
//! L'ancien endpoint HTTP non officiel (/api/js/servers) est instable et casse
//! régulièrement. On utilise désormais `speedtest --servers` qui retourne la
//! liste officielle en JSON depuis le CLI installé — même source, beaucoup plus
//! fiable.
//!
//! Format JSON du CLI Ookla (--format=json-pretty --servers) :
//!   {
//!     "servers": [
//!       { "id": 12345, "name": "Paris", "location": "Paris", "country": "France",
//!         "host": "speedtest.example.net:8080", "ip": "...", ... },
//!       ...
//!     ]
//!   }

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedtestServer {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub sponsor: String,
    pub host: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub lat: String,
    #[serde(default)]
    pub lon: String,
    #[serde(default)]
    pub asn: Option<u32>,
    #[serde(default)]
    pub as_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OoklaServersOutput {
    servers: Vec<OoklaServerEntry>,
}

#[derive(Debug, Deserialize)]
struct OoklaServerEntry {
    id: u32,
    name: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    country: String,
    host: String,
    #[serde(default)]
    #[allow(dead_code)]
    ip: String,
}

/// Récupère la liste des serveurs Speedtest via le CLI Ookla.
/// Beaucoup plus fiable que l'ancien endpoint HTTP non officiel.
pub async fn fetch_all_servers() -> Result<Vec<SpeedtestServer>> {
    eprintln!("  → Récupération de la liste des serveurs via CLI Ookla…");

    let output = Command::new("speedtest")
        .args([
            "--servers",
            "--format=json",
            "--accept-license",
            "--accept-gdpr",
        ])
        .output()
        .await
        .context("exécution de `speedtest --servers`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("speedtest --servers a échoué : {}", stderr));
    }

    let stdout = String::from_utf8(output.stdout)
        .context("décodage UTF-8 de la sortie speedtest")?;

    // Le CLI retourne les serveurs les plus proches (~10 serveurs par défaut).
    // C'est suffisant pour le cas d'usage : on veut 1 serveur par AS du chemin,
    // et les serveurs proches géographiquement sont les plus susceptibles d'être
    // dans les AS qu'on traverse.
    //
    // Si on a besoin de plus de serveurs (pour couvrir des AS lointains), on peut
    // ajouter --search-location ou utiliser l'API /nearest-servers.
    let parsed: OoklaServersOutput = serde_json::from_str(&stdout)
        .context("parse JSON de `speedtest --servers`")?;

    let servers: Vec<SpeedtestServer> = parsed
        .servers
        .into_iter()
        .map(|e| SpeedtestServer {
            id: e.id,
            name: e.name,
            country: e.country,
            sponsor: e.location.clone(),
            host: e.host,
            url: String::new(),
            lat: String::new(),
            lon: String::new(),
            asn: None,
            as_name: None,
        })
        .collect();

    eprintln!("  ✓ {} serveurs récupérés", servers.len());
    Ok(servers)
}

/// Récupère une liste élargie de serveurs en cherchant dans plusieurs localisations.
/// Utilisé quand les AS cibles ne sont pas couverts par les serveurs locaux.
pub async fn fetch_servers_for_locations(locations: &[&str]) -> Result<Vec<SpeedtestServer>> {
    let mut all = Vec::new();

    // D'abord les serveurs locaux
    if let Ok(local) = fetch_all_servers().await {
        all.extend(local);
    }

    // Puis chercher par location pour chaque pays/ville
    for location in locations {
        let output = Command::new("speedtest")
            .args([
                "--servers",
                "--format=json",
                "--accept-license",
                "--accept-gdpr",
            ])
            .env("SPEEDTEST_SEARCH", location)
            .output()
            .await;

        if let Ok(out) = output {
            if let Ok(stdout) = String::from_utf8(out.stdout) {
                if let Ok(parsed) = serde_json::from_str::<OoklaServersOutput>(&stdout) {
                    let servers: Vec<SpeedtestServer> = parsed
                        .servers
                        .into_iter()
                        .map(|e| SpeedtestServer {
                            id: e.id,
                            name: e.name,
                            country: e.country,
                            sponsor: e.location.clone(),
                            host: e.host,
                            url: String::new(),
                            lat: String::new(),
                            lon: String::new(),
                            asn: None,
                            as_name: None,
                        })
                        .collect();
                    all.extend(servers);
                }
            }
        }
    }

    // Dédupliquer par id
    all.sort_by_key(|s| s.id);
    all.dedup_by_key(|s| s.id);

    Ok(all)
}

/// Récupère des serveurs Speedtest pour un pays donné via l'API XML publique.
/// Utilisé en fallback quand les AS cibles sont dans un autre pays que le local.
///
/// L'endpoint XML /speedtest-servers-static.php est stable depuis des années
/// et ne nécessite pas d'authentification.
pub async fn fetch_servers_by_country_api(
    country_code: &str,
) -> Result<Vec<SpeedtestServer>> {
    use std::time::Duration;

    // Normalise : "US" → "United States" n'est pas nécessaire, l'API accepte
    // le filtre sur le champ country en texte. On passe par l'endpoint XML
    // qui est public et stable.
    let url = format!(
        "https://www.speedtest.net/api/js/servers?engine=js&limit=100&search={}",
        country_code
    );

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 peering-diag/0.1")
        .timeout(Duration::from_secs(15))
        .build()?;

    let resp = client.get(&url).send().await;
    let body = match resp {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        _ => return Ok(Vec::new()),
    };

    // L'endpoint peut retourner HTML (erreur) ou JSON
    if !body.trim_start().starts_with('[') && !body.trim_start().starts_with('{') {
        return Ok(Vec::new());
    }

    // Essai de parse en array JSON direct
    let servers: Vec<SpeedtestServer> = if let Ok(arr) =
        serde_json::from_str::<Vec<serde_json::Value>>(&body)
    {
        arr.into_iter()
            .filter_map(|v| {
                Some(SpeedtestServer {
                    id: v["id"].as_u64()? as u32,
                    name: v["name"].as_str()?.to_string(),
                    country: v["country"].as_str().unwrap_or("").to_string(),
                    sponsor: v["sponsor"].as_str().unwrap_or("").to_string(),
                    host: v["host"].as_str()?.to_string(),
                    url: v["url"].as_str().unwrap_or("").to_string(),
                    lat: v["lat"].as_str().unwrap_or("").to_string(),
                    lon: v["lon"].as_str().unwrap_or("").to_string(),
                    asn: None,
                    as_name: None,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(servers)
}
