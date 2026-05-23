//! Requêtes HTTP vers les serveurs Looking Glass et parser de traceroute.

use crate::lg::db::{LgServer, QueryMethod};
use anyhow::{anyhow, Result};
use std::net::IpAddr;
use std::time::Duration;

/// Un hop parsé depuis la sortie texte d'un traceroute.
#[derive(Debug, Clone)]
pub struct TraceHop {
    pub ttl: u8,
    /// Hostname ou IP renvoyé par le routeur. `None` = pas de réponse (`* * *`).
    pub host: Option<String>,
    /// RTT mesurés (en ms). Généralement 1 à 3 valeurs.
    pub rtts_ms: Vec<f64>,
}

impl TraceHop {
    /// RTT médian arrondi à 1 décimale, ou `None` si aucune mesure.
    pub fn median_ms(&self) -> Option<f64> {
        if self.rtts_ms.is_empty() {
            return None;
        }
        let mut v = self.rtts_ms.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mid = v.len() / 2;
        Some(if v.len() % 2 == 0 {
            (v[mid - 1] + v[mid]) / 2.0
        } else {
            v[mid]
        })
    }
}

/// Supprime les balises HTML en scannant `<…>` caractère par caractère.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Normalise les entités HTML les plus communes.
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
}

/// Extrait le contenu d'un bloc `<pre>…</pre>` (insensible à la casse).
/// Si aucun `<pre>` n'est trouvé, retourne le texte complet (déjà strippé).
fn extract_pre(html: &str) -> String {
    let lower = html.to_lowercase();
    if let Some(start_tag) = lower.find("<pre") {
        // Saute jusqu'au `>` fermant la balise ouvrante.
        if let Some(rel) = html[start_tag..].find('>') {
            let content_start = start_tag + rel + 1;
            if let Some(rel_end) = lower[content_start..].find("</pre>") {
                return strip_html(&html[content_start..content_start + rel_end]);
            }
        }
    }
    strip_html(html)
}

/// Parse une sortie textuelle de traceroute (format BSD/Linux standard).
///
/// Lignes attendues :
/// ```text
///  1  router.example.com (192.168.1.1)  0.5 ms  0.4 ms  0.5 ms
///  2  * * *
///  3  10.0.0.1  1.2 ms
/// ```
pub fn parse_traceroute(text: &str) -> Vec<TraceHop> {
    let mut hops = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut tokens = line.split_whitespace().peekable();

        // Premier token : numéro TTL (entier).
        let ttl = match tokens.next().and_then(|t| t.parse::<u8>().ok()) {
            Some(n) => n,
            None => continue,
        };

        // Deuxième token : hostname/IP ou `*`.
        let first = match tokens.next() {
            Some(t) => t,
            None => continue,
        };

        if first == "*" {
            hops.push(TraceHop { ttl, host: None, rtts_ms: Vec::new() });
            continue;
        }

        let host = first.trim_end_matches(',').to_string();

        // Collecte les tokens restants pour extraire les RTT.
        let remaining: Vec<&str> = tokens.collect();
        let mut rtts_ms = Vec::new();
        let mut i = 0;

        while i < remaining.len() {
            let tok = remaining[i];

            // Ignore le `(IP)` entre parenthèses.
            if tok.starts_with('(') {
                while i < remaining.len() && !remaining[i].ends_with(')') {
                    i += 1;
                }
                i += 1;
                continue;
            }

            if tok == "*" {
                i += 1;
                continue;
            }

            // Valeur RTT : nombre décimal suivi de "ms".
            if let Ok(v) = tok.trim_end_matches(',').parse::<f64>() {
                if remaining.get(i + 1).map_or(false, |&t| t == "ms") {
                    rtts_ms.push(v);
                    i += 2;
                    continue;
                }
            }

            i += 1;
        }

        hops.push(TraceHop {
            ttl,
            host: Some(host),
            rtts_ms,
        });
    }

    hops
}

/// Requête un serveur LG de type `HttpGet` et retourne les hops parsés.
/// Retourne une erreur si la méthode est `Manual` ou si la requête échoue.
pub async fn query_lg_http(server: &LgServer, my_ip: IpAddr) -> Result<Vec<TraceHop>> {
    let url_template = match &server.method {
        QueryMethod::HttpGet { url_template } => *url_template,
        QueryMethod::Manual { .. } => return Err(anyhow!("serveur manuel")),
    };

    let url = url_template.replace("{IP}", &my_ip.to_string());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90)) // traceroute 30 hops × ~2s = ~60s max
        .user_agent("Mozilla/5.0 peering-diag/0.1 (+https://github.com/mbourliot/peering-diag)")
        .build()?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("requête LG {} : {}", url, e))?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(anyhow!("HTTP {} depuis {}", status, url));
    }

    if body.trim().is_empty() {
        return Err(anyhow!("réponse vide du serveur LG ({})", url));
    }

    // Extrait le texte pertinent et parse.
    let text = extract_pre(&body);
    let hops = parse_traceroute(&text);

    if hops.is_empty() {
        // Montre un extrait de la réponse pour faciliter le debug.
        let preview: String = text.lines()
            .filter(|l| !l.trim().is_empty())
            .take(3)
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(anyhow!(
            "format inattendu — début de réponse : {:?}",
            if preview.len() > 120 { &preview[..120] } else { &preview }
        ));
    }

    Ok(hops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_traceroute() {
        let input = r#"
traceroute to 1.2.3.4 (1.2.3.4), 30 hops max
 1  router.he.net (216.66.68.1)  0.286 ms  0.262 ms  0.253 ms
 2  * * *
 3  xe-0.r04.amstnl02.nl.bb.gin.ntt.net (129.250.2.1)  5.234 ms  5.198 ms  5.201 ms
 4  10.0.0.1  1.2 ms
"#;
        let hops = parse_traceroute(input);
        assert_eq!(hops.len(), 4);

        assert_eq!(hops[0].ttl, 1);
        assert_eq!(hops[0].host.as_deref(), Some("router.he.net"));
        assert_eq!(hops[0].rtts_ms.len(), 3);
        assert!((hops[0].rtts_ms[0] - 0.286).abs() < 0.001);

        assert_eq!(hops[1].ttl, 2);
        assert!(hops[1].host.is_none());
        assert!(hops[1].rtts_ms.is_empty());

        assert_eq!(hops[2].ttl, 3);
        assert_eq!(hops[2].host.as_deref(), Some("xe-0.r04.amstnl02.nl.bb.gin.ntt.net"));
        assert_eq!(hops[2].rtts_ms.len(), 3);

        assert_eq!(hops[3].ttl, 4);
        assert_eq!(hops[3].host.as_deref(), Some("10.0.0.1"));
        assert_eq!(hops[3].rtts_ms.len(), 1);
        assert!((hops[3].rtts_ms[0] - 1.2).abs() < 0.001);
    }

    #[test]
    fn strip_html_basic() {
        let html = "<pre>traceroute<br/> 1  test.com  1.0 ms</pre>";
        let text = extract_pre(html);
        assert!(text.contains("traceroute"));
        assert!(text.contains("test.com"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn median_rtt() {
        let hop = TraceHop {
            ttl: 1,
            host: Some("x".into()),
            rtts_ms: vec![10.0, 20.0, 30.0],
        };
        assert_eq!(hop.median_ms(), Some(20.0));
    }
}
