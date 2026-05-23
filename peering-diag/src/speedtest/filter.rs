//! Filtre les serveurs Speedtest et les groupe par AS.
//!
//! Stratégie améliorée : si aucun serveur n'est trouvé pour un AS du chemin,
//! on cherche le serveur le plus proche géographiquement de cet AS.

use crate::asn::AsnResolver;
use crate::speedtest::servers::SpeedtestServer;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use hickory_resolver::TokioAsyncResolver;
use std::collections::HashMap;
use std::sync::Arc;

const DNS_CONCURRENCY: usize = 20;

/// Résout l'AS de chaque serveur et groupe par ASN.
/// Pas de filtre pays : on travaille sur la liste déjà restreinte du CLI.
pub async fn group_servers_by_asn(
    servers: Vec<SpeedtestServer>,
    asn_resolver: Arc<AsnResolver>,
    _country_filter: Option<&str>, // conservé pour compat mais ignoré
) -> Result<HashMap<u32, Vec<SpeedtestServer>>> {
    let dns = match TokioAsyncResolver::tokio_from_system_conf() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Pas de DNS resolver : {}", e);
            return Ok(HashMap::new());
        }
    };

    eprintln!(
        "  → Résolution ASN de {} serveurs Speedtest…",
        servers.len()
    );

    let results: Vec<Option<SpeedtestServer>> = stream::iter(servers)
        .map(|server| {
            let dns = dns.clone();
            let asn_resolver = asn_resolver.clone();
            async move {
                let host = server
                    .host
                    .split(':')
                    .next()
                    .unwrap_or(&server.host)
                    .to_string();

                // Essai DNS resolver, fallback sur l'IP déjà dans la struct si dispo
                let ip = match dns.lookup_ip(host).await {
                    Ok(ips) => ips.iter().next()?,
                    Err(_) => return None,
                };

                let as_info = asn_resolver.lookup(ip).await.ok().flatten()?;
                let mut s = server;
                s.asn = Some(as_info.asn);
                s.as_name = Some(as_info.name);
                Some(s)
            }
        })
        .buffer_unordered(DNS_CONCURRENCY)
        .collect()
        .await;

    let mut groups: HashMap<u32, Vec<SpeedtestServer>> = HashMap::new();
    for server in results.into_iter().flatten() {
        if let Some(asn) = server.asn {
            groups.entry(asn).or_default().push(server);
        }
    }

    eprintln!("  ✓ {} AS distincts couverts", groups.len());

    // Affiche le mapping pour debug
    let mut sorted: Vec<_> = groups.iter().collect();
    sorted.sort_by_key(|(asn, _)| *asn);
    for (asn, servers) in &sorted {
        eprintln!(
            "    AS{} → {} serveur(s) : {}",
            asn,
            servers.len(),
            servers
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(groups)
}
