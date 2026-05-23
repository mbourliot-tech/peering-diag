//! Vue historique des diagnostics stockés en base SQLite.
//!
//! Trois modes :
//!   - Chronologique (défaut) : liste des runs avec verdict + métriques clés
//!   - Par heure (`--by-hour`) : pattern heures de pointe (0–23)
//!   - Par hop (`--hop IP|ASN`) : évolution temporelle d'un hop précis

use anyhow::Result;
use colored::*;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use rusqlite::{params, Connection};

// ─── Structures internes ──────────────────────────────────────────────────────

struct RunRow {
    run_id: i64,
    timestamp: String,
    target: String,
    verdict: String,      // "Healthy" | "Degraded" | "Faulty"
    finding: String,      // description du finding principal
    max_loss_aller: f64,
    max_loss_retour: f64,
    avg_rtt_ms: f64,
    dl_mbps: f64,
    hour: u8,             // heure extraite du timestamp
}

struct HopRow {
    timestamp: String,
    asn: Option<i64>,
    as_name: Option<String>,
    loss_pct: f64,
    avg_ms: f64,
    max_ms: f64,
    stdev_ms: f64,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn verdict_icon(status: &str) -> &'static str {
    match status {
        "Healthy"  => "✔ SAIN    ",
        "Degraded" => "⚠ DÉGRADÉ ",
        "Faulty"   => "✖ FAULTY  ",
        _          => "? ?       ",
    }
}

fn verdict_color(status: &str, s: String) -> ColoredString {
    match status {
        "Healthy"  => s.green(),
        "Degraded" => s.yellow(),
        "Faulty"   => s.red(),
        _          => s.normal(),
    }
}

fn verdict_score(status: &str) -> f64 {
    match status { "Faulty" => 2.0, "Degraded" => 1.0, _ => 0.0 }
}

fn score_to_status(score: f64) -> &'static str {
    if score >= 1.5 { "Faulty" } else if score >= 0.5 { "Degraded" } else { "Healthy" }
}

fn score_to_label(score: f64) -> &'static str {
    if score >= 1.5 { "FAULTY  " } else if score >= 0.5 { "DÉGRADÉ " } else { "SAIN    " }
}

fn bar(n: usize, max: usize) -> String {
    if max == 0 { return String::new(); }
    "█".repeat((n * 4 / max).min(4))
}

/// Extrait verdict + finding principal du JSON sérialisé du rapport.
fn extract_from_json(payload_json: &str) -> (String, String) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload_json) else {
        return ("?".into(), "—".into());
    };
    let verdict = v["verdict"]["status"].as_str().unwrap_or("?").to_string();
    let finding = v["findings"]
        .as_array()
        .and_then(|fs| {
            fs.iter()
                .find(|f| {
                    let sev = f["severity"].as_str().unwrap_or("");
                    sev == "Critical" || sev == "Warning"
                })
                .and_then(|f| f["description"].as_str())
        })
        .unwrap_or("—")
        .to_string();
    (verdict, finding)
}

/// Extrait l'heure UTC (0–23) depuis un timestamp RFC3339.
fn hour_from_ts(ts: &str) -> u8 {
    ts.find('T')
        .and_then(|i| ts.get(i + 1..i + 3))
        .and_then(|h| h.parse::<u8>().ok())
        .unwrap_or(0)
}

/// Formate un timestamp RFC3339 en "YYYY-MM-DD HH:MM".
fn fmt_ts(ts: &str) -> String {
    if ts.len() >= 16 { ts[..16].replace('T', " ") } else { ts.to_string() }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max { format!("{}…", &s[..max.saturating_sub(1)]) } else { s.to_string() }
}

// ─── Requête commune : liste des runs ─────────────────────────────────────────

fn fetch_runs(
    conn: &Connection,
    target: Option<&str>,
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<RunRow>> {
    // Sous-requêtes corrélées pour éviter la multiplication des lignes
    // due aux JOINs avec hop_samples / speedtest_samples.
    let sql = "
        SELECT
            r.id,
            r.timestamp,
            r.target,
            r.payload_json,
            COALESCE((
                SELECT MAX(h.loss_pct)
                FROM hop_samples h
                WHERE h.report_id = r.id
                  AND h.suspected_ratelimit = 0
                  AND h.ip IS NOT NULL
            ), 0.0) AS max_loss_aller,
            COALESCE((
                SELECT MAX(rh.loss_pct)
                FROM return_hop_samples rh
                WHERE rh.report_id = r.id
                  AND rh.ip IS NOT NULL
            ), 0.0) AS max_loss_retour,
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
          AND (?2 IS NULL OR r.timestamp >= ?2)
        ORDER BY r.timestamp DESC
        LIMIT ?3
    ";

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        params![target, since, limit as i64],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, f64>(7)?,
            ))
        },
    )?;

    let mut result = Vec::new();
    for row in rows {
        let (run_id, timestamp, target_str, payload_json, loss_aller, loss_retour, avg_rtt, dl) = row?;
        let (verdict, finding) = extract_from_json(&payload_json);
        let hour = hour_from_ts(&timestamp);
        result.push(RunRow {
            run_id,
            timestamp,
            target: target_str,
            verdict,
            finding,
            max_loss_aller:  loss_aller,
            max_loss_retour: loss_retour,
            avg_rtt_ms: avg_rtt,
            dl_mbps: dl,
            hour,
        });
    }
    Ok(result)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── hour_from_ts ──────────────────────────────────────────────────────────

    #[test]
    fn test_hour_from_ts_standard() {
        assert_eq!(hour_from_ts("2024-01-15T14:30:00Z"), 14);
        assert_eq!(hour_from_ts("2024-01-15T00:00:00Z"), 0);
        assert_eq!(hour_from_ts("2024-01-15T23:59:59Z"), 23);
    }

    #[test]
    fn test_hour_from_ts_malformed_returns_zero() {
        assert_eq!(hour_from_ts(""), 0);
        assert_eq!(hour_from_ts("no-separator"), 0);
        assert_eq!(hour_from_ts("2024-01-15Txx:00:00Z"), 0);
    }

    // ── extract_from_json ─────────────────────────────────────────────────────

    #[test]
    fn test_extract_healthy_no_findings() {
        let json = r#"{"verdict":{"status":"Healthy"},"findings":[]}"#;
        let (verdict, finding) = extract_from_json(json);
        assert_eq!(verdict, "Healthy");
        assert_eq!(finding, "—");
    }

    #[test]
    fn test_extract_faulty_with_critical_finding() {
        let json = r#"{
            "verdict": {"status": "Faulty"},
            "findings": [
                {"severity": "Info", "description": "info"},
                {"severity": "Critical", "description": "Haute perte de paquets"}
            ]
        }"#;
        let (verdict, finding) = extract_from_json(json);
        assert_eq!(verdict, "Faulty");
        assert_eq!(finding, "Haute perte de paquets");
    }

    #[test]
    fn test_extract_picks_first_critical_or_warning() {
        let json = r#"{
            "verdict": {"status": "Degraded"},
            "findings": [
                {"severity": "Warning", "description": "Premier warning"},
                {"severity": "Critical", "description": "Critical après"}
            ]
        }"#;
        let (verdict, finding) = extract_from_json(json);
        assert_eq!(verdict, "Degraded");
        // find() retourne le premier match (Warning ici, avant Critical)
        assert_eq!(finding, "Premier warning");
    }

    #[test]
    fn test_extract_ignores_info_findings() {
        let json = r#"{
            "verdict": {"status": "Healthy"},
            "findings": [{"severity": "Info", "description": "info only"}]
        }"#;
        let (_, finding) = extract_from_json(json);
        assert_eq!(finding, "—");
    }

    #[test]
    fn test_extract_invalid_json_returns_defaults() {
        let (verdict, finding) = extract_from_json("not json at all");
        assert_eq!(verdict, "?");
        assert_eq!(finding, "—");
    }
}

// ─── Vue chronologique ────────────────────────────────────────────────────────

/// Affiche la liste des N derniers runs avec leurs métriques clés.
pub fn show_chronological(
    conn: &Connection,
    target: Option<&str>,
    last_n: usize,
    since: Option<&str>,
) -> Result<()> {
    let runs = fetch_runs(conn, target, since, last_n)?;

    if runs.is_empty() {
        println!("{}", "Aucun run trouvé dans cette base.".yellow());
        return Ok(());
    }

    let first_ts = runs.last().map(|r| fmt_ts(&r.timestamp)).unwrap_or_default();
    let last_ts  = runs.first().map(|r| fmt_ts(&r.timestamp)).unwrap_or_default();
    let tgt      = runs.first().map(|r| r.target.as_str()).unwrap_or("?");

    println!(
        "Cible : {} — {} run(s) entre {} et {}",
        tgt.bold(),
        runs.len(),
        first_ts.dimmed(),
        last_ts.dimmed(),
    );
    println!();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "#ID", "Timestamp", "Verdict", "↑ Aller%", "↓ Retour%", "RTT moy", "DL Mbps", "Finding principal",
    ]);

    // Affichage en ordre chronologique (plus ancien → plus récent)
    for run in runs.iter().rev() {
        let ts = fmt_ts(&run.timestamp);

        let verdict_str = verdict_icon(&run.verdict);
        let verdict_cell = match run.verdict.as_str() {
            "Healthy"  => Cell::new(verdict_str).fg(Color::Green),
            "Degraded" => Cell::new(verdict_str).fg(Color::Yellow),
            "Faulty"   => Cell::new(verdict_str).fg(Color::Red),
            _          => Cell::new(verdict_str),
        };

        let loss_aller_cell = if run.max_loss_aller > 1.0 {
            Cell::new(format!("{:.1}", run.max_loss_aller)).fg(Color::Red)
        } else if run.max_loss_aller > 0.0 {
            Cell::new(format!("{:.1}", run.max_loss_aller)).fg(Color::Yellow)
        } else {
            Cell::new("0.0").fg(Color::Green)
        };

        let loss_retour_cell = if run.max_loss_retour > 1.0 {
            Cell::new(format!("{:.1}", run.max_loss_retour)).fg(Color::Red)
        } else if run.max_loss_retour > 0.0 {
            Cell::new(format!("{:.1}", run.max_loss_retour)).fg(Color::Yellow)
        } else {
            Cell::new("0.0").fg(Color::Green)
        };

        let rtt_cell = if run.avg_rtt_ms > 150.0 {
            Cell::new(format!("{:.0} ms", run.avg_rtt_ms)).fg(Color::Red)
        } else if run.avg_rtt_ms > 50.0 {
            Cell::new(format!("{:.0} ms", run.avg_rtt_ms)).fg(Color::Yellow)
        } else {
            Cell::new(format!("{:.0} ms", run.avg_rtt_ms))
        };

        let dl_str = if run.dl_mbps > 0.0 { format!("{:.0}", run.dl_mbps) } else { "—".into() };
        let finding_short = truncate(&run.finding, 42);

        table.add_row(vec![
            Cell::new(run.run_id),
            Cell::new(&ts),
            verdict_cell,
            loss_aller_cell,
            loss_retour_cell,
            rtt_cell,
            Cell::new(&dl_str),
            Cell::new(&finding_short),
        ]);
    }

    println!("{table}");
    Ok(())
}

// ─── Vue par heure ────────────────────────────────────────────────────────────

/// Agrège les runs par heure de la journée (UTC) pour détecter les pics de congestion.
pub fn show_by_hour(conn: &Connection, target: Option<&str>) -> Result<()> {
    let runs = fetch_runs(conn, target, None, 100_000)?;

    if runs.is_empty() {
        println!("{}", "Aucun run trouvé dans cette base.".yellow());
        return Ok(());
    }

    let tgt = runs.first().map(|r| r.target.as_str()).unwrap_or("?");
    println!(
        "Pattern heures de pointe — {} ({} runs, heures UTC)",
        tgt.bold(),
        runs.len()
    );
    println!();

    // Groupement par heure
    let mut by_hour: Vec<Vec<&RunRow>> = (0..24).map(|_| Vec::new()).collect();
    for run in &runs {
        by_hour[run.hour as usize].push(run);
    }

    let max_samples = by_hour.iter().map(|v| v.len()).max().unwrap_or(1);

    println!(
        "{:<6} {:<14} {:>7} {:>9} {:>10} {:>7}",
        "Heure", "Verdict moy", "Perte%", "RTT moy", "DL moy", "Runs"
    );
    println!("{}", "─".repeat(60).dimmed());

    for (hour, samples) in by_hour.iter().enumerate() {
        if samples.is_empty() { continue; }

        let n = samples.len();
        let avg_loss  = samples.iter().map(|r| r.max_loss_aller.max(r.max_loss_retour)).sum::<f64>() / n as f64;
        let avg_rtt   = samples.iter().map(|r| r.avg_rtt_ms).sum::<f64>()   / n as f64;
        let avg_dl    = samples.iter().map(|r| r.dl_mbps).sum::<f64>()      / n as f64;
        let avg_score = samples.iter().map(|r| verdict_score(&r.verdict)).sum::<f64>() / n as f64;

        let status = score_to_status(avg_score);
        let label  = score_to_label(avg_score);
        let b      = bar(n, max_samples);

        let bad = samples.iter().filter(|r| r.verdict != "Healthy").count();
        let peak = if bad == n && n > 1 { "  ← pic" } else { "" };

        let dl_str = if avg_dl > 0.0 { format!("{:.0} Mbps", avg_dl) } else { "—".into() };

        let line = format!(
            "{:02}h    {} {:<4}  {:>6.1}%  {:>7.0} ms  {:>10}  {:>4}{}",
            hour, label, b, avg_loss, avg_rtt, dl_str, n, peak
        );
        println!("{}", verdict_color(status, line));
    }
    Ok(())
}

// ─── Vue par hop ──────────────────────────────────────────────────────────────

/// Affiche l'évolution temporelle d'un hop identifié par son IP ou son ASN.
pub fn show_hop(
    conn: &Connection,
    target: Option<&str>,
    hop_filter: &str,
    last_n: usize,
) -> Result<()> {
    // Parsing du filtre : "AS1299" ou "1299" → ASN ; sinon → IP
    let upper = hop_filter.to_uppercase();
    let filter_asn: Option<i64> = if let Some(stripped) = upper.strip_prefix("AS") {
        stripped.parse::<i64>().ok()
    } else {
        hop_filter.parse::<i64>().ok()
    };
    let filter_ip: Option<&str> = if filter_asn.is_none() { Some(hop_filter) } else { None };

    let forward_rows = query_hop_aller(conn, target, filter_asn, filter_ip, last_n)?;
    let return_rows  = query_hop_retour(conn, target, filter_asn, filter_ip, last_n)?;

    if forward_rows.is_empty() && return_rows.is_empty() {
        println!(
            "{}",
            format!("Aucun hop correspondant à '{}' dans la base.", hop_filter).yellow()
        );
        return Ok(());
    }

    let hop_label = match (filter_asn, filter_ip) {
        (Some(asn), _) => format!("AS{}", asn),
        (_, Some(ip))  => ip.to_string(),
        _              => hop_filter.to_string(),
    };
    println!(
        "Évolution du hop {} — {}",
        hop_label.bold().cyan(),
        target.unwrap_or("toutes cibles").bold()
    );
    println!();

    if !forward_rows.is_empty() {
        println!("{}", "Chemin aller :".bold().underline());
        print_hop_table(&forward_rows);
        println!();
    }
    if !return_rows.is_empty() {
        println!("{}", "Chemin retour (Globalping) :".bold().underline());
        print_hop_table(&return_rows);
        println!();
    }
    Ok(())
}

fn query_hop_aller(
    conn: &Connection,
    target: Option<&str>,
    filter_asn: Option<i64>,
    filter_ip: Option<&str>,
    limit: usize,
) -> Result<Vec<HopRow>> {
    let sql = "
        SELECT r.timestamp, h.asn, h.as_name, h.loss_pct,
               COALESCE(h.avg_rtt_ms, 0.0), COALESCE(h.max_rtt_ms, 0.0),
               COALESCE(h.jitter_ms, 0.0)
        FROM reports r
        INNER JOIN hop_samples h ON h.report_id = r.id
        WHERE (?1 IS NULL OR r.target = ?1)
          AND (?2 IS NULL OR h.asn = ?2)
          AND (?3 IS NULL OR h.ip  = ?3)
        ORDER BY r.timestamp DESC
        LIMIT ?4
    ";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        params![target, filter_asn, filter_ip, limit as i64],
        row_to_hop,
    )?;
    let mut v: Vec<HopRow> = rows.collect::<rusqlite::Result<_>>()?;
    v.reverse();
    Ok(v)
}

fn query_hop_retour(
    conn: &Connection,
    target: Option<&str>,
    filter_asn: Option<i64>,
    filter_ip: Option<&str>,
    limit: usize,
) -> Result<Vec<HopRow>> {
    let sql = "
        SELECT r.timestamp, rh.asn, rh.as_name, rh.loss_pct,
               COALESCE(rh.avg_ms, 0.0), COALESCE(rh.max_ms, 0.0),
               COALESCE(rh.stdev_ms, 0.0)
        FROM reports r
        INNER JOIN return_hop_samples rh ON rh.report_id = r.id
        WHERE (?1 IS NULL OR r.target = ?1)
          AND (?2 IS NULL OR rh.asn = ?2)
          AND (?3 IS NULL OR rh.ip  = ?3)
        ORDER BY r.timestamp DESC
        LIMIT ?4
    ";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        params![target, filter_asn, filter_ip, limit as i64],
        row_to_hop,
    )?;
    let mut v: Vec<HopRow> = rows.collect::<rusqlite::Result<_>>()?;
    v.reverse();
    Ok(v)
}

fn row_to_hop(row: &rusqlite::Row<'_>) -> rusqlite::Result<HopRow> {
    Ok(HopRow {
        timestamp: row.get(0)?,
        asn:       row.get(1)?,
        as_name:   row.get(2)?,
        loss_pct:  row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
        avg_ms:    row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
        max_ms:    row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
        stdev_ms:  row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
    })
}

fn print_hop_table(rows: &[HopRow]) {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Timestamp", "ASN", "Opérateur", "Perte%", "RTT moy", "RTT max", "StDev",
    ]);

    for row in rows {
        let ts      = fmt_ts(&row.timestamp);
        let asn_str = row.asn.map(|a| format!("AS{}", a)).unwrap_or_default();
        let name    = truncate(row.as_name.as_deref().unwrap_or("—"), 24);

        let degraded = row.loss_pct > 1.0 || row.avg_ms > 150.0;
        let ts_cell  = if degraded { Cell::new(&ts).fg(Color::Red) } else { Cell::new(&ts) };

        let loss_cell = if row.loss_pct > 1.0 {
            Cell::new(format!("{:.1}%", row.loss_pct)).fg(Color::Red)
        } else if row.loss_pct > 0.0 {
            Cell::new(format!("{:.1}%", row.loss_pct)).fg(Color::Yellow)
        } else {
            Cell::new("0.0%").fg(Color::Green)
        };

        let avg_cell = if row.avg_ms > 150.0 {
            Cell::new(format!("{:.1}", row.avg_ms)).fg(Color::Red)
        } else if row.avg_ms > 50.0 {
            Cell::new(format!("{:.1}", row.avg_ms)).fg(Color::Yellow)
        } else {
            Cell::new(format!("{:.1}", row.avg_ms))
        };

        table.add_row(vec![
            ts_cell,
            Cell::new(&asn_str),
            Cell::new(&name),
            loss_cell,
            avg_cell,
            Cell::new(format!("{:.1}", row.max_ms)),
            Cell::new(format!("{:.1}", row.stdev_ms)),
        ]);
    }
    println!("{table}");
}

// ─── Vue détail d'un run ──────────────────────────────────────────────────────

/// Affiche le détail hop par hop d'un run spécifique (chemin aller + retour).
pub fn show_run_detail(conn: &Connection, run_id: i64) -> Result<()> {
    // Vérifier que le run existe
    let (timestamp, target): (String, String) = conn
        .query_row(
            "SELECT timestamp, target FROM reports WHERE id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| anyhow::anyhow!("Run #{} introuvable dans la base.", run_id))?;

    println!(
        "Détail du run #{} — {} — {}",
        run_id,
        target.bold(),
        fmt_ts(&timestamp).dimmed(),
    );
    println!();

    // ── Chemin aller ─────────────────────────────────────────────────────────
    let sql_aller = "
        SELECT ttl, ip, asn, as_name,
               COALESCE(loss_pct, -1.0),
               COALESCE(avg_rtt_ms, 0.0),
               COALESCE(min_rtt_ms, 0.0),
               COALESCE(max_rtt_ms, 0.0),
               COALESCE(jitter_ms, 0.0),
               suspected_ratelimit
        FROM hop_samples
        WHERE report_id = ?1
        ORDER BY ttl
    ";

    let mut stmt = conn.prepare(sql_aller)?;
    struct AllerHop {
        ttl: i64, ip: Option<String>, asn: Option<i64>, as_name: Option<String>,
        loss: f64, avg: f64, min: f64, max: f64, jitter: f64, ratelimit: bool,
    }
    let aller_hops: Vec<AllerHop> = stmt.query_map(params![run_id], |row| {
        Ok(AllerHop {
            ttl:       row.get(0)?,
            ip:        row.get(1)?,
            asn:       row.get(2)?,
            as_name:   row.get(3)?,
            loss:      row.get(4)?,
            avg:       row.get(5)?,
            min:       row.get(6)?,
            max:       row.get(7)?,
            jitter:    row.get(8)?,
            ratelimit: row.get::<_, i32>(9)? != 0,
        })
    })?.collect::<rusqlite::Result<_>>()?;

    if !aller_hops.is_empty() {
        println!("{}", "Chemin aller :".bold().underline());
        let mut table = Table::new();
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["TTL", "IP", "ASN", "Opérateur", "Perte%", "RTT moy", "RTT min", "RTT max", "Jitter"]);

        for h in &aller_hops {
            let ip_str   = h.ip.as_deref().unwrap_or("*");
            let asn_str  = h.asn.map(|a| format!("AS{}", a)).unwrap_or_default();
            let name_str = truncate(h.as_name.as_deref().unwrap_or("—"), 22);

            let (loss_cell, ttl_cell) = if h.ratelimit {
                (Cell::new("(rate-lim)").fg(Color::DarkGrey), Cell::new(h.ttl).fg(Color::DarkGrey))
            } else if h.loss < 0.0 {
                // NULL stocké → pas de mesure
                (Cell::new("—"), Cell::new(h.ttl))
            } else if h.loss > 1.0 {
                (Cell::new(format!("{:.1}%", h.loss)).fg(Color::Red), Cell::new(h.ttl).fg(Color::Red))
            } else if h.loss > 0.0 {
                (Cell::new(format!("{:.1}%", h.loss)).fg(Color::Yellow), Cell::new(h.ttl))
            } else {
                (Cell::new("0.0%").fg(Color::Green), Cell::new(h.ttl))
            };

            let avg_cell = if h.avg > 150.0 {
                Cell::new(format!("{:.1}", h.avg)).fg(Color::Red)
            } else if h.avg > 50.0 {
                Cell::new(format!("{:.1}", h.avg)).fg(Color::Yellow)
            } else {
                Cell::new(format!("{:.1}", h.avg))
            };

            table.add_row(vec![
                ttl_cell,
                Cell::new(ip_str),
                Cell::new(&asn_str),
                Cell::new(&name_str),
                loss_cell,
                avg_cell,
                Cell::new(format!("{:.1}", h.min)),
                Cell::new(format!("{:.1}", h.max)),
                Cell::new(format!("{:.1}", h.jitter)),
            ]);
        }
        println!("{table}");
        println!();
    }

    // ── Chemin retour (Globalping) ────────────────────────────────────────────
    let sql_retour = "
        SELECT ttl, ip, asn, as_name,
               COALESCE(loss_pct, 0.0),
               COALESCE(avg_ms, 0.0),
               COALESCE(min_ms, 0.0),
               COALESCE(max_ms, 0.0),
               COALESCE(stdev_ms, 0.0)
        FROM return_hop_samples
        WHERE report_id = ?1
        ORDER BY ttl
    ";

    let mut stmt2 = conn.prepare(sql_retour)?;
    struct RetourHop {
        ttl: i64, ip: Option<String>, asn: Option<i64>, as_name: Option<String>,
        loss: f64, avg: f64, min: f64, max: f64, stdev: f64,
    }
    let retour_hops: Vec<RetourHop> = stmt2.query_map(params![run_id], |row| {
        Ok(RetourHop {
            ttl:     row.get(0)?,
            ip:      row.get(1)?,
            asn:     row.get(2)?,
            as_name: row.get(3)?,
            loss:    row.get(4)?,
            avg:     row.get(5)?,
            min:     row.get(6)?,
            max:     row.get(7)?,
            stdev:   row.get(8)?,
        })
    })?.collect::<rusqlite::Result<_>>()?;

    if !retour_hops.is_empty() {
        println!("{}", "Chemin retour (Globalping) :".bold().underline());
        let mut table = Table::new();
        table.set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["TTL", "IP", "ASN", "Opérateur", "Perte%", "RTT moy", "RTT min", "RTT max", "StDev"]);

        for h in &retour_hops {
            let ip_str   = h.ip.as_deref().unwrap_or("*");
            let asn_str  = h.asn.map(|a| format!("AS{}", a)).unwrap_or_default();
            let name_str = truncate(h.as_name.as_deref().unwrap_or("—"), 22);

            let loss_cell = if h.loss > 1.0 {
                Cell::new(format!("{:.1}%", h.loss)).fg(Color::Red)
            } else if h.loss > 0.0 {
                Cell::new(format!("{:.1}%", h.loss)).fg(Color::Yellow)
            } else {
                Cell::new("0.0%").fg(Color::Green)
            };

            let avg_cell = if h.avg > 150.0 {
                Cell::new(format!("{:.1}", h.avg)).fg(Color::Red)
            } else if h.avg > 50.0 {
                Cell::new(format!("{:.1}", h.avg)).fg(Color::Yellow)
            } else {
                Cell::new(format!("{:.1}", h.avg))
            };

            table.add_row(vec![
                Cell::new(h.ttl),
                Cell::new(ip_str),
                Cell::new(&asn_str),
                Cell::new(&name_str),
                loss_cell,
                avg_cell,
                Cell::new(format!("{:.1}", h.min)),
                Cell::new(format!("{:.1}", h.max)),
                Cell::new(format!("{:.1}", h.stdev)),
            ]);
        }
        println!("{table}");
    } else {
        println!("{}", "Pas de données de chemin retour pour ce run.".dimmed());
    }

    Ok(())
}
