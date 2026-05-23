# Plan — Tests temporels et vue historique

## Objectif

Ajouter deux nouvelles commandes (`watch` + `history`) et étendre le schéma SQLite pour obtenir une vue complète dans le temps : tendances horaires, détection des congestions d'heures de pointe, évolution hop-par-hop.

---

## Ce qui existe déjà

| Existant | État |
|---|---|
| Tables `reports`, `hop_samples`, `speedtest_samples` | ✅ opérationnel |
| `--db` sur `aller`/`diag` | ✅ stocke le chemin aller |
| Chemin retour (Globalping) | ❌ jamais persisté en DB |
| Mode périodique | ❌ absent |
| Vue historique | ❌ absent |

---

## Phase 1 — Schéma SQLite étendu

**Fichier :** `src/report/storage.rs`

Trois ajouts au schéma, migrés via une table `schema_version` (idempotent) :

```sql
-- Table de version pour migrations futures
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);

-- Hops du chemin retour (Globalping)
CREATE TABLE IF NOT EXISTS return_hop_samples (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id   INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    ttl         INTEGER NOT NULL,
    host        TEXT,
    ip          TEXT,
    asn         INTEGER,
    as_name     TEXT,
    loss_pct    REAL,
    snt         INTEGER,
    last_ms     REAL,
    avg_ms      REAL,
    min_ms      REAL,
    max_ms      REAL,
    stdev_ms    REAL
);

-- Métadonnées par série de tests watch
CREATE TABLE IF NOT EXISTS watch_series (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at  TEXT NOT NULL,
    target      TEXT NOT NULL,
    interval_s  INTEGER NOT NULL
);
-- + colonne watch_series_id optionnelle dans reports
```

Nouvelle fonction : `store_return_hops(conn, report_id, hops: &[MtrHop])`.

**Impact :** `src/lg/engine.rs` doit accepter un `Option<db_path>` pour stocker les hops retour quand `--db` est fourni dans `diag` ou `retour`.

---

## Phase 2 — Commande `watch`

**Fichier :** `src/main.rs`

```
peering-diag watch <cible> --db historique.sqlite [options]
```

| Option | Défaut | Description |
|---|---|---|
| `--interval` | `15` | Minutes entre deux runs |
| `--count` | `0` | Nombre de runs (0 = infini jusqu'à Ctrl+C) |
| `--no-speedtest` | off | Skip la phase speedtest (watch plus rapide) |
| `--my-ip` | auto | IP publique pour le retour |
| `--db` | *requis* | Base SQLite de stockage |
| `--quiet` | off | Affiche seulement verdict + métriques clés (pas tout le tableau MTR) |

**Comportement :**

```
peering-diag watch ftp.exemple.com --interval 15 --db historique.sqlite

⟳  Run #1  [2026-05-21 18:00]  ftp.exemple.com
   Aller  : ✖ FAULTY   — Peering AS5511→AS1299, perte 4.2%, jitter 45ms
   Retour : ✔ HEALTHY  — RTT moy 42ms, 0% perte

⟳  Run #2  [2026-05-21 18:15]  ftp.exemple.com
   ...

^C  Arrêt. 4 runs effectués  (2 Faulty, 1 Degraded, 1 Healthy)
    DB : historique.sqlite
```

**Implémentation :** boucle `tokio::time::interval` + `tokio::signal::ctrl_c()` pour arrêt propre. Chaque run appelle `run_aller_inner` + `run_retour` avec `db_path` fourni.

---

## Phase 3 — Commande `history`

**Fichiers :** `src/main.rs` + nouveau `src/report/history.rs`

```
peering-diag history <db> [options]
```

| Option | Description |
|---|---|
| `--target <cible>` | Filtre sur une cible (si la DB contient plusieurs cibles) |
| `--last N` | Affiche les N derniers runs (défaut : 20) |
| `--since <datetime>` | Filtre depuis une date (`2026-05-21T18:00`) |
| `--by-hour` | Agrège par heure de la journée (pattern heures de pointe) |
| `--hop <IP\|ASN>` | Zoom sur un hop précis : évolution de sa perte et RTT |

**Vue chronologique (`history historique.sqlite`) :**

```
Cible : ftp.exemple.com — 12 runs entre 2026-05-21 08:00 et 2026-05-21 23:00

┌──────────────────────┬──────────┬────────┬──────────┬──────────┬─────────────────────────────┐
│ Timestamp            │ Verdict  │ Perte% │ RTT moy  │ DL Mbps  │ Finding principal           │
├──────────────────────┼──────────┼────────┼──────────┼──────────┼─────────────────────────────┤
│ 2026-05-21 08:00     │ ✔ SAIN   │  0.0   │  89 ms   │  820 Mbps│ —                           │
│ 2026-05-21 12:00     │ ⚠ DÉGR.  │  0.8   │  97 ms   │  580 Mbps│ [JITTER] hop 5, stdev 22ms  │
│ 2026-05-21 18:00     │ ✖ FAULT  │  4.2   │ 145 ms   │   11 Mbps│ [PEERING] AS5511→AS1299     │
│ 2026-05-21 19:00     │ ✖ FAULT  │  6.1   │ 162 ms   │    8 Mbps│ [PEERING] AS5511→AS1299     │
│ 2026-05-21 22:00     │ ⚠ DÉGR.  │  1.2   │ 105 ms   │  210 Mbps│ [JITTER] hop 5, stdev 18ms  │
│ 2026-05-21 23:00     │ ✔ SAIN   │  0.0   │  91 ms   │  800 Mbps│ —                           │
└──────────────────────┴──────────┴────────┴──────────┴──────────┴─────────────────────────────┘
```

**Vue par heure (`history historique.sqlite --by-hour`) :**

```
Pattern heures de pointe — ftp.exemple.com (12 runs sur 30 jours)

Heure   Verdict moyen   Perte%   RTT moy   DL moy    Samples
──────────────────────────────────────────────────────────────
08h     SAIN            0.1      89 ms     815 Mbps    8
12h     DÉGRADÉ         0.6      95 ms     520 Mbps    8
18h     FAULTY ████     4.8     158 ms      14 Mbps    9   ← pic
19h     FAULTY ████     5.1     165 ms       9 Mbps    9   ← pic
20h     FAULTY ███      3.2     130 ms      45 Mbps    9
21h     DÉGRADÉ         1.1     110 ms     180 Mbps    8
23h     SAIN            0.0      90 ms     790 Mbps    8
```

**Vue par hop (`history historique.sqlite --hop AS1299`) :**

```
Évolution du hop AS1299 (ARELION) — ftp.exemple.com

Timestamp            Perte%   RTT moy   RTT max   StDev
──────────────────────────────────────────────────────────
2026-05-21 08:00      0.0      88 ms     91 ms     1.2
2026-05-21 18:00      4.2     143 ms    312 ms    45.1   ← dégradé
2026-05-21 19:00      6.1     161 ms    450 ms    67.8   ← dégradé
```

---

## Phase 4 — Analyse temporelle

**Fichier :** nouveau `src/report/temporal.rs`

### `detect_peak_hours(conn, target) -> Vec<PeakHourFinding>`

- Groupe les runs par heure de la journée (0–23)
- Identifie les tranches où `avg(loss_pct) > 1%` ou `avg(verdict_score) > 1.5`
- Retourne une liste de créneaux à risque avec sévérité

### `detect_degradation_trend(conn, target, last_n: usize) -> Option<TrendFinding>`

- Régression linéaire sur les N derniers runs (perte, RTT)
- Si pente positive significative → finding `TENDANCE_DÉGRADANTE`
- Exemple : *"RTT augmente de +8ms par semaine sur les 30 derniers runs"*

Ces fonctions s'appellent en fin de `history` pour ajouter un bloc de conclusions :

```
═══════════════════════════════════════════════
  Analyse temporelle (30 runs sur 14 jours)

  ✖ Congestion récurrente 18h–21h (9/9 runs dégradés ou faulty)
    → Peering AS5511↔AS1299 saturé systématiquement le soir

  ⚠ Tendance à la dégradation sur 30 jours
    → RTT moyen +12ms, débit –18% par rapport à il y a 14 jours
═══════════════════════════════════════════════
```

---

## Résumé des fichiers touchés

| Fichier | Changement |
|---|---|
| `src/report/storage.rs` | Tables `return_hop_samples`, `watch_series`, migration versionnée ; `store_return_hops()` |
| `src/lg/engine.rs` | Accepte `Option<db_path>` pour stocker les hops retour |
| `src/main.rs` | Commandes `Watch` et `History` ; `run_watch()`, `run_history()` |
| `src/report/history.rs` | *(nouveau)* Requêtes SQLite, rendu des tableaux chronologiques et par heure |
| `src/report/temporal.rs` | *(nouveau)* `detect_peak_hours`, `detect_degradation_trend` |
| `src/report/mod.rs` | Exports des nouveaux modules |

---

## Ordre d'implémentation recommandé

1. **Phase 1** (storage) — fondation, rien d'autre ne marche sans
2. **Phase 2** (watch) — génère les données à analyser
3. **Phase 3** (history, vue chronologique) — valeur immédiate, simple
4. **Phase 3** (history, vues `--by-hour` et `--hop`) — analyse plus riche
5. **Phase 4** (temporal.rs) — conclusions automatiques

Les phases 1+2+3 peuvent tenir en une session, la phase 4 en une seconde.
