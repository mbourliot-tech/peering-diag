import { useState, useEffect } from 'react'
import { fetchWatchList, startWatch, stopWatch, fmtTs } from '../api'
import { VerdictBadge } from '../components/VerdictBadge'
import { TerminalOutput } from '../components/TerminalOutput'
import type { WatchSeriesJson } from '../api'

const inputCls = "bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 placeholder-slate-600 transition-all"
const labelCls = "block text-sm font-medium text-slate-400 mb-1.5"

export function WatchPage() {
  const [target,      setTarget]      = useState('')
  const [intervalMin, setIntervalMin] = useState(15)
  const [noSpeedtest, setNoSpeedtest] = useState(false)
  const [sessions,    setSessions]    = useState<WatchSeriesJson[]>([])
  const [loading,     setLoading]     = useState(false)
  const [starting,    setStarting]    = useState(false)
  const [error,       setError]       = useState('')
  const [liveJobId,   setLiveJobId]   = useState<string | null>(null)

  async function loadSessions() {
    setLoading(true)
    try { setSessions(await fetchWatchList()) }
    catch (e) { setError(String(e)) }
    finally { setLoading(false) }
  }

  useEffect(() => {
    loadSessions()
    const timer = window.setInterval(loadSessions, 15000)
    return () => window.clearInterval(timer)
  }, [])

  async function handleStart() {
    if (!target.trim()) { setError('Cible requise'); return }
    setError('')
    setStarting(true)
    try {
      const jobId = await startWatch(target.trim(), intervalMin * 60, noSpeedtest)
      setLiveJobId(jobId)
      await loadSessions()
    } catch (e) {
      setError(String(e))
    } finally {
      setStarting(false)
    }
  }

  async function handleStop(jobId: string) {
    await stopWatch(jobId)
    await loadSessions()
  }

  function intervalLabel(s: number) {
    return s % 60 === 0 ? `${s / 60} min` : `${s}s`
  }

  const activeSessions = sessions.filter(s => s.job_id !== null)
  const stoppedSessions = sessions.filter(s => s.job_id === null)

  return (
    <div className="space-y-8">

      {/* En-tête */}
      <div>
        <h1 className="text-3xl font-bold text-white">Watch — Surveillance continue</h1>
        <p className="text-slate-500 mt-1">Surveillez une cible en continu et consultez l'évolution dans l'historique.</p>
      </div>

      {/* Stats actives */}
      {sessions.length > 0 && (
        <div className="grid grid-cols-2 md:grid-cols-3 gap-4">
          {[
            { label: 'Sessions actives',  value: activeSessions.length,  color: 'text-emerald-400', bg: 'from-emerald-900/20 to-slate-900' },
            { label: 'Sessions arrêtées', value: stoppedSessions.length, color: 'text-slate-400',   bg: 'from-slate-800 to-slate-900' },
            { label: 'Runs total',        value: sessions.reduce((s, x) => s + x.run_count, 0), color: 'text-sky-400', bg: 'from-sky-900/20 to-slate-900' },
          ].map(s => (
            <div key={s.label} className={`rounded-2xl p-5 bg-gradient-to-b ${s.bg}`}
              style={{ border: '1px solid rgba(255,255,255,0.07)' }}>
              <div className={`text-2xl font-bold ${s.color}`}>{s.value}</div>
              <div className="text-slate-500 text-sm mt-1">{s.label}</div>
            </div>
          ))}
        </div>
      )}

      {/* Formulaire démarrage */}
      <div className="rounded-2xl p-6 space-y-5"
        style={{ background: 'linear-gradient(135deg, #0f1929 0%, #111827 100%)', border: '1px solid rgba(255,255,255,0.07)' }}>
        <div className="flex items-center gap-3 mb-2">
          <div className="w-1 h-6 rounded-full bg-blue-500" />
          <h2 className="text-base font-bold text-white">Démarrer une surveillance</h2>
        </div>

        <div className="flex flex-wrap gap-5 items-end">
          <div>
            <label className={labelCls}>Cible</label>
            <input type="text" value={target} onChange={e => setTarget(e.target.value)}
              placeholder="google.com ou 8.8.8.8"
              onKeyDown={e => e.key === 'Enter' && handleStart()}
              className={`${inputCls} w-52`} />
          </div>
          <div>
            <label className={labelCls}>Intervalle (min)</label>
            <input type="number" min={1} max={60} value={intervalMin}
              onChange={e => setIntervalMin(+e.target.value)}
              className={`${inputCls} w-24`} />
          </div>
          <label className="flex items-center gap-2.5 cursor-pointer select-none mb-0.5">
            <div
              onClick={() => setNoSpeedtest(v => !v)}
              className={`w-10 h-6 rounded-full transition-all duration-200 relative ${noSpeedtest ? 'bg-blue-600' : 'bg-slate-700'}`}
            >
              <span className={`absolute top-1 w-4 h-4 bg-white rounded-full shadow transition-all duration-200 ${noSpeedtest ? 'left-5' : 'left-1'}`} />
            </div>
            <span className="text-sm text-slate-300">Sans speedtest</span>
          </label>
          <button onClick={handleStart} disabled={starting}
            className="flex items-center gap-2 px-6 py-2.5 rounded-xl text-white font-semibold text-sm disabled:opacity-40 shadow-lg"
            style={{ background: 'linear-gradient(135deg,#2563eb,#1d4ed8)', boxShadow: '0 4px 20px rgba(37,99,235,0.3)' }}>
            {starting
              ? <><span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />Démarrage…</>
              : <>▶ Démarrer</>}
          </button>
        </div>
        {error && <div className="px-4 py-3 rounded-xl bg-red-500/10 border border-red-500/30 text-red-400 text-sm">⚠ {error}</div>}
      </div>

      {/* Terminal de démarrage */}
      {liveJobId && (
        <div>
          <h3 className="text-base font-semibold text-slate-300 mb-3">Démarrage en cours</h3>
          <TerminalOutput jobId={liveJobId} onDone={() => { setLiveJobId(null); loadSessions() }} />
        </div>
      )}

      {/* Sessions actives */}
      {activeSessions.length > 0 && (
        <div className="space-y-3">
          <div className="flex items-center gap-3">
            <div className="w-1 h-6 rounded-full bg-emerald-500" />
            <h2 className="text-base font-bold text-white">Sessions actives</h2>
            <button onClick={loadSessions} disabled={loading}
              className="ml-auto text-xs text-slate-500 hover:text-slate-300 transition-colors">
              {loading ? '⟳' : '↺ Rafraîchir'}
            </button>
          </div>
          <div className="grid gap-3">
            {activeSessions.map(s => (
              <div key={s.id} className="rounded-2xl p-5 flex items-center gap-4"
                style={{ background: '#0c1a14', border: '1px solid rgba(34,197,94,0.2)' }}>
                <div className="flex items-center gap-2">
                  <span className="w-2.5 h-2.5 rounded-full bg-emerald-400 animate-pulse" />
                  <span className="text-xs text-emerald-500 font-medium">ACTIF</span>
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-white font-semibold text-base truncate">{s.target}</div>
                  <div className="text-slate-500 text-xs mt-0.5">
                    toutes {intervalLabel(s.interval_s)} · depuis {fmtTs(s.started_at)} · {s.run_count} runs
                  </div>
                </div>
                {s.last_verdict && <VerdictBadge verdict={s.last_verdict} size="sm" />}
                {s.job_id && (
                  <button onClick={() => handleStop(s.job_id!)}
                    className="px-4 py-2 rounded-xl text-sm font-semibold text-red-400 border border-red-500/30 hover:bg-red-500/10 transition-all">
                    ■ Stop
                  </button>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Sessions arrêtées */}
      {stoppedSessions.length > 0 && (
        <div className="space-y-3">
          <div className="flex items-center gap-3">
            <div className="w-1 h-6 rounded-full bg-slate-600" />
            <h2 className="text-base font-bold text-slate-400">Sessions arrêtées</h2>
          </div>
          <div className="rounded-2xl overflow-hidden" style={{ border: '1px solid rgba(255,255,255,0.06)' }}>
            <table className="w-full text-sm">
              <thead>
                <tr className="text-xs text-slate-600 uppercase tracking-wider"
                  style={{ background: '#0f1929', borderBottom: '1px solid rgba(255,255,255,0.05)' }}>
                  <th className="px-5 py-3 text-left">Cible</th>
                  <th className="px-5 py-3 text-left">Intervalle</th>
                  <th className="px-5 py-3 text-left">Démarré</th>
                  <th className="px-5 py-3 text-right">Runs</th>
                  <th className="px-5 py-3 text-center">Dernier verdict</th>
                </tr>
              </thead>
              <tbody>
                {stoppedSessions.map((s, i) => (
                  <tr key={s.id}
                    style={{ background: i % 2 === 0 ? '#0a1020' : '#0c1428', borderBottom: '1px solid rgba(255,255,255,0.03)' }}>
                    <td className="px-5 py-3 text-slate-400">{s.target}</td>
                    <td className="px-5 py-3 text-slate-600">{intervalLabel(s.interval_s)}</td>
                    <td className="px-5 py-3 text-slate-600 font-mono text-xs">{fmtTs(s.started_at)}</td>
                    <td className="px-5 py-3 text-right text-slate-500 font-mono">{s.run_count}</td>
                    <td className="px-5 py-3 text-center">
                      {s.last_verdict
                        ? <VerdictBadge verdict={s.last_verdict} size="sm" />
                        : <span className="text-slate-700">—</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {sessions.length === 0 && !loading && (
        <div className="text-center py-20 text-slate-600">
          <div className="text-5xl mb-4">👁</div>
          <div className="text-lg">Aucune session de surveillance</div>
          <div className="text-sm mt-2">Démarrez une surveillance ci-dessus pour commencer.</div>
        </div>
      )}

      {/* Info */}
      <div className="rounded-2xl px-5 py-4 text-sm text-slate-500 flex items-start gap-3"
        style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.05)' }}>
        <span className="text-blue-500 text-lg">ℹ</span>
        <span>Watch enregistre chaque run en base de données. Consultez <strong className="text-slate-400">Historique</strong> pour l'analyse temporelle et les graphiques.</span>
      </div>
    </div>
  )
}
