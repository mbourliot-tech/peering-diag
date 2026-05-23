# peering-diag

Outil de diagnostic de peering réseau écrit en Rust. Détecte et localise les problèmes d'interconnexion entre opérateurs (peering, congestion, perte de paquets, jitter, routage sous-optimal) en combinant un MTR AS-aware, des speedtests segmentés par AS, et un traceroute retour automatique via Globalping.

---

## Sommaire

- [Ce que ça fait](#ce-que-ça-fait)
- [Installation Windows](#installation-windows)
- [Installation Linux](#installation-linux)
- [Utilisation](#utilisation)
- [Lire les résultats](#lire-les-résultats)
  - [Le tableau MTR (chemin aller)](#le-tableau-mtr-chemin-aller)
  - [Le tableau des speedtests](#le-tableau-des-speedtests)
  - [Le tableau MTR retour](#le-tableau-mtr-retour)
  - [La section Analyse](#la-section-analyse)
  - [Le verdict](#le-verdict)
  - [La sortie ecmp](#la-sortie-ecmp)
- [Privilèges réseau](#privilèges-réseau)
- [Comment ça marche](#comment-ça-marche)
- [Structure du code](#structure-du-code)
- [Limitations](#limitations)
- [Évolutions prévues](#évolutions-prévues)

---

## Ce que ça fait

Quand un transfert FTP (ou HTTP, ou autre) est lent vers un serveur précis alors que le reste fonctionne bien, le coupable est souvent une **interconnexion saturée entre deux AS (Autonomous Systems)** sur le chemin — typiquement aux heures de pointe. Un `ping` ou un `tracert` classique ne suffit pas à le prouver.

`peering-diag` automatise le diagnostic complet :

1. **MTR longue durée** — traceroute répété sur 15 rounds, annoté avec l'ASN de chaque routeur. Filtre les faux positifs (ICMP rate-limiting, ECMP). Calcule latence min/avg/max et jitter sur chaque hop.
2. **Speedtests segmentés (cascade multi-méthodes)** — mesure le débit vers chaque AS du chemin. Pour couvrir aussi les AS de transit (qui n'ont pas de serveur Speedtest grand public), l'outil tente plusieurs méthodes par ordre de fiabilité : Speedtest direct → base statique Tier-1 → iperf3 → téléchargement HTTP → Speedtest géographiquement proche. La chute de débit entre deux AS consécutifs chiffre le débit effectif de leur interconnexion.
3. **Analyse automatique** — 7 checks indépendants (perte, jitter, bufferbloat, latence, routage, peering, rate-limit) avec verdict global et actions suggérées.
4. **Chemin retour via Globalping** — un problème de peering peut être asymétrique (aller OK, retour dégradé). L'outil interroge automatiquement les sondes Globalping hébergées dans les AS du chemin aller pour tracer le retour en 5 rounds. Les statistiques (Loss%, Snt, Avg, Min, Max, StDev) sont agrégées hop-par-hop, style MTR. La même analyse en 7 points s'applique au chemin retour.
5. **Sonde ECMP TCP** — détecte le déséquilibre de charge entre plusieurs chemins parallèles (ECMP) en envoyant des connexions TCP avec des ports source différents. Chaque port force un chemin distinct dans le hash ECMP. Aucun droit administrateur requis (TCP standard, pas de raw socket).

### Exemple de verdict

```
╔══ PHASE 1 — CHEMIN ALLER ══╗

[tableau MTR + speedtests]

═══════════════════════════════════════════
  ✖ PROBLÈME DÉTECTÉ

  Problème de peering confirmé (AS5511 → AS1299).
  Le débit est dégradé à l'interconnexion entre opérateurs.
  Utiliser un VPN ou un serveur relais pour contourner.
═══════════════════════════════════════════

╔══ PHASE 2 — CHEMIN RETOUR ══╗

[tableau MTR retour via Globalping]

═══════════════════════════════════════════
  ✔ CHEMIN SAIN

  Chemin retour sain (RTT moyen : 42ms, 0% de perte).
  Aucune anomalie détectée sur le trajet retour.
═══════════════════════════════════════════
```

---

## Installation Windows

### 1. Installer Rust

Télécharger et exécuter l'installateur depuis [rustup.rs](https://rustup.rs) :

```powershell
# Ou via winget
winget install Rustlang.Rustup
```

Fermer et rouvrir PowerShell après l'installation. Vérifier :

```powershell
rustc --version
cargo --version
```

### 2. Installer les outils de build C++ (requis par certaines dépendances)

Si ce n'est pas déjà fait, installer les **Build Tools for Visual Studio** :

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
```

Pendant l'installation, cocher **"Développement Desktop en C++"** dans les workloads.

Alternative : installer Visual Studio Community (gratuit) avec le même workload.

### 3. Installer le CLI Speedtest Ookla (optionnel mais recommandé)

```powershell
winget install Ookla.Speedtest.CLI
```

Vérifier :

```powershell
speedtest --version
```

Sans ce CLI, utiliser `--no-speedtest` — le MTR fonctionne seul.

### 3 bis. Installer iperf3 (optionnel)

iperf3 sert de méthode de mesure complémentaire dans la cascade speedtest, notamment pour les AS de transit (TATA, Cogent, Arelion, Lumen, NTT, Hurricane Electric…) qui exposent des serveurs iperf3 publics mais pas de serveur Speedtest.

```powershell
winget install iPerf.iPerf3
```

Vérifier :

```powershell
iperf3 --version
```

Si ni `speedtest` ni `iperf3` ne sont installés, la phase 2 est ignorée et seul le MTR s'exécute.

### 4. Compiler peering-diag

```powershell
# Cloner ou extraire les sources
cd C:\chemin\vers\peering-diag

# Compiler en mode release
cargo build --release
```

La compilation prend 2 à 5 minutes la première fois (téléchargement et compilation des dépendances). Les compilations suivantes sont quasi-instantanées.

Le binaire est dans :
```
target\release\peering-diag.exe
```

### 5. Lancer en administrateur

Les raw sockets ICMP (nécessaires au MTR) requièrent les droits administrateur sous Windows.

**Méthode recommandée** : ouvrir PowerShell en tant qu'administrateur avant de lancer.

```powershell
# Depuis un PowerShell admin :
cd C:\chemin\vers\peering-diag
.\target\release\peering-diag.exe diag ftp.exemple.com
```

> Les commandes `retour` et `lg` n'utilisent que des requêtes HTTP (API Globalping) — elles fonctionnent sans droits élevés.

### Vérification de l'installation

```powershell
.\target\release\peering-diag.exe check-env
```

Sortie attendue :
```
Vérification de l'environnement...
  ✔ speedtest CLI : Speedtest by Ookla 1.2.0.84 (ea6b6773cf) Windows AMD64
  ✔ iperf3 : disponible
```

`iperf3` est optionnel : s'il manque, la ligne affiche un avertissement mais le diagnostic fonctionne (la cascade utilise les autres méthodes).

---

## Installation Linux

### 1. Installer Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Vérifier :

```bash
rustc --version
cargo --version
```

### 2. Installer les dépendances système

**Debian / Ubuntu :**

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev
```

**Fedora / RHEL / Rocky :**

```bash
sudo dnf install -y gcc pkg-config openssl-devel
```

**Arch Linux :**

```bash
sudo pacman -S base-devel pkg-config openssl
```

### 3. Installer le CLI Speedtest Ookla (optionnel)

**Debian / Ubuntu :**

```bash
# Ajouter le dépôt Ookla
curl -s https://packagecloud.io/install/repositories/ookla/speedtest-cli/script.deb.sh | sudo bash
sudo apt install speedtest
```

**Fedora / RHEL :**

```bash
curl -s https://packagecloud.io/install/repositories/ookla/speedtest-cli/script.rpm.sh | sudo bash
sudo dnf install speedtest
```

**Arch Linux (AUR) :**

```bash
yay -S speedtest-cli-bin
# ou
paru -S speedtest-cli-bin
```

Vérifier :

```bash
speedtest --version
```

### 3 bis. Installer iperf3 (optionnel)

Méthode de mesure complémentaire pour les AS de transit qui exposent un serveur iperf3 public.

```bash
# Debian / Ubuntu
sudo apt install -y iperf3

# Fedora / RHEL
sudo dnf install -y iperf3

# Arch Linux
sudo pacman -S iperf3
```

Si ni `speedtest` ni `iperf3` ne sont installés, la phase 2 est ignorée et seul le MTR s'exécute.

### 4. Compiler peering-diag

```bash
cd /chemin/vers/peering-diag
cargo build --release
```

Le binaire est dans :
```
target/release/peering-diag
```

### 5. Gérer les privilèges réseau

Les raw sockets ICMP nécessitent soit root soit la capability `CAP_NET_RAW`.

**Option A — capability (recommandé, pas besoin de sudo à chaque fois) :**

```bash
sudo setcap cap_net_raw=eip ./target/release/peering-diag
```

À refaire après chaque recompilation.

**Option B — sudo à chaque lancement :**

```bash
sudo ./target/release/peering-diag diag ftp.exemple.com
```

**Option C — script wrapper pratique :**

```bash
# Créer /usr/local/bin/peering-diag
sudo install -m 755 ./target/release/peering-diag /usr/local/bin/peering-diag
sudo setcap cap_net_raw=eip /usr/local/bin/peering-diag
# Puis lancer depuis n'importe où sans sudo :
peering-diag diag ftp.exemple.com
```

> Les commandes `retour` et `lg` n'utilisent que HTTP (API Globalping) — `setcap` n'est pas requis pour elles.

### Vérification de l'installation

```bash
./target/release/peering-diag check-env
```

---

## Utilisation

### Commandes disponibles

| Commande | Description | Durée typique |
|---|---|---|
| `peering-diag diag <cible>` | Diagnostic complet : chemin aller (MTR + speedtests) **puis** chemin retour (Globalping) | 8 à 15 min |
| `peering-diag aller <cible>` | Chemin aller uniquement : MTR + speedtests + analyse | 5 à 10 min |
| `peering-diag retour <cible>` | Chemin retour uniquement : 5 rounds Globalping + analyse symétrique | 2 à 3 min |
| `peering-diag lg <cible>` | Chemin retour + URLs Looking Glass pour investigation manuelle | 2 à 3 min |
| `peering-diag mtr <cible>` | MTR brut : chemin réseau + analyse (sans speedtests) | 2 à 3 min |
| `peering-diag ecmp <cible>` | Sonde ECMP TCP : détecte un déséquilibre de charge sur N chemins parallèles | 30 s à 2 min |
| `peering-diag check-env` | Vérifie que speedtest CLI et iperf3 sont installés | Immédiat |

### Options de `aller`

`aller` remplace l'ancienne commande `diag` pour le chemin aller uniquement.

| Option | Défaut | Description |
|---|---|---|
| `--rounds` | `15` | Rounds MTR. Plus = plus précis mais plus long. 10 minimum, 30 recommandé pour un diagnostic sérieux. |
| `--probes` | `3` | Probes par TTL par round. Augmenter à 5 pour plus de précision sur la perte. |
| `--max-hops` | `30` | TTL maximum. Rarement utile de modifier. |
| `--no-speedtest` | désactivé | Skip la phase 2 (speedtests). Utile pour un diagnostic rapide ou si le CLI Ookla n'est pas installé. |
| `--max-speedtests` | `5` | Nombre max de speedtests à lancer (un par AS du chemin). |
| `--json <fichier>` | — | Exporte le rapport complet en JSON. Utile pour partager ou archiver. |
| `--db <fichier>` | — | Stocke le résultat en base SQLite. Permet de comparer plusieurs runs dans le temps. |

### Options de `diag`

`diag` enchaîne `aller` puis `retour`. Il accepte toutes les options de `aller` plus :

| Option | Défaut | Description |
|---|---|---|
| `--my-ip <IP>` | auto-détecté | IP publique à utiliser comme cible du traceroute retour. Détectée automatiquement via un service tiers si non fournie. |

### Options de `retour`

| Option | Défaut | Description |
|---|---|---|
| `--my-ip <IP>` | auto-détecté | IP publique à utiliser comme cible du traceroute retour. Utile si la détection automatique échoue (VPN, NAT à plusieurs niveaux). |

### Options de `mtr`

| Option | Défaut | Description |
|---|---|---|
| `--rounds` | `15` | Rounds MTR. |
| `--max-hops` | `30` | TTL maximum. |

### Options de `ecmp`

| Option | Défaut | Description |
|---|---|---|
| `--port` | `443` | Port TCP destination. Utiliser le port du service réel (21 pour FTP/FTPS, 443 pour HTTPS, 22 pour SSH…). |
| `--flows` | `8` | Nombre de chemins ECMP à sonder. Chaque flow utilise un port source différent pour forcer un chemin distinct dans le hash de répartition. |
| `--probes` | `5` | Nombre de probes par flow. Plus = mesures de perte et de RTT plus précises. |
| `--ttl` | `64` | TTL des paquets TCP. La valeur par défaut (64) atteint la cible. Réduire pour limiter la mesure à N hops. |

> **Aucun droit administrateur requis** — la sonde `ecmp` utilise des connexions TCP standard (socket stream), pas de raw socket. Elle fonctionne en utilisateur normal, contrairement aux commandes `diag`, `aller` et `mtr`.

### Exemples

```bash
# Diagnostic complet aller + retour (commande principale)
peering-diag diag ftp.exemple.com

# Diagnostic complet avec IP publique spécifiée manuellement (derrière un VPN)
peering-diag diag ftp.exemple.com --my-ip 203.0.113.42

# Chemin aller uniquement (MTR + speedtests)
peering-diag aller ftp.exemple.com

# Chemin aller approfondi (30 rounds, 5 probes/round)
peering-diag aller ftp.exemple.com --rounds 30 --probes 5

# Chemin aller avec export JSON pour partage ou archivage
peering-diag aller ftp.exemple.com --json rapport.json

# Chemin aller avec historisation SQLite
peering-diag aller ftp.exemple.com --db historique.sqlite

# Chemin retour uniquement (Globalping, sans droits admin)
peering-diag retour ftp.exemple.com

# Chemin retour + URLs Looking Glass pour investigation manuelle
peering-diag lg ftp.exemple.com

# MTR seulement (sans speedtest, diagnostic rapide)
peering-diag mtr ftp.exemple.com

# Sonde ECMP : vérifier l'équilibre de charge vers un serveur HTTPS (8 flows)
peering-diag ecmp monserveur.example.com

# Sonde ECMP sur le port FTP/FTPS
peering-diag ecmp ftp.example.com --port 21

# Sonde ECMP approfondie : 12 flows, 10 probes par flow
peering-diag ecmp monserveur.example.com --flows 12 --probes 10
```

---

## Lire les résultats

### Le tableau MTR (chemin aller)

```
+-----+-----------------+-------------------------------+-------+------------+--------+------+------+-------+--------+------------+
| TTL | IP              | Hostname                      | ASN   | AS Name    | Loss%  | Min  | Avg  | Max   | Jitter | Note       |
+-----+-----------------+-------------------------------+-------+------------+--------+------+------+-------+--------+------------+
| 1   | 192.168.1.1     | livebox.home                  |       |            | 0.0    | 0.2  | 1.1  | 3.8   | 0.8    |            |
| 2   | 80.15.224.213   |                               |       |            | 0.0    | 0.9  | 2.7  | 9.2   | 1.5    |            |
| 3   | 81.253.130.5    | lag-41.nipoi204.rbci.orange.. |       |            | 88.9*  | 4.3  | 7.2  | 12.9  | 3.8    | rate-limit |
| 4   | 80.231.246.26   | ix-bundle-22.qcore2.pvu-par.. | 6453  | TATA COMM  | 86.7*  | 7.9  | 9.0  | 13.8  | 1.6    | rate-limit |
| 5   | 80.231.245.12   | if-bundle-12-2.qcore1.pvu-p.. | 6453  | TATA COMM  | 4.2    | 87.7 | 88.9 | 91.6  | 1.2    |            |
```

**Colonnes :**

- **TTL** : numéro du saut (1 = ton routeur, dernier = la cible)
- **IP / Hostname** : identité du routeur à ce saut
- **ASN / AS Name** : opérateur propriétaire de ce routeur
- **Loss%** : taux de perte. Un `*` indique que c'est probablement de l'ICMP rate-limiting, pas une vraie perte
- **Min / Avg / Max** : latence en millisecondes (RTT)
- **Jitter** : instabilité de la latence. Élevé = congestion probable
- **Note** : `rate-limit` = perte apparente non réelle, `ECMP` = plusieurs chemins parallèles détectés

**Pièges à éviter :**

- Une perte de 80-100% sur un hop intermédiaire n'est PAS forcément un problème — si les hops suivants ont 0% de perte, c'est du rate-limiting ICMP normal
- Un bond de RTT de 70-80ms entre deux hops consécutifs peut être normal si c'est la traversée transatlantique (France → États-Unis)
- La colonne Jitter est l'indicateur le plus fiable de congestion réelle

### Le tableau des speedtests

```
+-------+------------+--------------------------+---------+---------+---------+--------+----------+
| AS    | AS Name    | Endpoint                 | DL Mbps | UL Mbps | Ping ms | Δ DL   | Méthode  |
+-------+------------+--------------------------+---------+---------+---------+--------+----------+
| 5511  | ORANGE     | Paris (Orange)           | 820.1   | 198.2   | 12.1    | —      | direct   |
| 1299  | ARELION    | Arelion Frankfurt        | 11.4    | 95.3    | 95.2    | -808.7 | tier1-db |
+-------+------------+--------------------------+---------+---------+---------+--------+----------+
```

La colonne **Δ DL** (delta download) est la clé : elle montre la chute de débit entre deux AS consécutifs. Une chute brutale identifie le segment problématique.

Dans cet exemple : 820 Mbps chez Orange → 11 Mbps chez Arelion = l'interconnexion Orange↔Arelion est saturée.

La colonne **Méthode** indique comment la mesure a été obtenue (par fiabilité décroissante) :

- `direct` — serveur Speedtest situé dans l'AS exact (le plus fiable)
- `tier1-db` — serveur Speedtest connu en dur pour cet AS (base statique Tier-1)
- `iperf3` — serveur iperf3 public proche
- `http` — téléchargement d'un fichier de test HTTP (download seul, pas d'upload ni de ping)
- `proxy` — aucun serveur trouvé pour cet AS : on utilise le premier serveur Speedtest local disponible. La mesure reflète **ton débit d'accès local**, pas le chemin vers cet AS. Le delta Δ DL est supprimé pour cette ligne.

Les colonnes `UL Mbps` et `Ping ms` affichent `—` quand la méthode ne les fournit pas (cas du `http`).

### Le tableau MTR retour

Le chemin retour est mesuré depuis des sondes Globalping hébergées dans les AS identifiés sur le chemin aller. Les résultats sont agrégés sur 5 rounds, style MTR :

```
+-----+----------------------------------+-------+------------+--------+-----+---------+------+------+------+-------+
| Hop | Hôte                             | ASN   | Opérateur  | Perte% | Snt | Dernier | Moy  | Min  | Max  | StDev |
+-----+----------------------------------+-------+------------+--------+-----+---------+------+------+------+-------+
|   1 | core1.as6453.par-gw.tata.net     | 6453  | TATA COMM  |   0.0  |   5 |    8.2  |  7.9 |  7.4 |  8.5 |   0.4 |
|   2 | ix-ae1.qcore2.lon.tata.net       | 6453  | TATA COMM  |   0.0  |   5 |   13.1  | 12.8 | 12.3 | 13.5 |   0.4 |
|   3 | 80.15.201.14                     | 5511  | ORANGE FR  |  20.0  |   5 |   15.4  | 14.9 | 14.1 | 16.2 |   0.7 |
|   4 | 192.168.1.1                      |       |            |   0.0  |   5 |   16.1  | 15.8 | 15.3 | 16.5 |   0.4 |
+-----+----------------------------------+-------+------------+--------+-----+---------+------+------+------+-------+
```

**Colonnes :**

- **Hop** : numéro du saut depuis la sonde Globalping (côté cible) vers ton IP publique
- **Hôte** : hostname résolu ou adresse IP du routeur
- **ASN / Opérateur** : AS propriétaire du routeur, résolu par l'outil
- **Perte%** : taux de perte agrégé sur N rounds Globalping
- **Snt** : nombre total de paquets envoyés (rounds × probes)
- **Dernier** : RTT du dernier paquet reçu, en ms
- **Moy / Min / Max / StDev** : statistiques RTT agrégées sur tous les rounds, en ms

**Lecture :**

Le chemin retour est souvent différent du chemin aller — les routeurs ne font pas nécessairement la même décision BGP dans les deux sens. Un problème de peering peut être **asymétrique** : le chemin aller peut être sain (RTT normal, 0% de perte) pendant que le retour passe par un lien congestionné.

La perte sur un hop intermédiaire du retour doit être interprétée comme pour l'aller : si les hops suivants ont 0% de perte, c'est probablement du rate-limiting ICMP.

### La section Analyse

Entre le tableau et le verdict, chaque anomalie est détaillée. Le format est identique pour le chemin aller et le chemin retour :

```
  ✖ [PEERING] Chute de débit majeure à l'interconnexion ORANGE → ARELION : -98%
    820 Mbps (ORANGE) → 11 Mbps (ARELION). Perte 809 Mbps (98% du débit).
    → Peering congestionné entre AS5511 et AS1299. Tester via un VPN avec
      sortie différente pour confirmer. Si confirmé, contacter le FAI avec
      ce rapport comme preuve.

  ⚠ [JITTER] Jitter élevé au hop 5 : 45ms
    RTT varie de 25ms à 180ms (jitter 45ms). Peut indiquer une congestion
    intermittente.
    → Relancer le test à différentes heures pour voir si le jitter augmente
      en heure de pointe (18h-23h).

  ℹ [INFO] 3 hop(s) limitent leurs réponses ICMP — perte apparente non réelle
    hop 3 (89%), hop 4 (87%), hop 6 (100%)
```

Catégories de findings :
- **[PERTE]** — perte de paquets réelle
- **[LATENCE]** — latence excessive
- **[JITTER]** — instabilité de latence (congestion)
- **[BUFFERBLOAT]** — files d'attente saturées
- **[PEERING]** — problème d'interconnexion entre AS
- **[ROUTAGE]** — chemin sous-optimal ou détour géographique
- **[INFO]** — observation neutre (rate-limit, latence physique normale)

### Le verdict

Chaque rapport se termine par un verdict global :

```
═══════════════════════════════════════════
  ✔ CHEMIN SAIN

  Chemin réseau sain (RTT moyen : 89ms). Débit maximal mesuré :
  935 Mbps. Aucune anomalie détectée sur ce run. Si des problèmes
  persistent, relancer le test en heure de pointe.
═══════════════════════════════════════════
```

Trois états possibles :
- **✔ CHEMIN SAIN** — aucune anomalie détectée
- **⚠ DÉGRADÉ** — anomalies mineures, surveillance recommandée
- **✖ PROBLÈME DÉTECTÉ** — problème confirmé avec cause identifiée et action suggérée

### La sortie `ecmp`

```
Cible : monserveur.example.com (203.0.113.10) port 443
Exploration de 8 chemins ECMP (5 probes/flux, TTL 64)…

SrcPort   Perte      Min    Médian       Max     Issue
──────────────────────────────────────────────────────
33434        0%     21.3      21.5      22.1   ouvert
33435        0%     21.2      21.4      21.9   ouvert
33436       60%     21.4     145.2     312.0   ouvert   ← dégradé
33437        0%     21.1      21.6      22.4   ouvert
33438        0%     21.3      21.5      22.0   ouvert
33439       80%    120.1     210.4     450.2   timeout  ← dégradé
33440        0%     21.5      21.7      22.3   ouvert
33441        0%     21.2      21.4      21.8   ouvert

═══════════════════════════════════════════
  ✖ Déséquilibre ECMP détecté — 2/8 chemins dégradés.
     Baseline : 21.5 ms   Flows dégradés : 33436, 33439
═══════════════════════════════════════════
```

**Lecture :**

- **SrcPort** : port source utilisé pour ce flow. Les routeurs ECMP hashent le 5-tuple (src IP, dst IP, src port, dst port, protocole) — chaque port source force un chemin différent.
- **Perte** : taux de probes sans réponse TCP (ni SYN-ACK ni RST reçu dans le délai).
- **Min / Médian / Max** : RTT du TCP handshake en millisecondes.
- **Issue** : `ouvert` = serveur a répondu (SYN-ACK ou RST), `timeout` = aucune réponse.

**Verdict :**

- **✔ Chemins ECMP homogènes** — tous les flows ont des RTT et des taux de perte similaires : aucun chemin n'est congestionné, ou il n'y a qu'un seul chemin physique (pas d'ECMP).
- **✖ Déséquilibre ECMP détecté** — un ou plusieurs flows présentent une perte ou un RTT nettement supérieurs aux autres. Indique qu'un lien ECMP spécifique est congestionné ou défaillant. Corrélé avec le port source, ce résultat permet de confirmer un problème de hash de répartition de charge.

> Un résultat homogène ne signifie pas forcément l'absence d'ECMP — si tous les chemins sont sains, ils auront des RTT identiques. La sonde est utile quand il y a un problème : elle révèle si la dégradation touche tous les flows (problème global) ou seulement certains (déséquilibre ECMP).

---

## Privilèges réseau

Les commandes `diag`, `aller` et `mtr` utilisent des **raw sockets ICMP** pour envoyer des paquets avec un TTL contrôlé (technique du traceroute). Ces sockets nécessitent des droits élevés sur tous les OS.

Les commandes `retour` et `lg` n'utilisent que des **requêtes HTTP** vers l'API Globalping — **aucun droit élevé requis**.

La commande `ecmp` utilise des **connexions TCP standard** (`connect()` via socket stream) — **aucun droit élevé requis**.

| Commande | Windows | Linux | macOS |
|---|---|---|---|
| `diag`, `aller`, `mtr` | PowerShell en Administrateur | `sudo setcap cap_net_raw=eip ./peering-diag` | `sudo ./peering-diag` |
| `retour`, `lg` | Utilisateur normal | Utilisateur normal | Utilisateur normal |
| `ecmp` | Utilisateur normal | Utilisateur normal | Utilisateur normal |
| `check-env` | Utilisateur normal | Utilisateur normal | Utilisateur normal |

> Sur Linux, après chaque recompilation il faut réappliquer `setcap` car l'exécutable est remplacé. Les commandes `retour`, `lg` et `ecmp` ne sont pas affectées par cette contrainte.

**Pourquoi `diag`/`aller`/`mtr` nécessitent root ?** Les raw sockets permettent de forger des paquets IP arbitraires — les OS les réservent aux administrateurs. `peering-diag` n'utilise cette capacité que pour envoyer des Echo Request ICMP avec TTL contrôlé.

---

## Comment ça marche

### Phase 1 — MTR raffiné

L'outil envoie des paquets ICMP Echo Request avec un TTL incrémental (1, 2, 3, ...). Chaque routeur sur le chemin décrémente le TTL et, quand il atteint 0, renvoie un message "Time Exceeded" en révélant son adresse IP — c'est le principe du traceroute.

Améliorations par rapport à un `tracert` classique :

**Plusieurs rounds** — 15 rounds par défaut au lieu de 3. On calcule des statistiques complètes (min, avg, max, jitter) au lieu d'avoir 3 mesures isolées. Ça permet de détecter les congestions intermittentes.

**Lookup ASN automatique** — chaque IP est résolue vers son ASN (numéro d'AS) via le service whois de Team Cymru, puis vers son nom d'opérateur. Cache local de 24h. Fallback automatique vers ipinfo.io si Cymru ne répond pas.

**Détection ICMP rate-limiting** — beaucoup de routeurs limitent leurs réponses ICMP Time Exceeded (RFC 1812) sans pour autant avoir de problème. L'heuristique : un hop qui montre de la perte mais dont les hops suivants ont 0% de perte est marqué "rate-limit". Sa perte est affichée en gris avec un `*` et ignorée dans l'analyse.

**Détection ECMP** — les backbones modernes répartissent le trafic sur plusieurs chemins parallèles. Un même TTL peut donc donner plusieurs IP de réponse différentes. L'outil les trace toutes et annote le hop `ECMP`.

**Reverse DNS parallèle** — résolution des hostnames en parallèle (16 simultanés) avec timeout de 1.5s par IP. Pas de blocage même si le DNS ne répond pas.

### Phase 2 — Speedtests segmentés (cascade multi-méthodes)

Le CLI Speedtest Ookla retourne la liste des serveurs les plus proches géographiquement. L'outil résout chaque serveur vers son ASN et les groupe par opérateur. Pour chaque AS traversé dans le chemin MTR, on cherche à mesurer le débit ; le delta entre deux AS consécutifs donne le débit effectif de leur interconnexion.

Le problème : les AS de transit (TATA, Arelion, Cogent, Lumen, NTT, Hurricane Electric…) n'hébergent pas de serveur Speedtest grand public — ce sont des backbones, pas des FAI. Pour les couvrir quand même, chaque AS passe par une **cascade de méthodes** essayées dans l'ordre, de la plus fiable à la plus approximative. La première qui réussit gagne :

1. **Speedtest direct** — un serveur Speedtest se trouve dans l'AS exact.
2. **Base Tier-1 statique** — base d'endpoints connus maintenue en dur (`tier1_db.rs`), associant un AS à un serveur Speedtest, iperf3 ou fichier HTTP de test vérifié manuellement (serveurs officiels Orange, Free, Bouygues, SFR, OVH, Hetzner, Hurricane Electric…).
3. **iperf3** — test de débit TCP brut vers un serveur iperf3 public proche (4 flux parallèles, 8 s par sens, warmup ignoré). Indépendant du protocole Ookla.
4. **Téléchargement HTTP** — récupération d'un fichier de test public (jusqu'à 50 MB) avec mesure du débit réel obtenu. Download uniquement.
5. **Proxy local** — aucun serveur trouvé pour cet AS : le premier serveur Speedtest local disponible est utilisé comme proxy. La mesure reflète le débit d'accès local, pas le chemin vers l'AS cible. Le Δ DL est supprimé pour cette ligne.

Si aucune méthode n'aboutit pour un AS, il est ignoré et signalé. Un cooldown sépare les tests pour ne pas saturer la connexion et fausser les mesures.

> La base Tier-1 (`tier1_db.rs`) est statique et maintenue manuellement : vérifier périodiquement que les serveurs référencés sont toujours actifs.

### Phase 3 — Chemin retour via Globalping

Un problème de peering peut être **asymétrique** : le FAI A envoie bien le trafic vers B (aller OK), mais le FAI B achemine le retour par un lien saturé vers A. Le MTR aller ne voit pas ça.

Pour mesurer le chemin retour, l'outil :

1. **Identifie les AS du chemin aller** — extrait les ASN des hops MTR avec changement d'opérateur.
2. **Détecte l'IP publique** — via un service tiers (ou l'option `--my-ip`), pour savoir quelle IP cibler depuis la sonde.
3. **Choisit la meilleure sonde Globalping** — pour chaque AS du chemin, cherche une sonde disponible dans cet AS, puis dans la ville détectée depuis les hostnames MTR, puis dans le pays. Prend la première qui répond.
4. **Lance N rounds** (5 par défaut) de traceroute depuis cette sonde vers l'IP publique, avec `limit: 1` par round pour forcer la même sonde.
5. **Agrège par hop** — merge les RTTs par TTL sur tous les rounds. Calcule Loss%, Snt, Avg, Min, Max, StDev exactement comme MTR.
6. **Résout les ASN** des hops retour — via `AsnResolver`, en parallèle (8 simultanés).
7. **Analyse les résultats** — 7 checks identiques au chemin aller : perte, jitter, bufferbloat, latence, routage, peering, rate-limit. Verdict indépendant.

Globalping est un service public gratuit (~500 mesures/heure, sans clé API) avec des sondes distribuées dans des centaines d'AS dans le monde.

### Phase 4 — Sonde ECMP TCP

Les réseaux de backbone modernes répartissent le trafic sur plusieurs liens parallèles (ECMP — Equal-Cost Multi-Path) en hashant le 5-tuple IP (IP source, IP destination, port source, port destination, protocole). Un MTR classique envoie tous ses probes avec le même 5-tuple : il mesure toujours le même chemin, et rate les autres.

La commande `ecmp` contourne ça en variant le **port source** d'une connexion TCP standard : chaque port force un chemin différent dans le hash ECMP. Aucun raw socket n'est nécessaire — c'est un `connect()` TCP ordinaire avec `SO_REUSEADDR` et `SO_LINGER 0` (fermeture par RST, sans TIME_WAIT pour permettre la réutilisation immédiate du port).

Le RTT mesuré est le RTT du TCP handshake (SYN → SYN-ACK). Il est indépendant du protocole applicatif — FTPS, HTTPS, SSH, etc. — tant que le port TCP est ouvert. Si le serveur répond RST (port fermé), le RTT est quand même capturé : le RST revient par le même chemin ECMP que le SYN-ACK l'aurait fait.

La détection de déséquilibre utilise deux critères, comparés à la baseline (médiane du flow le plus rapide) :
- **Perte** : dégradé si `perte_flow > baseline_perte + 20%`
- **RTT** : dégradé si `médiane_flow > baseline + max(20ms, 50% de la baseline)`

### Analyse automatique

7 checks indépendants s'exécutent sur les données collectées — identiques pour le chemin aller et le chemin retour :

| Check | Seuils | Catégorie |
|---|---|---|
| Perte sur la cible | >0.5% warning, >5% critical | PERTE |
| Perte propagée sur le chemin | >1% sur 3+ hops consécutifs | PERTE / PEERING |
| Jitter | >20ms warning, >50ms critical (relatif au RTT) | JITTER |
| Bufferbloat | ratio max/min RTT >5x sur même hop | BUFFERBLOAT |
| Latence finale | >80ms (EU) ou >150ms (US) warning | LATENCE |
| Chute de débit speedtest | >20% warning, >50% critical | PEERING |
| Détour géographique | pattern ville A → B → A dans les hostnames | ROUTAGE |

Le verdict global synthétise tous les findings et génère une conclusion en langage naturel avec une action suggérée.

---

## Structure du code

```
src/
├── main.rs                  CLI (clap) + orchestration des phases
├── lib.rs                   exports publics
├── types.rs                 structures partagées : Hop, AsInfo, Finding, Verdict...
│
├── asn/
│   ├── mod.rs
│   └── lookup.rs            lookup IP→ASN via Cymru whois bulk + fallback ipinfo.io
│
├── mtr/
│   ├── mod.rs
│   ├── probe.rs             envoi paquets ICMP avec TTL contrôlé (socket2)
│   ├── engine.rs            boucle de rounds parallèles + enrichissement AS + DNS
│   ├── heuristics.rs        détection rate-limit, ECMP, point de dégradation
│   └── tcp_probe.rs         sonde ECMP TCP (connect() multi-flow, détection déséquilibre)
│
├── speedtest/
│   ├── mod.rs
│   ├── servers.rs           récupération liste serveurs via `speedtest --servers`
│   ├── filter.rs            résolution ASN des serveurs + groupement
│   ├── runner.rs            wrapper du CLI Ookla (`speedtest --format=json`)
│   ├── cascade.rs           orchestration de la cascade multi-méthodes par AS
│   ├── tier1_db.rs          base statique d'endpoints connus pour les AS de transit
│   ├── iperf.rs             wrapper iperf3 (download/upload, JSON)
│   └── http_measure.rs      mesure de débit par téléchargement HTTP
│
├── lg/
│   ├── mod.rs               exports + résolution hostname→IP
│   ├── engine.rs            orchestration : MTR discovery + Globalping + LG manuel
│   ├── globalping.rs        client Globalping API (traceroute N rounds, agrégation MTR-style)
│   ├── analyzer.rs          analyse symétrique du chemin retour (7 checks, verdict)
│   ├── query.rs             requêtes Looking Glass manuelles
│   ├── db.rs                base de serveurs LG connus par AS
│   └── myip.rs              détection de l'IP publique de l'utilisateur
│
└── report/
    ├── mod.rs
    ├── analyzer.rs          7 checks indépendants + génération du verdict (chemin aller)
    ├── display.rs           affichage terminal coloré (comfy-table + colored)
    └── storage.rs           export JSON + persistance SQLite (rusqlite)
```

### Dépendances principales

| Crate | Usage |
|---|---|
| `tokio` | Runtime async |
| `socket2` | Raw sockets ICMP pour le MTR |
| `hickory-resolver` | Résolution DNS (forward + reverse) |
| `reqwest` | Requêtes HTTP (API Globalping, ipinfo.io fallback, API Speedtest) |
| `moka` | Cache async (résultats ASN) |
| `rusqlite` | Persistance SQLite |
| `colored` | Couleurs terminal cross-platform |
| `comfy-table` | Tableaux ASCII dans le terminal |
| `indicatif` | Barres de progression |
| `clap` | Parsing des arguments CLI |
| `serde` / `serde_json` | Sérialisation JSON |
| `chrono` | Timestamps |
| `anyhow` / `thiserror` | Gestion d'erreurs |

---

## Limitations

**Speedtest sur AS de transit** — TATA, Arelion, Cogent et autres backbones Tier-1 n'hébergent pas de serveurs Speedtest. La cascade multi-méthodes (base Tier-1, iperf3, HTTP, géo) couvre une partie de ces AS via des endpoints connus, mais la base est statique et maintenue manuellement : un AS de transit non référencé reste non mesuré (l'outil le signale et l'ignore). La mesure iperf3/HTTP n'est par ailleurs pas strictement comparable à un test Ookla.

**ECMP au niveau des hops intermédiaires** — le MTR (`diag`, `aller`, `mtr`) utilise le même 5-tuple IP pour tous ses probes, donc on mesure toujours le même chemin ECMP parmi N sur le trajet complet. La commande `ecmp` corrige ce problème **au niveau de la cible finale** (en variant le port source TCP), mais ne cartographie pas encore les chemins hop-par-hop. Pour une exploration complète des chemins intermédiaires, il faudrait un paris-traceroute TCP avec pcap (voir Évolutions prévues).

**Chemin retour dépend des sondes Globalping** — si aucune sonde n'est disponible dans les AS du chemin aller, l'outil tombe en fallback par pays. Si le pays lui-même n'a pas de sonde, la commande `retour` échoue avec un message explicatif. La couverture de Globalping est bonne sur les grands opérateurs et centres de données, mais peut manquer de sondes sur les FAI régionaux ou les AS de transit purs.

**Statistiques retour moins granulaires** — les rounds Globalping sont séquentiels et passent par l'API REST (HTTP). Les délais inter-rounds incluent le temps de création de mesure et de polling. Les statistiques (Avg, StDev…) sont fiables mais sur un volume de paquets plus faible qu'un MTR natif en 15 rounds.

**Instantané** — un seul run ne capture pas les problèmes intermittents (congestion uniquement aux heures de pointe). Relancer plusieurs fois à des heures différentes, ou utiliser un mode de surveillance périodique.

**Mapping IP→ASN** — l'ASN affiché est celui du préfixe BGP annoncé par l'IP, pas forcément l'opérateur physique du routeur. Sur les liens de transit, une IP peut appartenir à un AS alors que le routeur est opéré par un autre. En cas de doute, croiser avec [PeeringDB](https://www.peeringdb.com/).

**Privilèges requis** — les raw sockets ICMP nécessitent des droits élevés sur tous les OS pour les commandes `diag`, `aller` et `mtr`. Voir la section [Privilèges réseau](#privilèges-réseau).

---

## Évolutions prévues

- [ ] **Mode `watch`** — surveillance périodique (toutes les N minutes), stockage SQLite, détection automatique des patterns horaires
- [ ] **Dashboard web** — visualisation de l'historique via Axum + Chart.js (corrélation heure du jour ↔ débit, heatmap)
- [x] **Sonde ECMP TCP (Phase A)** — exploration multi-flow via connect() avec port source variable, détection de déséquilibre à la cible finale (sans droits admin)
- [ ] **Paris-traceroute TCP (Phase B)** — raw SYN + pcap pour mesurer les hops intermédiaires sur chaque chemin ECMP, pas seulement la cible finale
- [x] **Chemin retour automatique** — traceroute retour via sondes Globalping distribuées dans les AS du chemin aller, agrégation MTR-style (Loss%, Avg, Min, Max, StDev), analyse symétrique en 7 points, verdict indépendant
- [ ] **Intégration RIPE Atlas** — complément à Globalping pour les AS sans sonde disponible ; permet aussi de mesurer depuis des sondes résidentielles (pas seulement datacenter)
- [ ] **Mode `vpn-pivot`** — relancer automatiquement les mesures via plusieurs configs WireGuard pour confirmer un problème de peering par contournement
- [ ] **Support IPv6** — les probes ICMP actuels sont IPv4 uniquement

---

## Licence

MIT
