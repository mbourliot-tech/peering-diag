//! Détection de l'IP publique de l'utilisateur via api4.ipify.org.

use anyhow::{anyhow, Result};
use std::net::IpAddr;
use std::time::Duration;

/// Retourne l'IP publique IPv4 de l'utilisateur.
/// Utilisée comme cible des traceroutes Looking Glass (chemin retour).
pub async fn get_public_ip() -> Result<IpAddr> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let text = client
        .get("https://api4.ipify.org")
        .send()
        .await
        .map_err(|e| anyhow!("détection IP publique (api4.ipify.org) : {}", e))?
        .text()
        .await?;

    text.trim()
        .parse::<IpAddr>()
        .map_err(|_| anyhow!("réponse IP publique invalide : {:?}", text.trim()))
}
