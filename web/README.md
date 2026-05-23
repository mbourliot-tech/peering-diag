# peering-diag — Interface web

Frontend React de l'interface web intégrée à `peering-diag serve`.

## Stack

- **React 19** + **TypeScript**
- **Vite 8** (build + dev server)
- **TailwindCSS v4** (styling)
- **Recharts** (graphiques RTT, perte, tendance)
- **react-router-dom v7** (navigation SPA)

## Développement

```bash
# Installer les dépendances
npm install

# Lancer le serveur de dev (proxy API → localhost:7373)
npm run dev
```

Le backend doit tourner en parallèle :

```bash
# Dans peering-diag/
cargo run -- serve --db /tmp/dev.db
```

## Build production

```bash
npm run build
# → web/dist/  (servi par Axum via ServeDir)
```

## Structure

```
src/
├── App.tsx              ← Router + layout principal
├── api.ts               ← Fonctions fetch vers l'API backend
├── pages/
│   ├── DiagPage.tsx     ← Lancement de commandes + terminal SSE
│   ├── HistoryPage.tsx  ← Historique des runs + graphiques
│   ├── WatchPage.tsx    ← Sessions watch (démarrer/arrêter)
│   ├── CheckEnvPage.tsx ← Vérification de l'environnement
│   └── DbPage.tsx       ← Maintenance de la base SQLite
└── components/
    ├── TerminalOutput.tsx ← Affichage streaming SSE (style terminal)
    ├── HopChart.tsx       ← Graphiques RTT et perte par hop
    ├── HistoryChart.tsx   ← Tendance RTT/perte + pattern par heure
    └── VerdictBadge.tsx   ← Badge coloré Healthy/Degraded/Faulty
```

## Chunking

Le bundle est découpé en 3 chunks distincts pour optimiser le cache navigateur :

| Chunk | Contenu | Taille gzip |
|-------|---------|-------------|
| `vendor-react` | react + react-dom + react-router-dom | 71 kB |
| `vendor-charts` | recharts | 111 kB |
| pages (x5) | DiagPage, HistoryPage… | 1–3 kB chacune |
