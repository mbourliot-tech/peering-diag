# Pistes d'amélioration — peering-diag

> Revue de code du 2026-05-23. Classé par priorité avec références fichier:ligne.
> Top 3 par impact : **(1) tests**, **(2) timeout + limite jobs**, **(4) broadcast mort**.

---

## 🔴 Fiabilité (les plus impactantes)

### 1. Zéro test, alors que la CI lance `cargo test`
Aucun `#[test]` dans tout le projet. La CI (Sprint 3) passe « au vert » sans rien vérifier.
Le code est bien structuré (`lib.rs` + `bin`) donc testable. Cibles faciles, fort ROI :
- `report/maintenance.rs::human_bytes`
- `report/history.rs::hour_from_ts`, `extract_verdict_finding`
- `web/handlers/diag.rs::build_args` (tester la validation Sprint 2 : cible en `-`, commande hors whitelist)
- `mtr/heuristics.rs` (logique de verdict — cœur métier)

### 2. Aucun timeout ni limite de concurrence sur les jobs
`web/jobs.rs::JobManager::spawn` — pas de borne : on peut lancer une infinité de
sous-processus (chacun relance le binaire complet). Un `mtr` qui hang tourne indéfiniment.
→ `tokio::sync::Semaphore` pour plafonner les jobs simultanés + timeout par job (`tokio::time::timeout`).

### 3. Buffer de lignes non borné
`web/jobs.rs`, champ `job.lines` — une session `watch` qui tourne des jours accumule
toutes les lignes en mémoire sans limite.
→ `VecDeque` avec cap (ex. 5000 lignes, drop des plus vieilles).
⚠ Interagit avec le replay SSE par offset → prévoir un offset de base.

---

## 🟠 Architecture

### 4. Le canal broadcast est du code mort
`web/jobs.rs:39, 48, 208` — `broadcast::channel(256)` créé et `tx.send()` appelé à chaque
ligne, mais `sse.rs` ignore le canal et fait du **polling toutes les 150 ms**.
→ Latence ajoutée + CPU gaspillé + code trompeur.
Refactor : le stream SSE rejoue le buffer puis `subscribe()` au broadcast → vrai temps réel,
plus de polling.

### 5. Sortie 100 % texte → l'UI web reste un terminal
214 `println!` (compromis assumé du subprocess). Pour une UI riche (hops interactifs, graphes
en direct pendant le run), un mode `--format json` émettant des événements structurés par ligne
permettrait au frontend de faire le rendu plutôt que d'afficher du texte ANSI strippé.

### 5b. Carte mondiale des hops (dépend du point 5)
Afficher la position géographique de chaque hop sur une carte interactive.
- **Backend** : enrichir `AsInfo` avec `lat/lon` via ipinfo.io (déjà utilisé pour l'ASN fallback),
  persister dans `hop_samples`. Endpoint `GET /api/history/run/:id/geo` pour les runs terminés.
- **Frontend** : `react-leaflet` (Leaflet.js), polyligne entre hops, popup RTT/perte au clic.
- **Phase 1 (sans point 5)** : carte sur l'historique uniquement (hops déjà en base) — faisable
  en 1-2 jours.
- **Phase 2 (avec point 5)** : carte temps réel pendant le run via événements JSON structurés.

---

## 🟡 Fonctionnalités réseau

### 6. Pas de support IPv6
`mtr/probe.rs:62` (TODO), sockets en `Domain::IPV4`. Limitation réelle pour du diagnostic
de peering moderne. Gros chantier mais structurant.

### 7. Sockets RAW ICMP = privilèges requis
`mtr/probe.rs:76` — `SOCK_RAW`/`ICMPV4` exige admin (Windows) / `CAP_NET_RAW` ou root (Linux).
À vérifier : que `check-env` détecte le manque de privilèges et affiche un message clair
plutôt qu'un échec cryptique.

---

## 🟢 Distribution / build

### 8. `Cargo.lock` non versionné  ⟵ quick win (2 min)
Confirmé absent du suivi git. Pour un **binaire**, `Cargo.lock` doit être commité
(builds reproductibles, CI déterministe).
→ Retirer du `.gitignore` et committer.

---

## 🔵 Frontend

### 9. Pas d'Error Boundary React
Si une page lève une exception → écran blanc. Un `<ErrorBoundary>` autour du `<Suspense>`
(dans `App.tsx`) afficherait un message propre.

### 10. `EventSource` sans reconnexion maîtrisée
`api.ts::streamJob` — `onerror` ferme direct. Une stratégie de backoff éviterait de perdre
le flux sur micro-coupure.

---

## Ordre d'attaque suggéré
1. **(8) Cargo.lock** — 2 min, débloque la reproductibilité
2. **(1) base de tests** + **(2) borne sur les jobs** — rendent la CI réellement utile
3. **(4) broadcast mort / vrai temps réel** — nettoie l'archi et améliore le SSE
