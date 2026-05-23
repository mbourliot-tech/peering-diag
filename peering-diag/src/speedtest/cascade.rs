//! Cascade de méthodes de mesure par ordre de fiabilité.
//!
//! Pour chaque AS : Speedtest direct → Tier-1 DB → iperf3 → HTTP → géo → skip

use crate::asn::AsnResolver;
use crate::speedtest::{
    filter::group_servers_by_asn,
    http_measure::measure_http_download,
    iperf::run_iperf3,
    runner::run_speedtest,
    servers::SpeedtestServer,
    tier1_db::{get_endpoints_for_asn, MeasureMethod},
};
use crate::types::{AsInfo, SpeedtestResult};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MeasureResult {
    pub inner: SpeedtestResult,
    pub method: MeasureMethod,
    pub endpoint_label: String,
}

pub async fn measure_for_asn(
    asn: u32,
    as_info: &Option<AsInfo>,
    local_by_asn: &HashMap<u32, Vec<SpeedtestServer>>,
    geo_by_asn: &HashMap<u32, Vec<SpeedtestServer>>,
    iperf3_available: bool,
) -> Option<MeasureResult> {
    let as_label = as_info.as_ref()
        .map(|a| a.display())
        .unwrap_or_else(|| format!("AS{}", asn));
    let as_name = as_info.as_ref().map(|a| a.name.clone());

    // ── 1. Speedtest direct dans l'AS ───────────────────────────────────
    if let Some(server) = local_by_asn.get(&asn).and_then(|s| s.first()) {
        eprintln!("    [direct] {} via {} ({})", as_label, server.name, server.sponsor);
        match run_speedtest(server.id, &server.sponsor).await {
            Ok(mut r) => {
                r.asn = Some(asn);
                r.as_name = as_name.clone();
                return Some(MeasureResult {
                    endpoint_label: format!("{} ({})", server.name, server.sponsor),
                    method: MeasureMethod::SpeedtestDirect,
                    inner: r,
                });
            }
            Err(e) => eprintln!("      ✖ speedtest échoué : {}", e),
        }
    }

    // ── 2. Base Tier-1 statique ──────────────────────────────────────────
    for endpoint in get_endpoints_for_asn(asn) {
        match endpoint.method {
            MeasureMethod::SpeedtestTier1Db => {
                if let Ok(server_id) = endpoint.endpoint.parse::<u32>() {
                    eprintln!("    [tier1-db] {} via {} ({})",
                        as_label, endpoint.description, endpoint.location);
                    match run_speedtest(server_id, endpoint.location).await {
                        Ok(mut r) => {
                            r.asn = Some(asn);
                            r.as_name = as_name.clone();
                            return Some(MeasureResult {
                                endpoint_label: format!("{} — {}", endpoint.description, endpoint.location),
                                method: MeasureMethod::SpeedtestTier1Db,
                                inner: r,
                            });
                        }
                        Err(e) => eprintln!("      ✖ tier1-db échoué : {}", e),
                    }
                }
            }
            MeasureMethod::Iperf3 if iperf3_available => {
                eprintln!("    [iperf3] {} via {} ({})",
                    as_label, endpoint.endpoint, endpoint.location);
                match run_iperf3(endpoint.endpoint, endpoint.location).await {
                    Ok(ir) => {
                        return Some(MeasureResult {
                            endpoint_label: format!("iperf3 {} — {}", endpoint.endpoint, endpoint.location),
                            method: MeasureMethod::Iperf3,
                            inner: make_result(asn, as_name.clone(), endpoint.description, endpoint.location,
                                ir.download_mbps, ir.upload_mbps, 0.0),
                        });
                    }
                    Err(e) => eprintln!("      ✖ iperf3 échoué : {}", e),
                }
            }
            MeasureMethod::HttpDownload => {
                eprintln!("    [http] {} via {} ({})",
                    as_label, endpoint.description, endpoint.location);
                match measure_http_download(endpoint.endpoint, endpoint.description).await {
                    Ok(hr) => {
                        return Some(MeasureResult {
                            endpoint_label: format!("HTTP {} — {}", endpoint.description, endpoint.location),
                            method: MeasureMethod::HttpDownload,
                            inner: make_result(asn, as_name.clone(), endpoint.description, endpoint.location,
                                hr.download_mbps, 0.0, 0.0),
                        });
                    }
                    Err(e) => eprintln!("      ✖ HTTP échoué : {}", e),
                }
            }
            _ => {} // iperf3 non dispo ou méthode non applicable
        }
    }

    // ── 3. Proxy local (aucun serveur dans l'AS ni en base Tier-1) ──────
    // On mesure depuis le premier serveur Speedtest local disponible.
    // Ce n'est PAS une mesure du chemin vers cet AS — c'est le débit local.
    // Méthode étiquetée "proxy" : le delta est supprimé dans display/analyzer.
    if let Some(server) = geo_by_asn.get(&asn).and_then(|s| s.first()) {
        eprintln!("    [proxy] {} → {} ({}) — AS non couvert, mesure locale",
            as_label, server.name, server.sponsor);
        match run_speedtest(server.id, &server.sponsor).await {
            Ok(mut r) => {
                r.asn = Some(asn);
                r.as_name = as_name;
                return Some(MeasureResult {
                    endpoint_label: format!("{} ({}) — proxy local", server.name, server.sponsor),
                    method: MeasureMethod::Proxy,
                    inner: r,
                });
            }
            Err(e) => eprintln!("      ✖ proxy échoué : {}", e),
        }
    }

    // ── 4. Aucune méthode disponible ─────────────────────────────────────
    eprintln!("    ✖ Aucune méthode disponible pour {}", as_label);
    None
}

fn make_result(
    asn: u32,
    as_name: Option<String>,
    server_name: &str,
    location: &str,
    download_mbps: f64,
    upload_mbps: f64,
    ping_ms: f64,
) -> SpeedtestResult {
    SpeedtestResult {
        timestamp: Utc::now(),
        server_id: 0,
        server_name: server_name.to_string(),
        sponsor: location.to_string(),
        asn: Some(asn),
        as_name,
        download_mbps,
        upload_mbps,
        ping_ms,
        jitter_ms: None,
        method: None,
        endpoint_label: None,
    }
}

/// Construit la map AS → serveur proxy pour les AS non couverts (aucun serveur
/// Speedtest dans l'AS ni en base Tier-1). Utilise le premier serveur local
/// disponible comme proxy — la mesure reflète le débit local, pas le chemin
/// vers l'AS cible. Réutilise la map déjà résolue — pas de double résolution.
pub async fn build_geo_servers(
    _geo_hint: Option<&str>,
    asns_needed: &[u32],
    local_by_asn: &HashMap<u32, Vec<SpeedtestServer>>,
) -> HashMap<u32, Vec<SpeedtestServer>> {
    if asns_needed.is_empty() {
        return HashMap::new();
    }
    // Prendre le premier serveur dispo parmi tous les AS locaux
    let proxy = local_by_asn.values().flat_map(|v| v.iter()).next().cloned();
    let mut result = HashMap::new();
    if let Some(server) = proxy {
        for asn in asns_needed {
            result.entry(*asn).or_insert_with(|| vec![server.clone()]);
        }
    }
    result
}

/// Version publique pour construire les serveurs géo depuis les serveurs bruts.
/// Utilisée dans main.rs quand on n'a pas encore la map by_asn.
pub async fn build_geo_servers_from_raw(
    _geo_hint: Option<&str>,
    asns_needed: &[u32],
    raw_servers: &[SpeedtestServer],
    asn_resolver: Arc<AsnResolver>,
) -> HashMap<u32, Vec<SpeedtestServer>> {
    if asns_needed.is_empty() {
        return HashMap::new();
    }
    let local_by_asn = match group_servers_by_asn(raw_servers.to_vec(), asn_resolver, None).await {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };
    build_geo_servers(_geo_hint, asns_needed, &local_by_asn).await
}
