//! Moteur MTR : exécute des rounds de traceroute en parallèle et agrège.

use crate::asn::AsnResolver;
use crate::mtr::heuristics::flag_icmp_ratelimiting;
use crate::mtr::probe::{icmp_probe, random_identifier, ProbeConfig, ProbeResult};
use crate::types::Hop;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct MtrConfig {
    pub target: IpAddr,
    pub max_hops: u8,
    pub probes_per_round: u32,
    pub rounds: u32,
    pub probe_timeout: Duration,
    pub round_interval: Duration,
    pub concurrent_probes: usize,
    pub payload_size: usize,
}

impl Default for MtrConfig {
    fn default() -> Self {
        Self {
            target: "1.1.1.1".parse().unwrap(),
            max_hops: 30,
            probes_per_round: 3,
            rounds: 10,
            probe_timeout: Duration::from_secs(2),
            round_interval: Duration::from_millis(500),
            concurrent_probes: 8,
            payload_size: 56,
        }
    }
}

pub struct Mtr {
    config: MtrConfig,
    asn_resolver: Arc<AsnResolver>,
    dns_resolver: Option<TokioAsyncResolver>,
}

impl Mtr {
    pub fn new(config: MtrConfig, asn_resolver: Arc<AsnResolver>) -> Self {
        // Cloudflare plutôt que le resolver système : plus fiable pour les PTR
        // de IPs étrangères, et résiste aux rafales (16 requêtes parallèles).
        let mut opts = ResolverOpts::default();
        opts.timeout = Duration::from_millis(1500);
        opts.attempts = 1;
        let dns_resolver = Some(TokioAsyncResolver::tokio(ResolverConfig::cloudflare(), opts));
        Self {
            config,
            asn_resolver,
            dns_resolver,
        }
    }

    pub async fn run(&self) -> Result<Vec<Hop>> {
        let hops: Arc<Mutex<HashMap<u8, Hop>>> = Arc::new(Mutex::new(HashMap::new()));
        let ip_counts: Arc<Mutex<HashMap<(u8, IpAddr), u32>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let max_useful_ttl: Arc<Mutex<u8>> = Arc::new(Mutex::new(self.config.max_hops));

        let total_steps = self.config.rounds * self.config.max_hops as u32;
        let pb = ProgressBar::new(total_steps as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} round {msg}",
            )
            .unwrap()
            .progress_chars("##-"),
        );

        for round in 1..=self.config.rounds {
            pb.set_message(format!("{}/{}", round, self.config.rounds));

            let current_max_ttl = *max_useful_ttl.lock().await;
            let ttls: Vec<u8> = (1..=current_max_ttl).collect();

            stream::iter(ttls)
                .for_each_concurrent(self.config.concurrent_probes, |ttl| {
                    let hops = hops.clone();
                    let ip_counts = ip_counts.clone();
                    let max_useful_ttl = max_useful_ttl.clone();
                    let target = self.config.target;
                    let probes_per_round = self.config.probes_per_round;
                    let timeout = self.config.probe_timeout;
                    let payload_size = self.config.payload_size;
                    let pb = pb.clone();

                    async move {
                        for probe_idx in 0..probes_per_round {
                            let cfg = ProbeConfig {
                                target,
                                ttl,
                                timeout,
                                identifier: random_identifier(),
                                sequence: (round * 1000 + probe_idx) as u16,
                                payload_size,
                            };

                            let result = match icmp_probe(&cfg).await {
                                Ok(r) => r,
                                Err(e) => {
                                    tracing::debug!("probe error TTL={}: {}", ttl, e);
                                    ProbeResult {
                                        responder: None,
                                        rtt: None,
                                        reached_target: false,
                                    }
                                }
                            };

                            {
                                let mut hops_guard = hops.lock().await;
                                let hop = hops_guard.entry(ttl).or_insert_with(|| Hop::new(ttl));
                                hop.sent += 1;
                                if let Some(ip) = result.responder {
                                    hop.received += 1;
                                    if let Some(rtt) = result.rtt {
                                        hop.rtt_samples.push(rtt);
                                    }
                                    let mut counts = ip_counts.lock().await;
                                    *counts.entry((ttl, ip)).or_insert(0) += 1;
                                }
                            }

                            if result.reached_target {
                                let mut max = max_useful_ttl.lock().await;
                                if ttl < *max {
                                    *max = ttl;
                                }
                            }
                        }
                        pb.inc(1);
                    }
                })
                .await;

            tokio::time::sleep(self.config.round_interval).await;
        }
        pb.finish_with_message("done");

        // === Construction finale ===
        let hops_map = hops.lock().await.clone();
        let ip_counts_map = ip_counts.lock().await.clone();

        let mut sorted: Vec<Hop> = hops_map.into_values().collect();
        sorted.sort_by_key(|h| h.ttl);

        for hop in &mut sorted {
            let mut ips_with_count: Vec<(IpAddr, u32)> = ip_counts_map
                .iter()
                .filter(|((ttl, _), _)| *ttl == hop.ttl)
                .map(|((_, ip), count)| (*ip, *count))
                .collect();
            ips_with_count.sort_by(|a, b| b.1.cmp(&a.1));
            hop.ips_seen = ips_with_count.iter().map(|(ip, _)| *ip).collect();
            hop.primary_ip = ips_with_count.first().map(|(ip, _)| *ip);
        }

        let max_ttl = *max_useful_ttl.lock().await;
        sorted.retain(|h| h.ttl <= max_ttl);

        let responding_hops = sorted.iter().filter(|h| h.primary_ip.is_some()).count();
        eprintln!(
            "  ✓ MTR terminé : {} hops dont {} ont répondu",
            sorted.len(),
            responding_hops
        );

        // === Lookup ASN ===
        let all_ips: Vec<IpAddr> = sorted.iter().filter_map(|h| h.primary_ip).collect();
        let unique_public: Vec<IpAddr> = {
            use std::collections::HashSet;
            let mut seen = HashSet::new();
            all_ips
                .iter()
                .filter(|ip| seen.insert(**ip))
                .copied()
                .collect()
        };

        eprintln!(
            "  → Lookup ASN de {} IP (Cymru + fallback ipinfo.io)…",
            unique_public.len()
        );
        let asn_start = std::time::Instant::now();
        let as_map = self.asn_resolver.lookup_bulk(&unique_public).await?;
        let resolved_as = as_map.values().filter(|v| v.is_some()).count();
        eprintln!(
            "  ✓ ASN résolus : {}/{} ({:.1}s)",
            resolved_as,
            unique_public.len(),
            asn_start.elapsed().as_secs_f64()
        );

        for hop in &mut sorted {
            if let Some(ip) = hop.primary_ip {
                hop.as_info = as_map.get(&ip).cloned().flatten();
            }
        }

        // === Reverse DNS en parallèle avec timeout court ===
        if let Some(ref resolver) = self.dns_resolver {
            let ips: Vec<(usize, IpAddr)> = sorted
                .iter()
                .enumerate()
                .filter_map(|(i, h)| h.primary_ip.map(|ip| (i, ip)))
                .collect();

            eprintln!(
                "  → Reverse DNS de {} IP (parallèle, timeout 1.5s)…",
                ips.len()
            );
            let dns_start = std::time::Instant::now();

            let dns_pb = ProgressBar::new(ips.len() as u64);
            dns_pb.set_style(
                ProgressStyle::with_template("    [{bar:30.green/blue}] {pos}/{len} {msg}")
                    .unwrap()
                    .progress_chars("##-"),
            );

            let hostnames: Vec<(usize, Option<String>)> = stream::iter(ips)
                .map(|(idx, ip)| {
                    let resolver = resolver.clone();
                    let dns_pb = dns_pb.clone();
                    async move {
                        let lookup = tokio::time::timeout(
                            Duration::from_millis(1500),
                            resolver.reverse_lookup(ip),
                        )
                        .await;
                        let hostname = match lookup {
                            Ok(Ok(r)) => r.iter().next().map(|n| {
                                n.to_string().trim_end_matches('.').to_string()
                            }),
                            _ => None,
                        };
                        dns_pb.inc(1);
                        if let Some(ref h) = hostname {
                            dns_pb.set_message(h.clone());
                        }
                        (idx, hostname)
                    }
                })
                .buffer_unordered(16)
                .collect()
                .await;

            dns_pb.finish_and_clear();

            let resolved = hostnames.iter().filter(|(_, h)| h.is_some()).count();
            eprintln!(
                "  ✓ DNS résolus : {}/{} ({:.1}s)",
                resolved,
                hostnames.len(),
                dns_start.elapsed().as_secs_f64()
            );

            for (idx, hostname) in hostnames {
                sorted[idx].hostname = hostname;
            }
        }

        // === Détection ICMP rate-limit ===
        eprintln!("  → Analyse heuristique (ICMP rate-limit, ECMP)…");
        flag_icmp_ratelimiting(&mut sorted);
        let rate_limited = sorted.iter().filter(|h| h.suspected_icmp_ratelimit).count();
        let ecmp = sorted.iter().filter(|h| h.ips_seen.len() > 1).count();
        eprintln!(
            "  ✓ {} hop(s) en ICMP rate-limit, {} hop(s) en ECMP",
            rate_limited, ecmp
        );

        Ok(sorted)
    }
}
