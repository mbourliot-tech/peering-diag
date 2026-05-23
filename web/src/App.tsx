import { BrowserRouter, Routes, Route, NavLink, Navigate } from 'react-router-dom'
import { DiagPage }     from './pages/DiagPage'
import { HistoryPage }  from './pages/HistoryPage'
import { WatchPage }    from './pages/WatchPage'
import { CheckEnvPage } from './pages/CheckEnvPage'
import { DbPage }       from './pages/DbPage'

const NAV = [
  { to: '/diag',      icon: '🔬', label: 'Diagnostic' },
  { to: '/history',   icon: '📋', label: 'Historique' },
  { to: '/watch',     icon: '👁',  label: 'Watch' },
  { to: '/check-env', icon: '🔧', label: 'Environnement' },
  { to: '/db',        icon: '🗄',  label: 'Base de données' },
]

export default function App() {
  return (
    <BrowserRouter>
      <div className="min-h-screen flex flex-col">

        {/* ── Header ──────────────────────────────────────────────────────── */}
        <header className="sticky top-0 z-50 border-b border-white/5 backdrop-blur-md"
          style={{ background: 'rgba(6,11,20,0.92)' }}>
          <div className="max-w-7xl mx-auto px-6 h-16 flex items-center gap-6">

            {/* Logo */}
            <div className="flex items-center gap-3 shrink-0">
              <div className="w-9 h-9 rounded-xl flex items-center justify-center text-lg shadow-lg"
                style={{ background: 'linear-gradient(135deg, #2563eb, #7c3aed)' }}>
                📡
              </div>
              <div className="leading-tight">
                <div className="text-white font-bold text-base">peering-diag</div>
                <div className="text-slate-500 text-xs">Diagnostic réseau</div>
              </div>
            </div>

            {/* Nav */}
            <nav className="flex gap-1 ml-2">
              {NAV.map(n => (
                <NavLink
                  key={n.to}
                  to={n.to}
                  className={({ isActive }) =>
                    `flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all duration-150 ${
                      isActive
                        ? 'bg-blue-600/20 text-blue-400 border border-blue-500/30'
                        : 'text-slate-400 hover:text-slate-200 hover:bg-white/5 border border-transparent'
                    }`
                  }
                >
                  <span className="text-base leading-none">{n.icon}</span>
                  <span>{n.label}</span>
                </NavLink>
              ))}
            </nav>
          </div>
        </header>

        {/* ── Contenu ─────────────────────────────────────────────────────── */}
        <main className="flex-1 max-w-7xl mx-auto w-full px-6 py-8">
          <Routes>
            <Route path="/"          element={<Navigate to="/diag" replace />} />
            <Route path="/diag"      element={<DiagPage />} />
            <Route path="/history"   element={<HistoryPage />} />
            <Route path="/watch"     element={<WatchPage />} />
            <Route path="/check-env" element={<CheckEnvPage />} />
            <Route path="/db"        element={<DbPage />} />
          </Routes>
        </main>

        {/* ── Footer ──────────────────────────────────────────────────────── */}
        <footer className="border-t border-white/5 py-3 text-center text-xs text-slate-700">
          peering-diag — interface locale — {new Date().getFullYear()}
        </footer>
      </div>
    </BrowserRouter>
  )
}
