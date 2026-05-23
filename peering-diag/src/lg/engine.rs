//! Orchestration du diagnostic Looking Glass.
//!
//! Points d'entrée publics :
//!   - `run_retour`  : chemin retour seul (Globalping MTR-style)
//!   - `run_lg`      : chemin retour + URLs Looking Glass manuelles

use crate::{
    asn::AsnResolver,
    lg::{
        analyzer::analyze_return,
        db::{self, QueryMethod},
        globalping::{GlobalpingClient, MtrHop},
        query::{query_lg_http, TraceHop},
    },
    mtr::{Mtr, MtrConfig},
    types::{Finding, FindingCategory, Hop, Severity, VerdictStatus},
};
use crate::report::storage::{init_db, store_return_hops};
use anyhow::Result;
use colored::*;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use futures::stream::{self, StreamExt};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

// ─── Points d'entrée publics ─────────────────────────────────────────────────

/// Diagnostic du chemin retour : MTR aller (découverte) + Globalping retour + analyse.
/// Utilisé par la commande `retour` et par `diag` en phase 2.
///
/// `db`    : si `Some((db_path, report_id))`, les hops retour sont persistés.
/// `quiet` : si `true`, supprime les tableaux MTR et n'affiche qu'un résumé compact.
pub async fn run_retour(
    target: &str,
    my_ip: IpAddr,
    db: Option<(PathBuf, i64)>,
    quiet: bool,
) -> Result<()> {
    let (resolver, path_asns, hops) = discover_path(target, my_ip, quiet).await?;
    run_globalping_return(&path_asns, &hops, my_ip, resolver, db, quiet).await
}

/// Diagnostic complet Looking Glass : chemin retour + URLs Looking Glass manuelles.
/// Utilisé par la commande `lg`.
pub async fn run_lg(
    target: &str,
    my_ip: IpAddr,
    db: Option<(PathBuf, i64)>,
    quiet: bool,
) -> Result<()> {
    let (resolver, path_asns, hops) = discover_path(target, my_ip, quiet).await?;
    run_globalping_return(&path_asns, &hops, my_ip, resolver, db, quiet).await?;
    run_lg_manual(&path_asns, my_ip).await
}

// ─── Phase 1 : découverte du chemin aller (partagée) ─────────────────────────

/// MTR 3 rounds pour découvrir les AS du chemin.
/// En mode `quiet`, supprime tout affichage (utilisé par `watch`).
async fn discover_path(
    target: &str,
    my_ip: IpAddr,
    quiet: bool,
) -> Result<(Arc<AsnResolver>, Vec<(u32, String)>, Vec<Hop>)> {
    let target_ip = super::resolve_target(target).await?;

    if !quiet {
        println!(
            "{} {} ({})   IP publique : {}",
            "Cible :".bold(),
            target,
            target_ip.to_string().dimmed(),
            my_ip.to_string().cyan().bold(),
        );
        println!();
        println!("{}", "Découverte du chemin aller (MTR 3 rounds)…".dimmed());
    }

    let asn_resolver = Arc::new(AsnResolver::new());
    let mtr = Mtr::new(
        MtrConfig {
            target: target_ip,
            rounds: 3,
            max_hops: 30,
            probes_per_round: 1,
            ..Default::default()
        },
        asn_resolver.clone(),
    );
    let hops = mtr.run().await?;

    let mut path_asns: Vec<(u32, String)> = Vec::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for hop in &hops {
        if let Some(ref info) = hop.as_info {
            if seen.insert(info.asn) {
                path_asns.push((info.asn, info.name.clone()));
            }
        }
    }

    if !quiet {
        println!("{:<4} {:<50} {:<8} {}", "Hop", "Hôte", "ASN", "Opérateur");
        println!("{}", "─".repeat(90).dimmed());
        for hop in &hops {
            let host = hop.hostname.as_deref().unwrap_or("");
            let addr = hop.primary_ip.map(|a| a.to_string()).unwrap_or_else(|| "*".into());
            let display = if host.is_empty() { addr.clone() } else { format!("{} ({})", host, addr) };
            let (asn_str, name_str) = if let Some(ref info) = hop.as_info {
                (format!("AS{}", info.asn), info.name.chars().take(30).collect::<String>())
            } else {
                ("—".into(), "—".into())
            };
            println!(
                "{:<4} {:<50} {:<8} {}",
                hop.ttl,
                truncate(&display, 50),
                asn_str.cyan().to_string(),
                name_str.dimmed().to_string(),
            );
        }
        println!();

        if path_asns.is_empty() {
            println!("{}", "⚠ Aucun AS public détecté dans le chemin.".yellow());
        } else {
            let path_str = path_asns
                .iter()
                .map(|(asn, name)| format!("AS{} ({})", asn, name))
                .collect::<Vec<_>>()
                .join(" → ");
            println!("Chemin aller : {}", path_str.cyan());
        }
        println!();
    }

    Ok((asn_resolver, path_asns, hops))
}

// ─── Phase 2 : chemin retour Globalping ──────────────────────────────────────

async fn run_globalping_return(
    path_asns: &[(u32, String)],
    hops: &[Hop],
    my_ip: IpAddr,
    asn_resolver: Arc<AsnResolver>,
    db: Option<(PathBuf, i64)>,
    quiet: bool,
) -> Result<()> {
    let dest_asn = path_asns.last().map(|(asn, _)| *asn);
    let target_country = hops
        .iter()
        .rev()
        .find_map(|h| h.as_info.as_ref().and_then(|a| a.country.clone()))
        .unwrap_or_else(|| "US".to_string());
    let city_hint = detect_city_from_hops(hops);

    if !quiet {
        let location_label = city_hint
            .as_deref()
            .map(|c| c.to_string())
            .unwrap_or_else(|| target_country.clone());
        println!(
            "{}",
            "═══════════════════════════════════════════════════".bright_blue()
        );
        println!(
            " {} {}  ← depuis {}",
            "Chemin retour (Globalping — 5 rounds)".bold(),
            my_ip.to_string().cyan().bold(),
            location_label.dimmed(),
        );
        println!(
            "{}",
            "═══════════════════════════════════════════════════".bright_blue()
        );
        println!();
        print!("  Lancement des sondes Globalping (5 rounds)… ");
    }

    let gp = GlobalpingClient::new();
    match gp
        .traceroute_mtr(my_ip, dest_asn, city_hint.as_deref(), &target_country, 5)
        .await
    {
        Ok(mut trace) => {
            if !quiet {
                println!("{}", "✔".green());
                println!(
                    "  {} sonde AS{} · {} · {}",
                    "↳".cyan(),
                    trace.probe.asn,
                    trace.probe.network.dimmed(),
                    trace.probe.city,
                );
                println!();
            }

            // Résolution ASN en parallèle
            let ip_jobs: Vec<(usize, IpAddr)> = trace
                .hops
                .iter()
                .enumerate()
                .filter_map(|(i, h)| h.ip.map(|ip| (i, ip)))
                .collect();

            let lookups: Vec<(usize, _)> = stream::iter(ip_jobs)
                .map(|(i, ip)| {
                    let resolver = asn_resolver.clone();
                    async move { (i, resolver.lookup(ip).await.ok().flatten()) }
                })
                .buffer_unordered(8)
                .collect()
                .await;

            for (i, info) in lookups {
                trace.hops[i].as_info = info;
            }

            let (findings, verdict) = analyze_return(&trace.hops);

            // Persistance des hops retour si --db fourni
            if let Some((ref db_path, report_id)) = db {
                match init_db(db_path) {
                    Ok(conn) => {
                        if let Err(e) = store_return_hops(&conn, report_id, &trace.hops) {
                            eprintln!("  {} Stockage hops retour : {}", "⚠".yellow(), e);
                        }
                    }
                    Err(e) => eprintln!("  {} Ouverture DB pour retour : {}", "⚠".yellow(), e),
                }
            }

            if quiet {
                // Résumé compact (mode watch)
                let avg_rtt = trace
                    .hops
                    .iter()
                    .rev()
                    .find(|h| h.avg_ms > 0.0)
                    .map(|h| format!("{:.0}ms", h.avg_ms))
                    .unwrap_or_else(|| "—".into());
                let icmp_rl_flags: Vec<bool> = (0..trace.hops.len())
                    .map(|i| crate::lg::analyzer::is_suspected_ratelimit_pub(&trace.hops, i))
                    .collect();
                let max_loss = trace.hops.iter().zip(&icmp_rl_flags)
                    .filter(|(_, &rl)| !rl)
                    .map(|(h, _)| h.loss_pct)
                    .fold(0.0f64, f64::max);
                let (icon, label) = match verdict.status {
                    VerdictStatus::Healthy  => ("✔", "SAIN    "),
                    VerdictStatus::Degraded => ("⚠", "DÉGRADÉ "),
                    VerdictStatus::Faulty   => ("✖", "FAULTY  "),
                };
                let line = format!(
                    "   Retour : {} {}  — RTT moy {}, {:.1}% perte",
                    icon, label, avg_rtt, max_loss
                );
                println!("{}", match verdict.status {
                    VerdictStatus::Healthy  => line.green(),
                    VerdictStatus::Degraded => line.yellow(),
                    VerdictStatus::Faulty   => line.red(),
                });
            } else {
                print_mtr_return_table(&trace.hops);
                println!();
                print_return_findings(&findings, &verdict);
            }
        }
        Err(e) => {
            if quiet {
                println!("   Retour : {} ERREUR   — Globalping: {}", "✖".red(), e);
            } else {
                println!("{}", "échec".red());
                println!("  {} {}", "⚠".yellow(), format!("Globalping : {}", e).red());
                println!();
            }
        }
    }
    Ok(())
}

// ─── Phase 3 : URLs Looking Glass manuelles ──────────────────────────────────

async fn run_lg_manual(path_asns: &[(u32, String)], my_ip: IpAddr) -> Result<()> {
    let mut covered: BTreeSet<u32> = BTreeSet::new();
    let mut entries: Vec<(u32, String, Vec<&'static db::LgServer>)> = Vec::new();

    for (asn, name) in path_asns {
        let servers = db::servers_for_asn(*asn);
        if !servers.is_empty() {
            covered.insert(*asn);
            entries.push((*asn, name.clone(), servers));
        }
    }
    if !covered.contains(&6939) {
        let he = db::servers_for_asn(6939);
        if !he.is_empty() {
            entries.push((6939, "Hurricane Electric".to_string(), he));
        }
    }

    if entries.is_empty() {
        println!("{}", "Aucun Looking Glass connu pour les AS de ce chemin.".yellow());
        return Ok(());
    }

    let auto_queries: Vec<(&db::LgServer, IpAddr)> = entries
        .iter()
        .flat_map(|(_, _, servers)| servers.iter().copied())
        .filter(|s| matches!(s.method, QueryMethod::HttpGet { .. }))
        .map(|s| (s, my_ip))
        .collect();

    let http_results: Vec<((u32, &str), Result<Vec<TraceHop>, String>)> =
        stream::iter(auto_queries)
            .map(|(server, ip)| async move {
                let key = (server.asn, server.node);
                let result = query_lg_http(server, ip).await.map_err(|e| e.to_string());
                (key, result)
            })
            .buffer_unordered(4)
            .collect()
            .await;

    println!(
        "{}",
        "═══════════════════════════════════════════════════".bright_blue()
    );
    println!(
        " {} {}",
        "Chemins retour — Looking Glass vers".bold(),
        my_ip.to_string().cyan().bold()
    );
    println!(
        "{}",
        "═══════════════════════════════════════════════════".bright_blue()
    );
    println!();

    for (asn, as_name, servers) in &entries {
        let is_ref = *asn == 6939 && !covered.contains(asn);
        let label = if is_ref {
            format!("AS{} · {} (Tier-1 de référence)", asn, as_name)
        } else {
            format!("AS{} · {}", asn, as_name)
        };
        println!("{}", format!("▶ {}", label).bold().white());

        for server in servers {
            match &server.method {
                QueryMethod::HttpGet { .. } => {
                    let key = (server.asn, server.node);
                    if let Some((_, outcome)) = http_results.iter().find(|(k, _)| *k == key) {
                        match outcome {
                            Ok(hops) => {
                                println!(
                                    "  {} [{}]  {} ({} hops)",
                                    "auto".green(),
                                    server.node,
                                    "traceroute reçu".green(),
                                    hops.iter().filter(|h| h.host.is_some()).count()
                                );
                                print_lg_traceroute(hops);
                            }
                            Err(e) => {
                                println!(
                                    "  {} [{}]  {}",
                                    "auto".dimmed(),
                                    server.node,
                                    format!("échec ({e})").red()
                                );
                                print_manual_url(server, my_ip);
                            }
                        }
                    }
                }
                QueryMethod::Manual { url, note } => {
                    println!("  {} [{}]", "→".cyan(), server.node);
                    println!("  ┌─ URL  → {}", url.cyan());
                    println!("  └─ Note → {}  IP : {}", note, my_ip.to_string().cyan().bold());
                }
            }
            println!();
        }
    }
    Ok(())
}

// ─── Affichage MTR retour ─────────────────────────────────────────────────────

fn print_mtr_return_table(hops: &[MtrHop]) {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Hop", "Hôte", "ASN", "Opérateur", "Perte%", "Snt", "Dernier", "Moy", "Min", "Max",
        "StDev",
    ]);

    for hop in hops {
        let display = match (&hop.host, &hop.ip) {
            (Some(h), _) => truncate(h, 45),
            (None, Some(ip)) => ip.to_string(),
            (None, None) => "*".to_string(),
        };
        let loss_cell = if hop.loss_pct > 50.0 {
            Cell::new(format!("{:.1}%", hop.loss_pct)).fg(Color::Red)
        } else if hop.loss_pct > 0.0 {
            Cell::new(format!("{:.1}%", hop.loss_pct)).fg(Color::Yellow)
        } else {
            Cell::new("0.0%").fg(Color::Green)
        };
        let avg_cell = if hop.avg_ms > 0.0 {
            let val = format!("{:.1}", hop.avg_ms);
            if hop.avg_ms > 150.0 { Cell::new(val).fg(Color::Red) }
            else if hop.avg_ms > 50.0 { Cell::new(val).fg(Color::Yellow) }
            else { Cell::new(val) }
        } else {
            Cell::new("—")
        };
        let na = "—";
        table.add_row(vec![
            Cell::new(hop.ttl.to_string()),
            Cell::new(&display),
            Cell::new(hop.as_info.as_ref().map(|a| format!("AS{}", a.asn)).unwrap_or_default()),
            Cell::new(hop.as_info.as_ref().map(|a| truncate(&a.name, 22)).unwrap_or_default()),
            loss_cell,
            Cell::new(hop.snt.to_string()),
            Cell::new(hop.last_ms.map_or(na.to_string(), |v| format!("{:.1}", v))),
            avg_cell,
            Cell::new(if hop.min_ms > 0.0 { format!("{:.1}", hop.min_ms) } else { na.to_string() }),
            Cell::new(if hop.max_ms > 0.0 { format!("{:.1}", hop.max_ms) } else { na.to_string() }),
            Cell::new(format!("{:.1}", hop.stdev_ms)),
        ]);
    }
    println!("{table}");
}

fn print_return_findings(findings: &[Finding], verdict: &crate::types::Verdict) {
    let non_info: Vec<_> = findings.iter().filter(|f| f.severity != Severity::Info).collect();
    let info_no_action: Vec<_> = findings
        .iter()
        .filter(|f| f.severity == Severity::Info && f.action.is_none())
        .collect();

    if !non_info.is_empty() || !info_no_action.is_empty() {
        println!("{}", "Analyse du chemin retour".bold().underline());
        println!();
    }

    for finding in &non_info {
        let (icon, colored): (&str, ColoredString) = match finding.severity {
            Severity::Critical => ("✖", format!("[PERTE/PEERING/LATENCE] {}", finding.description).red().bold()),
            Severity::Warning  => ("⚠", format!("[{}] {}", category_label(finding.category), finding.description).yellow().bold()),
            Severity::Info     => ("ℹ", format!("[INFO] {}", finding.description).blue().bold()),
        };
        println!("  {} {}", icon, colored);
        println!("    {}", finding.evidence.dimmed());
        if let Some(ref action) = finding.action {
            println!("    {} {}", "→".bright_cyan(), action.bright_cyan());
        }
        println!();
    }

    if !info_no_action.is_empty() {
        println!("{}", "Contexte".bold().dimmed());
        for f in &info_no_action {
            println!("  {} {}", "ℹ".blue(), f.description.dimmed());
            println!("    {}", f.evidence.dimmed());
        }
        println!();
    }

    println!("{}", "═══════════════════════════════════════════════════".bright_blue());
    let (icon, label, color): (&str, &str, &str) = match verdict.status {
        VerdictStatus::Healthy  => ("✔", "CHEMIN RETOUR SAIN", "green"),
        VerdictStatus::Degraded => ("⚠", "CHEMIN RETOUR DÉGRADÉ", "yellow"),
        VerdictStatus::Faulty   => ("✖", "PROBLÈME RETOUR DÉTECTÉ", "red"),
    };
    let line = format!("  {} {}", icon, label);
    let colored = match color {
        "green"  => line.green().bold(),
        "yellow" => line.yellow().bold(),
        _        => line.red().bold(),
    };
    println!("{}", colored);
    println!();
    for part in wrap_text(&verdict.summary, 72) {
        println!("  {}", part);
    }
    println!("{}", "═══════════════════════════════════════════════════".bright_blue());
    println!();
}

// ─── Affichage LG simple ──────────────────────────────────────────────────────

fn print_lg_traceroute(hops: &[TraceHop]) {
    for hop in hops {
        let host = hop.host.as_deref().unwrap_or("*");
        let rtt_str = if hop.rtts_ms.is_empty() {
            "* * *".dimmed().to_string()
        } else {
            let raw = hop.rtts_ms.iter().map(|v| format!("{:.1}ms", v)).collect::<Vec<_>>().join("  ");
            colorize_rtt(hop.median_ms().unwrap_or(0.0), raw)
        };
        println!("       {:>3}  {:<50}  {}", hop.ttl, truncate(host, 48), rtt_str);
    }
}

fn print_manual_url(server: &db::LgServer, my_ip: IpAddr) {
    let url = match &server.method {
        QueryMethod::HttpGet { url_template } => url_template.replace("{IP}", &my_ip.to_string()),
        QueryMethod::Manual { url, .. } => url.to_string(),
    };
    println!("  ┌─ Accès manuel → {}", url.cyan());
    println!("  └─ IP cible     → {}", my_ip.to_string().cyan().bold());
}

// ─── Utilitaires ──────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max { format!("{}…", &s[..max - 1]) } else { s.to_string() }
}

fn category_label(cat: FindingCategory) -> &'static str {
    match cat {
        FindingCategory::PacketLoss        => "PERTE",
        FindingCategory::HighLatency       => "LATENCE",
        FindingCategory::Jitter            => "JITTER",
        FindingCategory::Bufferbloat       => "BUFFERBLOAT",
        FindingCategory::RoutingAnomaly    => "ROUTAGE",
        FindingCategory::PeeringCongestion => "PEERING",
        FindingCategory::LocalIssue        => "LOCAL",
        FindingCategory::IcmpRateLimit     => "INFO",
        FindingCategory::PhysicalLatency   => "INFO",
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

fn detect_city_from_hops(hops: &[Hop]) -> Option<String> {
    let patterns: &[(&str, &str)] = &[
        ("njy", "Newark"), ("newark", "Newark"), ("ewr", "Newark"),
        ("nto", "New York"), ("nyc", "New York"), ("newyork", "New York"), ("jfk", "New York"),
        ("lax", "Los Angeles"), ("losangeles", "Los Angeles"),
        ("chi", "Chicago"), ("chicago", "Chicago"), ("ord", "Chicago"),
        ("ash", "Ashburn"), ("ashburn", "Ashburn"), ("iad", "Ashburn"),
        ("dfw", "Dallas"), ("dallas", "Dallas"),
        ("sfo", "San Francisco"), ("sjc", "San Jose"),
        ("sea", "Seattle"), ("seattle", "Seattle"),
        ("mia", "Miami"), ("miami", "Miami"),
        ("atl", "Atlanta"), ("atlanta", "Atlanta"),
        ("fra", "Frankfurt"), ("frankfurt", "Frankfurt"),
        ("ams", "Amsterdam"), ("amsterdam", "Amsterdam"),
        ("lon", "London"), ("london", "London"), ("lhr", "London"),
        ("par", "Paris"), ("paris", "Paris"), ("pye", "Paris"),
        ("sin", "Singapore"), ("singapore", "Singapore"),
        ("tyo", "Tokyo"), ("tokyo", "Tokyo"),
        ("hkg", "Hong Kong"), ("hongkong", "Hong Kong"),
    ];
    for hop in hops.iter().rev() {
        if let Some(ref h) = hop.hostname {
            let lower = h.to_lowercase();
            for (kw, city) in patterns {
                if lower.contains(kw) { return Some(city.to_string()); }
            }
        }
    }
    None
}

fn colorize_rtt(ms: f64, s: String) -> String {
    if ms < 50.0 { s.normal().to_string() }
    else if ms < 150.0 { s.yellow().to_string() }
    else { s.red().to_string() }
}
