//! Base statique de serveurs de mesure pour les AS Tier-1 et backbones.
//!
//! Les opérateurs de transit (TATA, Cogent, Arelion, Lumen, Zayo, NTT...)
//! n'apparaissent jamais dans les résultats locaux du CLI Ookla car ce sont
//! des opérateurs de backbone, pas des FAI grand public. On maintient ici
//! une base de serveurs connus pour ces AS.
//!
//! Sources :
//! - IDs Speedtest.net vérifiés manuellement
//! - Serveurs iperf3 publics listés sur iperf.fr et iperf3serverlist.net
//! - Endpoints HTTP de test publics

use std::collections::HashMap;

/// Type de serveur de mesure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasureMethod {
    /// Serveur Speedtest.net dans l'AS exact.
    SpeedtestDirect,
    /// Serveur Speedtest.net connu en dur pour cet AS (base statique).
    SpeedtestTier1Db,
    /// Serveur iperf3 public proche géographiquement.
    Iperf3,
    /// Téléchargement HTTP depuis un endpoint de test public.
    HttpDownload,
    /// Serveur Speedtest.net géographiquement proche (pas dans l'AS exact).
    SpeedtestGeo,
    /// Proxy local — mesure approximative, AS non couvert.
    Proxy,
}

impl MeasureMethod {
    pub fn label(&self) -> &'static str {
        match self {
            MeasureMethod::SpeedtestDirect  => "direct",
            MeasureMethod::SpeedtestTier1Db => "tier1-db",
            MeasureMethod::Iperf3           => "iperf3",
            MeasureMethod::HttpDownload     => "http",
            MeasureMethod::SpeedtestGeo     => "géo",
            MeasureMethod::Proxy            => "proxy",
        }
    }

    /// Plus la valeur est basse, plus la méthode est fiable.
    pub fn reliability_rank(&self) -> u8 {
        match self {
            MeasureMethod::SpeedtestDirect  => 1,
            MeasureMethod::SpeedtestTier1Db => 2,
            MeasureMethod::Iperf3           => 3,
            MeasureMethod::HttpDownload     => 4,
            MeasureMethod::SpeedtestGeo     => 5,
            MeasureMethod::Proxy            => 6,
        }
    }
}

/// Un endpoint de mesure connu pour un AS donné.
#[derive(Debug, Clone)]
pub struct KnownEndpoint {
    pub asn: u32,
    pub as_name: &'static str,
    pub method: MeasureMethod,
    pub location: &'static str,
    /// Identifiant ou URL selon la méthode.
    /// - SpeedtestTier1Db : ID numérique du serveur Speedtest.net
    /// - Iperf3 : "host:port"
    /// - HttpDownload : URL complète d'un fichier de test
    pub endpoint: &'static str,
    pub description: &'static str,
}

/// Base statique des endpoints connus.
/// Mise à jour manuelle — vérifier périodiquement que les serveurs sont toujours actifs.
///
/// Serveurs iperf3 Hurricane Electric (iperf.he.net, iperf.nyc.he.net…) :
/// serveur public à client unique — échoue avec "the server is busy" quand occupé.
/// Normal, non bloquant : la cascade passe à la méthode suivante.
pub fn get_known_endpoints() -> Vec<KnownEndpoint> {
    vec![
        // ─── TATA Communications (AS6453) ───────────────────────────────
        KnownEndpoint {
            asn: 6453,
            as_name: "TATA COMMUNICATIONS",
            method: MeasureMethod::Iperf3,
            location: "New York, US",
            endpoint: "iperf.he.net:5201",
            description: "Hurricane Electric NYC (adjacent TATA)",
        },
        KnownEndpoint {
            asn: 6453,
            as_name: "TATA COMMUNICATIONS",
            method: MeasureMethod::HttpDownload,
            location: "New Jersey, US",
            endpoint: "http://speedtest.reliablesite.net/100MB.test",
            description: "Fichier test 100MB US (secours TATA)",
        },

        // ─── Cogent (AS174) ──────────────────────────────────────────────
        KnownEndpoint {
            asn: 174,
            as_name: "COGENT-174",
            method: MeasureMethod::SpeedtestTier1Db,
            location: "Paris, FR",
            endpoint: "51308",
            description: "Cogent Paris Speedtest",
        },
        KnownEndpoint {
            asn: 174,
            as_name: "COGENT-174",
            method: MeasureMethod::Iperf3,
            location: "Paris, FR",
            endpoint: "iperf.par.fr.as5410.net:9200",
            description: "iperf3 Paris (via Bouygues/Cogent)",
        },

        // ─── Arelion / Telia (AS1299) ────────────────────────────────────
        KnownEndpoint {
            asn: 1299,
            as_name: "ARELION-AS",
            method: MeasureMethod::SpeedtestTier1Db,
            location: "Frankfurt, DE",
            endpoint: "14236",
            description: "Arelion Frankfurt Speedtest",
        },
        KnownEndpoint {
            asn: 1299,
            as_name: "ARELION-AS",
            method: MeasureMethod::Iperf3,
            location: "Stockholm, SE",
            endpoint: "speedtest.telia.net:5201",
            description: "Telia/Arelion iperf3 Stockholm",
        },

        // ─── Lumen / CenturyLink (AS3356) ────────────────────────────────
        KnownEndpoint {
            asn: 3356,
            as_name: "LEVEL3",
            method: MeasureMethod::Iperf3,
            location: "Dallas, US",
            endpoint: "iperf.he.net:5201",
            description: "iperf3 US (adjacent Lumen)",
        },

        // ─── NTT (AS2914) ─────────────────────────────────────────────────
        KnownEndpoint {
            asn: 2914,
            as_name: "NTT-LTD-2914",
            method: MeasureMethod::Iperf3,
            location: "Frankfurt, DE",
            endpoint: "iperf.fra.de.as5410.net:9200",
            description: "iperf3 Frankfurt (adjacent NTT)",
        },

        // ─── Zayo (AS6461) ────────────────────────────────────────────────
        KnownEndpoint {
            asn: 6461,
            as_name: "ZAYO-6461",
            method: MeasureMethod::HttpDownload,
            location: "Denver, US",
            endpoint: "http://speedtest.reliablesite.net/100MB.test",
            description: "Fichier test 100MB US",
        },

        // ─── Hurricane Electric (AS6939) ──────────────────────────────────
        KnownEndpoint {
            asn: 6939,
            as_name: "HURRICANE",
            method: MeasureMethod::Iperf3,
            location: "Fremont, US",
            endpoint: "iperf.he.net:5201",
            description: "Hurricane Electric iperf3 officiel",
        },
        KnownEndpoint {
            asn: 6939,
            as_name: "HURRICANE",
            method: MeasureMethod::Iperf3,
            location: "New York, US",
            endpoint: "iperf.nyc.he.net:5201",
            description: "Hurricane Electric NYC iperf3",
        },
        KnownEndpoint {
            asn: 6939,
            as_name: "HURRICANE",
            method: MeasureMethod::Iperf3,
            location: "Paris, FR",
            endpoint: "iperf.par.he.net:5201",
            description: "Hurricane Electric Paris iperf3",
        },

        // ─── Orange / France Télécom (AS5511) ────────────────────────────
        KnownEndpoint {
            asn: 5511,
            as_name: "ORANGE-AS",
            method: MeasureMethod::SpeedtestTier1Db,
            location: "Paris, FR",
            endpoint: "10916",
            description: "Orange France Paris Speedtest",
        },

        // ─── Bouygues Telecom (AS5410) ────────────────────────────────────
        KnownEndpoint {
            asn: 5410,
            as_name: "Bouygues Telecom",
            method: MeasureMethod::Iperf3,
            location: "Paris, FR",
            endpoint: "iperf.par.fr.as5410.net:9200",
            description: "Bouygues Telecom iperf3 officiel Paris",
        },
        KnownEndpoint {
            asn: 5410,
            as_name: "Bouygues Telecom",
            method: MeasureMethod::Iperf3,
            location: "Lyon, FR",
            endpoint: "iperf.lyo.fr.as5410.net:9200",
            description: "Bouygues Telecom iperf3 officiel Lyon",
        },

        // ─── SFR (AS15557) ────────────────────────────────────────────────
        KnownEndpoint {
            asn: 15557,
            as_name: "LDCOMNET",
            method: MeasureMethod::Iperf3,
            location: "Paris, FR",
            endpoint: "iperf.par.fr.as15557.net:9200",
            description: "SFR iperf3 Paris",
        },

        // ─── Free / Iliad (AS12322) ───────────────────────────────────────
        KnownEndpoint {
            asn: 12322,
            as_name: "PROXAD",
            method: MeasureMethod::Iperf3,
            location: "Paris, FR",
            endpoint: "iperf.par.fr.as12322.net:9200",
            description: "Free iperf3 Paris",
        },

        // ─── Hetzner (AS24940) ────────────────────────────────────────────
        KnownEndpoint {
            asn: 24940,
            as_name: "HETZNER-AS",
            method: MeasureMethod::HttpDownload,
            location: "Nuremberg, DE",
            endpoint: "http://speed.hetzner.de/100MB.bin",
            description: "Hetzner fichier test officiel 100MB",
        },
        KnownEndpoint {
            asn: 24940,
            as_name: "HETZNER-AS",
            method: MeasureMethod::HttpDownload,
            location: "Helsinki, FI",
            endpoint: "http://hel.speed.hetzner.com/100MB.bin",
            description: "Hetzner Helsinki fichier test 100MB",
        },

        // ─── OVH (AS16276) ────────────────────────────────────────────────
        KnownEndpoint {
            asn: 16276,
            as_name: "OVH",
            method: MeasureMethod::HttpDownload,
            location: "Paris, FR",
            endpoint: "http://proof.ovh.net/files/100Mb.dat",
            description: "OVH fichier test officiel 100MB",
        },

        // ─── Interserver (AS19318) ────────────────────────────────────────
        KnownEndpoint {
            asn: 19318,
            as_name: "Interserver, Inc",
            method: MeasureMethod::Iperf3,
            location: "New Jersey, US",
            endpoint: "iperf.nyc.he.net:5201",
            description: "iperf3 NYC (adjacent Interserver NJ)",
        },
    ]
}

/// Retourne les endpoints connus pour un ASN donné, triés par fiabilité.
pub fn get_endpoints_for_asn(asn: u32) -> Vec<KnownEndpoint> {
    let mut endpoints: Vec<KnownEndpoint> = get_known_endpoints()
        .into_iter()
        .filter(|e| e.asn == asn)
        .collect();
    endpoints.sort_by_key(|e| e.method.reliability_rank());
    endpoints
}

/// Retourne un index ASN → endpoints pour un batch de lookups.
pub fn get_endpoints_index() -> HashMap<u32, Vec<KnownEndpoint>> {
    let mut index: HashMap<u32, Vec<KnownEndpoint>> = HashMap::new();
    for endpoint in get_known_endpoints() {
        index.entry(endpoint.asn).or_default().push(endpoint);
    }
    for endpoints in index.values_mut() {
        endpoints.sort_by_key(|e| e.method.reliability_rank());
    }
    index
}
