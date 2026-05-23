//! Analyse du chemin retour Globalping — calquée sur report/analyzer.rs.
//!
//! Même catégories de findings que le trajet aller :
//!   ✖ Critical / ⚠ Warning / ℹ Info
//!
//! Spécificités retour :
//!   - Pas de speedtest (non applicable).
//!   - Pas de détection ECMP (sonde unique).
//!   - `suspected_icmp_ratelimit` calculé heuristiquement (hop N perd mais N+1 répond).
//!   - `as_info` résolu par engine.rs avant l'appel.

use crate::lg::globalping::MtrHop;
use crate::types::{Finding, FindingCategory, Severity, Verdict, VerdictStatus};

pub fn analyze_return(hops: &[MtrHop]) -> (Vec<Finding>, Verdict) {
    // Pré-calcul du rate-limit ICMP pour chaque hop (évite de re-parcourir le slice)
    let icmp_rl: Vec<bool> = (0..hops.len())
        .map(|i| is_suspected_ratelimit(hops, i))
        .collect();

    let mut findings = Vec::new();
    findings.extend(check_loss(hops, &icmp_rl));
    findings.extend(check_latency(hops, &icmp_rl));
    findings.extend(check_jitter(hops, &icmp_rl));
    findings.extend(check_bufferbloat(hops, &icmp_rl));
    findings.extend(check_routing(hops));
    findings.extend(check_peering(hops, &icmp_rl));
    findings.extend(report_icmp_ratelimit(hops, &icmp_rl));

    let verdict = build_verdict(&findings, hops);
    (findings, verdict)
}

// ─── Heuristique ICMP rate-limit ─────────────────────────────────────────────

/// Retourne true si la perte au hop `idx` est probablement du filtrage ICMP :
/// du trafic continue de passer (un hop suivant répond avec bien moins de perte).
pub fn is_suspected_ratelimit_pub(hops: &[MtrHop], idx: usize) -> bool {
    is_suspected_ratelimit(hops, idx)
}

fn is_suspected_ratelimit(hops: &[MtrHop], idx: usize) -> bool {
    let loss = hops[idx].loss_pct;
    if loss < 1.0 {
        return false;
    }
    // Si un hop suivant (dans les 4 prochains) répond avec ≥15 pts de perte en moins
    hops[idx + 1..]
        .iter()
        .filter(|h| h.snt > 0)
        .take(4)
        .any(|h| h.loss_pct + 15.0 < loss)
}

// ═══════════════════════════════════════════════════════════
// PERTE DE PAQUETS
// ═══════════════════════════════════════════════════════════

fn check_loss(hops: &[MtrHop], icmp_rl: &[bool]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Perte sur le dernier hop (côté utilisateur depuis la sonde)
    if let Some(last) = hops.last() {
        let loss = last.loss_pct;
        if loss > 0.5 && !icmp_rl[hops.len() - 1] {
            findings.push(Finding {
                severity: if loss > 5.0 { Severity::Critical } else { Severity::Warning },
                category: FindingCategory::PacketLoss,
                description: format!("Perte de paquets retour vers l'utilisateur : {:.1}%", loss),
                evidence: format!(
                    "{} paquets envoyés, {:.1}% perdus depuis {} (dernier hop visible)",
                    last.snt,
                    loss,
                    last.host.as_deref().unwrap_or("?")
                ),
                action: Some(
                    "La perte sur le chemin retour est indépendante du chemin aller. \
                     Peut indiquer une congestion côté transit ou FAI de l'utilisateur."
                        .to_string(),
                ),
            });
        }
    }

    // Perte réelle qui se propage (pas le dernier hop, pas du rate-limit)
    for (i, hop) in hops.iter().enumerate() {
        if icmp_rl[i] || hop.loss_pct < 1.0 || i == hops.len() - 1 {
            continue;
        }

        // Vérifie que la perte se propage sur ≥2 des 3 hops suivants non-rate-limited
        let propagates = hops[i..]
            .iter()
            .enumerate()
            .filter(|(j, h)| {
                let abs = i + j;
                h.snt > 0 && h.avg_ms > 0.0 && abs < icmp_rl.len() && !icmp_rl[abs]
            })
            .take(3)
            .filter(|(_, h)| h.loss_pct >= hop.loss_pct - 2.0)
            .count()
            >= 2;

        if !propagates {
            continue;
        }

        let prev_as = hops[..i].iter().rev().find_map(|h| h.as_info.as_ref());
        let cur_as = hop.as_info.as_ref();
        let as_changed = prev_as.map(|a| a.asn) != cur_as.map(|a| a.asn)
            && prev_as.is_some()
            && cur_as.is_some();

        findings.push(Finding {
            severity: if hop.loss_pct > 5.0 { Severity::Critical } else { Severity::Warning },
            category: if as_changed {
                FindingCategory::PeeringCongestion
            } else {
                FindingCategory::PacketLoss
            },
            description: format!(
                "Perte retour propagée au hop {} : {:.1}%",
                hop.ttl, hop.loss_pct
            ),
            evidence: format!(
                "Perte propagée sur les hops suivants. {}",
                if as_changed {
                    format!(
                        "Frontière AS : {} → {}",
                        prev_as.map(|a| a.display()).unwrap_or_else(|| "?".to_string()),
                        cur_as.map(|a| a.display()).unwrap_or_else(|| "?".to_string())
                    )
                } else {
                    format!(
                        "Congestion interne {}",
                        cur_as.map(|a| a.display()).unwrap_or_default()
                    )
                }
            ),
            action: Some(if as_changed {
                format!(
                    "Problème probable à l'interconnexion {} → {} sur le chemin retour. \
                     Différent du chemin aller — asymétrie de routage à investiguer.",
                    prev_as.map(|a| a.display()).unwrap_or_else(|| "?".to_string()),
                    cur_as.map(|a| a.display()).unwrap_or_else(|| "?".to_string())
                )
            } else {
                format!(
                    "Congestion interne sur le chemin retour chez {}.",
                    cur_as.map(|a| a.display()).unwrap_or_else(|| "?".to_string())
                )
            }),
        });
        break;
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// JITTER
// ═══════════════════════════════════════════════════════════

fn check_jitter(hops: &[MtrHop], icmp_rl: &[bool]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (i, hop) in hops.iter().enumerate() {
        if icmp_rl[i] || hop.snt < 5 || hop.avg_ms == 0.0 {
            continue;
        }
        let stdev = hop.stdev_ms;
        if stdev < 20.0 {
            continue;
        }
        let relative = stdev / hop.avg_ms.max(1.0);
        if relative < 0.2 {
            continue;
        }

        // Vérifie que le jitter se propage (sinon c'est du déprio ICMP locale)
        let propagates = hops[i + 1..]
            .iter()
            .find(|h| h.snt > 0 && h.avg_ms > 0.0)
            .map(|h| h.stdev_ms >= stdev * 0.5)
            .unwrap_or(false);
        if !propagates {
            continue;
        }

        findings.push(Finding {
            severity: if stdev > 50.0 && relative > 0.3 {
                Severity::Critical
            } else {
                Severity::Warning
            },
            category: FindingCategory::Jitter,
            description: format!(
                "Jitter retour élevé au hop {} : {:.0}ms (écart-type)",
                hop.ttl, stdev
            ),
            evidence: format!(
                "RTT min {:.1}ms  avg {:.1}ms  max {:.1}ms — variation {:.0}ms ({:.0}% du RTT moyen). {}",
                hop.min_ms,
                hop.avg_ms,
                hop.max_ms,
                stdev,
                relative * 100.0,
                hop.as_info.as_ref().map(|a| a.display()).unwrap_or_default()
            ),
            action: Some(
                "Congestion intermittente sur le chemin retour. \
                 Tester à différentes heures (18h–23h) pour confirmer."
                    .to_string(),
            ),
        });
        break;
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// BUFFERBLOAT
// ═══════════════════════════════════════════════════════════

fn check_bufferbloat(hops: &[MtrHop], icmp_rl: &[bool]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (i, hop) in hops.iter().enumerate() {
        if icmp_rl[i] || hop.snt < 5 || hop.min_ms < 10.0 || hop.max_ms == 0.0 {
            continue;
        }
        let ratio = hop.max_ms / hop.min_ms;
        let absolute_jump = hop.max_ms - hop.min_ms;
        // Mêmes gardes que is_bufferbloated() côté aller :
        // min_ms >= 10ms et saut absolu > 80ms évitent les faux positifs
        // sur chemins courts (variation ICMP naturelle sur 5 rounds).
        if ratio < 5.0 || absolute_jump < 80.0 {
            continue;
        }

        findings.push(Finding {
            severity: Severity::Warning,
            category: FindingCategory::Bufferbloat,
            description: format!(
                "Bufferbloat retour au hop {} (ratio {:.1}x)",
                hop.ttl, ratio
            ),
            evidence: format!(
                "RTT min {:.0}ms vs max {:.0}ms — les files d'attente se remplissent sous charge. {}",
                hop.min_ms,
                hop.max_ms,
                hop.as_info.as_ref().map(|a| a.display()).unwrap_or_default()
            ),
            action: Some(
                "Surcharge réseau sur le chemin retour. \
                 Le débit entrant sera instable en période de forte utilisation."
                    .to_string(),
            ),
        });
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// LATENCE
// ═══════════════════════════════════════════════════════════

fn check_latency(hops: &[MtrHop], icmp_rl: &[bool]) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Latence finale (dernier hop non rate-limited avec une réponse)
    let last_valid = hops
        .iter()
        .enumerate()
        .rev()
        .find(|(i, h)| !icmp_rl[*i] && h.avg_ms > 0.0);

    if let Some((_, last)) = last_valid {
        let rtt = last.avg_ms;
        let is_us = is_transatlantic(hops);
        let (warn, crit, label) = if is_us {
            (150.0, 300.0, "destination US")
        } else {
            (80.0, 150.0, "destination")
        };

        if rtt > crit {
            findings.push(Finding {
                severity: Severity::Critical,
                category: FindingCategory::HighLatency,
                description: format!("Latence retour critique : {:.0}ms", rtt),
                evidence: format!(
                    "RTT moyen {:.0}ms depuis la sonde vers votre IP (seuil critique : {}ms pour {}). \
                     Débit TCP entrant fortement limité par le BDP.",
                    rtt, crit, label
                ),
                action: Some(
                    "Latence retour excessive — chemin sous-optimal ou congestion côté transit. \
                     Comparer avec la latence aller pour quantifier l'asymétrie."
                        .to_string(),
                ),
            });
        } else if rtt > warn {
            findings.push(Finding {
                severity: Severity::Warning,
                category: FindingCategory::HighLatency,
                description: format!("Latence retour élevée : {:.0}ms", rtt),
                evidence: format!(
                    "RTT moyen {:.0}ms (seuil : {}ms pour {}). \
                     Avec {:.0}ms RTT retour, fenêtre TCP 64KB → ~{:.0} Mbps max en entrée.",
                    rtt,
                    warn,
                    label,
                    rtt,
                    (65536.0 * 8.0) / (rtt / 1000.0) / 1_000_000.0
                ),
                action: Some(format!(
                    "Utiliser plusieurs connexions parallèles pour compenser \
                     la limite BDP ({:.0}ms RTT retour).",
                    rtt
                )),
            });
        }
    }

    // Bond de latence physique (premier saut > 50ms sans perte)
    for i in 1..hops.len() {
        if hops[i].avg_ms == 0.0 || hops[i - 1].avg_ms == 0.0 {
            continue;
        }
        let jump = hops[i].avg_ms - hops[i - 1].avg_ms;
        if jump < 50.0 {
            continue;
        }
        // Si pas de perte simultanée → latence physique, pas une congestion
        if !icmp_rl[i] && hops[i].loss_pct < 2.0 {
            findings.push(Finding {
                severity: Severity::Info,
                category: FindingCategory::PhysicalLatency,
                description: format!(
                    "Bond de latence physique retour au hop {} : +{:.0}ms",
                    hops[i].ttl, jump
                ),
                evidence: format!(
                    "RTT passe de {:.0}ms à {:.0}ms — liaison longue distance (normal).",
                    hops[i - 1].avg_ms, hops[i].avg_ms
                ),
                action: None,
            });
        }
        break; // Premier bond significatif seulement
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// ROUTAGE
// ═══════════════════════════════════════════════════════════

fn check_routing(hops: &[MtrHop]) -> Vec<Finding> {
    let mut findings = Vec::new();

    let real_hops = hops.iter().filter(|h| h.host.is_some() || h.ip.is_some()).count();
    if real_hops > 20 {
        findings.push(Finding {
            severity: Severity::Warning,
            category: FindingCategory::RoutingAnomaly,
            description: format!("Chemin retour long : {} hops", real_hops),
            evidence: format!(
                "{} hops visibles sur le chemin retour (seuil normal : 20). \
                 Un chemin long augmente la latence et les points de défaillance.",
                real_hops
            ),
            action: Some(
                "Routage retour sous-optimal — transit avec trop de sauts. \
                 Comparer avec le chemin aller."
                    .to_string(),
            ),
        });
    }

    if let Some((from, via, back)) = detect_geo_detour(hops) {
        findings.push(Finding {
            severity: Severity::Warning,
            category: FindingCategory::RoutingAnomaly,
            description: format!(
                "Détour géographique retour : {} → {} → {}",
                from, via, back
            ),
            evidence: format!(
                "Le chemin retour passe par {} avant de revenir vers {}. \
                 Ce détour ajoute de la latence inutilement.",
                via, back
            ),
            action: Some(
                "Détour sur le chemin retour — accord de peering sous-optimal chez le transit. \
                 Difficile à corriger sans changer de serveur ou de FAI de destination."
                    .to_string(),
            ),
        });
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// PEERING (frontières AS)
// ═══════════════════════════════════════════════════════════

fn check_peering(hops: &[MtrHop], icmp_rl: &[bool]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for i in 1..hops.len() {
        if icmp_rl[i] {
            continue;
        }
        let prev = &hops[i - 1];
        let cur = &hops[i];

        // Uniquement aux frontières AS connues
        let prev_asn = prev.as_info.as_ref().map(|a| a.asn);
        let cur_asn = cur.as_info.as_ref().map(|a| a.asn);
        if prev_asn == cur_asn || prev_asn.is_none() || cur_asn.is_none() {
            continue;
        }

        let latency_jump = if prev.avg_ms > 0.0 && cur.avg_ms > 0.0 {
            (cur.avg_ms - prev.avg_ms).max(0.0)
        } else {
            0.0
        };

        // Problème = perte élevée OU (saut de latence + perte simultanés)
        let has_issue = cur.loss_pct > 10.0
            || (latency_jump > 30.0 && cur.loss_pct > 2.0);

        if !has_issue {
            continue;
        }

        findings.push(Finding {
            severity: if cur.loss_pct > 10.0 { Severity::Critical } else { Severity::Warning },
            category: FindingCategory::PeeringCongestion,
            description: format!(
                "Dégradation retour à l'interconnexion {} → {}",
                prev.as_info.as_ref().map(|a| a.display()).unwrap_or_default(),
                cur.as_info.as_ref().map(|a| a.display()).unwrap_or_default(),
            ),
            evidence: format!(
                "Hop {} : perte {:.1}%  RTT {:.0}ms{}",
                cur.ttl,
                cur.loss_pct,
                cur.avg_ms,
                if latency_jump > 0.0 {
                    format!("  (+{:.0}ms à la frontière AS)", latency_jump)
                } else {
                    String::new()
                }
            ),
            action: Some(format!(
                "Congestion ou lien saturé sur le chemin retour entre {} et {}. \
                 Ce problème est chez le transit, pas chez l'hébergeur de destination.",
                prev.as_info.as_ref().map(|a| a.display()).unwrap_or_default(),
                cur.as_info.as_ref().map(|a| a.display()).unwrap_or_default(),
            )),
        });
    }

    findings
}

// ═══════════════════════════════════════════════════════════
// ICMP RATE-LIMIT (informatif)
// ═══════════════════════════════════════════════════════════

fn report_icmp_ratelimit(hops: &[MtrHop], icmp_rl: &[bool]) -> Vec<Finding> {
    let rate_limited: Vec<_> = hops
        .iter()
        .zip(icmp_rl)
        .filter(|(_, &rl)| rl)
        .map(|(h, _)| h)
        .collect();

    if rate_limited.is_empty() {
        return vec![];
    }

    vec![Finding {
        severity: Severity::Info,
        category: FindingCategory::IcmpRateLimit,
        description: format!(
            "{} hop(s) filtrent ICMP sur le chemin retour (perte apparente, non réelle)",
            rate_limited.len()
        ),
        evidence: rate_limited
            .iter()
            .map(|h| format!("hop {} ({:.0}%)", h.ttl, h.loss_pct))
            .collect::<Vec<_>>()
            .join(", "),
        action: None,
    }]
}

// ═══════════════════════════════════════════════════════════
// VERDICT
// ═══════════════════════════════════════════════════════════

fn build_verdict(findings: &[Finding], hops: &[MtrHop]) -> Verdict {
    let has_critical = findings.iter().any(|f| f.severity == Severity::Critical);
    let has_warning = findings.iter().any(|f| f.severity == Severity::Warning);

    let last_rtt = hops
        .iter()
        .rev()
        .find(|h| h.avg_ms > 0.0)
        .map(|h| h.avg_ms);

    let rtt_str = last_rtt
        .map(|r| format!(" (RTT moyen : {:.0}ms)", r))
        .unwrap_or_default();

    let (status, summary) = match (has_critical, has_warning) {
        (true, _) => {
            let cause = if findings.iter().any(|f| f.category == FindingCategory::PeeringCongestion) {
                "Congestion détectée sur le chemin retour à une interconnexion AS. \
                 Le chemin aller et retour ne passent probablement pas par les mêmes opérateurs — \
                 asymétrie de routage confirmée."
                    .to_string()
            } else if findings.iter().any(|f| f.category == FindingCategory::PacketLoss) {
                "Perte de paquets réelle sur le chemin retour. \
                 Le trafic entrant est affecté indépendamment du chemin sortant. \
                 Identifier le segment fautif et contacter l'opérateur de transit retour."
                    .to_string()
            } else if findings.iter().any(|f| {
                f.category == FindingCategory::HighLatency && f.severity == Severity::Critical
            }) {
                format!(
                    "Latence retour excessive{}. \
                     Le débit TCP entrant est fortement limité par le BDP. \
                     Utiliser plusieurs connexions parallèles pour compenser.",
                    rtt_str
                )
            } else {
                format!("Problème sérieux détecté sur le chemin retour{}.", rtt_str)
            };
            (VerdictStatus::Faulty, cause)
        }
        (false, true) => {
            let cause = if findings.iter().any(|f| f.category == FindingCategory::Jitter) {
                format!(
                    "Jitter élevé sur le chemin retour{} — congestion intermittente possible. \
                     Relancer en heure de pointe (18h–23h) pour confirmer.",
                    rtt_str
                )
            } else if findings.iter().any(|f| f.category == FindingCategory::RoutingAnomaly) {
                format!(
                    "Routage retour sous-optimal{}. \
                     Détour ou chemin long qui augmente la latence.",
                    rtt_str
                )
            } else if findings.iter().any(|f| f.category == FindingCategory::HighLatency) {
                format!(
                    "Latence retour notable{}. \
                     Utiliser plusieurs connexions parallèles pour maximiser le débit entrant.",
                    rtt_str
                )
            } else {
                format!("Anomalies mineures sur le chemin retour{}.", rtt_str)
            };
            (VerdictStatus::Degraded, cause)
        }
        (false, false) => (
            VerdictStatus::Healthy,
            format!(
                "Chemin retour sain{}. \
                 Aucune anomalie détectée sur les {} rounds de sondage.",
                rtt_str,
                hops.first().map(|h| h.snt).unwrap_or(0)
            ),
        ),
    };

    Verdict { status, summary }
}

// ─── Helpers géographiques ────────────────────────────────────────────────────

fn is_transatlantic(hops: &[MtrHop]) -> bool {
    hops.iter().any(|h| {
        let from_host = h.host.as_ref().map(|n| {
            let l = n.to_lowercase();
            l.contains("newark") || l.contains("njy")
                || l.contains("newyork") || l.contains("nto")
                || l.contains("chicago") || l.contains("ashburn")
                || l.contains("losangeles") || l.contains("dallas")
        }).unwrap_or(false);
        let from_as = h.as_info.as_ref()
            .and_then(|a| a.country.as_deref())
            .map(|c| c == "US")
            .unwrap_or(false);
        from_host || from_as
    })
}

/// Vérifie si `code` apparaît comme segment isolé dans le hostname
/// (séparé par `.`, `-` ou `_`), pour éviter les faux positifs :
/// "fra" dans "infra" ou "france" ne doit pas être détecté comme Frankfurt.
fn hostname_has_code(hostname: &str, code: &str) -> bool {
    hostname
        .split(|c: char| c == '.' || c == '-' || c == '_')
        .any(|seg| seg == code)
}

fn detect_geo_detour(hops: &[MtrHop]) -> Option<(String, String, String)> {
    let geo: Vec<(usize, &str)> = hops.iter().enumerate().filter_map(|(i, h)| {
        let name = h.host.as_ref()?;
        let l = name.to_lowercase();
        let city = if l.contains("paris") || l.contains("pvu") || l.contains("pye") { "Paris" }
            else if l.contains("london") || l.contains("ldn") || l.contains("lhr") { "Londres" }
            // "fra" seul (code aéroport FRA) mais pas "infra", "france", "infra-" etc.
            else if l.contains("frankfurt") || hostname_has_code(&l, "fra") { "Frankfurt" }
            // "ams" seul mais pas "amsterdam" déjà couvert, ni d'autres mots contenant "ams"
            else if l.contains("amsterdam") || hostname_has_code(&l, "ams") { "Amsterdam" }
            else if l.contains("newark") || l.contains("njy") { "Newark" }
            else if l.contains("newyork") || l.contains("nto") { "New York" }
            else if l.contains("ashburn") { "Ashburn" }
            else { return None; };
        Some((i, city))
    }).collect();

    for i in 0..geo.len() {
        for j in (i + 1)..geo.len() {
            for k in (j + 1)..geo.len() {
                let (_, a) = geo[i];
                let (_, b) = geo[j];
                let (_, c) = geo[k];
                if a == c && a != b {
                    return Some((a.to_string(), b.to_string(), c.to_string()));
                }
            }
        }
    }
    None
}
