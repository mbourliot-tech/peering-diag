//! Envoi de probes traceroute en ICMP avec gestion explicite des Time Exceeded.
//!
//! `surge-ping` ne nous convient pas car il ne donne pas accès aux paquets
//! ICMP Time Exceeded — il les traite comme des erreurs. Pour un traceroute,
//! on doit absolument capturer ces messages pour identifier les routeurs
//! intermédiaires.
//!
//! On utilise donc `socket2` directement pour ouvrir un raw socket ICMP,
//! envoyer un Echo Request avec un TTL donné, et lire la réponse — qui peut
//! être soit un Echo Reply (cible atteinte) soit un Time Exceeded (routeur
//! intermédiaire) soit un Destination Unreachable.
//!
//! Privilèges :
//! - Linux : CAP_NET_RAW ou root
//! - macOS : root
//! - Windows : pas de root requis pour SOCK_DGRAM ICMP, mais avec quelques
//!   limitations. Sur Windows, l'API native IcmpSendEcho est plus simple ;
//!   on garde socket2 pour le code portable.

use anyhow::{Context, Result};
use rand::Rng;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::io;
use std::mem::MaybeUninit;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::task;

/// Résultat d'un probe traceroute.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub responder: Option<IpAddr>,
    pub rtt: Option<Duration>,
    pub reached_target: bool,
}

#[derive(Debug, Clone)]
pub struct ProbeConfig {
    pub target: IpAddr,
    pub ttl: u8,
    pub timeout: Duration,
    pub identifier: u16,
    pub sequence: u16,
    pub payload_size: usize,
}

/// Envoie un probe ICMP et attend la réponse.
///
/// On exécute dans une tâche bloquante car `socket2` est synchrone et l'attente
/// avec timeout côté OS est plus fiable qu'une intégration tokio-aware.
pub async fn icmp_probe(config: &ProbeConfig) -> Result<ProbeResult> {
    let cfg = config.clone();
    task::spawn_blocking(move || icmp_probe_blocking(cfg))
        .await
        .context("join blocking probe task")?
}

fn icmp_probe_blocking(cfg: ProbeConfig) -> Result<ProbeResult> {
    match cfg.target {
        IpAddr::V4(target) => probe_v4(target, &cfg),
        IpAddr::V6(_) => {
            // TODO: support IPv6 (Icmpv6 + Time Exceeded encoding différent)
            Ok(ProbeResult {
                responder: None,
                rtt: None,
                reached_target: false,
            })
        }
    }
}

fn probe_v4(target: Ipv4Addr, cfg: &ProbeConfig) -> Result<ProbeResult> {
    // Création du socket raw ICMP.
    // Sous Linux non-root, on peut utiliser Type::DGRAM avec net.ipv4.ping_group_range,
    // mais ça ne reçoit pas les Time Exceeded. On utilise donc RAW.
    let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
        .context("création du socket raw ICMP (CAP_NET_RAW ou root requis)")?;

    socket
        .set_ttl(cfg.ttl as u32)
        .context("définition du TTL")?;
    socket
        .set_read_timeout(Some(cfg.timeout))
        .context("définition du read timeout")?;

    // Construction du paquet ICMP Echo Request
    let packet = build_icmp_echo_request(cfg.identifier, cfg.sequence, cfg.payload_size);

    let dest = SocketAddr::new(IpAddr::V4(target), 0);
    let dest_sa: SockAddr = dest.into();

    let start = Instant::now();
    socket
        .send_to(&packet, &dest_sa)
        .context("envoi du paquet ICMP")?;

    // Boucle de réception : on peut recevoir des paquets pour d'autres pings
    // si on partage le socket, donc on filtre par identifier/sequence.
    let deadline = start + cfg.timeout;
    let mut buf = [MaybeUninit::<u8>::uninit(); 1500];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(ProbeResult {
                responder: None,
                rtt: None,
                reached_target: false,
            });
        }
        socket.set_read_timeout(Some(remaining)).ok();

        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                let data: &[u8] = unsafe {
                    std::slice::from_raw_parts(buf.as_ptr() as *const u8, n)
                };
                let from_ip = match from.as_socket() {
                    Some(SocketAddr::V4(v4)) => IpAddr::V4(*v4.ip()),
                    _ => continue,
                };

                if let Some(parsed) = parse_icmp_response(data, cfg.identifier, cfg.sequence) {
                    match parsed {
                        IcmpResponse::EchoReply => {
                            return Ok(ProbeResult {
                                responder: Some(from_ip),
                                rtt: Some(start.elapsed()),
                                reached_target: true,
                            });
                        }
                        IcmpResponse::TimeExceeded | IcmpResponse::DestUnreachable => {
                            return Ok(ProbeResult {
                                responder: Some(from_ip),
                                rtt: Some(start.elapsed()),
                                reached_target: false,
                            });
                        }
                        IcmpResponse::NotForUs => {
                            continue;
                        }
                    }
                }
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                return Ok(ProbeResult {
                    responder: None,
                    rtt: None,
                    reached_target: false,
                });
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Construit un paquet ICMP Echo Request complet (type 8, code 0).
fn build_icmp_echo_request(identifier: u16, sequence: u16, payload_size: usize) -> Vec<u8> {
    let mut packet = Vec::with_capacity(8 + payload_size);
    packet.push(8); // type = Echo Request
    packet.push(0); // code
    packet.push(0); // checksum high (placeholder)
    packet.push(0); // checksum low (placeholder)
    packet.extend_from_slice(&identifier.to_be_bytes());
    packet.extend_from_slice(&sequence.to_be_bytes());
    for i in 0..payload_size {
        packet.push((i & 0xff) as u8);
    }
    let checksum = icmp_checksum(&packet);
    packet[2] = (checksum >> 8) as u8;
    packet[3] = (checksum & 0xff) as u8;
    packet
}

/// Checksum standard Internet (RFC 1071).
fn icmp_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

enum IcmpResponse {
    EchoReply,
    TimeExceeded,
    DestUnreachable,
    NotForUs,
}

/// Parse une réponse ICMP reçue sur un socket RAW.
///
/// Sous Linux/macOS, les sockets RAW reçoivent l'en-tête IP complet (20 octets)
/// suivi du paquet ICMP. Sous Windows, certaines versions retournent uniquement
/// l'ICMP. On essaie les deux offsets.
fn parse_icmp_response(data: &[u8], expect_id: u16, expect_seq: u16) -> Option<IcmpResponse> {
    for skip in [20usize, 0usize] {
        if data.len() < skip + 8 {
            continue;
        }
        let icmp = &data[skip..];
        let icmp_type = icmp[0];
        match icmp_type {
            // Echo Reply
            0 => {
                let id = u16::from_be_bytes([icmp[4], icmp[5]]);
                let seq = u16::from_be_bytes([icmp[6], icmp[7]]);
                if id == expect_id && seq == expect_seq {
                    return Some(IcmpResponse::EchoReply);
                }
                return Some(IcmpResponse::NotForUs);
            }
            // Time Exceeded
            11 => {
                if icmp.len() < 8 + 20 + 8 {
                    return Some(IcmpResponse::NotForUs);
                }
                let inner_icmp = &icmp[8 + 20..];
                let id = u16::from_be_bytes([inner_icmp[4], inner_icmp[5]]);
                let seq = u16::from_be_bytes([inner_icmp[6], inner_icmp[7]]);
                if id == expect_id && seq == expect_seq {
                    return Some(IcmpResponse::TimeExceeded);
                }
                return Some(IcmpResponse::NotForUs);
            }
            // Destination Unreachable
            3 => {
                if icmp.len() < 8 + 20 + 8 {
                    return Some(IcmpResponse::NotForUs);
                }
                let inner_icmp = &icmp[8 + 20..];
                let id = u16::from_be_bytes([inner_icmp[4], inner_icmp[5]]);
                let seq = u16::from_be_bytes([inner_icmp[6], inner_icmp[7]]);
                if id == expect_id && seq == expect_seq {
                    return Some(IcmpResponse::DestUnreachable);
                }
                return Some(IcmpResponse::NotForUs);
            }
            8 => return Some(IcmpResponse::NotForUs),
            _ => return Some(IcmpResponse::NotForUs),
        }
    }
    None
}

pub fn random_identifier() -> u16 {
    rand::thread_rng().gen()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_known_value() {
        let mut p = vec![8u8, 0, 0, 0, 0, 1, 0, 1];
        let c = icmp_checksum(&p);
        p[2] = (c >> 8) as u8;
        p[3] = (c & 0xff) as u8;
        assert_eq!(icmp_checksum(&p), 0);
    }

    #[test]
    fn test_build_echo_request_format() {
        let p = build_icmp_echo_request(0x1234, 0x5678, 32);
        assert_eq!(p.len(), 8 + 32);
        assert_eq!(p[0], 8);
        assert_eq!(p[1], 0);
        assert_eq!(u16::from_be_bytes([p[4], p[5]]), 0x1234);
        assert_eq!(u16::from_be_bytes([p[6], p[7]]), 0x5678);
    }
}
