import { lazy, Suspense } from 'react'
import { BrowserRouter, Routes, Route, NavLink, Navigate } from 'react-router-dom'
import { ErrorBoundary } from './components/ErrorBoundary'
import { ThemeProvider, useTheme } from './contexts/ThemeContext'

const DiagPage     = lazy(() => import('./pages/DiagPage').then(m => ({ default: m.DiagPage })))
const HistoryPage  = lazy(() => import('./pages/HistoryPage').then(m => ({ default: m.HistoryPage })))
const WatchPage    = lazy(() => import('./pages/WatchPage').then(m => ({ default: m.WatchPage })))
const CheckEnvPage = lazy(() => import('./pages/CheckEnvPage').then(m => ({ default: m.CheckEnvPage })))
const DbPage       = lazy(() => import('./pages/DbPage').then(m => ({ default: m.DbPage })))

const NAV = [
  { to: '/diag',      icon: '🔬', label: 'Diagnostic' },
  { to: '/history',   icon: '📋', label: 'Historique' },
  { to: '/watch',     icon: '👁',  label: 'Watch' },
  { to: '/check-env', icon: '🔧', label: 'Environnement' },
  { to: '/db',        icon: '🗄',  label: 'Base de données' },
]

function ThemeToggle() {
  const { theme, toggle } = useTheme()
  const isDash = theme === 'dashboard'

  return (
    <button
      onClick={toggle}
      title={isDash ? 'Passer en mode terminal' : 'Passer en mode dashboard'}
      style={{
        display:      'flex',
        alignItems:   'center',
        gap:          8,
        padding:      '6px 14px',
        borderRadius: 10,
        border:       `1px solid ${isDash ? 'rgba(56,139,253,0.4)' : 'rgba(255,255,255,0.1)'}`,
        background:   isDash ? 'rgba(56,139,253,0.12)' : 'rgba(255,255,255,0.04)',
        color:        isDash ? '#388bfd' : '#94a3b8',
        fontSize:     13,
        fontWeight:   600,
        cursor:       'pointer',
        transition:   'all 0.2s',
        whiteSpace:   'nowrap',
      }}
    >
      <span style={{ fontSize: 16 }}>{isDash ? '📊' : '⌨️'}</span>
      <span>{isDash ? 'Dashboard' : 'Terminal'}</span>
    </button>
  )
}

function AppShell() {
  const { theme } = useTheme()
  const isDash = theme === 'dashboard'

  return (
    <div className="min-h-screen flex flex-col" style={{ background: 'var(--bg-base)' }}>

      {/* ── Header ─────────────────────────────────────────────────────────── */}
      <header className="sticky top-0 z-50"
        style={{
          background:   'var(--bg-nav)',
          borderBottom: `1px solid ${isDash ? 'rgba(56,139,253,0.15)' : 'rgba(255,255,255,0.05)'}`,
          backdropFilter: 'blur(12px)',
          boxShadow:    isDash ? '0 1px 0 rgba(56,139,253,0.1)' : 'none',
        }}>
        <div className="max-w-7xl mx-auto px-6 flex items-center gap-4"
          style={{ height: isDash ? 64 : 60 }}>

          {/* Logo */}
          <div className="flex items-center gap-3 shrink-0">
            <div className="flex items-center justify-center text-lg"
              style={{
                width:        isDash ? 40 : 36,
                height:       isDash ? 40 : 36,
                borderRadius: isDash ? 12 : 10,
                background:   'linear-gradient(135deg, #2563eb, #7c3aed)',
                boxShadow:    isDash ? '0 4px 12px rgba(37,99,235,0.4)' : 'none',
                transition:   'all 0.2s',
              }}>
              📡
            </div>
            <div className="leading-tight">
              <div className="font-bold text-base" style={{ color: 'var(--text-primary)' }}>
                peering-diag
              </div>
              <div className="text-xs" style={{ color: 'var(--text-muted)' }}>
                {isDash ? 'Network Dashboard' : 'Diagnostic réseau'}
              </div>
            </div>
          </div>

          {/* Nav */}
          <nav className="flex gap-1 ml-2">
            {NAV.map(n => (
              <NavLink key={n.to} to={n.to}
                className={({ isActive }) =>
                  `flex items-center gap-2 px-4 rounded-lg text-sm font-medium transition-all duration-150 ${
                    isActive ? '' : 'hover:bg-white/5 border border-transparent'
                  }`
                }
                style={({ isActive }) => ({
                  height:     isDash ? 36 : 32,
                  background: isActive
                    ? isDash ? 'rgba(56,139,253,0.15)' : 'rgba(37,99,235,0.2)'
                    : undefined,
                  color:      isActive
                    ? isDash ? '#388bfd' : '#60a5fa'
                    : '#94a3b8',
                  border:     isActive
                    ? `1px solid ${isDash ? 'rgba(56,139,253,0.3)' : 'rgba(96,165,250,0.3)'}`
                    : undefined,
                  boxShadow:  isActive && isDash
                    ? '0 0 12px rgba(56,139,253,0.15)'
                    : undefined,
                })}
              >
                <span className="text-base leading-none">{n.icon}</span>
                <span>{n.label}</span>
              </NavLink>
            ))}
          </nav>

          {/* Toggle thème */}
          <div className="ml-auto">
            <ThemeToggle />
          </div>
        </div>

        {/* Ligne d'accent dashboard */}
        {isDash && (
          <div style={{ height: 2, background: 'linear-gradient(90deg, #388bfd 0%, #7c3aed 50%, transparent 100%)', opacity: 0.6 }} />
        )}
      </header>

      {/* ── Contenu ─────────────────────────────────────────────────────────── */}
      <main className="flex-1 max-w-7xl mx-auto w-full px-6 py-8">
        <ErrorBoundary>
          <Suspense fallback={
            <div className="flex items-center justify-center py-20 gap-3" style={{ color: 'var(--text-muted)' }}>
              <span className="inline-block w-5 h-5 border-2 border-slate-700 border-t-blue-500 rounded-full animate-spin" />
              Chargement…
            </div>
          }>
            <Routes>
              <Route path="/"          element={<Navigate to="/diag" replace />} />
              <Route path="/diag"      element={<DiagPage />} />
              <Route path="/history"   element={<HistoryPage />} />
              <Route path="/watch"     element={<WatchPage />} />
              <Route path="/check-env" element={<CheckEnvPage />} />
              <Route path="/db"        element={<DbPage />} />
            </Routes>
          </Suspense>
        </ErrorBoundary>
      </main>

      {/* ── Footer ─────────────────────────────────────────────────────────── */}
      <footer style={{ borderTop: '1px solid var(--border)', padding: '12px 0', textAlign: 'center', fontSize: 12, color: 'var(--text-muted)' }}>
        peering-diag — interface locale — {new Date().getFullYear()}
      </footer>
    </div>
  )
}

export default function App() {
  return (
    <ThemeProvider>
      <BrowserRouter>
        <AppShell />
      </BrowserRouter>
    </ThemeProvider>
  )
}
