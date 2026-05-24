//! Module Looking Glass — diagnostic du chemin réseau retour.
//!
//! Identifie les AS du chemin aller (via MTR), puis interroge leurs serveurs
//! Looking Glass publics pour obtenir un traceroute depuis ces AS vers
//! l'IP publique de l'utilisateur — révélant le chemin retour.

pub mod analyzer;
pub mod db;
pub mod engine;
pub mod globalping;
pub mod myip;
pub mod query;

pub use engine::{collect_retour_json, run_lg, run_retour};
pub use myip::get_public_ip;

use anyhow::{Context, Result};
use colored::Colorize;
use std::net::IpAddr;

/// Résout un hostname en IP (réutilisé par engine sans dépendance sur main).
///
/// Préfère systématiquement IPv4 — le MTR ne supporte pas encore ICMPv6.
pub async fn resolve_target(target: &str) -> Result<IpAddr> {
    if let Ok(ip) = target.parse::<IpAddr>() {
        return Ok(ip);
    }
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((target, 0))
        .await
        .context("résolution DNS")?
        .map(|sa| sa.ip())
        .collect();
    if addrs.is_empty() {
        anyhow::bail!("résolution DNS : aucune IP trouvée pour '{}'", target);
    }
    // Préférer IPv4 — le MTR ne supporte pas encore ICMPv6
    if let Some(&v4) = addrs.iter().find(|ip| ip.is_ipv4()) {
        return Ok(v4);
    }
    eprintln!(
        "  {} '{}' ne résout qu'en IPv6 — le MTR IPv6 n'est pas encore implémenté.",
        "⚠".yellow(), target
    );
    Ok(addrs[0])
}
