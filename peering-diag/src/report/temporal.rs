//! Analyse temporelle des données historiques.
//!
//! Deux détections :
//!   - Heures de pointe récurrentes : plages horaires où >50% des runs sont non-Healthy
//!   - Tendance à la dégradation : régression linéaire sur RTT/perte/débit

use anyhow::Result;
use colored::*;
use rusqlite::{params, Connection};

// ─── Structures publiques ─────────────────────────────────────────────────────

/// Plage horaire consécutive avec congestion récurrente.
#[derive(Debug)]
pub struct PeakHourFinding {
    /// Heures UTC concernées (0–23).
    pub hours: Vec<u8>,
    /// Nombre de runs dégradés ou faulty dans cette plage.
    pub bad_count: usize,
    /// Nombre total de runs dans cette plage.
    pub total_count: usize,
    /// Perte moyenne (%) sur cette plage.
    pub avg_loss: f64,
    /// RTT moyen (ms) sur cette plage.
    pub avg_rtt: f64,
}

/// Tendance à la dégradation sur les N derniers runs.
#[derive(Debug)]
pub struct TrendFinding {
    /// Nombre de runs analysés.
    pub last_n: usize,
    /// Durée couverte (jours).
    pub span_days: f64,
    /// Delta RTT total estimé (ms) sur la période : slope × (n-1).
    pub rtt_delta_ms: f64,
    /// Delta perte totale estimée (%) sur la période.
    pub loss_delta_pct: f64,
    /// Delta débit relatif (%) sur la période (négatif = dégradation).
    pub dl_delta_rel_pct: f64,
}

// ─── Régression linéaire ──────────────────────────────────────────────────────

/// Calcule la pente d'une série temporelle discrète (x = indice 0..n-1).
fn linear_slope(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let sum_x  = n * (n - 1.0) / 2.0;
    let sum_x2 = n * (n - 1.0) * (2.0 * n - 1.0) / 6.0;
    let sum_y: f64  = values.iter().sum();
    let sum_xy: f64 = values.iter().enumerate().map(|(i, &y)| i as f64 * y).sum();
    let denom = n * sum_x2 - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return 0.0;
    }
    (n * sum_xy - sum_x * sum_y) / denom
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Extrait le verdict depuis le JSON sérialisé.
fn verdict_from_json(payload: &str) -> &'static str {
    // Parsing minimal pour éviter de dépendre de Deserialize sur DiagnosticReport.
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return "?";
    };
    match v["verdict"]["status"].as_str().unwrap_or("?") {
        "Healthy"  => "Healthy",
        "Degraded" => "Degraded",
        "Faulty"   => "Faulty",
        _          => "?",
    }
}

fn hour_from_ts(ts: &str) -> u8 {
    ts.find('T')
        .and_then(|i| ts.get(i + 1..i + 3))
        .and_then(|h| h.parse::<u8>().ok())
        .unwrap_or(0)
}

// ─── Requête commune ──────────────────────────────────────────────────────────

struct RunSample {
    timestamp:    String,
    payload_json: String,
    max_loss:     f64,
    avg_rtt:      f64,
    dl_mbps:      f64,
}

fn fetch_samples(
    conn: &Connection,
    target: Option<&str>,
    limit: usize,
) -> Result<Vec<RunSample>> {
    let sql = "
        SELECT
            r.timestamp,
            r.payload_json,
            MAX(
                COALESCE((
                    SELECT MAX(h.loss_pct)
                    FROM hop_samples h
                    WHERE h.report_id = r.id
                      AND h.suspected_ratelimit = 0
                      AND h.ip IS NOT NULL
                ), 0.0),
                COALESCE((
                    SELECT MAX(rh.loss_pct)
                    FROM return_hop_samples rh
                    WHERE rh.report_id = r.id
                      AND rh.ip IS NOT NULL
                ), 0.0)
            ) AS max_loss,
            COALESCE((
                SELECT AVG(h.avg_rtt_ms)
                FROM hop_samples h WHERE h.report_id = r.id AND h.avg_rtt_ms IS NOT NULL
            ), 0.0) AS avg_rtt,
            COALESCE((
                SELECT MAX(s.download_mbps)
                FROM speedtest_samples s WHERE s.report_id = r.id
            ), 0.0) AS dl_mbps
        FROM reports r
        WHERE (?1 IS NULL OR r.target = ?1)
        ORDER BY r.timestamp DESC
        LIMIT ?2
    ";

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![target, limit as i64], |row| {
        Ok(RunSample {
            timestamp:    row.get(0)?,
            payload_json: row.get(1)?,
            max_loss:     row.get(2)?,
            avg_rtt:      row.get(3)?,
            dl_mbps:      row.get(4)?,
        })
    })?;

    let mut samples: Vec<RunSample> = rows.collect::<rusqlite::Result<_>>()?;
    // Ordre chronologique pour la régression
    samples.reverse();
    Ok(samples)
}

// ─── Détection heures de pointe ───────────────────────────────────────────────

/// Détecte les plages horaires consécutives où >50% des runs sont non-Healthy,
/// avec au moins 2 échantillons dans la plage.
pub fn detect_peak_hours(
    conn: &Connection,
    target: Option<&str>,
) -> Result<Vec<PeakHourFinding>> {
    // On prend tous les runs disponibles (limite haute arbitraire)
    let samples = fetch_samples(conn, target, 100_000)?;

    if samples.is_empty() {
        return Ok(Vec::new());
    }

    // Agrégation par heure
    let mut by_hour: Vec<Vec<(&RunSample, bool)>> = (0..24).map(|_| Vec::new()).collect();
    for s in &samples {
        let verdict = verdict_from_json(&s.payload_json);
        let bad = verdict != "Healthy";
        let h = hour_from_ts(&s.timestamp) as usize;
        by_hour[h].push((s, bad));
    }

    // Stats par heure : (bad_count, total, avg_loss, avg_rtt)
    #[derive(Default, Clone)]
    struct HourStat {
        total:    usize,
        bad:      usize,
        sum_loss: f64,
        sum_rtt:  f64,
    }

    let stats: Vec<HourStat> = by_hour
        .iter()
        .map(|entries| {
            let mut s = HourStat::default();
            for (sample, bad) in entries {
                s.total    += 1;
                s.sum_loss += sample.max_loss;
                s.sum_rtt  += sample.avg_rtt;
                if *bad { s.bad += 1; }
            }
            s
        })
        .collect();

    // Marque les heures "dégradées" : >50% bad ET ≥2 échantillons
    let is_peak: Vec<bool> = stats.iter().map(|s| {
        s.total >= 2 && s.bad as f64 / s.total as f64 > 0.5
    }).collect();

    // Regroupe les heures consécutives (anneau 23→0)
    // On travaille sur [0..24], puis on teste 23→0 séparément.
    let mut findings = Vec::new();
    let mut visited = [false; 24];

    for start in 0..24usize {
        if visited[start] || !is_peak[start] {
            continue;
        }
        let mut hours: Vec<u8> = Vec::new();
        let mut h = start;
        loop {
            if visited[h] || !is_peak[h] { break; }
            visited[h] = true;
            hours.push(h as u8);
            h = (h + 1) % 24;
            if h == start { break; } // boucle complète (cas extrême : 24/24 heures peak)
        }
        if hours.is_empty() { continue; }

        let bad_count:   usize = hours.iter().map(|&hh| stats[hh as usize].bad).sum();
        let total_count: usize = hours.iter().map(|&hh| stats[hh as usize].total).sum();
        let sum_loss: f64 = hours.iter().map(|&hh| stats[hh as usize].sum_loss).sum();
        let sum_rtt:  f64 = hours.iter().map(|&hh| stats[hh as usize].sum_rtt).sum();

        findings.push(PeakHourFinding {
            hours,
            bad_count,
            total_count,
            avg_loss: if total_count > 0 { sum_loss / total_count as f64 } else { 0.0 },
            avg_rtt:  if total_count > 0 { sum_rtt  / total_count as f64 } else { 0.0 },
        });
    }

    // Trie par total_count décroissant (plage la plus représentée en premier)
    findings.sort_by(|a, b| b.total_count.cmp(&a.total_count));
    Ok(findings)
}

// ─── Détection de tendance ────────────────────────────────────────────────────

/// Régression linéaire sur les N derniers runs pour détecter une tendance.
///
/// Seuils de significativité :
///   - RTT : delta total > 10 ms
///   - Perte : delta total > 0.5 %
///   - Débit : variation relative > 10 %
pub fn detect_degradation_trend(
    conn: &Connection,
    target: Option<&str>,
    last_n: usize,
) -> Result<Option<TrendFinding>> {
    let samples = fetch_samples(conn, target, last_n)?;

    if samples.len() < 5 {
        return Ok(None);
    }

    let n = samples.len();

    // Durée de la période
    let span_days = {
        let first = &samples[0].timestamp;
        let last  = &samples[n - 1].timestamp;
        // Extraction grossière en jours depuis les timestamps RFC3339
        parse_span_days(first, last)
    };

    let rtts:   Vec<f64> = samples.iter().map(|s| s.avg_rtt).collect();
    let losses: Vec<f64> = samples.iter().map(|s| s.max_loss).collect();
    let dls:    Vec<f64> = samples.iter().map(|s| s.dl_mbps).collect();

    let rtt_slope  = linear_slope(&rtts);
    let loss_slope = linear_slope(&losses);
    let dl_slope   = linear_slope(&dls);

    let rtt_delta  = rtt_slope  * (n - 1) as f64;
    let loss_delta = loss_slope * (n - 1) as f64;
    let dl_delta   = dl_slope   * (n - 1) as f64;

    // Variation relative du débit (par rapport à la moyenne)
    let dl_mean = dls.iter().copied().sum::<f64>() / n as f64;
    let dl_delta_rel = if dl_mean > 0.1 { dl_delta / dl_mean * 100.0 } else { 0.0 };

    // Significatif si au moins un critère dépasse le seuil
    let significant = rtt_delta.abs() > 10.0
        || loss_delta.abs() > 0.5
        || dl_delta_rel.abs() > 10.0;

    if !significant {
        return Ok(None);
    }

    Ok(Some(TrendFinding {
        last_n: n,
        span_days,
        rtt_delta_ms: rtt_delta,
        loss_delta_pct: loss_delta,
        dl_delta_rel_pct: dl_delta_rel,
    }))
}

/// Estime la durée en jours entre deux timestamps RFC3339 (précision à la minute).
fn parse_span_days(first: &str, last: &str) -> f64 {
    fn parse_minutes(ts: &str) -> i64 {
        // "2025-01-15T14:32:00+01:00" → extraire YYYY MM DD HH MM depuis les positions fixes
        if ts.len() < 16 { return 0; }
        let y:  i64 = ts[0..4].parse().unwrap_or(0);
        let mo: i64 = ts[5..7].parse().unwrap_or(0);
        let d:  i64 = ts[8..10].parse().unwrap_or(0);
        let h:  i64 = ts.find('T').and_then(|i| ts.get(i+1..i+3)).and_then(|s| s.parse().ok()).unwrap_or(0);
        let mi: i64 = ts.find('T').and_then(|i| ts.get(i+4..i+6)).and_then(|s| s.parse().ok()).unwrap_or(0);
        // Approximation : 365.25 jours/an, 30.44 jours/mois
        (y * 525960) + (mo * 43829) + (d * 1440) + (h * 60) + mi
    }
    let delta_min = (parse_minutes(last) - parse_minutes(first)).abs();
    delta_min as f64 / 1440.0
}

// ─── Affichage ────────────────────────────────────────────────────────────────

/// Affiche le bloc d'analyse temporelle à la fin de `history`.
pub fn show_temporal_analysis(
    conn: &Connection,
    target: Option<&str>,
    last_n: usize,
) -> Result<()> {
    let peaks  = detect_peak_hours(conn, target)?;
    let trend  = detect_degradation_trend(conn, target, last_n)?;

    // N'afficher le bloc que s'il y a quelque chose à dire
    if peaks.is_empty() && trend.is_none() {
        return Ok(());
    }

    let sep = "═".repeat(51);
    println!();
    println!("{}", sep.cyan().bold());
    println!("  {}", "Analyse temporelle".bold());
    println!();

    // ── Heures de pointe ──────────────────────────────────────────────────────
    for peak in &peaks {
        let hour_range = format_hour_range(&peak.hours);
        let pct = peak.bad_count as f64 / peak.total_count as f64 * 100.0;

        let icon = if pct >= 80.0 { "✖" } else { "⚠" };
        let color_fn: fn(String) -> ColoredString = if pct >= 80.0 {
            |s| s.red()
        } else {
            |s| s.yellow()
        };

        let header = color_fn(format!(
            "{} Congestion récurrente {} ({}/{} runs dégradés ou faulty)",
            icon, hour_range, peak.bad_count, peak.total_count
        ));
        println!("  {}", header);

        let detail = if pct >= 100.0 {
            format!("100% des runs sur ce créneau sont dégradés")
        } else {
            format!("{:.0}% des runs sur ce créneau sont dégradés — perte moy {:.1}%, RTT moy {:.0} ms",
                pct, peak.avg_loss, peak.avg_rtt)
        };
        println!("    → {}", detail.dimmed());
        println!();
    }

    // ── Tendance ──────────────────────────────────────────────────────────────
    if let Some(t) = &trend {
        let degrading = t.rtt_delta_ms > 0.0 || t.loss_delta_pct > 0.0 || t.dl_delta_rel_pct < 0.0;
        let icon  = if degrading { "⚠" } else { "ℹ" };
        let label = if degrading { "Tendance à la dégradation" } else { "Tendance à l'amélioration" };

        let header = format!(
            "{} {} ({} runs sur {:.1} jours)",
            icon, label, t.last_n, t.span_days
        );
        let header_colored: ColoredString = if degrading {
            header.yellow()
        } else {
            header.green()
        };
        println!("  {}", header_colored);

        if t.rtt_delta_ms.abs() > 1.0 {
            let sign = if t.rtt_delta_ms > 0.0 { "+" } else { "" };
            println!("    → RTT moyen {}{:.0} ms total", sign, t.rtt_delta_ms);
        }
        if t.loss_delta_pct.abs() > 0.05 {
            let sign = if t.loss_delta_pct > 0.0 { "+" } else { "" };
            println!("    → Perte {}{:.1}% total", sign, t.loss_delta_pct);
        }
        if t.dl_delta_rel_pct.abs() > 1.0 {
            let sign = if t.dl_delta_rel_pct > 0.0 { "+" } else { "" };
            println!("    → Débit {}{:.0}% relatif", sign, t.dl_delta_rel_pct);
        }
        println!();
    }

    println!("{}", sep.cyan().bold());
    Ok(())
}

// ─── Formatage des plages horaires ────────────────────────────────────────────

fn format_hour_range(hours: &[u8]) -> String {
    if hours.is_empty() {
        return String::new();
    }
    if hours.len() == 1 {
        return format!("{:02}h UTC", hours[0]);
    }
    // Les heures sont déjà triées (ordre de parcours consécutif)
    let first = *hours.first().unwrap();
    let last  = *hours.last().unwrap();
    format!("{:02}h–{:02}h UTC", first, (last + 1) % 24)
}
