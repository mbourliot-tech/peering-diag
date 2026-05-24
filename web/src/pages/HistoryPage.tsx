import { useState, useEffect, useCallback, Fragment } from 'react'
import { fetchHistory, fetchByHour, fetchRunDetail, fmtTs } from '../api'
import { VerdictBadge } from '../components/VerdictBadge'
import { TrendChart, HourChart } from '../components/HistoryChart'
import { HopChart } from '../components/HopChart'
import { MapView } from '../components/MapView'
import type { RunJson, HourStatJson, RunDetailJson } from '../api'

type ViewMode = 'table' | 'by-hour'

const inputCls = "bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 placeholder-slate-600 transition-all"
const labelCls = "block text-sm font-medium text-slate-400 mb-1.5"

export function HistoryPage() {
  const [target,  setTarget]  = useState('')
  const [last,    setLast]    = useState(50)
  const [since,   setSince]   = useState('')
  const [view,    setView]    = useState<ViewMode>('table')

  const [runs,    setRuns]    = useState<RunJson[]>([])
  const [hours,   setHours]   = useState<HourStatJson[]>([])
  const [loading, setLoading] = useState(false)
  const [error,   setError]   = useState('')

  const [openId,  setOpenId]  = useState<number | null>(null)
  const [detail,  setDetail]  = useState<RunDetailJson | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [detailTab, setDetailTab] = useState<'hops' | 'map'>('hops')

  const load = useCallback(async () => {
    setError('')
    setLoading(true)
    try {
      if (view === 'table') {
        const data = await fetchHistory({ target: target.trim() || undefined, last, since: since || undefined })
        setRuns(data)
      } else {
        const data = await fetchByHour(target.trim() || undefined)
        setHours(data)
      }
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [target, last, since, view])

  useEffect(() => { load() }, [load])

  async function toggleDetail(id: number) {
    if (openId === id) { setOpenId(null); setDetail(null); return }
    setOpenId(id)
    setDetail(null)
    setDetailTab('hops')
    setDetailLoading(true)
    try {
      const d = await fetchRunDetail(id)
      setDetail(d)
    } finally {
      setDetailLoading(false)
    }
  }

  // Stats rapides
  const healthy  = runs.filter(r => r.verdict === 'Healthy').length
  const degraded = runs.filter(r => r.verdict === 'Degraded').length
  const avgRtt   = runs.filter(r => r.avg_rtt_ms > 0).reduce((s, r) => s + r.avg_rtt_ms, 0) /
                   (runs.filter(r => r.avg_rtt_ms > 0).length || 1)

  function lossCell(v: number) {
    const cls = v > 5 ? 'text-red-400' : v > 0 ? 'text-yellow-400' : 'text-emerald-400'
    return <span className={`font-mono font-semibold ${cls}`}>{v.toFixed(1)}%</span>
  }

  return (
    <div className="space-y-8">

      {/* En-tête */}
      <div>
        <h1 className="text-3xl font-bold text-white">Historique</h1>
        <p className="text-slate-500 mt-1">Consultez et analysez tous les runs passés.</p>
      </div>

      {/* Stats rapides (si données chargées) */}
      {runs.length > 0 && view === 'table' && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          {[
            { label: 'Total runs',  value: runs.length,          color: 'text-slate-300', bg: 'from-slate-800 to-slate-900' },
            { label: '✔ Sains',    value: healthy,               color: 'text-emerald-400', bg: 'from-emerald-900/20 to-slate-900' },
            { label: '⚠ Dégradés', value: degraded,             color: 'text-yellow-400', bg: 'from-yellow-900/20 to-slate-900' },
            { label: 'RTT moyen',   value: `${avgRtt.toFixed(0)} ms`, color: 'text-sky-400', bg: 'from-sky-900/20 to-slate-900' },
          ].map(s => (
            <div key={s.label} className={`rounded-2xl p-5 bg-gradient-to-b ${s.bg}`}
              style={{ border: '1px solid rgba(255,255,255,0.07)' }}>
              <div className={`text-2xl font-bold ${s.color}`}>{s.value}</div>
              <div className="text-slate-500 text-sm mt-1">{s.label}</div>
            </div>
          ))}
        </div>
      )}

      {/* Filtres */}
      <div className="rounded-2xl p-5 space-y-4"
        style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.07)' }}>
        <div className="flex flex-wrap gap-4 items-end">
          <div>
            <label className={labelCls}>Cible</label>
            <input type="text" value={target} onChange={e => setTarget(e.target.value)}
              placeholder="Toutes" className={`${inputCls} w-44`} />
          </div>
          <div>
            <label className={labelCls}>Derniers N</label>
            <select value={last} onChange={e => setLast(+e.target.value)} className={`${inputCls} w-auto`}>
              {[20, 50, 100, 200].map(n => <option key={n} value={n}>{n} runs</option>)}
            </select>
          </div>
          <div>
            <label className={labelCls}>Depuis</label>
            <input type="date" value={since} onChange={e => setSince(e.target.value)} className={inputCls} />
          </div>

          {/* Toggles vue */}
          <div className="flex gap-2 ml-auto items-end">
            {(['table', 'by-hour'] as ViewMode[]).map(v => (
              <button key={v} onClick={() => setView(v)}
                className={`px-4 py-2.5 rounded-xl text-sm font-semibold transition-all ${
                  view === v
                    ? 'text-white'
                    : 'bg-slate-800/60 text-slate-400 border border-slate-700/60 hover:text-slate-200'
                }`}
                style={view === v ? { background: 'linear-gradient(135deg,#2563eb,#1d4ed8)', border: '1px solid #3b82f6' } : {}}
              >
                {v === 'table' ? '📋 Tableau' : '🕐 Par heure'}
              </button>
            ))}
            <button onClick={load} disabled={loading}
              className="px-4 py-2.5 rounded-xl text-sm font-semibold bg-slate-800/60 text-slate-400 border border-slate-700/60 hover:text-slate-200 transition-all">
              {loading ? '⟳' : '↺'}
            </button>
          </div>
        </div>
        {error && <div className="px-4 py-3 rounded-xl bg-red-500/10 border border-red-500/30 text-red-400 text-sm">⚠ {error}</div>}
      </div>

      {/* Vue tableau */}
      {view === 'table' && (
        <>
          {runs.length === 0 && !loading && (
            <div className="text-center py-20 text-slate-600">
              <div className="text-5xl mb-4">📭</div>
              <div className="text-lg">Aucun run enregistré</div>
            </div>
          )}

          {runs.length > 0 && (
            <>
              <div className="rounded-2xl overflow-hidden" style={{ border: '1px solid rgba(255,255,255,0.07)' }}>
                <table className="w-full text-sm">
                  <thead>
                    <tr style={{ background: '#0f1929', borderBottom: '1px solid rgba(255,255,255,0.07)' }}
                      className="text-xs text-slate-500 uppercase tracking-wider">
                      <th className="px-5 py-3 text-left">#</th>
                      <th className="px-5 py-3 text-left">Date</th>
                      <th className="px-5 py-3 text-left">Cible</th>
                      <th className="px-5 py-3 text-center">Verdict</th>
                      <th className="px-5 py-3 text-right">↑ Aller</th>
                      <th className="px-5 py-3 text-right">↓ Retour</th>
                      <th className="px-5 py-3 text-right">RTT moy</th>
                      <th className="px-5 py-3 text-right">DL</th>
                      <th className="px-5 py-3 text-center">Hops</th>
                    </tr>
                  </thead>
                  <tbody>
                    {runs.map((r, i) => (
                      <Fragment key={r.id}>
                        <tr
                          className="transition-colors cursor-pointer"
                          style={{
                            background: openId === r.id ? 'rgba(37,99,235,0.08)' : i % 2 === 0 ? '#0a1020' : '#0c1428',
                            borderBottom: '1px solid rgba(255,255,255,0.04)',
                          }}
                          onClick={() => toggleDetail(r.id)}
                        >
                          <td className="px-5 py-3 text-slate-600 font-mono text-xs">{r.id}</td>
                          <td className="px-5 py-3 text-slate-400 font-mono text-xs whitespace-nowrap">{fmtTs(r.timestamp)}</td>
                          <td className="px-5 py-3 text-slate-200 font-medium">{r.target}</td>
                          <td className="px-5 py-3 text-center">
                            <VerdictBadge verdict={r.verdict} size="sm" />
                          </td>
                          <td className="px-5 py-3 text-right">{lossCell(r.max_loss_aller)}</td>
                          <td className="px-5 py-3 text-right">{lossCell(r.max_loss_retour)}</td>
                          <td className="px-5 py-3 text-right font-mono text-sm text-sky-400">
                            {r.avg_rtt_ms > 0 ? `${r.avg_rtt_ms.toFixed(1)} ms` : '—'}
                          </td>
                          <td className="px-5 py-3 text-right font-mono text-sm text-emerald-400">
                            {r.dl_mbps > 0 ? `${r.dl_mbps.toFixed(0)} Mb/s` : '—'}
                          </td>
                          <td className="px-5 py-3 text-center text-slate-500 text-xs">
                            {openId === r.id ? '▲' : '▼'}
                          </td>
                        </tr>

                        {openId === r.id && (
                          <tr>
                            <td colSpan={9} style={{ background: '#080e1c', borderBottom: '1px solid rgba(37,99,235,0.2)' }}
                              className="px-6 py-4">
                              {detailLoading && (
                                <div className="flex items-center gap-2 text-slate-400 text-sm py-4">
                                  <span className="inline-block w-4 h-4 border-2 border-slate-600 border-t-blue-400 rounded-full animate-spin" />
                                  Chargement des hops…
                                </div>
                              )}
                              {detail && (
                                <>
                                  {r.finding && (
                                    <div className="flex items-start gap-2 mb-4 px-4 py-3 rounded-xl bg-yellow-500/10 border border-yellow-500/20 text-yellow-300 text-sm">
                                      📋 {r.finding}
                                    </div>
                                  )}

                                  {/* Onglets Hops / Carte */}
                                  <div className="flex gap-2 mb-4">
                                    {(['hops', 'map'] as const).map(tab => (
                                      <button
                                        key={tab}
                                        onClick={() => setDetailTab(tab)}
                                        className={`px-3 py-1.5 rounded-lg text-xs font-semibold transition-all ${
                                          detailTab === tab
                                            ? 'text-white'
                                            : 'bg-slate-800/60 text-slate-400 border border-slate-700/60 hover:text-slate-200'
                                        }`}
                                        style={detailTab === tab ? { background: 'linear-gradient(135deg,#2563eb,#1d4ed8)', border: '1px solid #3b82f6' } : {}}
                                      >
                                        {tab === 'hops' ? '📊 Hops' : '🗺 Carte'}
                                      </button>
                                    ))}
                                  </div>

                                  {detailTab === 'hops' && (
                                    <>
                                      {detail.aller.length > 0
                                        ? <HopChart hops={detail.aller} />
                                        : <p className="text-slate-600 text-sm py-6 text-center">Pas de données hop disponibles</p>
                                      }
                                      {detail.retour.length > 0 && (
                                        <div className="mt-6">
                                          <h3 className="text-base font-semibold text-slate-300 mb-3">Retour (Globalping)</h3>
                                          <HopChart hops={detail.retour} />
                                        </div>
                                      )}
                                    </>
                                  )}

                                  {detailTab === 'map' && <MapView runId={detail.id} />}
                                </>
                              )}
                            </td>
                          </tr>
                        )}
                      </Fragment>
                    ))}
                  </tbody>
                </table>
              </div>

              {/* Graphique tendance */}
              <TrendChart runs={runs} />
            </>
          )}
        </>
      )}

      {/* Vue par heure */}
      {view === 'by-hour' && (
        hours.length > 0
          ? <HourChart hours={hours} />
          : <div className="text-center py-20 text-slate-600">
              <div className="text-5xl mb-4">📊</div>
              <div className="text-lg">Aucune donnée disponible</div>
            </div>
      )}
    </div>
  )
}
