//! Structures de données partagées dans tout le projet.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AsInfo {
    pub asn: u32,
    pub name: String,
    pub country: Option<String>,
    pub prefix: Option<String>,
}

impl AsInfo {
    pub fn display(&self) -> String {
        format!("AS{} ({})", self.asn, self.name)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Hop {
    pub ttl: u8,
    pub ips_seen: Vec<IpAddr>,
    pub primary_ip: Option<IpAddr>,
    pub hostname: Option<String>,
    pub as_info: Option<AsInfo>,
    pub rtt_samples: Vec<Duration>,
    pub sent: u32,
    pub received: u32,
    pub suspected_icmp_ratelimit: bool,
}

impl Hop {
    pub fn new(ttl: u8) -> Self {
        Self {
            ttl,
            ips_seen: Vec::new(),
            primary_ip: None,
            hostname: None,
            as_info: None,
            rtt_samples: Vec::new(),
            sent: 0,
            received: 0,
            suspected_icmp_ratelimit: false,
        }
    }

    pub fn loss_pct(&self) -> f64 {
        if self.sent == 0 { return 0.0; }
        (1.0 - self.received as f64 / self.sent as f64) * 100.0
    }

    pub fn avg_rtt_ms(&self) -> Option<f64> {
        if self.rtt_samples.is_empty() { return None; }
        let total: f64 = self.rtt_samples.iter().map(|d| d.as_secs_f64() * 1000.0).sum();
        Some(total / self.rtt_samples.len() as f64)
    }

    pub fn min_rtt_ms(&self) -> Option<f64> {
        self.rtt_samples.iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
    }

    pub fn max_rtt_ms(&self) -> Option<f64> {
        self.rtt_samples.iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    pub fn jitter_ms(&self) -> Option<f64> {
        if self.rtt_samples.len() < 2 { return None; }
        let mean = self.avg_rtt_ms()?;
        let variance: f64 = self.rtt_samples.iter()
            .map(|d| { let ms = d.as_secs_f64() * 1000.0; (ms - mean).powi(2) })
            .sum::<f64>() / (self.rtt_samples.len() - 1) as f64;
        Some(variance.sqrt())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SpeedtestResult {
    pub timestamp: DateTime<Utc>,
    pub server_id: u32,
    pub server_name: String,
    pub sponsor: String,
    pub asn: Option<u32>,
    pub as_name: Option<String>,
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub ping_ms: f64,
    pub jitter_ms: Option<f64>,
    /// Méthode de mesure utilisée (direct, tier1-db, iperf3, http, géo, proxy).
    #[serde(default)]
    pub method: Option<String>,
    /// Endpoint ou serveur utilisé pour la mesure.
    #[serde(default)]
    pub endpoint_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticReport {
    pub timestamp: DateTime<Utc>,
    pub target: String,
    pub target_ip: IpAddr,
    pub target_as: Option<AsInfo>,
    pub hops: Vec<Hop>,
    pub speedtests: Vec<SpeedtestResult>,
    pub findings: Vec<Finding>,
    /// Verdict global généré par l'analyzer.
    pub verdict: Verdict,
}

/// Une observation diagnostique avec action suggérée.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub category: FindingCategory,
    /// Description courte du problème.
    pub description: String,
    /// Preuves chiffrées.
    pub evidence: String,
    /// Action suggérée pour résoudre ou confirmer.
    pub action: Option<String>,
}

/// Verdict global du diagnostic.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub status: VerdictStatus,
    /// Phrase de conclusion en langage naturel.
    pub summary: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum VerdictStatus {
    /// Aucun problème détecté.
    Healthy,
    /// Anomalies mineures, surveillance recommandée.
    Degraded,
    /// Problème sérieux confirmé.
    Faulty,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum FindingCategory {
    /// Perte de paquets réelle.
    PacketLoss,
    /// Latence élevée ou bond de latence.
    HighLatency,
    /// Jitter élevé (congestion).
    Jitter,
    /// Bufferbloat (files d'attente pleines).
    Bufferbloat,
    /// Chemin réseau sous-optimal (détour, trop de hops).
    RoutingAnomaly,
    /// Problème d'interconnexion entre deux AS (peering).
    PeeringCongestion,
    /// Problème local (réseau domestique, FAI entry point).
    LocalIssue,
    /// ICMP rate-limiting (informatif, pas un vrai problème).
    IcmpRateLimit,
    /// Latence physique normale (transatlantique, etc.).
    PhysicalLatency,
}
