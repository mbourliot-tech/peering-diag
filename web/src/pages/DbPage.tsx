import { useState, useEffect, useCallback } from 'react'
import { fetchDbStats, vacuumDb, purgeDb, fmtTs } from '../api'
import type { DbStatsJson } from '../api'

const cardStyle = {
  background: 'linear-gradient(135deg, #0f1929 0%, #111827 100%)',
  border: '1px solid rgba(255,255,255,0.07)',
}

const sectionBar = (color: string) => (
  <div className={`w-1 h-6 rounded-full ${color}`} />
)

function Toast({ message, onClose }: { message: string; onClose: () => void }) {
  useEffect(() => {
    const t = window.setTimeout(onClose, 4000)
    return () => window.clearTimeout(t)
  }, [onClose])

  return (
    <div className="fixed bottom-6 right-6 z-50 flex items-center gap-3 px-5 py-3 rounded-2xl shadow-2xl text-sm text-white font-medium"
      style={{ background: 'linear-gradient(135deg,#1a3a1a,#0f2a0f)', border: '1px solid rgba(34,197,94,0.3)' }}>
      <span className="text-emerald-400 text-base">✔</span>
      {message}
      <button onClick={onClose} className="ml-2 text-slate-500 hover:text-slate-300 text-xs">✕</button>
    </div>
  )
}

function StatRow({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex items-baseline justify-between py-2"
      style={{ borderBottom: '1px solid rgba(255,255,255,0.04)' }}>
      <span className="text-slate-500 text-sm">{label}</span>
      <span className="text-slate-100 font-mono text-sm">{value}</span>
    </div>
  )
}

export function DbPage() {
  const [stats,         setStats]         = useState<DbStatsJson | null>(null)
  const [loadingStats,  setLoadingStats]  = useState(false)
  const [statsError,    setStatsError]    = useState('')

  // Purge older-than
  const [purgeDays,     setPurgeDays]     = useState(30)
  const [purgingDays,   setPurgingDays]   = useState(false)
  const [confirmDays,   setConfirmDays]   = useState(false)

  // Keep last
  const [keepLast,      setKeepLast]      = useState(100)
  const [purgingKeep,   setPurgingKeep]   = useState(false)
  const [confirmKeep,   setConfirmKeep]   = useState(false)

  // Vacuum
  const [vacuuming,     setVacuuming]     = useState(false)

  // Toast
  const [toast,         setToast]         = useState('')

  const showToast = (msg: string) => setToast(msg)

  const loadStats = useCallback(async () => {
    setLoadingStats(true)
    setStatsError('')
    try {
      setStats(await fetchDbStats())
    } catch (e) {
      setStatsError(String(e))
    } finally {
      setLoadingStats(false)
    }
  }, [])

  useEffect(() => { loadStats() }, [loadStats])

  async function handlePurgeDays() {
    if (!confirmDays) { setConfirmDays(true); return }
    setConfirmDays(false)
    setPurgingDays(true)
    try {
      const res = await purgeDb({ older_than_days: purgeDays })
      showToast(`${res.deleted} run(s) supprimés (plus anciens que ${purgeDays} jours)`)
      loadStats()
    } catch (e) {
      showToast(`Erreur : ${e}`)
    } finally {
      setPurgingDays(false)
    }
  }

  async function handlePurgeKeep() {
    if (!confirmKeep) { setConfirmKeep(true); return }
    setConfirmKeep(false)
    setPurgingKeep(true)
    try {
      const res = await purgeDb({ keep_last: keepLast })
      showToast(`${res.deleted} run(s) supprimés (gardé les ${keepLast} derniers)`)
      loadStats()
    } catch (e) {
      showToast(`Erreur : ${e}`)
    } finally {
      setPurgingKeep(false)
    }
  }

  async function handleVacuum() {
    setVacuuming(true)
    try {
      const res = await vacuumDb()
      showToast(res.message)
      loadStats()
    } catch (e) {
      showToast(`Erreur : ${e}`)
    } finally {
      setVacuuming(false)
    }
  }

  return (
    <div className="space-y-8">

      {/* En-tête */}
      <div>
        <h1 className="text-3xl font-bold text-white">Maintenance de la base</h1>
        <p className="text-slate-500 mt-1">Consultez les statistiques de la base SQLite, purgez les anciens runs et compactez le fichier.</p>
      </div>

      {/* ── Carte Stats ──────────────────────────────────────────────────── */}
      <div className="rounded-2xl p-6 space-y-1" style={cardStyle}>
        <div className="flex items-center gap-3 mb-4">
          {sectionBar('bg-sky-500')}
          <h2 className="text-base font-bold text-white">Statistiques</h2>
          <button
            onClick={loadStats}
            disabled={loadingStats}
            className="ml-auto text-xs text-slate-500 hover:text-slate-300 transition-colors disabled:opacity-40"
          >
            {loadingStats ? <span className="inline-block w-3 h-3 border border-slate-500 border-t-slate-300 rounded-full animate-spin" /> : '↺ Rafraîchir'}
          </button>
        </div>

        {statsError && (
          <div className="px-4 py-3 rounded-xl bg-red-500/10 border border-red-500/30 text-red-400 text-sm mb-3">
            ⚠ {statsError}
          </div>
        )}

        {stats ? (
          <div>
            <StatRow label="Runs"          value={stats.run_count.toLocaleString()} />
            <StatRow label="Hops"          value={stats.hop_count.toLocaleString()} />
            <StatRow label="Speedtests"    value={stats.speedtest_count.toLocaleString()} />
            <StatRow label="Watch series"  value={stats.watch_series_count.toLocaleString()} />
            <StatRow label="Plus ancien"   value={stats.oldest_run ? fmtTs(stats.oldest_run) : '—'} />
            <StatRow label="Plus récent"   value={stats.newest_run ? fmtTs(stats.newest_run) : '—'} />
            <StatRow label="Taille"        value={stats.human_size} />
          </div>
        ) : !loadingStats ? (
          <div className="text-slate-600 text-sm py-4">Aucune donnée</div>
        ) : (
          <div className="py-4 flex items-center gap-2 text-slate-500 text-sm">
            <span className="w-4 h-4 border-2 border-slate-600 border-t-slate-300 rounded-full animate-spin" />
            Chargement…
          </div>
        )}
      </div>

      {/* ── Carte Purge ──────────────────────────────────────────────────── */}
      <div className="rounded-2xl p-6 space-y-6" style={cardStyle}>
        <div className="flex items-center gap-3">
          {sectionBar('bg-red-500')}
          <h2 className="text-base font-bold text-white">Purge des données</h2>
        </div>

        {/* Option 1 : older than */}
        <div className="flex flex-wrap items-end gap-4">
          <div>
            <label className="block text-sm font-medium text-slate-400 mb-1.5">
              Supprimer les runs plus anciens que
            </label>
            <div className="flex items-center gap-2">
              <input
                type="number" min={1} value={purgeDays}
                onChange={e => { setPurgeDays(+e.target.value); setConfirmDays(false) }}
                className="bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 w-24 text-sm"
              />
              <span className="text-slate-500 text-sm">jours</span>
            </div>
          </div>
          <button
            onClick={handlePurgeDays}
            disabled={purgingDays}
            className={`px-5 py-2.5 rounded-xl text-sm font-semibold text-white transition-all disabled:opacity-40 ${
              confirmDays
                ? 'bg-red-600 hover:bg-red-500 ring-2 ring-red-400/50'
                : 'bg-red-700/80 hover:bg-red-600'
            }`}
          >
            {purgingDays
              ? <span className="flex items-center gap-2"><span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />Purge…</span>
              : confirmDays ? '⚠ Confirmer la suppression' : '🗑 Purger'}
          </button>
          {confirmDays && (
            <button onClick={() => setConfirmDays(false)} className="text-slate-500 hover:text-slate-300 text-sm">
              Annuler
            </button>
          )}
        </div>

        <div style={{ borderTop: '1px solid rgba(255,255,255,0.05)' }} />

        {/* Option 2 : keep last */}
        <div className="flex flex-wrap items-end gap-4">
          <div>
            <label className="block text-sm font-medium text-slate-400 mb-1.5">
              Garder seulement les derniers
            </label>
            <div className="flex items-center gap-2">
              <input
                type="number" min={1} value={keepLast}
                onChange={e => { setKeepLast(+e.target.value); setConfirmKeep(false) }}
                className="bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 w-24 text-sm"
              />
              <span className="text-slate-500 text-sm">runs</span>
            </div>
          </div>
          <button
            onClick={handlePurgeKeep}
            disabled={purgingKeep}
            className={`px-5 py-2.5 rounded-xl text-sm font-semibold text-white transition-all disabled:opacity-40 ${
              confirmKeep
                ? 'bg-red-600 hover:bg-red-500 ring-2 ring-red-400/50'
                : 'bg-red-700/80 hover:bg-red-600'
            }`}
          >
            {purgingKeep
              ? <span className="flex items-center gap-2"><span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />Purge…</span>
              : confirmKeep ? '⚠ Confirmer la suppression' : '🗑 Purger'}
          </button>
          {confirmKeep && (
            <button onClick={() => setConfirmKeep(false)} className="text-slate-500 hover:text-slate-300 text-sm">
              Annuler
            </button>
          )}
        </div>

        <div className="px-4 py-3 rounded-xl text-xs text-slate-600 flex items-start gap-2"
          style={{ background: 'rgba(239,68,68,0.04)', border: '1px solid rgba(239,68,68,0.1)' }}>
          <span className="text-red-500">⚠</span>
          La suppression est irréversible. Les hops et speedtests associés sont supprimés en cascade.
        </div>
      </div>

      {/* ── Carte Vacuum ─────────────────────────────────────────────────── */}
      <div className="rounded-2xl p-6" style={cardStyle}>
        <div className="flex items-center gap-3 mb-4">
          {sectionBar('bg-violet-500')}
          <h2 className="text-base font-bold text-white">Vacuum</h2>
        </div>
        <div className="flex flex-wrap items-center gap-6">
          <p className="text-slate-500 text-sm flex-1">
            Compacte le fichier SQLite et libère l'espace disque après une purge.
            L'opération peut prendre quelques secondes sur les grosses bases.
          </p>
          <button
            onClick={handleVacuum}
            disabled={vacuuming}
            className="flex items-center gap-2 px-6 py-2.5 rounded-xl text-white font-semibold text-sm disabled:opacity-40 transition-all"
            style={{ background: 'linear-gradient(135deg,#7c3aed,#5b21b6)', boxShadow: '0 4px 20px rgba(124,58,237,0.25)' }}
          >
            {vacuuming
              ? <><span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />En cours…</>
              : <>⚙ Lancer VACUUM</>}
          </button>
        </div>
      </div>

      {/* Toast */}
      {toast && <Toast message={toast} onClose={() => setToast('')} />}
    </div>
  )
}
