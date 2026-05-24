# Pistes d'amélioration — peering-diag

> Revue initiale : 2026-05-23. Mis à jour : 2026-05-24 (v0.2.0).
> Items 1–5b, 8, 9, 10, 11 traités. Ce qui reste :

---

## ✅ Traités (v0.1.0 → v0.2.0)

| # | Description |
|---|-------------|
| 1 | 49 tests unitaires (maintenance, history, diag, db, heuristics) |
| 2 | Timeout 10 min + Semaphore 8 jobs max |
| 3 | Buffer lignes borné à 5000 (VecDeque) |
| 4 | SSE temps réel via broadcast (suppression polling 150ms) |
| 5 | `--format json` sur diag/aller/mtr/retour/ecmp |
| 5b | Carte mondiale des hops (Leaflet + géoloc ip-api.com + toggles aller/retour) |
| 8 | Cargo.lock versionné |
| 9 | Error Boundary React |
| 10 | Reconnexion SSE backoff exponentiel |
| 11 | Bouton Stop pour tous les jobs web |
| — | Double thème Terminal / Dashboard |
| — | Liste déroulante cibles dans l'historique |
| — | Fix F5 → 404 (fallback index.html) |

---

## 🔴 Fonctionnalités majeures

### (6) Support IPv6
`mtr/probe.rs` — sockets en `Domain::IPV4` uniquement. Gros chantier structurant :
- Nouveau socket ICMPv6
- Résolution DNS préférant IPv6 si demandé (`--ipv6` flag)
- Adaptation de l'affichage, DB et heuristiques

### (7) Alertes watch
Aucune notification quand une session watch détecte un problème (`Degraded` ou `Faulty`).
- Webhook configurable (POST JSON sur URL arbitraire)
- Seuils configurables : perte > X%, RTT > Y ms
- Optionnellement : email SMTP

---

## 🟠 Frontend — Dashboard incomplet

- `WatchPage`, `CheckEnvPage`, `DbPage` ignorent le thème dashboard (classes Tailwind hardcodées)
- Transition visuelle abrupte lors du switch de thème (ajouter `transition: all 0.2s` global)
- En mode dashboard, les KPI cards n'apparaissent pas si le run n'est pas stocké en DB
  (`diag --format json` bypass la DB)

---

## 🟡 Frontend — Carte

- Le décalage de superposition (`0.004°`) est fixe — le rendre dynamique selon le zoom courant
- Pas de zoom automatique pour englober tous les hops (centré sur le premier hop uniquement)
- Optimisation mobile : carte difficilement utilisable sur petit écran
- Carte pour les runs watch : les séries watch ne sont pas visualisables sur la carte
  (endpoint `/api/watch/:id/map` manquant)

---

## 🟡 Frontend — Historique

- Pagination réelle (actuellement juste un filtre "derniers N")
- Export CSV/JSON de l'historique depuis l'interface

---

## 🟢 Backend — Dette technique

- `detect_city_from_hops` et `detect_geo_hint` dupliqués dans `main.rs` et `lg/engine.rs`
- `wrap_text` et `category_label` dupliqués dans `report/display.rs` et `lg/engine.rs`
- Geo cache : pas d'éviction automatique des entrées de plus de 30 jours

---

## 🟢 Backend — Comportements à corriger

- `diag --format json` bypass la DB même si `--db` est aussi passé
- `watch` n'a pas de `--format json`
- Si ip-api.com est indisponible, la géoloc échoue silencieusement (pas de retry ni fallback)
