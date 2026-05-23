//! CLI peering-diag.

use anyhow::{Context, Result};
use chrono::Local;
use clap::{Parser, Subcommand};
use colored::*;
use peering_diag::{
    asn::AsnResolver,
    mtr::{
        detect_ecmp_imbalance, explore_ecmp_to_target, EcmpExploreConfig, Mtr, MtrConfig,
    },
    report::{display::print_report, export_json, init_db, maintenance, store_report, store_watch_series},
    speedtest::cascade::build_geo_servers,
    speedtest::{
        check_iperf3, check_speedtest_cli, fetch_all_servers,
        group_servers_by_asn, measure_for_asn, COOLDOWN_BETWEEN_TESTS,
    },
    types::{AsInfo, DiagnosticReport, Hop, Severity, SpeedtestResult, VerdictStatus},
};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "peering-diag",
    about = "Diagnostic de problèmes de peering : MTR AS-aware + speedtests + chemin retour",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Diagnostic complet : chemin aller (MTR + speedtests) puis chemin retour (Globalping)
    Diag {
        target: String,
        /// Rounds MTR aller
        #[arg(long, default_value_t = 15)]
        rounds: u32,
        #[arg(long, default_value_t = 3)]
        probes: u32,
        #[arg(long, default_value_t = 30)]
        max_hops: u8,
        #[arg(long)]
        no_speedtest: bool,
        #[arg(long)]
        json: Option<PathBuf>,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long, default_value_t = 5)]
        max_speedtests: usize,
        /// IP publique locale (détectée automatiquement si absent)
        #[arg(long)]
        my_ip: Option<String>,
    },
    /// Diagnostic du chemin aller uniquement (MTR AS-aware + speedtests + analyse)
    Aller {
        target: String,
        #[arg(long, default_value_t = 15)]
        rounds: u32,
        #[arg(long, default_value_t = 3)]
        probes: u32,
        #[arg(long, default_value_t = 30)]
        max_hops: u8,
        #[arg(long)]
        no_speedtest: bool,
        #[arg(long)]
        json: Option<PathBuf>,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long, default_value_t = 5)]
        max_speedtests: usize,
    },
    /// Diagnostic du chemin retour uniquement (Globalping multi-rounds depuis la destination)
    Retour {
        /// Cible (hostname ou IP) — même cible que pour `diag` ou `aller`
        target: String,
        /// IP publique locale (détectée automatiquement si absent)
        #[arg(long)]
        my_ip: Option<String>,
    },
    /// Lance uniquement le MTR (sans speedtests ni retour)
    Mtr {
        target: String,
        #[arg(long, default_value_t = 15)]
        rounds: u32,
        #[arg(long, default_value_t = 30)]
        max_hops: u8,
    },
    /// Explore les chemins ECMP vers une cible (sonde TCP par port de service)
    Ecmp {
        target: String,
        #[arg(long, default_value_t = 443)]
        port: u16,
        #[arg(long, default_value_t = 8)]
        flows: u16,
        #[arg(long, default_value_t = 5)]
        probes: u32,
        #[arg(long, default_value_t = 64)]
        ttl: u8,
    },
    /// Chemin retour Globalping + URLs Looking Glass manuelles
    Lg {
        /// Cible (hostname ou IP)
        target: String,
        #[arg(long)]
        my_ip: Option<String>,
    },
    /// Affiche l'historique des diagnostics stockés dans une base SQLite
    History {
        /// Chemin vers la base SQLite
        db: PathBuf,
        /// Filtre sur une cible spécifique
        #[arg(long)]
        target: Option<String>,
        /// Affiche les N derniers runs
        #[arg(long, default_value_t = 20)]
        last: usize,
        /// Filtre depuis une date (ex : 2026-05-21T18:00)
        #[arg(long)]
        since: Option<String>,
        /// Agrège par heure de la journée (détection pics de congestion)
        #[arg(long)]
        by_hour: bool,
        /// Zoom sur un hop précis : IP ou ASN (ex : AS1299, 1.2.3.4)
        #[arg(long)]
        hop: Option<String>,
        /// Affiche le détail hop par hop d'un run spécifique (ID visible dans le tableau)
        #[arg(long)]
        run: Option<i64>,
    },
    /// Surveillance périodique : runs automatiques à intervalles réguliers
    Watch {
        /// Cible à surveiller (hostname ou IP)
        target: String,
        /// Intervalle entre deux runs (minutes)
        #[arg(long, default_value_t = 15)]
        interval: u64,
        /// Nombre de runs (0 = infini jusqu'à Ctrl+C)
        #[arg(long, default_value_t = 0)]
        count: u64,
        /// Passe la phase speedtest (runs plus rapides)
        #[arg(long)]
        no_speedtest: bool,
        /// IP publique locale (détectée automatiquement si absent)
        #[arg(long)]
        my_ip: Option<String>,
        /// Base SQLite de stockage (requis)
        #[arg(long)]
        db: PathBuf,
        /// Affiche seulement verdict + métriques clés (sans tableau MTR)
        #[arg(long)]
        quiet: bool,
    },
    /// Vérifie que l'environnement est prêt (speedtest CLI, iperf3)
    CheckEnv,
    /// Maintenance de la base SQLite (stats, purge, vacuum)
    Db {
        /// Chemin vers la base SQLite
        #[arg(long)]
        db: PathBuf,
        /// Afficher les statistiques de la base
        #[arg(long)]
        stats: bool,
        /// Compacter la base (SQLite VACUUM)
        #[arg(long)]
        vacuum: bool,
        /// Supprimer les runs plus anciens que N jours
        #[arg(long)]
        purge_older_than: Option<u32>,
        /// Garder seulement les N derniers runs (supprimer le reste)
        #[arg(long)]
        keep_last: Option<usize>,
    },
    /// Lance le serveur web (interface navigateur)
    Serve {
        /// Port d'écoute (localhost uniquement)
        #[arg(long, default_value_t = 7373)]
        port: u16,
        /// Base SQLite (créée si absente). Défaut : peering-diag.db dans le dossier courant.
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "peering_diag=info".into()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    println!("{}", format!("peering-diag v{}", env!("CARGO_PKG_VERSION")).dimmed());
    match cli.command {
        Command::Diag { target, rounds, probes, max_hops, no_speedtest, json, db, max_speedtests, my_ip } =>
            run_diag(&target, rounds, probes, max_hops, no_speedtest, json, db, max_speedtests, my_ip.as_deref()).await,
        Command::Aller { target, rounds, probes, max_hops, no_speedtest, json, db, max_speedtests } =>
            run_aller(&target, rounds, probes, max_hops, no_speedtest, json, db, max_speedtests).await,
        Command::Retour { target, my_ip } =>
            run_retour_cmd(&target, my_ip.as_deref()).await,
        Command::Mtr { target, rounds, max_hops } =>
            run_mtr_only(&target, rounds, max_hops).await,
        Command::Ecmp { target, port, flows, probes, ttl } =>
            run_ecmp(&target, port, flows, probes, ttl).await,
        Command::Lg { target, my_ip } =>
            run_lg_cmd(&target, my_ip.as_deref()).await,
        Command::History { db, target, last, since, by_hour, hop, run } =>
            run_history(&db, target.as_deref(), last, since.as_deref(), by_hour, hop.as_deref(), run),
        Command::Watch { target, interval, count, no_speedtest, my_ip, db, quiet } =>
            run_watch(&target, interval, count, no_speedtest, my_ip.as_deref(), db, quiet).await,
        Command::CheckEnv => check_env().await,
        Command::Db { db, stats, vacuum, purge_older_than, keep_last } =>
            run_db_maintenance(&db, stats, vacuum, purge_older_than, keep_last),
        Command::Serve { port, db } => {
            // Chemin par défaut : peering-diag.db à côté de l'exe (ou dans le dossier courant)
            let db_path = db.unwrap_or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("peering-diag.db")))
                    .unwrap_or_else(|| PathBuf::from("peering-diag.db"))
            });
            peering_diag::web::run_serve(port, db_path).await
        }
    }
}

// ─── Commande diag (aller + retour) ──────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_diag(
    target: &str,
    rounds: u32,
    probes: u32,
    max_hops: u8,
    no_speedtest: bool,
    json_path: Option<PathBuf>,
    db_path: Option<PathBuf>,
    max_speedtests: usize,
    my_ip_str: Option<&str>,
) -> Result<()> {
    // Détection IP publique en premier (nécessaire pour la phase retour)
    let my_ip = resolve_my_ip(my_ip_str).await?;

    // ── Phase 1 : chemin aller ─────────────────────────────────────────────
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".bright_blue()
    );
    println!(
        "{}",
        "║          PHASE 1 — CHEMIN ALLER                  ║".bright_blue().bold()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".bright_blue()
    );
    println!();
    let (report_id, _) = run_aller_inner(target, rounds, probes, max_hops, no_speedtest, json_path, db_path.clone(), max_speedtests, false, None).await?;

    // ── Phase 2 : chemin retour ────────────────────────────────────────────
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".bright_blue()
    );
    println!(
        "{}",
        "║          PHASE 2 — CHEMIN RETOUR                 ║".bright_blue().bold()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".bright_blue()
    );
    println!();
    // Si --db fourni, les hops retour seront stockés sous le même rapport
    let db_info = db_path.zip(report_id);
    peering_diag::lg::run_retour(target, my_ip, db_info, false).await
}

// ─── Commande aller ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_aller(
    target: &str,
    rounds: u32,
    probes: u32,
    max_hops: u8,
    no_speedtest: bool,
    json_path: Option<PathBuf>,
    db_path: Option<PathBuf>,
    max_speedtests: usize,
) -> Result<()> {
    run_aller_inner(target, rounds, probes, max_hops, no_speedtest, json_path, db_path, max_speedtests, false, None).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_aller_inner(
    target: &str,
    rounds: u32,
    probes: u32,
    max_hops: u8,
    no_speedtest: bool,
    json_path: Option<PathBuf>,
    db_path: Option<PathBuf>,
    max_speedtests: usize,
    quiet: bool,
    watch_series_id: Option<i64>,
) -> Result<(Option<i64>, DiagnosticReport)> {
    let target_ip = resolve_target(target).await?;
    println!("{} {} ({})", "Cible :".bold(), target, target_ip.to_string().dimmed());

    let asn_resolver = Arc::new(AsnResolver::new());
    let target_as = asn_resolver.lookup(target_ip).await?;
    if let Some(ref a) = target_as { println!("AS cible : {}", a.display()); }
    println!();

    println!("{}", "▶ MTR AS-aware".bold().cyan());
    let mtr = Mtr::new(
        MtrConfig { target: target_ip, rounds, max_hops, probes_per_round: probes, ..Default::default() },
        asn_resolver.clone(),
    );
    let hops = mtr.run().await?;
    println!();

    let speedtests = if no_speedtest {
        Vec::new()
    } else {
        match run_cascade_speedtests(&hops, asn_resolver.clone(), max_speedtests).await {
            Ok(r) => r,
            Err(e) => { eprintln!("{} speedtests : {}", "✖".red(), e); Vec::new() }
        }
    };

    let report = DiagnosticReport::build(target.to_string(), target_ip, target_as, hops, speedtests);
    if !quiet {
        print_report(&report);
    }

    if let Some(path) = json_path {
        export_json(&report, &path)?;
        if !quiet {
            println!("JSON exporté : {}", path.display().to_string().green());
        }
    }
    if let Some(path) = db_path {
        let mut conn = init_db(&path)?;
        let id = store_report(&mut conn, &report, watch_series_id)?;
        if !quiet {
            println!("Stocké en DB (id={}) : {}", id, path.display().to_string().green());
        }
        return Ok((Some(id), report));
    }
    Ok((None, report))
}

// ─── Commande retour ──────────────────────────────────────────────────────────

async fn run_retour_cmd(target: &str, my_ip_str: Option<&str>) -> Result<()> {
    let my_ip = resolve_my_ip(my_ip_str).await?;
    peering_diag::lg::run_retour(target, my_ip, None, false).await
}

// ─── Commande mtr (inchangée) ─────────────────────────────────────────────────

async fn run_mtr_only(target: &str, rounds: u32, max_hops: u8) -> Result<()> {
    let target_ip = resolve_target(target).await?;
    println!("Cible : {} ({})", target.bold(), target_ip.to_string().dimmed());
    let asn_resolver = Arc::new(AsnResolver::new());
    let mtr = Mtr::new(
        MtrConfig { target: target_ip, rounds, max_hops, ..Default::default() },
        asn_resolver.clone(),
    );
    let hops = mtr.run().await?;
    let target_as = asn_resolver.lookup(target_ip).await?;
    let report = DiagnosticReport::build(target.to_string(), target_ip, target_as, hops, vec![]);
    print_report(&report);
    Ok(())
}

// ─── Commande lg (inchangée) ──────────────────────────────────────────────────

async fn run_lg_cmd(target: &str, my_ip_str: Option<&str>) -> Result<()> {
    let my_ip = resolve_my_ip(my_ip_str).await?;
    peering_diag::lg::run_lg(target, my_ip, None, false).await
}

// ─── Commande ecmp (inchangée) ────────────────────────────────────────────────

async fn run_ecmp(target: &str, port: u16, flows: u16, probes: u32, ttl: u8) -> Result<()> {
    let target_ip = resolve_target(target).await?;
    println!(
        "{} {} ({}) port {}",
        "Cible :".bold(), target, target_ip.to_string().dimmed(), port
    );
    println!(
        "{}",
        format!("Exploration de {} chemins ECMP ({} probes/flux, TTL {})…", flows, probes, ttl).cyan()
    );
    println!();

    let cfg = EcmpExploreConfig {
        target: target_ip,
        dst_port: port,
        flows,
        probes_per_flow: probes,
        ttl,
        timeout: std::time::Duration::from_secs(2),
    };
    let stats = explore_ecmp_to_target(&cfg).await;

    println!("{:<8} {:>6} {:>8} {:>9} {:>9} {:>9}", "SrcPort", "Perte", "Min", "Médian", "Max", "Issue");
    println!("{}", "─".repeat(54).dimmed());
    for f in &stats {
        let outcome = match f.outcome {
            peering_diag::mtr::tcp_probe::TcpOutcome::Open => "ouvert".green(),
            peering_diag::mtr::tcp_probe::TcpOutcome::Closed => "fermé(RST)".yellow(),
            peering_diag::mtr::tcp_probe::TcpOutcome::Timeout => "timeout".red(),
            peering_diag::mtr::tcp_probe::TcpOutcome::Unreachable => "injoignable".red(),
        };
        let loss = f.loss_pct();
        let loss_str = if loss > 0.0 { format!("{:.0}%", loss).red().to_string() } else { "0%".green().to_string() };
        println!(
            "{:<8} {:>15} {:>8} {:>9} {:>9}   {}",
            f.src_port,
            loss_str,
            f.min_rtt_ms().map(|v| format!("{:.1}", v)).unwrap_or_else(|| "—".into()),
            f.median_rtt_ms().map(|v| format!("{:.1}", v)).unwrap_or_else(|| "—".into()),
            f.max_rtt_ms().map(|v| format!("{:.1}", v)).unwrap_or_else(|| "—".into()),
            outcome,
        );
    }
    println!();

    let imbalance = detect_ecmp_imbalance(&stats);
    println!("{}", "═══════════════════════════════════════════".bright_blue());
    if imbalance.is_imbalanced() {
        println!(
            "  {} DÉSÉQUILIBRE ECMP : {}/{} chemins dégradés",
            "⚠".yellow(), imbalance.degraded_flows, imbalance.total_flows
        );
        if let Some(b) = imbalance.baseline_ms {
            println!("  Référence (meilleur chemin) : {:.0}ms", b);
        }
        for (src_port, reason) in &imbalance.details {
            println!("    {} flux src:{} — {}", "✖".red(), src_port, reason);
        }
        println!();
        println!(
            "  {} chemins ECMP sur {} sont dégradés.",
            imbalance.degraded_flows, imbalance.total_flows
        );
    } else {
        let reached = stats.iter().filter(|f| f.reached > 0).count();
        if reached == 0 {
            println!("  {} Aucun flux n'a atteint la cible (port filtré ou TTL trop court).", "✖".red());
        } else {
            println!("  {} Chemins ECMP homogènes — aucun déséquilibre détecté.", "✔".green());
        }
    }
    println!("{}", "═══════════════════════════════════════════".bright_blue());
    Ok(())
}

// ─── Commande check-env ───────────────────────────────────────────────────────

fn check_raw_icmp_privilege() -> bool {
    Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4)).is_ok()
}

async fn check_env() -> Result<()> {
    println!("{}", "Vérification de l'environnement...".bold());

    // Sockets RAW ICMP — requis pour toutes les commandes de diagnostic
    if check_raw_icmp_privilege() {
        println!("  {} sockets RAW ICMP : OK", "✔".green());
    } else {
        println!("  {} sockets RAW ICMP : accès refusé", "✖".red());
        #[cfg(target_os = "windows")]
        println!("    Windows : relancer en tant qu'Administrateur");
        #[cfg(not(target_os = "windows"))]
        println!("    Linux   : sudo peering-diag …  ou  setcap cap_net_raw+ep <binaire>");
    }

    match check_speedtest_cli().await {
        Ok(version) => println!("  {} speedtest CLI : {}", "✔".green(), version.lines().next().unwrap_or("")),
        Err(e) => {
            println!("  {} speedtest CLI : {}", "✖".red(), e);
            println!("    Windows : winget install Ookla.Speedtest.CLI");
            println!("    Linux   : voir https://www.speedtest.net/apps/cli");
        }
    }
    if check_iperf3().await {
        println!("  {} iperf3 : disponible", "✔".green());
    } else {
        println!("  {} iperf3 : non installé (optionnel)", "⚠".yellow());
        println!("    Windows : winget install iPerf.iPerf3");
        println!("    Linux   : apt install iperf3 / dnf install iperf3");
    }
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn resolve_target(target: &str) -> Result<IpAddr> {
    if let Ok(ip) = target.parse::<IpAddr>() { return Ok(ip); }
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((target, 0))
        .await
        .context("résolution DNS")?
        .map(|sa| sa.ip())
        .collect();
    if addrs.is_empty() {
        anyhow::bail!("résolution DNS : aucune IP trouvée pour '{}'", target);
    }
    // Préférer IPv4 — le MTR ne supporte pas encore ICMPv6
    if let Some(&v4) = addrs.iter().find(|ip| ip.is_ipv4()) {
        return Ok(v4);
    }
    eprintln!(
        "  {} '{}' ne résout qu'en IPv6 — le MTR IPv6 n'est pas encore implémenté.",
        "⚠".yellow(), target
    );
    Ok(addrs[0])
}

async fn resolve_my_ip(my_ip_str: Option<&str>) -> Result<IpAddr> {
    if let Some(s) = my_ip_str {
        return s.parse::<IpAddr>().context("--my-ip : adresse IP invalide");
    }
    print!("Détection de l'IP publique… ");
    let ip = peering_diag::lg::get_public_ip().await?;
    println!("{}", ip.to_string().cyan());
    Ok(ip)
}

async fn run_cascade_speedtests(
    hops: &[Hop],
    asn_resolver: Arc<AsnResolver>,
    max_tests: usize,
) -> Result<Vec<SpeedtestResult>> {
    println!("{}", "▶ Speedtests (cascade multi-méthodes)".bold().cyan());

    let speedtest_ok = check_speedtest_cli().await.is_ok();
    let iperf3_ok = check_iperf3().await;

    if !speedtest_ok && !iperf3_ok {
        return Err(anyhow::anyhow!(
            "Ni speedtest CLI ni iperf3 ne sont installés. \
             Installer au moins l'un des deux (voir `peering-diag check-env`)."
        ));
    }

    eprintln!("  Outils : speedtest={} iperf3={}",
        if speedtest_ok { "✔".green() } else { "✖".red() },
        if iperf3_ok { "✔".green() } else { "✖".dimmed() }
    );

    let mut path_asns: Vec<(u32, Option<AsInfo>)> = Vec::new();
    let mut seen = BTreeSet::new();
    for hop in hops {
        if let Some(ref info) = hop.as_info {
            if seen.insert(info.asn) {
                path_asns.push((info.asn, Some(info.clone())));
            }
        }
    }

    if path_asns.is_empty() {
        eprintln!("  Aucun AS public dans le chemin — skip.");
        return Ok(Vec::new());
    }

    let asn_list: Vec<u32> = path_asns.iter().map(|(a, _)| *a).collect();
    eprintln!("  AS sur le chemin : {:?}", asn_list);

    let local_servers = if speedtest_ok { fetch_all_servers().await.unwrap_or_default() } else { Vec::new() };
    let local_by_asn = if !local_servers.is_empty() {
        group_servers_by_asn(local_servers.clone(), asn_resolver.clone(), None).await.unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    let uncovered: Vec<u32> = path_asns.iter()
        .map(|(asn, _)| *asn)
        .filter(|asn| !local_by_asn.contains_key(asn))
        .collect();

    if !uncovered.is_empty() {
        eprintln!("  AS sans serveur Speedtest local : {:?}", uncovered);
        eprintln!("  → Cascade : base Tier-1 → iperf3 → HTTP → proxy");
    }

    let geo_by_asn = build_geo_servers(
        detect_geo_hint(hops).as_deref(),
        &uncovered,
        &local_by_asn,
    ).await;

    let mut results = Vec::new();
    for (asn, as_info) in path_asns.iter().take(max_tests) {
        let label = as_info.as_ref()
            .map(|a| a.display())
            .unwrap_or_else(|| format!("AS{}", asn));

        eprintln!("  → Mesure vers {}…", label);
        match measure_for_asn(*asn, as_info, &local_by_asn, &geo_by_asn, iperf3_ok).await {
            Some(measure) => {
                eprintln!(
                    "    {} [{}] ↓ {:.1} Mbps  ↑ {:.1} Mbps  ping {:.1}ms",
                    "✓".green(), measure.method.label(),
                    measure.inner.download_mbps, measure.inner.upload_mbps, measure.inner.ping_ms
                );
                let mut result = measure.inner;
                result.method = Some(measure.method.label().to_string());
                result.endpoint_label = Some(measure.endpoint_label);
                results.push(result);
            }
            None => eprintln!("    {} Aucune méthode disponible pour {}", "⚠".yellow(), label),
        }
        if results.len() < max_tests {
            tokio::time::sleep(COOLDOWN_BETWEEN_TESTS).await;
        }
    }
    println!();
    Ok(results)
}

// ─── Commande history ─────────────────────────────────────────────────────────

fn run_history(
    db_path: &std::path::Path,
    target: Option<&str>,
    last_n: usize,
    since: Option<&str>,
    by_hour: bool,
    hop: Option<&str>,
    run: Option<i64>,
) -> Result<()> {
    let conn = init_db(db_path)?;
    if let Some(run_id) = run {
        peering_diag::report::history::show_run_detail(&conn, run_id)?;
    } else if let Some(hop_filter) = hop {
        peering_diag::report::history::show_hop(&conn, target, hop_filter, last_n)?;
    } else if by_hour {
        peering_diag::report::history::show_by_hour(&conn, target)?;
        peering_diag::report::temporal::show_temporal_analysis(&conn, target, last_n)?;
    } else {
        peering_diag::report::history::show_chronological(&conn, target, last_n, since)?;
        peering_diag::report::temporal::show_temporal_analysis(&conn, target, last_n)?;
    }
    Ok(())
}

// ─── Commande watch ───────────────────────────────────────────────────────────

#[derive(Default)]
struct WatchStats {
    healthy: u32,
    degraded: u32,
    faulty: u32,
}

impl WatchStats {
    fn record(&mut self, report: &DiagnosticReport) {
        match report.verdict.status {
            VerdictStatus::Healthy  => self.healthy  += 1,
            VerdictStatus::Degraded => self.degraded += 1,
            VerdictStatus::Faulty   => self.faulty   += 1,
        }
    }

    fn summary(&self) -> String {
        format!(
            "{} sain · {} dégradé · {} faulty",
            self.healthy, self.degraded, self.faulty
        )
    }
}

/// Affiche une ligne de résumé compacte du diagnostic aller (mode watch --quiet).
fn print_aller_summary(report: &DiagnosticReport) {
    let (icon, label) = match report.verdict.status {
        VerdictStatus::Healthy  => ("✔", "SAIN    "),
        VerdictStatus::Degraded => ("⚠", "DÉGRADÉ "),
        VerdictStatus::Faulty   => ("✖", "FAULTY  "),
    };

    // Finding le plus sévère (Critical > Warning)
    let key_finding = report
        .findings
        .iter()
        .find(|f| f.severity == Severity::Critical)
        .or_else(|| report.findings.iter().find(|f| f.severity == Severity::Warning));

    let detail = if let Some(f) = key_finding {
        f.description.clone()
    } else {
        let avg_rtt = report
            .hops
            .iter()
            .rev()
            .find_map(|h| h.avg_rtt_ms().filter(|&v| v > 0.0))
            .map(|v| format!("{:.0}ms", v))
            .unwrap_or_else(|| "—".into());
        let max_loss = report.hops.iter()
            .filter(|h| !h.suspected_icmp_ratelimit)
            .map(|h| h.loss_pct())
            .fold(0.0f64, f64::max);
        let dl = report.speedtests.iter().map(|s| s.download_mbps).fold(0.0f64, f64::max);
        let dl_str = if dl > 0.0 { format!(", {:.0} Mbps↓", dl) } else { "".into() };
        format!("RTT moy {}, {:.1}% perte{}", avg_rtt, max_loss, dl_str)
    };

    let line = format!("   Aller  : {} {}  — {}", icon, label, detail);
    println!("{}", match report.verdict.status {
        VerdictStatus::Healthy  => line.green(),
        VerdictStatus::Degraded => line.yellow(),
        VerdictStatus::Faulty   => line.red(),
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_watch(
    target: &str,
    interval_min: u64,
    count: u64,
    no_speedtest: bool,
    my_ip_str: Option<&str>,
    db_path: PathBuf,
    quiet: bool,
) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc as StdArc;
    use std::time::Duration as StdDuration;

    // ── Résolution IP publique ─────────────────────────────────────────────
    let my_ip = resolve_my_ip(my_ip_str).await?;

    // ── Création de la série watch en DB ──────────────────────────────────
    let interval_s = interval_min * 60;
    let started_at = Local::now().to_rfc3339();
    let series_id = {
        let conn = init_db(&db_path)?;
        store_watch_series(&conn, &started_at, target, interval_s as i64)?
    };

    // ── Bannière ───────────────────────────────────────────────────────────
    println!(
        "{}",
        format!(
            "⟳  watch {}  — intervalle {}min  — série #{}  — DB: {}",
            target.bold(),
            interval_min,
            series_id,
            db_path.display()
        )
        .cyan()
    );
    if count > 0 {
        println!("   {} runs maximum.", count);
    }
    println!("   Ctrl+C pour arrêter (arrêt après le run en cours).");
    println!();

    // ── Arrêt propre via Ctrl+C ────────────────────────────────────────────
    let stop = StdArc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stop_clone.store(true, Ordering::SeqCst);
    });

    // ── Boucle principale ──────────────────────────────────────────────────
    let mut runs_done = 0u64;
    let mut stats = WatchStats::default();
    let mut timer = tokio::time::interval(StdDuration::from_secs(interval_s));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        timer.tick().await; // premier tick immédiat

        if stop.load(Ordering::SeqCst) {
            break;
        }

        runs_done += 1;
        let now = Local::now();
        println!(
            "{}  [{}]  {}",
            format!("⟳  Run #{}", runs_done).cyan().bold(),
            now.format("%Y-%m-%d %H:%M"),
            target,
        );

        // ── Chemin aller ──────────────────────────────────────────────────
        match run_aller_inner(
            target, 5, 3, 30, no_speedtest, None,
            Some(db_path.clone()), 3, quiet, Some(series_id),
        )
        .await
        {
            Ok((report_id, report)) => {
                if quiet {
                    print_aller_summary(&report);
                }
                stats.record(&report);

                // ── Chemin retour ─────────────────────────────────────────
                let db_info = report_id.map(|id| (db_path.clone(), id));
                if let Err(e) =
                    peering_diag::lg::run_retour(target, my_ip, db_info, quiet).await
                {
                    if quiet {
                        println!("   Retour : {} ERREUR   — {}", "✖".red(), e);
                    } else {
                        eprintln!("  {} Retour échoué : {}", "⚠".yellow(), e);
                    }
                }
            }
            Err(e) => {
                eprintln!("  {} Run #{} échoué : {}", "✖".red(), runs_done, e);
            }
        }

        println!();

        if count > 0 && runs_done >= count {
            break;
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
    }

    // ── Résumé final ───────────────────────────────────────────────────────
    println!(
        "{}  {} run(s) effectués  ({})",
        "  Arrêt.".bold(),
        runs_done,
        stats.summary()
    );
    println!("    DB : {}", db_path.display().to_string().green());
    Ok(())
}

// ─── Commande db ─────────────────────────────────────────────────────────────

fn run_db_maintenance(
    db_path: &std::path::Path,
    show_stats: bool,
    do_vacuum: bool,
    purge_days: Option<u32>,
    keep_last: Option<usize>,
) -> Result<()> {
    let nothing = !show_stats && !do_vacuum && purge_days.is_none() && keep_last.is_none();
    if nothing {
        println!("{}", "Commande `db` : spécifiez au moins une option.".yellow());
        println!("  --stats                  Afficher les statistiques");
        println!("  --purge-older-than <N>   Supprimer les runs plus anciens que N jours");
        println!("  --keep-last <N>          Garder seulement les N derniers runs");
        println!("  --vacuum                 Compacter la base (SQLite VACUUM)");
        return Ok(());
    }

    let conn = init_db(db_path)?;

    if show_stats {
        let s = maintenance::get_stats(&conn, db_path)?;
        println!("{}", "📊  Statistiques de la base".bold());
        println!("  {:<16}: {}", "Runs", s.run_count.to_string().cyan());
        println!("  {:<16}: {}", "Hops", format_count(s.hop_count).cyan());
        println!("  {:<16}: {}", "Speedtests", s.speedtest_count.to_string().cyan());
        println!("  {:<16}: {}", "Watch series", s.watch_series_count.to_string().cyan());
        if let Some(ref ts) = s.oldest_run {
            println!("  {:<16}: {}", "Plus ancien", fmt_ts(ts).dimmed());
        }
        if let Some(ref ts) = s.newest_run {
            println!("  {:<16}: {}", "Plus récent", fmt_ts(ts).dimmed());
        }
        println!("  {:<16}: {}", "Taille", human_bytes(s.db_size_bytes).cyan());
        println!();
    }

    if let Some(days) = purge_days {
        let deleted = maintenance::purge_older_than(&conn, days)?;
        println!(
            "  {} {} run(s) supprimés (plus anciens que {} jours)",
            "🗑".red(), deleted.to_string().bold(), days
        );
    }

    if let Some(keep) = keep_last {
        let deleted = maintenance::purge_keep_last(&conn, keep)?;
        println!(
            "  {} {} run(s) supprimés (gardé les {} derniers)",
            "🗑".red(), deleted.to_string().bold(), keep
        );
    }

    if do_vacuum {
        maintenance::vacuum(&conn)?;
        println!("  {} VACUUM terminé", "✔".green());
    }

    Ok(())
}

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

fn format_count(n: i64) -> String {
    // Affichage avec séparateurs de milliers simples
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(' '); }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn fmt_ts(ts: &str) -> String {
    // "2026-05-01T08:12:00+02:00" → "2026-05-01 08:12"
    ts.replace('T', " ").get(..16).unwrap_or(ts).to_string()
}

fn detect_geo_hint(hops: &[Hop]) -> Option<String> {
    let keywords = [
        ("newark", "Newark"), ("njy", "Newark"),
        ("newyork", "New York"), ("nto", "New York"),
        ("chicago", "Chicago"), ("chi", "Chicago"),
        ("ashburn", "Ashburn"), ("ash", "Ashburn"),
        ("losangeles", "Los Angeles"), ("lax", "Los Angeles"),
        ("dallas", "Dallas"), ("dfw", "Dallas"),
        ("london", "London"), ("ldn", "London"),
        ("paris", "Paris"), ("pvu", "Paris"), ("pye", "Paris"),
        ("frankfurt", "Frankfurt"), ("fra", "Frankfurt"),
        ("amsterdam", "Amsterdam"), ("ams", "Amsterdam"),
    ];
    for hop in hops.iter().rev() {
        if let Some(ref h) = hop.hostname {
            let lower = h.to_lowercase();
            for (kw, city) in &keywords {
                if lower.contains(kw) { return Some(city.to_string()); }
            }
        }
    }
    None
}
