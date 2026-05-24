import { useState } from 'react'
import { startJob, stopJob, fetchRunDetail } from '../api'
import { TerminalOutput } from '../components/TerminalOutput'
import { HopChart } from '../components/HopChart'
import { KpiCard } from '../components/KpiCard'
import { useTheme } from '../contexts/ThemeContext'
import { useJobStream, parseProgress } from '../hooks/useJobStream'
import type { RunDetailJson } from '../api'

const COMMANDS = [
  { value: 'diag',      icon: '🔬', label: 'diag',      desc: 'MTR aller + retour + speedtest' },
  { value: 'aller',     icon: '→',  label: 'aller',     desc: 'Traceroute aller uniquement' },
  { value: 'mtr',       icon: '⚡', label: 'mtr',       desc: 'MTR brut sans analyse' },
  { value: 'retour',    icon: '←',  label: 'retour',    desc: 'Chemin retour via Globalping' },
  { value: 'lg',        icon: '🔍', label: 'lg',        desc: 'Looking Glass externe' },
  { value: 'ecmp',      icon: '⑂',  label: 'ecmp',      desc: 'Exploration ECMP multi-chemins' },
  { value: 'check-env', icon: '✓',  label: 'check-env', desc: 'Vérifier les outils installés' },
]

const HOP_COMMANDS = ['diag', 'aller']

// ── Version Dashboard ─────────────────────────────────────────────────────────

function DashboardRunProgress({ jobId, target, onStop, onDone }: { jobId: string; target: string; onStop: () => void; onDone: () => void }) {
  const { lines, done, elapsed } = useJobStream(jobId, onDone)
  const prog = parseProgress(lines)
  const mm = String(Math.floor(elapsed / 60)).padStart(2, '0')
  const ss = String(elapsed % 60).padStart(2, '0')

  const [showLogs, setShowLogs] = useState(false)

  const phasePct = prog.totalPhases > 0 ? (prog.phaseIndex / prog.totalPhases) * 100 : 0
  const roundPct = prog.totalRounds > 0 ? (prog.currentRound / prog.totalRounds) * 100 : 0

  return (
    <div style={{
      background:   'var(--bg-card2)',
      border:       '1px solid var(--border)',
      boxShadow:    'var(--card-shadow)',
      borderRadius: 'var(--card-radius)',
      padding:      '2rem',
    }}>
      {/* En-tête */}
      <div className="flex items-start justify-between gap-4 mb-6">
        <div>
          <div className="flex items-center gap-3 mb-1">
            {!done && <span className="w-3 h-3 rounded-full bg-blue-400 animate-pulse" />}
            {done  && <span className="w-3 h-3 rounded-full bg-emerald-400" />}
            <span style={{ fontSize: '1.25rem', fontWeight: 700, color: done ? '#34d399' : 'var(--text-primary)' }}>
              {done ? '✔ Terminé' : prog.phase || 'Initialisation…'}
            </span>
          </div>
          <div style={{ color: 'var(--text-muted)', fontSize: 14 }}>
            Cible : <span style={{ color: '#388bfd', fontWeight: 600 }}>{target}</span>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span style={{ color: 'var(--text-muted)', fontFamily: 'monospace', fontSize: 13 }}>
            ⏱ {prog.elapsed || `${mm}:${ss}`}
          </span>
          {!done && (
            <button onClick={onStop}
              style={{
                padding:      '6px 16px',
                borderRadius: 8,
                border:       '1px solid rgba(239,68,68,0.4)',
                background:   'rgba(239,68,68,0.1)',
                color:        '#f87171',
                fontSize:     13,
                fontWeight:   600,
                cursor:       'pointer',
              }}>
              ■ Arrêter
            </button>
          )}
        </div>
      </div>

      {/* Barres de progression */}
      {!done && (
        <div className="space-y-4">
          {prog.phaseIndex > 0 && (
            <div>
              <div className="flex justify-between mb-2" style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                <span>Phase {prog.phaseIndex} / {prog.totalPhases}</span>
                <span>{Math.round(phasePct)}%</span>
              </div>
              <div style={{ height: 8, borderRadius: 4, background: 'rgba(255,255,255,0.06)', overflow: 'hidden' }}>
                <div style={{
                  height: '100%', borderRadius: 4,
                  width: `${phasePct}%`,
                  background: 'linear-gradient(90deg,#388bfd,#7c3aed)',
                  transition: 'width 0.5s ease',
                }} />
              </div>
            </div>
          )}
          {prog.totalRounds > 0 && (
            <div>
              <div className="flex justify-between mb-2" style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                <span>Rounds MTR</span>
                <span style={{ fontFamily: 'monospace' }}>{prog.currentRound} / {prog.totalRounds}</span>
              </div>
              <div style={{ height: 8, borderRadius: 4, background: 'rgba(255,255,255,0.06)', overflow: 'hidden' }}>
                <div style={{
                  height: '100%', borderRadius: 4,
                  width: `${roundPct}%`,
                  background: 'linear-gradient(90deg,#0ea5e9,#06b6d4)',
                  transition: 'width 0.3s ease',
                }} />
              </div>
            </div>
          )}
        </div>
      )}

      {/* Logs collapsibles */}
      <div style={{ marginTop: '1.25rem' }}>
        <button
          onClick={() => setShowLogs(v => !v)}
          style={{ fontSize: 12, color: 'var(--text-muted)', background: 'none', border: 'none', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ transition: 'transform 0.2s', transform: showLogs ? 'rotate(90deg)' : '' }}>▶</span>
          {showLogs ? 'Masquer les logs' : 'Afficher les logs'}
          <span style={{ color: '#475569' }}>({lines.length} lignes)</span>
        </button>

        {showLogs && (
          <pre style={{
            marginTop:   '0.75rem',
            padding:     '1rem',
            borderRadius: 8,
            background:  '#0d1117',
            color:       '#4ade80',
            fontSize:    12,
            fontFamily:  'monospace',
            maxHeight:   300,
            overflowY:   'auto',
            border:      '1px solid rgba(255,255,255,0.06)',
          }}>
            {lines.join('\n') || ' '}
          </pre>
        )}
      </div>
    </div>
  )
}

function DashboardResults({ detail }: { detail: RunDetailJson }) {
  const aller  = detail.aller
  const retour = detail.retour

  // Calcul des KPIs
  const validHops = aller.filter(h => !h.ratelimit && h.ip)
  const avgRtt    = validHops.length
    ? validHops.reduce((s, h) => s + h.avg_ms, 0) / validHops.length
    : 0
  const maxLoss   = validHops.reduce((m, h) => Math.max(m, h.loss_pct ?? 0), 0)

  const verdictFromLoss = maxLoss > 5 ? 'Faulty' : maxLoss > 0 ? 'Degraded' : 'Healthy'
  const [verdictColor, verdictAccent] = verdictFromLoss === 'Healthy'
    ? ['#34d399', '#10b981']
    : verdictFromLoss === 'Degraded'
    ? ['#fbbf24', '#f59e0b']
    : ['#f87171', '#ef4444']

  return (
    <div className="space-y-8">
      {/* KPI cards */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))', gap: '1rem' }}>
        <KpiCard
          label="Verdict"
          value={verdictFromLoss === 'Healthy' ? '✔ Sain' : verdictFromLoss === 'Degraded' ? '⚠ Dégradé' : '✖ Problème'}
          icon="🏁"
          color={verdictColor}
          accent={verdictAccent}
        />
        <KpiCard
          label="RTT moyen"
          value={avgRtt > 0 ? `${avgRtt.toFixed(1)} ms` : '—'}
          icon="⏱"
          color="#60a5fa"
          accent="#388bfd"
          sub={`${aller.length} hops`}
        />
        <KpiCard
          label="Perte max"
          value={`${maxLoss.toFixed(1)}%`}
          icon="📦"
          color={maxLoss > 5 ? '#f87171' : maxLoss > 0 ? '#fbbf24' : '#34d399'}
          accent={maxLoss > 5 ? '#ef4444' : maxLoss > 0 ? '#f59e0b' : '#10b981'}
          sub="hops réels"
        />
        {retour.length > 0 && (
          <KpiCard
            label="Hops retour"
            value={retour.length}
            icon="↩"
            color="#a78bfa"
            accent="#7c3aed"
            sub="via Globalping"
          />
        )}
      </div>

      {/* Chemin aller */}
      {aller.length > 0 && (
        <div>
          <div className="flex items-center gap-3 mb-4">
            <div style={{ width: 4, height: 28, borderRadius: 2, background: '#388bfd' }} />
            <h2 style={{ fontSize: '1.1rem', fontWeight: 700, color: 'var(--text-primary)' }}>Chemin aller</h2>
          </div>
          <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 'var(--card-radius)', padding: '1.25rem', boxShadow: 'var(--card-shadow)' }}>
            <HopChart hops={aller} />
          </div>
        </div>
      )}

      {/* Chemin retour */}
      {retour.length > 0 && (
        <div>
          <div className="flex items-center gap-3 mb-4">
            <div style={{ width: 4, height: 28, borderRadius: 2, background: '#7c3aed' }} />
            <h2 style={{ fontSize: '1.1rem', fontWeight: 700, color: 'var(--text-primary)' }}>
              Chemin retour <span style={{ color: 'var(--text-muted)', fontWeight: 400, fontSize: '0.9rem' }}>via Globalping</span>
            </h2>
          </div>
          <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border)', borderRadius: 'var(--card-radius)', padding: '1.25rem', boxShadow: 'var(--card-shadow)' }}>
            <HopChart hops={retour} />
          </div>
        </div>
      )}
    </div>
  )
}

// ── Page principale ───────────────────────────────────────────────────────────

export function DiagPage() {
  const { theme } = useTheme()
  const isDash    = theme === 'dashboard'

  const [cmd,         setCmd]         = useState('diag')
  const [target,      setTarget]      = useState('')
  const [rounds,      setRounds]      = useState(10)
  const [noSpeedtest, setNoSpeedtest] = useState(false)
  const [port,        setPort]        = useState(33434)
  const [flows,       setFlows]       = useState(16)
  const [jobId,       setJobId]       = useState<string | null>(null)
  const [running,     setRunning]     = useState(false)
  const [detail,      setDetail]      = useState<RunDetailJson | null>(null)
  const [error,       setError]       = useState('')

  const needsTarget = cmd !== 'check-env'
  const hasOptions  = cmd === 'diag' || cmd === 'aller' || cmd === 'mtr'
  const hasEcmp     = cmd === 'ecmp'

  const card = { background: 'var(--bg-card2)', border: '1px solid var(--border)', borderRadius: 'var(--card-radius)', boxShadow: 'var(--card-shadow)' }
  const inputStyle = {
    background: 'var(--bg-card)', border: '1px solid var(--border)',
    borderRadius: isDash ? 10 : 12, padding: '10px 16px',
    color: 'var(--text-primary)', width: '100%',
    fontSize: 14, outline: 'none',
  }
  const labelStyle = { display: 'block', fontSize: 12, fontWeight: 500, color: 'var(--text-muted)', marginBottom: 6, letterSpacing: '0.04em', textTransform: 'uppercase' as const }

  async function handleStart() {
    setError('')
    setDetail(null)
    if (needsTarget && !target.trim()) { setError('Veuillez saisir une cible'); return }
    try {
      setRunning(true)
      const args: Record<string, unknown> = {}
      if (needsTarget) args.target = target.trim()
      if (rounds !== 10) args.rounds = rounds
      if (noSpeedtest) args.no_speedtest = true
      if (hasEcmp) { args.port = port; args.flows = flows }
      const id = await startJob(cmd, args)
      setJobId(id)
    } catch (e) {
      setError(String(e))
      setRunning(false)
    }
  }

  async function handleDone() {
    setRunning(false)
    if (!HOP_COMMANDS.includes(cmd)) return
    try {
      const runs = await fetch('/api/history?last=1').then(r => r.json())
      if (runs.length > 0) {
        const d = await fetchRunDetail(runs[0].id)
        setDetail(d)
      }
    } catch { /* pas de DB */ }
  }

  async function handleStop() {
    if (jobId) await stopJob(jobId)
    setRunning(false)
    setJobId(null)
    setDetail(null)
  }

  const selectedCmd = COMMANDS.find(c => c.value === cmd)

  return (
    <div className="space-y-8">

      {/* En-tête */}
      <div>
        <h1 style={{ fontSize: '1.75rem', fontWeight: 700, color: 'var(--text-primary)' }}>
          {isDash ? '📊 Diagnostic réseau' : 'Diagnostic réseau'}
        </h1>
        <p style={{ color: 'var(--text-muted)', marginTop: 4, fontSize: 14 }}>
          Lancez une analyse MTR, speedtest ou looking glass vers n'importe quelle cible.
        </p>
      </div>

      {/* Formulaire */}
      <div style={{ ...card, padding: isDash ? '1.75rem' : '1.5rem' }}>

        {/* Sélecteur commande */}
        <div style={{ marginBottom: '1.25rem' }}>
          <label style={labelStyle}>Commande</label>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            {COMMANDS.map(c => {
              const active = cmd === c.value
              return (
                <button key={c.value} onClick={() => setCmd(c.value)} title={c.desc}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 8,
                    padding: isDash ? '8px 16px' : '7px 14px',
                    borderRadius: isDash ? 10 : 12,
                    border: active
                      ? `1px solid ${isDash ? 'rgba(56,139,253,0.5)' : '#3b82f6'}`
                      : '1px solid var(--border)',
                    background: active
                      ? isDash ? 'rgba(56,139,253,0.15)' : 'linear-gradient(135deg,#2563eb,#1d4ed8)'
                      : 'var(--bg-card)',
                    color: active ? (isDash ? '#388bfd' : '#fff') : 'var(--text-muted)',
                    fontSize: 13, fontWeight: 600, cursor: 'pointer',
                    boxShadow: active && isDash ? '0 0 16px rgba(56,139,253,0.2)' : 'none',
                    transition: 'all 0.15s',
                  }}>
                  <span>{c.icon}</span><span>{c.label}</span>
                </button>
              )
            })}
          </div>
          {selectedCmd && <p style={{ color: 'var(--text-muted)', fontSize: 13, marginTop: 8 }}>{selectedCmd.desc}</p>}
        </div>

        {/* Cible + options */}
        <div style={{ display: 'grid', gridTemplateColumns: needsTarget ? '1fr auto' : '1fr', gap: '1rem', marginBottom: '1.25rem' }}>
          {needsTarget && (
            <div>
              <label style={labelStyle}>Cible</label>
              <input type="text" value={target} placeholder="google.com ou 8.8.8.8"
                onChange={e => setTarget(e.target.value)}
                onKeyDown={e => e.key === 'Enter' && handleStart()}
                style={inputStyle} />
            </div>
          )}
          {hasOptions && (
            <div style={{ display: 'flex', gap: '1rem', alignItems: 'flex-end', flexWrap: 'wrap' }}>
              <div>
                <label style={labelStyle}>Rounds MTR</label>
                <input type="number" min={1} max={30} value={rounds}
                  onChange={e => setRounds(+e.target.value)}
                  style={{ ...inputStyle, width: 90 }} />
              </div>
              {(cmd === 'diag' || cmd === 'aller') && (
                <label style={{ display: 'flex', alignItems: 'center', gap: 10, cursor: 'pointer', paddingBottom: 4 }}>
                  <div onClick={() => setNoSpeedtest(v => !v)}
                    style={{
                      width: 40, height: 24, borderRadius: 12, position: 'relative',
                      background: noSpeedtest ? '#388bfd' : 'rgba(255,255,255,0.1)',
                      transition: 'background 0.2s', cursor: 'pointer', flexShrink: 0,
                    }}>
                    <span style={{
                      position: 'absolute', top: 4,
                      left: noSpeedtest ? 20 : 4,
                      width: 16, height: 16, borderRadius: '50%', background: '#fff',
                      transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.4)',
                    }} />
                  </div>
                  <span style={{ fontSize: 13, color: 'var(--text-primary)' }}>
                    Sans speedtest <span style={{ color: 'var(--text-muted)' }}>(plus rapide)</span>
                  </span>
                </label>
              )}
            </div>
          )}
        </div>

        {hasEcmp && (
          <div style={{ display: 'flex', gap: '1rem', marginBottom: '1.25rem', flexWrap: 'wrap' }}>
            <div>
              <label style={labelStyle}>Port destination</label>
              <input type="number" min={1} max={65535} value={port}
                onChange={e => setPort(+e.target.value)}
                style={{ ...inputStyle, width: 120 }} />
            </div>
            <div>
              <label style={labelStyle}>Nombre de flows</label>
              <input type="number" min={2} max={64} value={flows}
                onChange={e => setFlows(+e.target.value)}
                style={{ ...inputStyle, width: 110 }} />
            </div>
          </div>
        )}

        {error && (
          <div style={{ padding: '10px 16px', borderRadius: 10, background: 'rgba(239,68,68,0.1)', border: '1px solid rgba(239,68,68,0.3)', color: '#f87171', fontSize: 13, marginBottom: '1rem' }}>
            ⚠ {error}
          </div>
        )}

        {/* Bouton lancer */}
        {!running && (
          <button onClick={handleStart}
            style={{
              display: 'flex', alignItems: 'center', gap: 10,
              padding: isDash ? '12px 28px' : '11px 28px',
              borderRadius: isDash ? 12 : 12,
              border: 'none',
              background: isDash
                ? 'linear-gradient(135deg, #388bfd, #2563eb)'
                : 'linear-gradient(135deg, #2563eb, #1d4ed8)',
              color: '#fff', fontWeight: 700, fontSize: 14,
              cursor: 'pointer',
              boxShadow: isDash ? '0 4px 20px rgba(56,139,253,0.4)' : '0 4px 20px rgba(37,99,235,0.35)',
              transition: 'all 0.15s',
            }}>
            ▶ Lancer le diagnostic
          </button>
        )}
      </div>

      {/* ── Résultats ── */}

      {/* Mode dashboard : progress card dédiée */}
      {isDash && jobId && running && (
        <DashboardRunProgress jobId={jobId} target={target} onStop={handleStop} onDone={handleDone} />
      )}

      {/* Mode terminal : TerminalOutput classique + bouton stop */}
      {!isDash && jobId && (
        <div className="space-y-3">
          {running && (
            <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
              <button onClick={handleStop}
                style={{ padding: '8px 18px', borderRadius: 10, border: '1px solid rgba(239,68,68,0.4)', background: 'rgba(239,68,68,0.1)', color: '#f87171', fontSize: 13, fontWeight: 600, cursor: 'pointer' }}>
                ■ Arrêter
              </button>
            </div>
          )}
          <TerminalOutput jobId={jobId} onDone={handleDone} />
        </div>
      )}

      {/* Mode dashboard : résultats structurés après le run */}
      {isDash && detail && !running && <DashboardResults detail={detail} />}

      {/* Mode terminal : graphiques hop-par-hop */}
      {!isDash && detail && detail.aller.length > 0 && (
        <div className="space-y-6">
          <div className="flex items-center gap-3">
            <div className="w-1 h-7 rounded-full bg-blue-500" />
            <h2 className="text-xl font-bold text-white">Chemin aller</h2>
          </div>
          <HopChart hops={detail.aller} />
          {detail.retour.length > 0 && (
            <>
              <div className="flex items-center gap-3 mt-4">
                <div className="w-1 h-7 rounded-full bg-violet-500" />
                <h2 className="text-xl font-bold text-white">Chemin retour <span className="text-slate-500 font-normal text-base">via Globalping</span></h2>
              </div>
              <HopChart hops={detail.retour} />
            </>
          )}
        </div>
      )}

    </div>
  )
}
