//! Base statique des serveurs Looking Glass par ASN.
//!
//! Deux méthodes de requête :
//! - `HttpGet` — on envoie une requête GET et on parse la réponse directement.
//! - `Manual`  — interface web JavaScript ou formulaire ; on génère l'URL et les
//!   instructions pour l'utilisateur.
//!
//! Les nœuds HttpGet sont vérifiés au moment de l'écriture. En cas de changement
//! d'interface, la requête échouera proprement et l'URL manuelle sera affichée.

#[derive(Debug, Clone)]
pub enum QueryMethod {
    /// Requête HTTP GET — `{IP}` est remplacé par l'IP de l'utilisateur.
    HttpGet { url_template: &'static str },
    /// Interface non requêtable automatiquement — URL + instructions pour l'humain.
    Manual { url: &'static str, note: &'static str },
}

#[derive(Debug, Clone)]
pub struct LgServer {
    pub asn: u32,
    pub as_name: &'static str,
    /// Emplacement du nœud, ex. "Frankfurt", "New York".
    pub node: &'static str,
    pub method: QueryMethod,
}

/// Base complète. On couvre les Tier-1 et grands transitaires communs sur les
/// chemins Europe ↔ Amérique du Nord.
pub static LG_DB: &[LgServer] = &[
    // ─── Hurricane Electric (AS6939) ─────────────────────────────────────────
    // Interface web avec formulaire — pas de GET direct disponible.
    // Nœuds Europe recommandés pour le retour vers une IP française.
    LgServer {
        asn: 6939,
        as_name: "Hurricane Electric",
        node: "Frankfurt / Amsterdam / London",
        method: QueryMethod::Manual {
            url: "https://lg.he.net/",
            note: "Sélectionner : Traceroute → nœud FRA, AMS ou LON → saisir l'IP",
        },
    },
    // Nœud US : voir depuis où le trafic US sort vers l'Europe.
    LgServer {
        asn: 6939,
        as_name: "Hurricane Electric",
        node: "New York / Los Angeles",
        method: QueryMethod::Manual {
            url: "https://lg.he.net/",
            note: "Sélectionner : Traceroute → nœud NYC ou LAX → saisir l'IP",
        },
    },

    // ─── TATA Communications (AS6453) ────────────────────────────────────────
    LgServer {
        asn: 6453,
        as_name: "TATA Communications",
        node: "Paris / Frankfurt",
        method: QueryMethod::Manual {
            url: "https://looking.tatacommunications.com/",
            note: "Sélectionner : Traceroute → nœud Paris ou Frankfurt → saisir l'IP",
        },
    },

    // ─── Cogent (AS174) ───────────────────────────────────────────────────────
    LgServer {
        asn: 174,
        as_name: "Cogent",
        node: "Paris / Frankfurt",
        method: QueryMethod::Manual {
            url: "http://lg.cogentco.com/",
            note: "Sélectionner : nœud Paris ou Frankfurt → Traceroute → saisir l'IP",
        },
    },

    // ─── Lumen / Level3 (AS3356) ─────────────────────────────────────────────
    LgServer {
        asn: 3356,
        as_name: "Lumen (Level3)",
        node: "Paris / Frankfurt",
        method: QueryMethod::Manual {
            url: "https://lookingglass.centurylink.net/",
            note: "Sélectionner : Traceroute → nœud Paris ou Frankfurt → saisir l'IP",
        },
    },

    // ─── Arelion / Telia (AS1299) ────────────────────────────────────────────
    LgServer {
        asn: 1299,
        as_name: "Arelion (Telia)",
        node: "Frankfurt",
        method: QueryMethod::Manual {
            url: "https://lg.arelion.com/",
            note: "Sélectionner : Traceroute → nœud Frankfurt → saisir l'IP",
        },
    },

    // ─── NTT (AS2914) ────────────────────────────────────────────────────────
    LgServer {
        asn: 2914,
        as_name: "NTT",
        node: "Frankfurt",
        method: QueryMethod::Manual {
            url: "https://www.gin.ntt.net/support/looking-glass/",
            note: "Sélectionner : Traceroute → nœud Frankfurt → saisir l'IP",
        },
    },

    // ─── GTT (AS3257) ────────────────────────────────────────────────────────
    LgServer {
        asn: 3257,
        as_name: "GTT",
        node: "Frankfurt",
        method: QueryMethod::Manual {
            url: "https://lg.gtt.net/",
            note: "Sélectionner : Traceroute → nœud Frankfurt → saisir l'IP",
        },
    },

    // ─── Zayo (AS6461) ───────────────────────────────────────────────────────
    LgServer {
        asn: 6461,
        as_name: "Zayo",
        node: "Frankfurt",
        method: QueryMethod::Manual {
            url: "https://lg.zayo.com/",
            note: "Sélectionner : Traceroute → nœud Frankfurt → saisir l'IP",
        },
    },

    // ─── Telecom Italia Sparkle (AS6762) ─────────────────────────────────────
    LgServer {
        asn: 6762,
        as_name: "Telecom Italia Sparkle",
        node: "Frankfurt",
        method: QueryMethod::Manual {
            url: "https://www.tisparkle.com/our-business/business-products/looking-glass",
            note: "Sélectionner : Traceroute → nœud Frankfurt → saisir l'IP",
        },
    },

    // ─── PCCW Global (AS3491) ────────────────────────────────────────────────
    LgServer {
        asn: 3491,
        as_name: "PCCW Global",
        node: "Frankfurt",
        method: QueryMethod::Manual {
            url: "https://www.pccwglobal.com/en/network/looking-glass",
            note: "Sélectionner : Traceroute → nœud Frankfurt → saisir l'IP",
        },
    },

    // ─── Colt (AS8220) ───────────────────────────────────────────────────────
    LgServer {
        asn: 8220,
        as_name: "Colt",
        node: "Paris / Frankfurt",
        method: QueryMethod::Manual {
            url: "https://lg.colt.net/",
            note: "Sélectionner : Traceroute → nœud Paris ou Frankfurt → saisir l'IP",
        },
    },

    // ─── RETN (AS9002) ───────────────────────────────────────────────────────
    LgServer {
        asn: 9002,
        as_name: "RETN",
        node: "Frankfurt",
        method: QueryMethod::Manual {
            url: "https://lg.retn.net/",
            note: "Sélectionner : Traceroute → nœud Frankfurt → saisir l'IP",
        },
    },
];

/// Retourne tous les serveurs LG connus pour un ASN donné.
pub fn servers_for_asn(asn: u32) -> Vec<&'static LgServer> {
    LG_DB.iter().filter(|s| s.asn == asn).collect()
}

/// Retourne tous les serveurs auto-requêtables (HttpGet) de la base.
pub fn auto_servers() -> Vec<&'static LgServer> {
    LG_DB
        .iter()
        .filter(|s| matches!(s.method, QueryMethod::HttpGet { .. }))
        .collect()
}
