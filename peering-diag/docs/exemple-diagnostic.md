# Exemple de diagnostic — ftp.unicron.network

Cible : serveur FTPS aux États-Unis (69.10.40.110), accessible depuis une connexion Orange France (AS5511).  
Objectif : comprendre pourquoi les transferts sont parfois lents en soirée.

---

## 1. Diagnostic complet

```powershell
.\peering-diag.exe diag ftp.unicron.network --rounds 15
```

### Sortie MTR

```
peering-diag v0.1.0

Phase 1 — MTR (15 rounds, 3 probes/TTL)
Résolution DNS de ftp.unicron.network… 69.10.40.110

+-----+----------------+--------------------------------------------+-------+------------+--------+------+------+------+--------+------------+
| TTL | IP             | Hostname                                   | ASN   | AS Name    | Loss%  | Min  | Avg  | Max  | Jitter | Note       |
+-----+----------------+--------------------------------------------+-------+------------+--------+------+------+------+--------+------------+
|   1 | 192.168.1.1    | livebox.home                               |       |            |   0.0  |  0.3 |  0.9 |  3.1 |    0.5 |            |
|   2 | 80.15.224.213  |                                            |       |            |   0.0  |  0.8 |  2.2 |  7.4 |    1.1 |            |
|   3 | 81.253.130.5   | lag-41.nipoi204.rbci.orange.fr             |       |            |  88.9* |  4.1 |  6.8 | 11.2 |    2.9 | rate-limit |
|   4 | 80.231.246.26  | ix-bundle-22.qcore2.pvu-par.as6453.net     |  6453 | TATA COMM  |  86.7* |  7.5 |  9.3 | 14.1 |    1.8 | rate-limit |
|   5 | 80.231.245.12  | if-bundle-12-2.qcore1.pvu-par.as6453.net   |  6453 | TATA COMM  |   4.2  | 87.4 | 88.6 | 91.3 |    1.1 |            |
|   6 | 64.86.252.97   | if-ae-40-2.tcore1.nyv-new-york.as6453.net  |  6453 | TATA COMM  |   0.0  | 87.9 | 89.1 | 92.4 |    1.3 |            |
|   7 | 216.6.57.5     |                                            |  3257 | GTT        |   0.0  | 88.1 | 89.4 | 91.8 |    1.2 |            |
|   8 | 69.10.40.110   | ftp.unicron.network                        | 40244 | PSYCHZ     |   0.0  | 88.0 | 89.2 | 92.1 |    1.3 |            |
+-----+----------------+--------------------------------------------+-------+------------+--------+------+------+------+--------+------------+
```

### Sortie speedtests

```
Phase 2 — Speedtests segmentés

  [direct]   AS5511 ORANGE → Paris (Orange) ………………………………… 819.4 / 197.8 Mbps  ping 11.2 ms
  [iperf3]   AS6453 TATA   → TATA Frankfurt (iperf) …………………… 336.1 / 312.4 Mbps

+-------+------------+--------------------------------+---------+---------+---------+---------+----------+
| AS    | AS Name    | Endpoint                       | DL Mbps | UL Mbps | Ping ms | Δ DL    | Méthode  |
+-------+------------+--------------------------------+---------+---------+---------+---------+----------+
|  5511 | ORANGE     | Paris (Orange)                 |   819.4 |   197.8 |    11.2 | —       | direct   |
|  6453 | TATA COMM  | TATA Frankfurt (iperf)         |   336.1 |   312.4 |       — |  -483.3 | iperf3   |
+-------+------------+--------------------------------+---------+---------+---------+---------+----------+
```

### Analyse

```
  ⚠ [PEERING] Chute de débit notable à l'interconnexion ORANGE → TATA : -59%
    819 Mbps (ORANGE) → 336 Mbps (TATA). Δ = -483 Mbps.
    → Dégradation modérée. Les mesures iperf3 (multi-flux TCP) sous-estiment
      parfois la bande passante disponible sur longue distance (BDP).
      Relancer en heure de pointe pour confirmer si la chute s'aggrave.

  ℹ [INFO] 2 hop(s) limitent leurs réponses ICMP — perte apparente non réelle
    hop 3 (89%), hop 4 (87%)

  ℹ [LATENCE] RTT final 89ms — normal pour une cible aux États-Unis depuis la France
    Latence physique attendue France→USA Est : 80–100ms.
```

### Verdict

```
═══════════════════════════════════════════
  ✔ CHEMIN SAIN

  Chemin réseau sain (RTT moyen : 89ms). Débit maximal mesuré :
  819 Mbps (Orange). La chute vers TATA (-59%) est dans la fourchette
  normale pour une mesure iperf3 inter-continentale. Aucune anomalie
  critique détectée sur ce run.
═══════════════════════════════════════════
```

---

## 2. Sonde ECMP

```powershell
.\peering-diag.exe ecmp ftp.unicron.network --port 6999 --flows 12
```

```
Cible : ftp.unicron.network (69.10.40.110) port 6999
Exploration de 12 chemins ECMP (5 probes/flux, TTL 64)…

SrcPort   Perte      Min    Médian       Max     Issue
──────────────────────────────────────────────────────
33434        0%     87.5      88.7      90.1   ouvert
33435        0%     87.3      88.2      90.3   ouvert
33436        0%     88.0      88.5      90.0   ouvert
33437        0%     88.1      88.2      89.7   ouvert
33438        0%     89.2      89.6      90.9   ouvert
33439        0%     89.5      89.7      92.2   ouvert
33440        0%     88.2      88.4      89.6   ouvert
33441        0%     88.0      89.2      91.0   ouvert
33442        0%     89.0      89.8      92.0   ouvert
33443        0%     88.4      88.6      90.5   ouvert
33444        0%     88.3      88.6      90.9   ouvert
33445        0%     89.2      89.5      91.7   ouvert

═══════════════════════════════════════════
  ✔ Chemins ECMP homogènes — aucun déséquilibre détecté.
═══════════════════════════════════════════
```

---

## 3. Conclusions

### Ce que les résultats confirment

**Le chemin réseau est sain en heure creuse.**  
0% de perte sur tous les hops qui comptent (hops 5 à 8). Les hops 3 et 4 affichent 87–89% de "perte" mais c'est de l'ICMP rate-limiting — les routeurs Orange et TATA déprioritisent les réponses ICMP TTL-exceeded sans pour autant bloquer le trafic. La preuve : les hops suivants ont 0% de perte.

**La latence est normale pour cette cible.**  
89ms France → USA Est est la latence physique attendue (propagation lumière dans la fibre transatlantique ≈ 70–80ms + traitement). Ce n'est pas un problème, c'est de la géographie.

**Pas de déséquilibre ECMP.**  
Les 12 flows TCP testent 12 chemins différents dans le hash de répartition de charge. Tous arrivent avec le même RTT (88–92ms) et 0% de perte. Aucun lien ECMP spécifique n'est congestionné ou défaillant.

**La chute Orange → TATA (-59%) est à surveiller, pas à alarmer.**  
336 Mbps sur un lien transatlantique via iperf3 (4 flux parallèles, 8s) reste honorable. La mesure iperf3 est affectée par le BDP (Bandwidth-Delay Product) : sur un RTT de 89ms, chaque flux TCP ne peut théoriquement pas dépasser ≈ `fenêtre_TCP / RTT`. Pour atteindre 819 Mbps il faudrait des fenêtres de 9 Mo par flux, ce que peu de serveurs configurent. La chute reflète en partie cette limite physique, pas forcément une congestion de peering.

### Ce que les résultats ne permettent pas de conclure

**On ne sait pas ce qui se passe aux heures de pointe.**  
Ce run a été effectué en heure creuse. La congestion de peering est typiquement un phénomène de soirée (18h–23h) quand le trafic résidentiel sature les liens d'interconnexion. Un chemin sain à 14h peut être dégradé à 21h sur le même trajet.

**On ne mesure pas le chemin retour.**  
Le diagnostic mesure uniquement le flux aller (ta machine → serveur). Un problème de peering peut être asymétrique : l'aller peut être propre alors que le retour (serveur → toi) est congestionné sur un lien différent. Les téléchargements lents depuis le serveur peuvent venir du retour.

**La mesure TATA est approximative.**  
Le serveur iperf3 utilisé (Frankfurt) n'est pas dans le même AS segment que le chemin mesuré (TATA Paris → New York). La mesure donne une indication du débit dans l'AS TATA, pas du débit exact sur le segment Paris–New York.

### Actions recommandées pour confirmer ou infirmer

| Action | Objectif | Commande |
|---|---|---|
| Relancer en soirée (20h–22h) | Voir si la congestion apparaît en heure de pointe | `peering-diag diag ftp.unicron.network` |
| Comparer le débit upload vs download | Identifier si le problème est asymétrique | Regarder la colonne UL vs DL dans le tableau speedtest |
| Relancer la sonde ECMP en soirée | Voir si un lien ECMP spécifique se dégrade | `peering-diag ecmp ftp.unicron.network --port 6999 --flows 12` |
| Tester depuis un autre FAI (mobile en 4G) | Isoler si le problème vient du peering Orange | Même commande depuis une autre connexion |
| Exporter en JSON pour comparer | Archiver le run de référence (heure creuse) | `peering-diag diag ftp.unicron.network --json ref-heure-creuse.json` |

### Schéma du chemin

```
[Ta machine] ──── Orange (AS5511) ────► TATA Paris (AS6453) ──── TATA New York ──── GTT (AS3257) ──── [ftp.unicron.network / Psychz AS40244]
     │                  │                       │                       │
   Livebox            ~4ms                    ~88ms                   ~89ms
   192.168.1.1      accès DSL             point d'entrée         arrivée cible
                                           transatlantique
                    
◄── France ─────────────────────────────────────────────── États-Unis ──────────────►
                              traversée transatlantique
                              (hop 4 → hop 5 : +80ms)
```

Le bond de latence entre hop 4 (9ms) et hop 5 (88ms) correspond à la **traversée du câble transatlantique** — c'est normal et attendu. Ce n'est pas un problème.

---

## 4. Cas dégradé — exemple de référence

Pour distinguer un chemin sain d'un chemin congestionné, voici à quoi ressemblerait un problème réel de peering sur le même trajet, typiquement observé en soirée :

### MTR en heure de pointe (exemple dégradé)

```
+-----+----------------+--------------------------------------------+-------+------------+--------+------+-------+-------+--------+
| TTL | IP             | Hostname                                   | ASN   | AS Name    | Loss%  | Min  | Avg   | Max   | Jitter |
+-----+----------------+--------------------------------------------+-------+------------+--------+------+-------+-------+--------+
|   1 | 192.168.1.1    | livebox.home                               |       |            |   0.0  |  0.3 |   0.9 |   3.1 |    0.5 |
|   2 | 80.15.224.213  |                                            |       |            |   0.0  |  0.8 |   2.2 |   7.4 |    1.1 |
|   3 | 81.253.130.5   | lag-41.nipoi204.rbci.orange.fr             |       |            |  88.9* |  4.1 |   6.8 |  11.2 |    2.9 |
|   4 | 80.231.246.26  | ix-bundle-22.qcore2.pvu-par.as6453.net     |  6453 | TATA COMM  |  86.7* |  7.5 |   9.3 |  14.1 |    1.8 |
|   5 | 80.231.245.12  | if-bundle-12-2.qcore1.pvu-par.as6453.net   |  6453 | TATA COMM  |  18.3  | 87.4 | 142.1 | 380.4 |   52.7 | ← problème
|   6 | 64.86.252.97   | if-ae-40-2.tcore1.nyv-new-york.as6453.net  |  6453 | TATA COMM  |  21.7  | 87.9 | 148.3 | 412.0 |   54.1 | ← propagé
|   7 | 216.6.57.5     |                                            |  3257 | GTT        |  19.4  | 88.1 | 145.2 | 398.7 |   50.3 | ← propagé
|   8 | 69.10.40.110   | ftp.unicron.network                        | 40244 | PSYCHZ     |  22.1  | 88.0 | 147.9 | 405.1 |   53.4 | ← cible atteinte
+-----+----------------+--------------------------------------------+-------+------------+--------+------+-------+-------+--------+
```

**Signaux d'alerte :**
- Jitter 52ms au hop 5 : la latence varie de 87ms à 380ms — file d'attente saturée sur le lien transatlantique
- Le jitter se propage aux hops 6, 7, 8 : c'est une congestion réelle (pas du rate-limiting ICMP)
- Perte 18–22% sur les 4 derniers hops : packets droppés dans les buffers saturés
- RTT avg passe de 9ms (hop 4) à 142ms (hop 5) : +133ms de latence de file d'attente

### Speedtest correspondant

```
+-------+------------+--------------------------------+---------+---------+---------+---------+----------+
| AS    | AS Name    | Endpoint                       | DL Mbps | UL Mbps | Ping ms | Δ DL    | Méthode  |
+-------+------------+--------------------------------+---------+---------+---------+---------+----------+
|  5511 | ORANGE     | Paris (Orange)                 |   821.0 |   198.4 |    11.3 | —       | direct   |
|  6453 | TATA COMM  | TATA Frankfurt (iperf)         |    11.2 |    9.8  |       — | -809.8  | iperf3   |
+-------+------------+--------------------------------+---------+---------+---------+---------+----------+
```

```
═══════════════════════════════════════════
  ✖ PROBLÈME DÉTECTÉ

  Problème de peering confirmé (AS5511 → AS6453).
  Chute de débit majeure : 821 Mbps → 11 Mbps (-99%).
  Jitter 52ms et perte 22% sur le lien transatlantique TATA.
  → Peering Orange/TATA saturé. Tester via un VPN avec sortie
    hors Orange (Free, Bouygues, ou VPN étranger) pour confirmer.
    Si confirmé, contacter le FAI avec ce rapport comme preuve.
═══════════════════════════════════════════
```

**Différences clés vs le run sain :**

| Indicateur | Run sain (heure creuse) | Run dégradé (heure de pointe) |
|---|---|---|
| Jitter hop 5 | 1.1ms | 52.7ms |
| RTT avg hop 5 | 88.6ms | 142.1ms |
| Perte hop 5–8 | 0–4% | 18–22% |
| Débit TATA iperf3 | 336 Mbps | 11 Mbps |
| Verdict | ✔ SAIN | ✖ PROBLÈME |
