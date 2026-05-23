# Changelog

Toutes les modifications notables de ce projet sont documentées dans ce fichier.

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.0.0/).

---

## [0.1.0] — 2026-05-23

### Ajouts

**CLI**
- `diag <cible>` — diagnostic complet : MTR AS-aware, speedtest segmenté, retour Globalping
- `aller <cible>` — traceroute MTR aller uniquement
- `retour <cible>` — chemin retour via l'API Globalping
- `mtr <cible>` — MTR brut (stdout coloré)
- `ecmp <cible>` — détection de routes ECMP (multi-path)
- `lg <cible>` — looking glass multi-sondes Globalping
- `history` — consultation de l'historique SQLite (tableau, par-heure, par-hop)
- `watch <cible>` — surveillance périodique automatique avec alertes
- `check-env` — vérification des dépendances système
- `db` — maintenance de la base SQLite (stats, purge, vacuum)
- `serve` — interface web intégrée (Axum, port 7373 par défaut)

**Interface web** (accessible via `serve`)
- Page Diagnostic : lancement de toutes les commandes avec streaming SSE temps réel
- Page Historique : tableau des runs, graphiques RTT/perte, vue par heure
- Page Watch : gestion des sessions de surveillance (démarrer / arrêter)
- Page Environnement : `check-env` via l'interface
- Page Base de données : statistiques, purge, vacuum

**Stockage**
- Base SQLite normalisée : `reports`, `hop_samples`, `return_hop_samples`, `speedtest_samples`, `watch_series`
- Cascade de suppression sur toutes les tables liées

**Analyse temporelle**
- Détection de pics horaires de dégradation
- Tendance linéaire sur la perte réseau (régression)
- Corrélation entre perte aller et retour

### Sécurité (Sprint 2)
- Whitelist des commandes autorisées dans l'API
- Validation de la cible (rejet si commence par `-`, longueur max 253)
- CORS restreint aux origines localhost uniquement
- Bornes sur tous les paramètres numériques (last, keep_last, older_than_days)
- Vérification `.ok` sur toutes les requêtes fetch du frontend
