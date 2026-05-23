//! Analyzer complet : détecte toutes les anomalies détectables sur un seul run.
//!
//! Hiérarchie des findings :
//!   ✖ Critical → problème sérieux, impact direct sur le débit/disponibilité
//!   ⚠ Warning  → anomalie confirmée, surveillance recommandée
//!   ℹ Info     → observation neutre, contexte pour l'interprétation
//!
//! Chaque finding inclut :
//!   - description : le problème en une phrase
//!   - evidence    : les chiffres qui le prouvent
//!   - action      : ce qu'on peut faire

use crate::mtr::heuristics::{find_degradation, is_bufferbloated, DegradationType};
use crate::types::{
    DiagnosticReport, Finding, FindingCategory, Hop, Severity,
    SpeedtestResult, Verdict, VerdictStatus,
};

pub fn analyze(hops: &[Hop], speedtests: &[SpeedtestResult]) -> (Vec<Finding>, Verdict) {
    let mut findings = Vec::new();

    findings.extend(check_packet_loss(hops));
    findings.extend(check_jitter(hops));
    findings.extend(check_bufferbloat(hops));
    findings.extend(check_latency(hops));
    findings.extend(check_routing(hops));
    findings.extend(check_speedtest_drop(speedtests));
    findings.extend(check_icmp_ratelimit(hops));

    let verdict = build_verdict(&findings, hops, speedtests);
    (findings, verdict)
}

// ═══════════════════════════════════════════════════════════
// PERTE DE PAQUETS
// ═══════════════════════════════════════════════════════════

fn check_packet_loss(hops: &[Hop]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Perte sur la cible finale (dernier hop)
    if let Some(last) = hops.last() {
        let loss = last.loss_pct();
        if loss > 0.5 && !last.suspected_icmp_ratelimit {
            findings.push(Finding {
                severity: if loss > 5.0 { Severity::Critical } else { Severity::Warning },
                category: FindingCategory::PacketLoss,
                description: format!(
                    "Perte de paquets sur la cible finale : {:.1}%",
                    loss
                ),
                evidence: format!(
                    "{} paquets envoyés, {} reçus ({:.1}% de perte) vers {}",
                    last.sent, last.received, loss,
                    last.primary_ip.map(|ip| ip.to_string()).unwrap_or_else(|| "?".to_string())
                ),
                action: Some(
                    "Vérifier la charge du serveur cible. Si persistant, contacter l'hébergeur.".to_string()
                ),
            });
        }
    }

    // Perte réelle qui se propage (pas rate-limit)
    // On cherche le premier hop non-rate-limited avec perte >1%
    for (i, hop) in hops.iter().enumerate() {
        if hop.suspected_icmp_ratelimit || hop.rtt_samples.is_empty() {
            continue;
        }
        let loss = hop.loss_pct();
        if loss < 1.0 { continue; }

        // Vérifie que la perte se propage sur au moins 2 hops suivants
        let propagates = hops[i..].iter()
            .filter(|h| !h.suspected_icmp_ratelimit && h.received > 0)
            .take(3)
            .filter(|h| h.loss_pct() >= loss - 2.0)
            .count() >= 2;

        if !propagates { continue; }

        // Pas déjà couvert par le "dernier hop"
        if i == hops.len() - 1 { continue; }

        let prev_as = hops[..i].iter().rev()
            .find_map(|h| h.as_info.as_ref())
            .map(|a| a.display())
            .unwrap_or_else(|| "AS inconnu".to_string());
        let cur_as = hop.as_info.as_ref()
            .map(|a| a.display())
            .unwrap_or_else(|| "AS inconnu".to_string());
        let as_changes = hops[..i].iter().rev()
            .find_map(|h| h.as_info.as_ref())
            != hop.as_info.as_ref();

        findings.push(Finding {
            severity: if loss > 5.0 { Severity::Critical } else { Severity::Warning },
            category: if as_changes {
                FindingCategory::PeeringCongestion
            } else {
                FindingCategory::PacketLoss
            },
            description: format!(
                "Perte de paquets réelle détectée au hop {} ({:.1}%)",
                hop.ttl, loss
            ),
            evidence: format!(
                "Perte propagée sur les hops suivants. Localisation : {} → {}",
                prev_as, cur_as
            ),
            action: Some(if as_changes {
                format!(
                    "Problème probable à l'interconnexion {} → {}. \
                     Tester via un VPN ou un chemin alternatif pour confirmer.",
                    prev_as, cur_as
                )
            } else {
                format!("Congestion interne chez {}. Contacter l'opérateur.", cur_as)
            }),
        });
        break; // On remonte le premier point de perte, les suivants sont des conséquences
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// JITTER
// ═══════════════════════════════════════════════════════════

fn check_jitter(hops: &[Hop]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (i, hop) in hops.iter().enumerate() {
        if hop.suspected_icmp_ratelimit || hop.rtt_samples.len() < 5 {
            continue;
        }
        let jitter = match hop.jitter_ms() {
            Some(j) => j,
            None => continue,
        };

        let avg = hop.avg_rtt_ms().unwrap_or(0.0);
        let relative_jitter = jitter / avg.max(1.0);

        if !(jitter > 20.0 && relative_jitter > 0.2) {
            continue;
        }

        // Si le jitter ne se propage pas au hop suivant répondant, c'est de la
        // déprioritisation ICMP locale (le routeur ralentit ses propres réponses
        // sous charge sans que le trafic transit soit affecté) — pas une congestion réelle.
        let next_jitter = hops[i + 1..]
            .iter()
            .find(|h| !h.suspected_icmp_ratelimit && h.rtt_samples.len() >= 3)
            .and_then(|h| h.jitter_ms());

        if let Some(nj) = next_jitter {
            if nj < jitter * 0.5 {
                continue;
            }
        }

        if jitter > 50.0 && relative_jitter > 0.3 {
            findings.push(Finding {
                severity: Severity::Critical,
                category: FindingCategory::Jitter,
                description: format!(
                    "Jitter critique au hop {} : {:.0}ms",
                    hop.ttl, jitter
                ),
                evidence: format!(
                    "RTT min {:.1}ms, avg {:.1}ms, max {:.1}ms — variation de {:.0}ms \
                     ({:.0}% du RTT moyen). Signe de congestion sérieuse.",
                    hop.min_rtt_ms().unwrap_or(0.0),
                    avg,
                    hop.max_rtt_ms().unwrap_or(0.0),
                    jitter,
                    relative_jitter * 100.0
                ),
                action: Some(format!(
                    "Congestion probable sur ce hop ({}). \
                     Relancer le test à différentes heures pour confirmer si c'est intermittent.",
                    hop.as_info.as_ref().map(|a| a.display()).unwrap_or_else(|| "AS inconnu".to_string())
                )),
            });
        } else {
            findings.push(Finding {
                severity: Severity::Warning,
                category: FindingCategory::Jitter,
                description: format!(
                    "Jitter élevé au hop {} : {:.0}ms",
                    hop.ttl, jitter
                ),
                evidence: format!(
                    "RTT varie de {:.1}ms à {:.1}ms (jitter {:.0}ms). \
                     Peut indiquer une congestion intermittente.",
                    hop.min_rtt_ms().unwrap_or(0.0),
                    hop.max_rtt_ms().unwrap_or(0.0),
                    jitter
                ),
                action: Some(
                    "Relancer le test à différentes heures pour voir si le jitter \
                     augmente en heure de pointe (18h-23h).".to_string()
                ),
            });
        }
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// BUFFERBLOAT
// ═══════════════════════════════════════════════════════════

fn check_bufferbloat(hops: &[Hop]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for hop in hops {
        if !is_bufferbloated(hop) || hop.suspected_icmp_ratelimit {
            continue;
        }
        let min = hop.min_rtt_ms().unwrap_or(0.0);
        let max = hop.max_rtt_ms().unwrap_or(0.0);
        let ratio = max / min.max(0.1);

        findings.push(Finding {
            severity: Severity::Warning,
            category: FindingCategory::Bufferbloat,
            description: format!(
                "Bufferbloat détecté au hop {} (ratio {:.1}x)",
                hop.ttl, ratio
            ),
            evidence: format!(
                "RTT min {:.0}ms vs max {:.0}ms sur ce hop. \
                 Les files d'attente du routeur se remplissent sous charge.",
                min, max
            ),
            action: Some(
                "Symptôme de surcharge réseau. Le débit FTP sera instable \
                 pendant les périodes de forte utilisation.".to_string()
            ),
        });
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// LATENCE
// ═══════════════════════════════════════════════════════════

fn check_latency(hops: &[Hop]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Latence finale vers la cible
    if let Some(last) = hops.last() {
        if let Some(rtt) = last.avg_rtt_ms() {
            // Détecter si la cible est aux US — depuis les hostnames ET le pays des AS
            // (fallback pays indispensable quand le reverse DNS échoue)
            let is_us = hops.iter().any(|h| {
                let from_hostname = h.hostname.as_ref().map(|n| {
                    let l = n.to_lowercase();
                    l.contains("newark") || l.contains("njy") || l.contains("newyork")
                    || l.contains("nto") || l.contains("chicago") || l.contains("ashburn")
                    || l.contains("losangeles") || l.contains("dallas")
                }).unwrap_or(false);
                let from_country = h.as_info.as_ref()
                    .and_then(|a| a.country.as_deref())
                    .map(|c| c == "US")
                    .unwrap_or(false);
                from_hostname || from_country
            });

            let (threshold_warn, threshold_crit, label) = if is_us {
                (150.0, 300.0, "cible US")
            } else {
                (80.0, 150.0, "cible")
            };

            if rtt > threshold_crit {
                findings.push(Finding {
                    severity: Severity::Critical,
                    category: FindingCategory::HighLatency,
                    description: format!("Latence critique vers la cible : {:.0}ms", rtt),
                    evidence: format!(
                        "RTT moyen {:.0}ms (seuil critique : {}ms pour {})",
                        rtt, threshold_crit, label
                    ),
                    action: Some(
                        "Latence trop élevée pour un transfert FTP efficace. \
                         Vérifier le routage et envisager un serveur relais géographiquement plus proche.".to_string()
                    ),
                });
            } else if rtt > threshold_warn {
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: FindingCategory::HighLatency,
                    description: format!("Latence élevée vers la cible : {:.0}ms", rtt),
                    evidence: format!(
                        "RTT moyen {:.0}ms (seuil warning : {}ms pour {}). \
                         Le débit TCP sera limité par le BDP (Bandwidth-Delay Product).",
                        rtt, threshold_warn, label
                    ),
                    action: Some(format!(
                        "Avec {:.0}ms de RTT, une connexion FTP simple est limitée à ~{:.0} Mbps \
                         (fenêtre TCP 64KB). Utiliser plusieurs connexions parallèles pour compenser.",
                        rtt,
                        (65536.0 * 8.0) / (rtt / 1000.0) / 1_000_000.0
                    )),
                });
            }
        }
    }

    // Bond de latence physique (transatlantique etc.)
    if let Some(degradation) = find_degradation(hops) {
        let hop = &hops[degradation.hop_index];
        let prev_rtt = hops[..degradation.hop_index].iter().rev()
            .find_map(|h| h.min_rtt_ms()).unwrap_or(0.0);

        if degradation.degradation_type == DegradationType::LatencyJump {
            let geo = classify_latency_jump(degradation.rtt_jump_ms, hops);
            findings.push(Finding {
                severity: Severity::Info,
                category: FindingCategory::PhysicalLatency,
                description: format!(
                    "Bond de latence physique au hop {} (+{:.0}ms) — {}",
                    hop.ttl, degradation.rtt_jump_ms, geo
                ),
                evidence: format!(
                    "RTT passe de {:.0}ms à {:.0}ms. Pas de perte propagée : \
                     ce n'est pas une congestion, c'est la distance physique.",
                    prev_rtt,
                    hop.min_rtt_ms().unwrap_or(0.0)
                ),
                action: None, // Rien à faire, c'est physique
            });
        }
    }

    findings
}

fn classify_latency_jump(jump_ms: f64, hops: &[Hop]) -> &'static str {
    // Identifie la traversée depuis les hostnames ET le pays des AS (fallback DNS)
    let has_us_hop = hops.iter().any(|h| {
        let from_hostname = h.hostname.as_ref().map(|n| {
            let l = n.to_lowercase();
            l.contains("newark") || l.contains("njy") || l.contains("newyork")
            || l.contains("nto") || l.contains("chicago") || l.contains("ashburn")
        }).unwrap_or(false);
        let from_country = h.as_info.as_ref()
            .and_then(|a| a.country.as_deref())
            .map(|c| c == "US")
            .unwrap_or(false);
        from_hostname || from_country
    });

    if has_us_hop && jump_ms > 60.0 {
        return "traversée transatlantique (normal)";
    }
    if jump_ms > 100.0 {
        return "liaison intercontinentale (normal)";
    }
    if jump_ms > 50.0 {
        return "liaison longue distance (normal)";
    }
    "bond géographique"
}

// ═══════════════════════════════════════════════════════════
// ROUTAGE
// ═══════════════════════════════════════════════════════════

fn check_routing(hops: &[Hop]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Trop de hops
    let real_hops = hops.iter().filter(|h| h.primary_ip.is_some()).count();
    if real_hops > 20 {
        findings.push(Finding {
            severity: Severity::Warning,
            category: FindingCategory::RoutingAnomaly,
            description: format!("Chemin long : {} hops jusqu'à la cible", real_hops),
            evidence: format!(
                "{} hops visibles (seuil normal : 20). \
                 Un chemin plus long augmente la latence et les points de défaillance potentiels.",
                real_hops
            ),
            action: Some(
                "Comparer avec un traceroute depuis un autre FAI ou une autre région. \
                 Si le chemin est anormalement long, signaler à l'opérateur.".to_string()
            ),
        });
    }

    // Détour géographique : détecter un AS qui fait "rebrousser chemin"
    // Ex: Paris → Londres → Paris est un détour inutile
    let geo_detour = detect_geo_detour(hops);
    if let Some((from, via, back)) = geo_detour {
        findings.push(Finding {
            severity: Severity::Warning,
            category: FindingCategory::RoutingAnomaly,
            description: format!("Détour géographique détecté : {} → {} → {}", from, via, back),
            evidence: format!(
                "Le trafic passe par {} alors qu'il revient ensuite vers {}. \
                 Ce détour ajoute de la latence inutilement.",
                via, back
            ),
            action: Some(
                "Ce type de détour est souvent dû à un accord de peering sous-optimal \
                 entre opérateurs. Difficile à corriger sans changer de FAI ou de transit.".to_string()
            ),
        });
    }

    findings
}

/// Détecte un détour géographique en analysant les hostnames des hops.
fn detect_geo_detour(hops: &[Hop]) -> Option<(String, String, String)> {
    // Extrait les villes depuis les hostnames dans l'ordre
    let geo_sequence: Vec<(usize, &str)> = hops.iter().enumerate()
        .filter_map(|(i, h)| {
            let hostname = h.hostname.as_ref()?;
            let lower = hostname.to_lowercase();
            let city = if lower.contains("paris") || lower.contains("pvu") || lower.contains("pye") { "Paris" }
                else if lower.contains("london") || lower.contains("ldn") { "Londres" }
                else if lower.contains("frankfurt") || lower.contains("fra") { "Frankfurt" }
                else if lower.contains("amsterdam") || lower.contains("ams") { "Amsterdam" }
                else if lower.contains("newark") || lower.contains("njy") { "Newark" }
                else if lower.contains("newyork") || lower.contains("nto") { "New York" }
                else if lower.contains("ashburn") { "Ashburn" }
                else { return None; };
            Some((i, city))
        })
        .collect();

    // Cherche un pattern A → B → A (rebrousse chemin)
    for i in 0..geo_sequence.len() {
        for j in (i + 1)..geo_sequence.len() {
            for k in (j + 1)..geo_sequence.len() {
                let (_, city_a) = geo_sequence[i];
                let (_, city_b) = geo_sequence[j];
                let (_, city_c) = geo_sequence[k];
                if city_a == city_c && city_a != city_b {
                    return Some((city_a.to_string(), city_b.to_string(), city_c.to_string()));
                }
            }
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════
// SPEEDTEST
// ═══════════════════════════════════════════════════════════

fn check_speedtest_drop(speedtests: &[SpeedtestResult]) -> Vec<Finding> {
    let mut findings = Vec::new();
    if speedtests.len() < 2 { return findings; }

    // Chute de débit entre AS consécutifs
    for i in 1..speedtests.len() {
        let prev = &speedtests[i - 1];
        let cur = &speedtests[i];

        // Skip si les deux mesures utilisent le même serveur proxy (pas significatif)
        if prev.server_id == cur.server_id { continue; }

        let drop_mbps = prev.download_mbps - cur.download_mbps;
        let drop_pct = if prev.download_mbps > 0.0 {
            drop_mbps / prev.download_mbps * 100.0
        } else { 0.0 };

        if drop_pct > 50.0 {
            findings.push(Finding {
                severity: Severity::Critical,
                category: FindingCategory::PeeringCongestion,
                description: format!(
                    "Chute de débit majeure à l'interconnexion {} → {} : -{:.0}%",
                    prev.as_name.as_deref().unwrap_or("?"),
                    cur.as_name.as_deref().unwrap_or("?"),
                    drop_pct
                ),
                evidence: format!(
                    "{:.0} Mbps ({}) → {:.0} Mbps ({}). \
                     Perte de {:.0} Mbps ({:.0}% du débit).",
                    prev.download_mbps, prev.as_name.as_deref().unwrap_or("?"),
                    cur.download_mbps, cur.as_name.as_deref().unwrap_or("?"),
                    drop_mbps, drop_pct
                ),
                action: Some(format!(
                    "Peering congestionné entre AS{} et AS{}. \
                     Tester via un VPN avec sortie différente pour confirmer. \
                     Si confirmé, contacter le FAI avec ce rapport comme preuve.",
                    prev.asn.unwrap_or(0),
                    cur.asn.unwrap_or(0)
                )),
            });
        } else if drop_pct > 20.0 {
            findings.push(Finding {
                severity: Severity::Warning,
                category: FindingCategory::PeeringCongestion,
                description: format!(
                    "Dégradation de débit à l'interconnexion {} → {} : -{:.0}%",
                    prev.as_name.as_deref().unwrap_or("?"),
                    cur.as_name.as_deref().unwrap_or("?"),
                    drop_pct
                ),
                evidence: format!(
                    "{:.0} Mbps → {:.0} Mbps (perte {:.0} Mbps).",
                    prev.download_mbps, cur.download_mbps, drop_mbps
                ),
                action: Some(
                    "Relancer le test à différentes heures. \
                     Si la dégradation s'accentue en heure de pointe, c'est du peering.".to_string()
                ),
            });
        }

        // Débit faible en absolu (même sans comparaison)
        if cur.download_mbps < 10.0 {
            findings.push(Finding {
                severity: Severity::Critical,
                category: FindingCategory::PeeringCongestion,
                description: format!(
                    "Débit très faible vers {} : {:.1} Mbps",
                    cur.as_name.as_deref().unwrap_or("?"),
                    cur.download_mbps
                ),
                evidence: format!(
                    "{:.1} Mbps mesuré vers {} (AS{}). \
                     En dessous de 10 Mbps, les transferts FTP seront très lents.",
                    cur.download_mbps,
                    cur.server_name,
                    cur.asn.unwrap_or(0)
                ),
                action: Some(
                    "Débit insuffisant pour un usage normal. \
                     Vérifier si la limitation vient du lien, du serveur Speedtest ou du peering.".to_string()
                ),
            });
        }
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// ICMP RATE-LIMIT (informatif)
// ═══════════════════════════════════════════════════════════

fn check_icmp_ratelimit(hops: &[Hop]) -> Vec<Finding> {
    let rate_limited: Vec<_> = hops.iter()
        .filter(|h| h.suspected_icmp_ratelimit)
        .collect();

    if rate_limited.is_empty() { return vec![]; }

    vec![Finding {
        severity: Severity::Info,
        category: FindingCategory::IcmpRateLimit,
        description: format!(
            "{} hop(s) limitent leurs réponses ICMP — perte apparente non réelle",
            rate_limited.len()
        ),
        evidence: rate_limited.iter()
            .map(|h| format!("hop {} ({:.0}%)", h.ttl, h.loss_pct()))
            .collect::<Vec<_>>()
            .join(", "),
        action: None,
    }]
}

// ═══════════════════════════════════════════════════════════
// VERDICT GLOBAL
// ═══════════════════════════════════════════════════════════

fn build_verdict(findings: &[Finding], hops: &[Hop], speedtests: &[SpeedtestResult]) -> Verdict {
    use crate::types::FindingCategory::*;

    let has_critical = findings.iter().any(|f| f.severity == Severity::Critical);
    let has_warning = findings.iter().any(|f| f.severity == Severity::Warning);

    let has_peering = findings.iter().any(|f| f.category == PeeringCongestion);
    let has_packet_loss = findings.iter().any(|f| f.category == PacketLoss);
    let has_jitter = findings.iter().any(|f| f.category == Jitter);
    let has_routing = findings.iter().any(|f| f.category == RoutingAnomaly);
    let has_latency_warn = findings.iter().any(|f|
        f.category == HighLatency && f.severity != Severity::Info
    );

    // Informations de contexte pour le verdict
    let target_rtt = hops.last().and_then(|h| h.avg_rtt_ms());
    let hop_count = hops.iter().filter(|h| h.primary_ip.is_some()).count();

    let (status, summary) = match (has_critical, has_warning) {
        (true, _) => {
            let cause = if has_peering {
                // Trouver le segment exact
                let segment = find_peering_segment(findings);
                format!("Problème de peering confirmé{}. \
                    Le débit est dégradé à l'interconnexion entre opérateurs. \
                    Utiliser un VPN ou un serveur relais pour contourner.",
                    segment
                )
            } else if has_packet_loss {
                format!("Perte de paquets réelle détectée sur le chemin ({} hops). \
                    Le transfert FTP sera dégradé et instable. \
                    Localiser le segment fautif et contacter l'opérateur responsable.",
                    hop_count
                )
            } else if has_jitter {
                "Congestion sérieuse détectée (jitter critique). \
                    Le réseau est surchargé sur ce chemin. \
                    Tester à des heures creuses pour confirmer si c'est de l'heure de pointe.".to_string()
            } else if has_latency_warn {
                let rtt_str = target_rtt.map(|r| format!("{:.0}ms", r)).unwrap_or_default();
                format!("Latence excessive vers la cible ({}). \
                    Le débit TCP sera limité par le BDP. \
                    Utiliser des connexions FTP parallèles pour compenser.",
                    rtt_str
                )
            } else {
                "Problème sérieux détecté sur le chemin réseau. \
                    Voir les findings ci-dessus pour le détail.".to_string()
            };
            (VerdictStatus::Faulty, cause)
        }
        (false, true) => {
            let cause = if has_jitter {
                "Jitter élevé détecté — possible congestion intermittente. \
                    Relancer le test en heure de pointe (18h-23h) pour confirmer.".to_string()
            } else if has_routing {
                "Routage sous-optimal détecté (détour géographique ou chemin long). \
                    Impact limité mais peut contribuer à la latence.".to_string()
            } else if has_latency_warn {
                let rtt_str = target_rtt.map(|r| format!("{:.0}ms", r)).unwrap_or_default();
                format!("Latence notable vers la cible ({}). \
                    Utiliser plusieurs connexions FTP parallèles pour maximiser le débit.",
                    rtt_str
                )
            } else {
                "Anomalies mineures détectées. Voir les findings pour le détail.".to_string()
            };
            (VerdictStatus::Degraded, cause)
        }
        (false, false) => {
            let rtt_str = target_rtt
                .map(|r| format!(" (RTT moyen : {:.0}ms)", r))
                .unwrap_or_default();
            let speedtest_str = if !speedtests.is_empty() {
                let max_dl = speedtests.iter()
                    .map(|s| s.download_mbps)
                    .fold(0.0f64, f64::max);
                format!(" Débit maximal mesuré : {:.0} Mbps.", max_dl)
            } else { String::new() };

            (
                VerdictStatus::Healthy,
                format!(
                    "Chemin réseau sain{}.{} \
                    Aucune anomalie détectée sur ce run. \
                    Si des problèmes persistent, relancer le test en heure de pointe.",
                    rtt_str, speedtest_str
                )
            )
        }
    };

    Verdict { status, summary }
}

fn find_peering_segment(findings: &[Finding]) -> String {
    findings.iter()
        .filter(|f| f.category == FindingCategory::PeeringCongestion && f.severity == Severity::Critical)
        .next()
        .map(|f| format!(" ({})", f.description))
        .unwrap_or_default()
}

impl DiagnosticReport {
    pub fn build(
        target: String,
        target_ip: std::net::IpAddr,
        target_as: Option<crate::types::AsInfo>,
        hops: Vec<Hop>,
        speedtests: Vec<SpeedtestResult>,
    ) -> Self {
        let (findings, verdict) = analyze(&hops, &speedtests);
        Self {
            timestamp: chrono::Utc::now(),
            target,
            target_ip,
            target_as,
            hops,
            speedtests,
            findings,
            verdict,
        }
    }
}
