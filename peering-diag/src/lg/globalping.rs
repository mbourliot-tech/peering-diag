//! Intégration Globalping pour les traceroutes retour automatiques.
//!
//! Workflow :
//! 1. POST /v1/measurements → crée la mesure, retourne l'ID.
//! 2. GET /v1/measurements/{id} → poll toutes les 2s jusqu'à "finished".
//! 3. Parse les hops JSON → agrège sur N rounds → MtrHop.
//!
//! Gratuit, sans clé API, ~500 mesures/heure.

use crate::lg::query::TraceHop;
use crate::types::AsInfo;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

const API: &str = "https://api.globalping.io/v1";
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(60);

// ─── Requête de création ──────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct CreateRequest {
    #[serde(rename = "type")]
    kind: &'static str,
    target: String,
    locations: Vec<Location>,
    limit: u32,
}

#[derive(Serialize, Clone)]
struct Location {
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    asn: Option<u32>,
}

#[derive(Deserialize)]
struct CreateResponse {
    id: String,
}

// ─── Résultat brut (désérialisation JSON) ────────────────────────────────────

#[derive(Deserialize)]
struct MeasurementResult {
    status: String,
    results: Vec<ProbeResult>,
}

#[derive(Deserialize)]
struct ProbeResult {
    probe: ProbeInfo,
    result: TraceResult,
}

#[derive(Deserialize, Clone)]
pub struct ProbeInfo {
    pub country: String,
    pub city: String,
    pub network: String,
    pub asn: u32,
}

#[derive(Deserialize)]
struct TraceResult {
    #[allow(dead_code)]
    status: Option<String>,
    #[serde(default)]
    hops: Vec<RawHop>,
}

#[derive(Deserialize)]
struct RawHop {
    #[serde(rename = "resolvedHostname")]
    resolved_hostname: Option<String>,
    #[serde(rename = "resolvedAddress")]
    resolved_address: Option<String>,
    #[serde(default)]
    timings: Vec<Timing>,
}

#[derive(Deserialize)]
struct Timing {
    rtt: Option<f64>,
}

// ─── Types publics ────────────────────────────────────────────────────────────

/// Traceroute simple (1 round, RTTs filtrés).
pub struct GlobalpingTrace {
    pub probe: ProbeInfo,
    pub hops: Vec<TraceHop>,
}

/// Un hop agrégé sur N rounds de traceroute (style MTR).
/// `as_info` est initialement None — renseigné par engine.rs via AsnResolver.
pub struct MtrHop {
    pub ttl: u8,
    /// Hostname résolu (resolvedHostname).
    pub host: Option<String>,
    /// Adresse IP (resolvedAddress), parsée comme IpAddr.
    pub ip: Option<IpAddr>,
    /// Info ASN remplie par engine.rs après la résolution.
    pub as_info: Option<AsInfo>,
    pub snt: u32,
    pub loss_pct: f64,
    pub last_ms: Option<f64>,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub stdev_ms: f64,
}

/// Résultat MTR-style retourné par `traceroute_mtr`.
pub struct GlobalpingMtrTrace {
    pub probe: ProbeInfo,
    pub hops: Vec<MtrHop>,
}

// Tuple interne : (hostname, ip, timings)
type RawHopTuple = (Option<String>, Option<IpAddr>, Vec<Option<f64>>);

// ─── Client ───────────────────────────────────────────────────────────────────

pub struct GlobalpingClient {
    http: reqwest::Client,
}

impl GlobalpingClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent("peering-diag/0.1 (https://github.com/mbourliot/peering-diag)")
                .build()
                .expect("build reqwest client"),
        }
    }

    // ── Méthode principale : N rounds agrégés style MTR ──────────────────────

    /// Lance N rounds de traceroute depuis la meilleure localisation disponible
    /// et agrège les timings par hop pour produire des stats style MTR.
    /// L'`as_info` des hops est laissée à `None` — à remplir par l'appelant.
    pub async fn traceroute_mtr(
        &self,
        target_ip: IpAddr,
        dest_asn: Option<u32>,
        city_hint: Option<&str>,
        country_code: &str,
        rounds: u32,
    ) -> Result<GlobalpingMtrTrace> {
        let mut per_ttl: BTreeMap<u8, (Option<String>, Option<IpAddr>, Vec<Option<f64>>)> =
            BTreeMap::new();
        let mut probe_info: Option<ProbeInfo> = None;
        let mut best_location: Option<Location> = None;
        let mut rounds_done = 0u32;

        // Candidats de localisation : ASN → ville → pays
        let candidates: Vec<Location> = {
            let mut v = Vec::new();
            if let Some(asn) = dest_asn {
                v.push(Location { asn: Some(asn), city: None, country: None });
            }
            if let Some(city) = city_hint {
                v.push(Location { city: Some(city.to_string()), asn: None, country: None });
            }
            v.push(Location { country: Some(country_code.to_uppercase()), city: None, asn: None });
            v
        };

        // Découverte de la localisation + round 1
        for loc in &candidates {
            if let Ok(id) = self.create_with_location(target_ip, loc.clone(), 1).await {
                if let Ok(raw) = self.wait_for_raw(&id).await {
                    if !raw.is_empty() {
                        best_location = Some(loc.clone());
                        for (pi, hops) in raw {
                            if probe_info.is_none() {
                                probe_info = Some(pi);
                            }
                            merge_raw_hops(&mut per_ttl, hops);
                        }
                        rounds_done = 1;
                        break;
                    }
                }
            }
        }

        let location = best_location
            .ok_or_else(|| anyhow!("aucune sonde Globalping disponible pour cette destination"))?;

        // Rounds supplémentaires
        for _ in rounds_done..rounds {
            if let Ok(id) = self.create_with_location(target_ip, location.clone(), 1).await {
                if let Ok(raw) = self.wait_for_raw(&id).await {
                    for (_, hops) in raw {
                        merge_raw_hops(&mut per_ttl, hops);
                    }
                }
            }
        }

        let probe = probe_info
            .ok_or_else(|| anyhow!("aucun résultat Globalping obtenu"))?;
        let hops = per_ttl
            .into_iter()
            .map(|(ttl, (host, ip, samples))| compute_mtr_hop(ttl, host, ip, &samples))
            .collect();

        Ok(GlobalpingMtrTrace { probe, hops })
    }

    // ── Méthodes internes ────────────────────────────────────────────────────

    async fn create_with_location(
        &self,
        target_ip: IpAddr,
        location: Location,
        n_probes: u32,
    ) -> Result<String> {
        let body = CreateRequest {
            kind: "traceroute",
            target: target_ip.to_string(),
            locations: vec![location],
            limit: n_probes,
        };

        let resp: CreateResponse = self
            .http
            .post(format!("{}/measurements", API))
            .json(&body)
            .send()
            .await
            .context("création mesure Globalping")?
            .error_for_status()
            .context("HTTP Globalping création")?
            .json()
            .await
            .context("parse réponse création Globalping")?;

        Ok(resp.id)
    }

    /// Poll jusqu'à "finished" et retourne les résultats simples (RTTs filtrés).
    async fn wait_for_results(&self, id: &str) -> Result<Vec<GlobalpingTrace>> {
        Ok(self
            .wait_for_raw(id)
            .await?
            .into_iter()
            .map(|(probe, hops)| GlobalpingTrace {
                probe,
                hops: hops
                    .into_iter()
                    .enumerate()
                    .map(|(i, (host, ip, timings))| TraceHop {
                        ttl: (i + 1) as u8,
                        host: host.or_else(|| ip.map(|a| a.to_string())),
                        rtts_ms: timings.into_iter().flatten().collect(),
                    })
                    .collect(),
            })
            .collect())
    }

    /// Poll jusqu'à "finished" et retourne les données brutes avec hostname,
    /// IP et timings (None = paquet perdu).
    async fn wait_for_raw(&self, id: &str) -> Result<Vec<(ProbeInfo, Vec<RawHopTuple>)>> {
        let url = format!("{}/measurements/{}", API, id);
        let deadline = tokio::time::Instant::now() + TIMEOUT;

        loop {
            tokio::time::sleep(POLL_INTERVAL).await;

            if tokio::time::Instant::now() > deadline {
                return Err(anyhow!("timeout Globalping après {}s", TIMEOUT.as_secs()));
            }

            let resp = self
                .http
                .get(&url)
                .send()
                .await
                .context("poll Globalping")?
                .error_for_status()
                .context("HTTP Globalping poll")?;

            let text = resp.text().await.context("lecture réponse Globalping")?;
            let m: MeasurementResult = serde_json::from_str(&text).map_err(|e| {
                anyhow!(
                    "parse résultat Globalping : {} — début : {:?}",
                    e,
                    &text.chars().take(200).collect::<String>()
                )
            })?;

            if m.status == "finished" {
                return Ok(m
                    .results
                    .into_iter()
                    .map(|pr| {
                        let hops = pr
                            .result
                            .hops
                            .into_iter()
                            .map(|h| {
                                let host = h.resolved_hostname;
                                let ip = h.resolved_address
                                    .as_deref()
                                    .and_then(|s| s.parse::<IpAddr>().ok());
                                let timings: Vec<Option<f64>> =
                                    h.timings.into_iter().map(|t| t.rtt).collect();
                                (host, ip, timings)
                            })
                            .collect();
                        (pr.probe, hops)
                    })
                    .collect());
            }
        }
    }

    #[allow(dead_code)]
    pub async fn traceroute_smart(
        &self,
        target_ip: IpAddr,
        dest_asn: Option<u32>,
        city_hint: Option<&str>,
        country_code: &str,
        n_probes: u32,
    ) -> Result<Vec<GlobalpingTrace>> {
        if let Some(asn) = dest_asn {
            let loc = Location { asn: Some(asn), city: None, country: None };
            if let Ok(id) = self.create_with_location(target_ip, loc, n_probes).await {
                if let Ok(traces) = self.wait_for_results(&id).await {
                    if !traces.is_empty() {
                        return Ok(traces);
                    }
                }
            }
        }
        if let Some(city) = city_hint {
            let loc = Location { city: Some(city.to_string()), asn: None, country: None };
            if let Ok(id) = self.create_with_location(target_ip, loc, n_probes).await {
                if let Ok(traces) = self.wait_for_results(&id).await {
                    if !traces.is_empty() {
                        return Ok(traces);
                    }
                }
            }
        }
        let loc = Location { country: Some(country_code.to_uppercase()), city: None, asn: None };
        let id = self.create_with_location(target_ip, loc, n_probes).await?;
        self.wait_for_results(&id).await
    }

    #[allow(dead_code)]
    pub async fn traceroute(
        &self,
        target_ip: IpAddr,
        country_code: &str,
        n_probes: u32,
    ) -> Result<Vec<GlobalpingTrace>> {
        let loc = Location { country: Some(country_code.to_uppercase()), city: None, asn: None };
        let id = self.create_with_location(target_ip, loc, n_probes).await?;
        self.wait_for_results(&id).await
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn merge_raw_hops(
    per_ttl: &mut BTreeMap<u8, (Option<String>, Option<IpAddr>, Vec<Option<f64>>)>,
    hops: Vec<RawHopTuple>,
) {
    for (idx, (host, ip, timings)) in hops.into_iter().enumerate() {
        let ttl = (idx + 1) as u8;
        let entry = per_ttl.entry(ttl).or_default();
        if entry.0.is_none() && host.is_some() {
            entry.0 = host;
        }
        if entry.1.is_none() && ip.is_some() {
            entry.1 = ip;
        }
        entry.2.extend(timings);
    }
}

fn compute_mtr_hop(
    ttl: u8,
    host: Option<String>,
    ip: Option<IpAddr>,
    samples: &[Option<f64>],
) -> MtrHop {
    let snt = samples.len() as u32;
    let valid: Vec<f64> = samples.iter().filter_map(|s| *s).collect();
    let lost = snt.saturating_sub(valid.len() as u32);
    let loss_pct = if snt > 0 {
        (lost as f64 / snt as f64) * 100.0
    } else {
        0.0
    };

    if valid.is_empty() {
        return MtrHop {
            ttl,
            host,
            ip,
            as_info: None,
            snt,
            loss_pct,
            last_ms: None,
            avg_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
            stdev_ms: 0.0,
        };
    }

    let avg = valid.iter().sum::<f64>() / valid.len() as f64;
    let min = valid.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = valid.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let variance =
        valid.iter().map(|&v| (v - avg).powi(2)).sum::<f64>() / valid.len() as f64;
    let stdev = variance.sqrt();
    let last = *valid.last().unwrap();

    MtrHop {
        ttl,
        host,
        ip,
        as_info: None,
        snt,
        loss_pct,
        last_ms: Some(last),
        avg_ms: avg,
        min_ms: min,
        max_ms: max,
        stdev_ms: stdev,
    }
}
