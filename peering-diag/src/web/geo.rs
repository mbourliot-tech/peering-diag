//! Géolocalisation d'adresses IP via ip-api.com avec cache SQLite.
//!
//! Gratuit, sans clé API, limite 45 req/min.
//! Les IPs privées/loopback sont ignorées.
//! Résultats mis en cache 30 jours dans la table `geo_cache`.

use crate::report::init_db;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GeoPoint {
    pub lat:     f64,
    pub lon:     f64,
    pub city:    Option<String>,
    pub country: Option<String>,
}

#[derive(Deserialize)]
struct IpApiResp {
    status:  String,
    lat:     Option<f64>,
    lon:     Option<f64>,
    city:    Option<String>,
    country: Option<String>,
}

// ─── Point d'entrée public ────────────────────────────────────────────────────

/// Géolocalise un lot d'IPs. Les IPs déjà en cache sont retournées directement ;
/// les autres sont interrogées via ip-api.com avec un délai de 1.5s entre requêtes.
pub async fn geolocate_batch(
    ips: Vec<String>,
    db_path: PathBuf,
) -> HashMap<String, GeoPoint> {
    let public: Vec<String> = ips.into_iter().filter(|ip| !is_private(ip)).collect();
    if public.is_empty() {
        return HashMap::new();
    }

    // ── Lecture du cache ─────────────────────────────────────────────────────
    let db_path2 = db_path.clone();
    let public2 = public.clone();
    let cached = tokio::task::spawn_blocking(move || -> HashMap<String, GeoPoint> {
        let Ok(conn) = init_db(&db_path2) else { return HashMap::new() };
        cache_load(&conn, &public2)
    })
    .await
    .unwrap_or_default();

    let to_fetch: Vec<String> = public
        .iter()
        .filter(|ip| !cached.contains_key(*ip))
        .cloned()
        .collect();

    if to_fetch.is_empty() {
        return cached;
    }

    // ── Requêtes HTTP ────────────────────────────────────────────────────────
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut fetched: HashMap<String, GeoPoint> = HashMap::new();
    for (i, ip) in to_fetch.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        let url = format!(
            "http://ip-api.com/json/{}?fields=status,lat,lon,city,country",
            ip
        );
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(data) = resp.json::<IpApiResp>().await {
                if data.status == "success" {
                    if let (Some(lat), Some(lon)) = (data.lat, data.lon) {
                        fetched.insert(
                            ip.clone(),
                            GeoPoint { lat, lon, city: data.city, country: data.country },
                        );
                    }
                }
            }
        }
    }

    // ── Écriture du cache ────────────────────────────────────────────────────
    if !fetched.is_empty() {
        let db_path3 = db_path.clone();
        let fetched2 = fetched.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(conn) = init_db(&db_path3) {
                for (ip, geo) in &fetched2 {
                    cache_save(&conn, ip, geo);
                }
            }
        })
        .await;
    }

    // ── Fusion ───────────────────────────────────────────────────────────────
    let mut result = cached;
    result.extend(fetched);
    result
}

// ─── Helpers cache ────────────────────────────────────────────────────────────

fn cache_load(conn: &Connection, ips: &[String]) -> HashMap<String, GeoPoint> {
    let mut out = HashMap::new();
    for ip in ips {
        if let Ok((lat, lon, city, country)) = conn.query_row(
            "SELECT lat, lon, city, country FROM geo_cache WHERE ip = ?1",
            params![ip],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)),
        ) {
            out.insert(ip.clone(), GeoPoint { lat, lon, city, country });
        }
    }
    out
}

fn cache_save(conn: &Connection, ip: &str, geo: &GeoPoint) {
    let _ = conn.execute(
        "INSERT OR REPLACE INTO geo_cache (ip, lat, lon, city, country, cached_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ip, geo.lat, geo.lon, geo.city, geo.country, Utc::now().to_rfc3339()],
    );
}

// ─── Détection IP privée ──────────────────────────────────────────────────────

fn is_private(ip: &str) -> bool {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_broadcast()
        }
        Ok(IpAddr::V6(v6)) => v6.is_loopback() || v6.is_unspecified(),
        Err(_) => true,
    }
}
