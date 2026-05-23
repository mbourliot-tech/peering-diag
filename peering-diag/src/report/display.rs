//! Affichage terminal coloré du rapport.

use crate::types::{DiagnosticReport, FindingCategory, Hop, Severity, SpeedtestResult, VerdictStatus};
use colored::*;
use comfy_table::{Cell, Color, ContentArrangement, Table};

pub fn print_report(report: &DiagnosticReport) {
    print_header(report);
    print_mtr_table(&report.hops);
    if !report.speedtests.is_empty() {
        print_speedtest_table(&report.speedtests);
    }
    print_findings(report);
    print_verdict(report);
}

fn print_header(report: &DiagnosticReport) {
    println!();
    println!("{}", "═══════════════════════════════════════════".bright_blue());
    println!(
        "  {} {}",
        "Rapport de diagnostic peering".bold(),
        format!("({})", report.timestamp.format("%Y-%m-%d %H:%M:%S UTC")).dimmed()
    );
    println!("{}", "═══════════════════════════════════════════".bright_blue());
    println!();
    println!("  Cible        : {}", report.target.bright_white());
    println!("  IP           : {}", report.target_ip.to_string().bright_white());
    if let Some(ref a) = report.target_as {
        println!("  AS cible     : {}", a.display().bright_white());
        if let Some(ref c) = a.country {
            println!("  Pays         : {}", c.bright_white());
        }
    }
    println!();
}

fn print_mtr_table(hops: &[Hop]) {
    println!("{}", "MTR (chemin réseau)".bold().underline());
    println!();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "TTL", "IP", "Hostname", "ASN", "AS Name",
        "Loss%", "Min", "Avg", "Max", "Jitter", "Note",
    ]);

    for hop in hops {
        let loss = hop.loss_pct();
        let loss_cell = if hop.suspected_icmp_ratelimit {
            Cell::new(format!("{:.1}*", loss)).fg(Color::DarkGrey)
        } else if loss > 5.0 {
            Cell::new(format!("{:.1}", loss)).fg(Color::Red)
        } else if loss > 0.0 {
            Cell::new(format!("{:.1}", loss)).fg(Color::Yellow)
        } else {
            Cell::new("0.0").fg(Color::Green)
        };

        // Colorier le jitter si élevé
        let jitter_cell = match hop.jitter_ms() {
            Some(j) if j > 50.0 => Cell::new(format!("{:.1}", j)).fg(Color::Red),
            Some(j) if j > 20.0 => Cell::new(format!("{:.1}", j)).fg(Color::Yellow),
            Some(j) => Cell::new(format!("{:.1}", j)),
            None => Cell::new(""),
        };

        let note = if hop.suspected_icmp_ratelimit { "rate-limit" }
            else if hop.ips_seen.len() > 1 { "ECMP" }
            else { "" };

        table.add_row(vec![
            Cell::new(hop.ttl.to_string()),
            Cell::new(hop.primary_ip.map(|ip| ip.to_string()).unwrap_or_else(|| "*".to_string())),
            Cell::new(hop.hostname.as_deref().unwrap_or("")),
            Cell::new(hop.as_info.as_ref().map(|a| a.asn.to_string()).unwrap_or_default()),
            Cell::new(hop.as_info.as_ref().map(|a| a.name.clone()).unwrap_or_default()),
            loss_cell,
            Cell::new(hop.min_rtt_ms().map(|v| format!("{:.1}", v)).unwrap_or_default()),
            Cell::new(hop.avg_rtt_ms().map(|v| format!("{:.1}", v)).unwrap_or_default()),
            Cell::new(hop.max_rtt_ms().map(|v| format!("{:.1}", v)).unwrap_or_default()),
            jitter_cell,
            Cell::new(note).fg(Color::DarkGrey),
        ]);
    }
    println!("{table}");
    println!("{}", "  * = perte apparente due à l'ICMP rate-limiting, pas une vraie perte".dimmed());
    println!();
}

fn print_speedtest_table(results: &[SpeedtestResult]) {
    println!("{}", "Débit par AS du chemin".bold().underline());
    println!();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["AS", "AS Name", "Endpoint", "DL Mbps", "UL Mbps", "Ping ms", "Δ DL", "Méthode"]);

    let mut prev_dl: Option<f64> = None;
    for r in results {
        // Skip les doublons de serveur (proxy utilisé plusieurs fois)
        let is_proxy = r.method.as_deref() == Some("proxy");

        let (delta_str, delta_color) = match prev_dl {
            Some(p) => {
                // Ne pas afficher de delta si même serveur proxy (non significatif)
                if is_proxy {
                    ("—".to_string(), None)
                } else {
                    let d = r.download_mbps - p;
                    let s = if d.abs() < 1.0 { "≈".to_string() } else { format!("{:+.1}", d) };
                    let color = if d < -50.0 { Some(Color::Red) }
                        else if d < -10.0 { Some(Color::Yellow) }
                        else { None };
                    (s, color)
                }
            }
            None => ("—".to_string(), None),
        };

        let delta_cell = match delta_color {
            Some(c) => Cell::new(&delta_str).fg(c),
            None => Cell::new(&delta_str),
        };

        // Colorier la méthode selon sa fiabilité
        let method_str = r.method.as_deref().unwrap_or("?");
        let method_cell = match method_str {
            "direct"   => Cell::new(method_str).fg(Color::Green),
            "tier1-db" => Cell::new(method_str).fg(Color::Cyan),
            "iperf3"   => Cell::new(method_str).fg(Color::Cyan),
            "http"     => Cell::new(method_str).fg(Color::Yellow),
            "géo"      => Cell::new(method_str).fg(Color::Yellow),
            "proxy"    => Cell::new(method_str).fg(Color::DarkGrey),
            _          => Cell::new(method_str),
        };

        // Afficher l'endpoint label si disponible, sinon le nom du serveur
        let endpoint = r.endpoint_label.as_deref().unwrap_or(&r.server_name);

        table.add_row(vec![
            Cell::new(r.asn.map(|a| a.to_string()).unwrap_or_default()),
            Cell::new(r.as_name.as_deref().unwrap_or("")),
            Cell::new(endpoint),
            Cell::new(format!("{:.1}", r.download_mbps)),
            Cell::new(if r.upload_mbps > 0.0 { format!("{:.1}", r.upload_mbps) } else { "—".to_string() }),
            Cell::new(if r.ping_ms > 0.0 { format!("{:.1}", r.ping_ms) } else { "—".to_string() }),
            delta_cell,
            method_cell,
        ]);
        prev_dl = Some(r.download_mbps);
    }
    println!("{table}");
    println!();
}

fn print_findings(report: &DiagnosticReport) {
    // Séparer les findings par sévérité, ignorer les purs Info (rate-limit, latence physique)
    // sauf si ce sont les seuls findings
    let actionable: Vec<_> = report.findings.iter()
        .filter(|f| f.severity != Severity::Info || f.action.is_some())
        .collect();

    let info_only: Vec<_> = report.findings.iter()
        .filter(|f| f.severity == Severity::Info)
        .collect();

    if actionable.is_empty() && info_only.is_empty() {
        return;
    }

    println!("{}", "Analyse".bold().underline());
    println!();

    // Afficher Critical et Warning en premier
    for finding in report.findings.iter().filter(|f| f.severity != Severity::Info) {
        let (icon, color) = match finding.severity {
            Severity::Critical => ("✖", "red"),
            Severity::Warning  => ("⚠", "yellow"),
            Severity::Info     => ("ℹ", "blue"),
        };

        let category_label = category_label(finding.category);
        let header = format!("{} [{}] {}", icon, category_label, finding.description);
        let colored = match color {
            "red"    => header.red().bold(),
            "yellow" => header.yellow().bold(),
            _        => header.blue().bold(),
        };
        println!("  {}", colored);
        println!("    {}", finding.evidence.dimmed());
        if let Some(ref action) = finding.action {
            println!("    {} {}", "→".bright_cyan(), action.bright_cyan());
        }
        println!();
    }

    // Findings Info en bas, regroupés, sans action (juste contexte)
    let info_no_action: Vec<_> = report.findings.iter()
        .filter(|f| f.severity == Severity::Info && f.action.is_none())
        .collect();

    if !info_no_action.is_empty() {
        println!("{}", "Contexte".bold().dimmed());
        for finding in &info_no_action {
            println!("  {} {}", "ℹ".blue(), finding.description.dimmed());
            println!("    {}", finding.evidence.dimmed());
        }
        println!();
    }
}

fn print_verdict(report: &DiagnosticReport) {
    println!("{}", "═══════════════════════════════════════════".bright_blue());
    let (icon, label, color) = match report.verdict.status {
        VerdictStatus::Healthy  => ("✔", "CHEMIN SAIN", "green"),
        VerdictStatus::Degraded => ("⚠", "DÉGRADÉ", "yellow"),
        VerdictStatus::Faulty   => ("✖", "PROBLÈME DÉTECTÉ", "red"),
    };

    let verdict_line = format!("  {} {}", icon, label);
    let colored = match color {
        "green"  => verdict_line.green().bold(),
        "yellow" => verdict_line.yellow().bold(),
        _        => verdict_line.red().bold(),
    };
    println!("{}", colored);
    println!();
    // Wrap le summary à 70 chars
    for line in wrap_text(&report.verdict.summary, 70) {
        println!("  {}", line);
    }
    println!("{}", "═══════════════════════════════════════════".bright_blue());
    println!();
}

fn category_label(cat: FindingCategory) -> &'static str {
    match cat {
        FindingCategory::PacketLoss       => "PERTE",
        FindingCategory::HighLatency      => "LATENCE",
        FindingCategory::Jitter           => "JITTER",
        FindingCategory::Bufferbloat      => "BUFFERBLOAT",
        FindingCategory::RoutingAnomaly   => "ROUTAGE",
        FindingCategory::PeeringCongestion=> "PEERING",
        FindingCategory::LocalIssue       => "LOCAL",
        FindingCategory::IcmpRateLimit    => "INFO",
        FindingCategory::PhysicalLatency  => "INFO",
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > width && !current.is_empty() {
            lines.push(current.clone());
            current.clear();
        }
        if !current.is_empty() { current.push(' '); }
        current.push_str(word);
    }
    if !current.is_empty() { lines.push(current); }
    lines
}
