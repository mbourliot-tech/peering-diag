//! Lookup ASN avec deux backends : Team Cymru (whois) en primaire, ipinfo.io (HTTPS) en fallback.
//!
//! Pourquoi deux backends ?
//! - Cymru est très rapide en bulk (1 connexion pour N IP) mais le port 43 est
//!   parfois filtré par les firewalls.
//! - ipinfo.io fonctionne en HTTPS (443) donc passe partout, mais c'est 1 requête par IP.
//!
//! On essaie Cymru d'abord. Si la réponse parse mal ou si la connexion échoue,
//! on bascule sur ipinfo.io pour les IP qui n'ont pas été résolues.

use crate::types::AsInfo;
use anyhow::{Context, Result};
use moka::future::Cache;
use serde::Deserialize;
use std::net::IpAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const CYMRU_HOST: &str = "whois.cymru.com:43";
const CYMRU_TIMEOUT: Duration = Duration::from_secs(15);
const IPINFO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct AsnResolver {
    cache: Cache<IpAddr, Option<AsInfo>>,
    http_client: reqwest::Client,
}

impl AsnResolver {
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .user_agent("peering-diag/0.1")
            .timeout(IPINFO_TIMEOUT)
            .build()
            .expect("build reqwest client");

        Self {
            cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(86_400))
                .build(),
            http_client,
        }
    }

    pub async fn lookup(&self, ip: IpAddr) -> Result<Option<AsInfo>> {
        if is_private_or_special(&ip) {
            return Ok(None);
        }
        if let Some(cached) = self.cache.get(&ip).await {
            return Ok(cached);
        }

        // Tente Cymru en single-IP
        let result = match timeout(CYMRU_TIMEOUT, query_cymru(&[ip])).await {
            Ok(Ok(mut map)) => map.remove(&ip).flatten(),
            _ => None,
        };

        let result = if result.is_none() {
            self.query_ipinfo(ip).await.ok().flatten()
        } else {
            result
        };

        self.cache.insert(ip, result.clone()).await;
        Ok(result)
    }

    /// Lookup en bulk : Cymru d'abord (1 connexion), fallback ipinfo pour le reste.
    pub async fn lookup_bulk(
        &self,
        ips: &[IpAddr],
    ) -> Result<std::collections::HashMap<IpAddr, Option<AsInfo>>> {
        let mut result = std::collections::HashMap::new();
        let mut to_query = Vec::new();

        // Cache hits
        for ip in ips {
            if is_private_or_special(ip) {
                result.insert(*ip, None);
                continue;
            }
            if let Some(cached) = self.cache.get(ip).await {
                result.insert(*ip, cached);
            } else {
                to_query.push(*ip);
            }
        }

        if to_query.is_empty() {
            return Ok(result);
        }

        tracing::debug!("Cymru bulk lookup pour {} IP", to_query.len());

        // Cymru en bulk
        let mut cymru_resolved = 0;
        for chunk in to_query.chunks(100) {
            match timeout(CYMRU_TIMEOUT, query_cymru(chunk)).await {
                Ok(Ok(map)) => {
                    for (ip, info) in map {
                        if info.is_some() {
                            cymru_resolved += 1;
                        }
                        self.cache.insert(ip, info.clone()).await;
                        result.insert(ip, info);
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!("Cymru bulk error: {}", e);
                    for ip in chunk {
                        result.entry(*ip).or_insert(None);
                    }
                }
                Err(_) => {
                    tracing::warn!("Cymru bulk timeout sur {} IP", chunk.len());
                    for ip in chunk {
                        result.entry(*ip).or_insert(None);
                    }
                }
            }
        }
        tracing::debug!("Cymru a résolu {}/{} IP", cymru_resolved, to_query.len());

        // Fallback ipinfo pour les IP non résolues
        let unresolved: Vec<IpAddr> = to_query
            .iter()
            .filter(|ip| matches!(result.get(ip), Some(None) | None))
            .copied()
            .collect();

        if !unresolved.is_empty() {
            tracing::debug!("Fallback ipinfo.io pour {} IP non résolues", unresolved.len());
            use futures::stream::{self, StreamExt};
            let fallback: Vec<(IpAddr, Option<AsInfo>)> = stream::iter(unresolved)
                .map(|ip| {
                    let resolver = self.clone();
                    async move {
                        let info = resolver.query_ipinfo(ip).await.ok().flatten();
                        (ip, info)
                    }
                })
                .buffer_unordered(10)
                .collect()
                .await;

            for (ip, info) in fallback {
                if info.is_some() {
                    self.cache.insert(ip, info.clone()).await;
                    result.insert(ip, info);
                }
            }
        }

        Ok(result)
    }

    /// Query ipinfo.io en HTTPS (sans token, free tier limité mais suffisant).
    async fn query_ipinfo(&self, ip: IpAddr) -> Result<Option<AsInfo>> {
        let url = format!("https://ipinfo.io/{}/json", ip);
        let response = self.http_client.get(&url).send().await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let info: IpInfoResponse = response.json().await?;
        Ok(info.into_as_info())
    }
}

impl Default for AsnResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Format de réponse ipinfo.io :
///   { "ip": "...", "org": "AS24940 Hetzner Online GmbH", "country": "DE", ... }
#[derive(Debug, Deserialize)]
struct IpInfoResponse {
    #[serde(default)]
    org: Option<String>,
    #[serde(default)]
    country: Option<String>,
}

impl IpInfoResponse {
    fn into_as_info(self) -> Option<AsInfo> {
        let org = self.org?;
        // Format "AS24940 Hetzner Online GmbH"
        if let Some(rest) = org.strip_prefix("AS") {
            let mut parts = rest.splitn(2, ' ');
            let asn_str = parts.next()?;
            let name = parts.next().unwrap_or("").trim().to_string();
            let asn: u32 = asn_str.parse().ok()?;
            return Some(AsInfo {
                asn,
                name,
                country: self.country,
                prefix: None,
            });
        }
        None
    }
}

/// Query Cymru bulk. Une connexion TCP, plusieurs IP.
async fn query_cymru(
    ips: &[IpAddr],
) -> Result<std::collections::HashMap<IpAddr, Option<AsInfo>>> {
    let mut stream = TcpStream::connect(CYMRU_HOST)
        .await
        .context("connexion Cymru")?;

    let mut request = String::from("begin\nverbose\n");
    for ip in ips {
        request.push_str(&ip.to_string());
        request.push('\n');
    }
    request.push_str("end\n");

    stream.write_all(request.as_bytes()).await?;
    // Demi-fermeture côté write pour signaler la fin de requête.
    // Important : sans ça, Cymru attend des données supplémentaires et timeout.
    stream.shutdown().await.ok();

    // Lecture jusqu'à EOF
    let mut response = String::new();
    let mut buf = vec![0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        response.push_str(&String::from_utf8_lossy(&buf[..n]));
    }

    let mut result = std::collections::HashMap::new();
    for ip in ips {
        result.insert(*ip, None);
    }

    // Format verbose Cymru :
    //   AS      | IP       | BGP Prefix       | CC | Registry | Allocated  | AS Name
    //   1299    | 8.8.8.8  | 8.8.8.0/24       | US | arin     | 1992-12-01 | ARELION-AS, US
    //
    // La première ligne est un en-tête "Bulk mode; whois.cymru.com [...]" qu'on skip.
    for resp_line in response.lines() {
        let trimmed = resp_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip header lines (commencent par "Bulk", ou contiennent "AS      |")
        if trimmed.starts_with("Bulk") || trimmed.starts_with("Error:") {
            continue;
        }
        // Skip ligne d'en-tête type "AS      | IP       | BGP Prefix..."
        if trimmed.starts_with("AS ") && trimmed.contains("BGP Prefix") {
            continue;
        }

        let parts: Vec<&str> = trimmed.split('|').map(|s| s.trim()).collect();
        if parts.len() < 7 {
            continue;
        }
        // parts[0] doit être un nombre (ASN). Si c'est "NA" ou autre, on skip.
        let asn: u32 = match parts[0].parse() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let ip: IpAddr = match parts[1].parse() {
            Ok(i) => i,
            Err(_) => continue,
        };

        result.insert(
            ip,
            Some(AsInfo {
                asn,
                name: parts[6].to_string(),
                country: if parts[3].is_empty() || parts[3] == "NA" {
                    None
                } else {
                    Some(parts[3].to_string())
                },
                prefix: if parts[2].is_empty() || parts[2] == "NA" {
                    None
                } else {
                    Some(parts[2].to_string())
                },
            }),
        );
    }

    Ok(result)
}

fn is_private_or_special(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 0x40)
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified() || v6.is_multicast(),
    }
}
