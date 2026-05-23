# Contribuer à peering-diag

Merci de votre intérêt pour le projet !

## Prérequis

| Outil | Version minimale | Usage |
|-------|-----------------|-------|
| Rust  | 1.78 (stable)   | Backend CLI + serveur |
| Node.js | 22 LTS        | Frontend React |
| npm   | 10+             | Gestion des dépendances JS |
| just  | 1.x *(optionnel)* | Recettes de développement |

## Mise en place

```bash
# Cloner le dépôt
git clone https://github.com/Michel/peering-diag
cd peering-diag

# Build complet
just build
# ou manuellement :
cd web && npm install && npm run build
cd peering-diag && cargo build --release
```

## Développement

```bash
# Lancer le backend (mode debug, avec interface web en dev)
just serve --db /tmp/dev.db

# Dans un autre terminal : lancer le serveur de dev Vite
# (proxy automatique vers localhost:7373)
just dev
```

## Vérification avant PR

```bash
just fmt        # Formatage Rust (cargo fmt)
just lint       # Clippy + ESLint
just test       # Tests Rust
just check      # cargo check + tsc --noEmit
```

## Structure du projet

```
peering-diag/       ← Crate Rust
  src/
    main.rs         ← Point d'entrée CLI (clap)
    diag/           ← Logique de diagnostic (MTR, speedtest…)
    report/         ← Stockage SQLite + analyse temporelle
    web/            ← Serveur Axum + handlers API + SSE

web/                ← Frontend React/Vite/TypeScript
  src/
    pages/          ← Pages de l'interface
    components/     ← Composants réutilisables
    api.ts          ← Client fetch vers le backend
```

## Conventions

- **Rust** : `cargo fmt` obligatoire, `clippy` sans warnings
- **TypeScript** : ESLint + TypeScript strict
- **Commits** : messages en français ou anglais, verbe à l'impératif en tête
- **Issues** : décrire le problème observé + OS + version + commande utilisée

## Rapport de bug

Ouvrir une issue en incluant :
1. La commande lancée
2. La sortie complète (stdout + stderr)
3. L'OS et la version de peering-diag (`peering-diag --version`)
