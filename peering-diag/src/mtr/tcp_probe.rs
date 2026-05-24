//! Sonde TCP « connect() » avec TTL et port source contrôlés — Phase A du mode
//! Paris-traceroute, sans raw socket ni Npcap (fonctionne partout, y compris
//! Windows sans privilège de forge de paquet).
//!
//! Principe : on ouvre une socket TCP normale, on fixe le **port source**
//! (= identifiant de flux ECMP) et le **TTL**, puis on lance `connect()` :
//!   - SYN-ACK (succès)        → port ouvert, cible atteinte → RTT mesuré
//!   - RST (refus)             → port fermé mais cible atteinte → RTT mesuré
//!   - timeout                 → TTL insuffisant, filtrage, ou perte
//!
//! Le port source pilote le hash ECMP du chemin : en variant ce port sur
//! plusieurs flux, on explore les chemins parallèles vers la même cible.
//! Si certains flux sont nettement plus dégradés que d'autres, c'est un
//! déséquilibre ECMP — exactement le « parfois rapide, parfois lent ».
//!
//! Limite Phase A : `connect()` ne révèle que le **dernier hop** (la cible) ;
//! les hops intermédiaires restent fournis par le moteur ICMP. La capture
//! TCP des hops intermédiaires viendra avec pcap/Npcap (Phase B/C).

use anyhow::{Context, Result};
use serde::Serialize;
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::task;

/// Plage de ports source utilisée pour les flux (hors plage éphémère usuelle
/// de l'OS pour limiter les collisions). Classique base traceroute.
pub const FLOW_SRC_PORT_BASE: u16 = 33434;

#[derive(Debug, Clone)]
pub struct TcpProbeConfig {
    pub target: IpAddr,
    pub dst_port: u16,
    /// Port source à utiliser. `None` = port éphémère choisi par l'OS.
    pub src_port: Option<u16>,
    pub ttl: u8,
    pub timeout: Duration,
}

/// Issue d'une sonde TCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TcpOutcome {
    /// SYN-ACK reçu : port ouvert, cible atteinte.
    Open,
    /// RST reçu : port fermé mais cible atteinte (RTT valide).
    Closed,
    /// Aucune réponse dans le délai (TTL insuffisant, filtrage, ou perte).
    Timeout,
    /// Erreur réseau (hôte/réseau injoignable signalé par ICMP).
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct TcpProbeResult {
    pub outcome: TcpOutcome,
    pub rtt: Option<Duration>,
    /// Port source effectivement utilisé (utile si l'OS en a choisi un).
    pub src_port: u16,
}

impl TcpProbeResult {
    /// La cible a-t-elle répondu (SYN-ACK ou RST) ?
    pub fn reached_target(&self) -> bool {
        matches!(self.outcome, TcpOutcome::Open | TcpOutcome::Closed)
    }
}

/// Envoie une sonde TCP et attend le résultat. Exécuté dans une tâche bloquante
/// car `socket2` est synchrone (même approche que le probe ICMP).
pub async fn tcp_probe(config: &TcpProbeConfig) -> Result<TcpProbeResult> {
    let cfg = config.clone();
    task::spawn_blocking(move || tcp_probe_blocking(cfg))
        .await
        .context("join blocking tcp probe task")?
}

fn tcp_probe_blocking(cfg: TcpProbeConfig) -> Result<TcpProbeResult> {
    // Phase A : IPv4 uniquement (le moteur ICMP est aussi IPv4-only pour l'instant).
    let target_v4 = match cfg.target {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => {
            return Ok(TcpProbeResult {
                outcome: TcpOutcome::Unreachable,
                rtt: None,
                src_port: cfg.src_port.unwrap_or(0),
            });
        }
    };

    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))
        .context("création socket TCP")?;

    // Port source fixe = identifiant de flux ECMP.
    if let Some(port) = cfg.src_port {
        // REUSEADDR + LINGER 0 permettent de réutiliser vite le même port
        // sur le balayage TTL d'un flux (RST à la fermeture, pas de TIME_WAIT).
        socket.set_reuse_address(true).ok();
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
        socket
            .bind(&bind_addr.into())
            .with_context(|| format!("bind port source {}", port))?;
    }

    socket.set_ttl(cfg.ttl as u32).context("set TTL")?;
    // Fermeture par RST → libère le port source immédiatement.
    socket.set_linger(Some(Duration::ZERO)).ok();

    let dest = SocketAddr::new(IpAddr::V4(target_v4), cfg.dst_port);

    let start = Instant::now();
    let res = socket.connect_timeout(&dest.into(), cfg.timeout);
    let elapsed = start.elapsed();

    let actual_src = cfg
        .src_port
        .or_else(|| {
            socket
                .local_addr()
                .ok()
                .and_then(|a| a.as_socket())
                .map(|s| s.port())
        })
        .unwrap_or(0);

    let (outcome, rtt) = match res {
        Ok(()) => (TcpOutcome::Open, Some(elapsed)),
        Err(e) => classify_connect_error(&e, elapsed),
    };

    Ok(TcpProbeResult {
        outcome,
        rtt,
        src_port: actual_src,
    })
}

/// Traduit l'erreur de `connect()` en issue de sonde.
fn classify_connect_error(e: &io::Error, elapsed: Duration) -> (TcpOutcome, Option<Duration>) {
    match e.kind() {
        // RST : la cible a répondu, on a un RTT valide.
        io::ErrorKind::ConnectionRefused => (TcpOutcome::Closed, Some(elapsed)),
        // Pas de réponse dans le délai.
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => (TcpOutcome::Timeout, None),
        _ => {
            // Certaines plateformes remontent ICMP host/net unreachable via raw_os_error.
            // Sur Windows : WSAEHOSTUNREACH=10065, WSAENETUNREACH=10051.
            // Sur Linux : EHOSTUNREACH=113, ENETUNREACH=101.
            match e.raw_os_error() {
                Some(113) | Some(101) | Some(10065) | Some(10051) => {
                    (TcpOutcome::Unreachable, None)
                }
                _ => (TcpOutcome::Timeout, None),
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════
// EXPLORATION ECMP VERS LA CIBLE
// ═══════════════════════════════════════════════════════════

/// Statistiques agrégées d'un flux (un port source = un chemin ECMP).
#[derive(Debug, Clone, Serialize)]
pub struct FlowStats {
    pub src_port: u16,
    pub sent: u32,
    pub reached: u32,
    pub rtts_ms: Vec<f64>,
    /// Issue majoritaire observée (pour l'affichage).
    pub outcome: TcpOutcome,
}

impl FlowStats {
    pub fn loss_pct(&self) -> f64 {
        if self.sent == 0 {
            return 0.0;
        }
        (1.0 - self.reached as f64 / self.sent as f64) * 100.0
    }

    pub fn min_rtt_ms(&self) -> Option<f64> {
        self.rtts_ms.iter().copied().fold(None, |acc, v| {
            Some(acc.map_or(v, |a: f64| a.min(v)))
        })
    }

    pub fn max_rtt_ms(&self) -> Option<f64> {
        self.rtts_ms.iter().copied().fold(None, |acc, v| {
            Some(acc.map_or(v, |a: f64| a.max(v)))
        })
    }

    pub fn median_rtt_ms(&self) -> Option<f64> {
        if self.rtts_ms.is_empty() {
            return None;
        }
        let mut v = self.rtts_ms.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 0 {
            (v[mid - 1] + v[mid]) / 2.0
        } else {
            v[mid]
        })
    }
}

/// Configuration de l'exploration ECMP.
#[derive(Debug, Clone)]
pub struct EcmpExploreConfig {
    pub target: IpAddr,
    pub dst_port: u16,
    /// Nombre de flux (ports source distincts) à tester.
    pub flows: u16,
    /// Probes par flux pour calculer les stats.
    pub probes_per_flow: u32,
    /// TTL utilisé pour atteindre la cible (assez grand pour le chemin complet).
    pub ttl: u8,
    pub timeout: Duration,
}

impl Default for EcmpExploreConfig {
    fn default() -> Self {
        Self {
            target: IpAddr::V4(Ipv4Addr::LOCALHOST),
            dst_port: 443,
            flows: 8,
            probes_per_flow: 5,
            ttl: 64,
            timeout: Duration::from_secs(2),
        }
    }
}

/// Explore les chemins ECMP vers la cible en variant le port source.
/// Les flux tournent en parallèle ; à l'intérieur d'un flux les probes sont
/// séquentielles (même port source réutilisé → chemin ECMP stable).
pub async fn explore_ecmp_to_target(cfg: &EcmpExploreConfig) -> Vec<FlowStats> {
    use futures::stream::{self, StreamExt};

    let flow_ports: Vec<u16> = (0..cfg.flows)
        .map(|i| FLOW_SRC_PORT_BASE.wrapping_add(i))
        .collect();

    stream::iter(flow_ports)
        .map(|src_port| {
            let cfg = cfg.clone();
            async move { run_flow(&cfg, src_port).await }
        })
        // Concurrence entre flux (chaque flux est séquentiel en interne).
        .buffer_unordered(cfg.flows as usize)
        .collect()
        .await
}

async fn run_flow(cfg: &EcmpExploreConfig, src_port: u16) -> FlowStats {
    let mut rtts_ms = Vec::new();
    let mut reached = 0u32;
    let mut last_outcome = TcpOutcome::Timeout;

    for _ in 0..cfg.probes_per_flow {
        let probe = TcpProbeConfig {
            target: cfg.target,
            dst_port: cfg.dst_port,
            src_port: Some(src_port),
            ttl: cfg.ttl,
            timeout: cfg.timeout,
        };
        if let Ok(r) = tcp_probe(&probe).await {
            last_outcome = r.outcome;
            if r.reached_target() {
                reached += 1;
                if let Some(rtt) = r.rtt {
                    rtts_ms.push(rtt.as_secs_f64() * 1000.0);
                }
            }
        }
        // Petite pause pour libérer proprement le port source entre probes.
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    FlowStats {
        src_port,
        sent: cfg.probes_per_flow,
        reached,
        rtts_ms,
        outcome: last_outcome,
    }
}

/// Verdict de déséquilibre ECMP.
#[derive(Debug, Clone, Serialize)]
pub struct EcmpImbalance {
    pub degraded_flows: usize,
    pub total_flows: usize,
    /// RTT médian de référence (meilleur flux), en ms.
    pub baseline_ms: Option<f64>,
    /// Détail par flux dégradé : (src_port, raison).
    pub details: Vec<(u16, String)>,
}

impl EcmpImbalance {
    pub fn is_imbalanced(&self) -> bool {
        self.degraded_flows > 0 && self.degraded_flows < self.total_flows
    }
}

/// Détecte un déséquilibre entre chemins ECMP : un flux est « dégradé » si sa
/// perte est nettement supérieure au meilleur flux, ou si son RTT médian
/// dépasse la référence de plus de max(20ms, 50%).
pub fn detect_ecmp_imbalance(flows: &[FlowStats]) -> EcmpImbalance {
    let reaching: Vec<&FlowStats> = flows.iter().filter(|f| f.reached > 0).collect();

    // RTT médian de référence = le plus bas parmi les flux qui atteignent.
    let baseline_ms = reaching
        .iter()
        .filter_map(|f| f.median_rtt_ms())
        .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.min(v))));

    let best_loss = flows
        .iter()
        .map(|f| f.loss_pct())
        .fold(f64::INFINITY, f64::min);

    let mut details = Vec::new();
    for f in flows {
        let mut reasons = Vec::new();

        // Perte anormale vs le meilleur flux (+20 points).
        if f.loss_pct() > best_loss + 20.0 {
            reasons.push(format!("perte {:.0}%", f.loss_pct()));
        }

        // RTT médian anormal vs la référence.
        if let (Some(med), Some(base)) = (f.median_rtt_ms(), baseline_ms) {
            let threshold = base + (base * 0.5).max(20.0);
            if med > threshold {
                reasons.push(format!("RTT médian {:.0}ms vs {:.0}ms", med, base));
            }
        }

        if !reasons.is_empty() {
            details.push((f.src_port, reasons.join(", ")));
        }
    }

    EcmpImbalance {
        degraded_flows: details.len(),
        total_flows: flows.len(),
        baseline_ms,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(src_port: u16, sent: u32, reached: u32, rtts: &[f64]) -> FlowStats {
        FlowStats {
            src_port,
            sent,
            reached,
            rtts_ms: rtts.to_vec(),
            outcome: TcpOutcome::Open,
        }
    }

    #[test]
    fn loss_pct_basic() {
        let f = flow(33434, 5, 4, &[10.0, 11.0, 12.0, 13.0]);
        assert!((f.loss_pct() - 20.0).abs() < 0.01);
    }

    #[test]
    fn median_odd_and_even() {
        let f_odd = flow(1, 3, 3, &[30.0, 10.0, 20.0]);
        assert_eq!(f_odd.median_rtt_ms(), Some(20.0));
        let f_even = flow(2, 4, 4, &[10.0, 20.0, 30.0, 40.0]);
        assert_eq!(f_even.median_rtt_ms(), Some(25.0));
    }

    #[test]
    fn imbalance_flags_high_rtt_flow() {
        // 3 flux sains ~90ms, 1 flux à 250ms → déséquilibre.
        let flows = vec![
            flow(33434, 5, 5, &[90.0, 91.0, 89.0, 90.0, 92.0]),
            flow(33435, 5, 5, &[88.0, 90.0, 91.0, 89.0, 90.0]),
            flow(33436, 5, 5, &[92.0, 91.0, 90.0, 93.0, 91.0]),
            flow(33437, 5, 5, &[250.0, 248.0, 252.0, 249.0, 251.0]),
        ];
        let imb = detect_ecmp_imbalance(&flows);
        assert!(imb.is_imbalanced());
        assert_eq!(imb.degraded_flows, 1);
        assert_eq!(imb.details[0].0, 33437);
    }

    #[test]
    fn imbalance_flags_lossy_flow() {
        let flows = vec![
            flow(33434, 5, 5, &[90.0, 90.0, 90.0, 90.0, 90.0]),
            flow(33435, 5, 1, &[90.0]), // 80% de perte
        ];
        let imb = detect_ecmp_imbalance(&flows);
        assert_eq!(imb.degraded_flows, 1);
        assert_eq!(imb.details[0].0, 33435);
    }

    #[test]
    fn no_imbalance_when_all_similar() {
        let flows = vec![
            flow(33434, 5, 5, &[90.0, 91.0, 89.0, 90.0, 92.0]),
            flow(33435, 5, 5, &[88.0, 90.0, 91.0, 89.0, 90.0]),
        ];
        let imb = detect_ecmp_imbalance(&flows);
        assert!(!imb.is_imbalanced());
        assert_eq!(imb.degraded_flows, 0);
    }
}
