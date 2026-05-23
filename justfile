# peering-diag — recettes de développement
# Prérequis : https://just.systems  (cargo install just)

frontend := "web"
backend  := "peering-diag"

# Affiche l'aide
default:
    @just --list

# ── Build ────────────────────────────────────────────────────────────────────

# Build complet : frontend React + backend Rust release
build: build-web build-release

# Build du frontend React (web/dist/)
build-web:
    cd {{frontend}} && npm install && npm run build

# Build Rust release
build-release:
    cd {{backend}} && cargo build --release

# Build Rust debug
build-debug:
    cd {{backend}} && cargo build

# ── Développement ────────────────────────────────────────────────────────────

# Lancer le serveur de dev Vite (proxy vers localhost:7373)
dev:
    cd {{frontend}} && npm run dev

# Lancer le backend en mode serve (debug)
serve *args:
    cd {{backend}} && cargo run -- serve {{args}}

# ── Qualité ──────────────────────────────────────────────────────────────────

# Vérification rapide (sans build complet)
check:
    cd {{backend}} && cargo check
    cd {{frontend}} && npx tsc --noEmit

# Lint Rust (clippy) + TypeScript (eslint)
lint:
    cd {{backend}} && cargo clippy -- -D warnings
    cd {{frontend}} && npm run lint

# Formatage Rust
fmt:
    cd {{backend}} && cargo fmt

# Tests Rust
test:
    cd {{backend}} && cargo test

# ── Nettoyage ────────────────────────────────────────────────────────────────

# Supprimer les artefacts de build
clean:
    cd {{backend}} && cargo clean
    rm -rf {{frontend}}/dist
