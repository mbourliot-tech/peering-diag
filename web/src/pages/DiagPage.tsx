import { useState } from 'react'
import { startJob, fetchRunDetail } from '../api'
import { TerminalOutput } from '../components/TerminalOutput'
import { HopChart } from '../components/HopChart'
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

// Styles partagés
const inputCls = "bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 placeholder-slate-600 transition-all w-full"
const labelCls = "block text-sm font-medium text-slate-400 mb-1.5"

export function DiagPage() {
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

  // Seules ces commandes stockent des hops MTR en base → graphiques disponibles
  const HOP_COMMANDS = ['diag', 'aller']

  async function handleDone() {
    setRunning(false)
    if (!HOP_COMMANDS.includes(cmd)) return   // ecmp, lg, retour… → pas de graphiques
    try {
      const runs = await fetch('/api/history?last=1').then(r => r.json())
      if (runs.length > 0) {
        const d = await fetchRunDetail(runs[0].id)
        setDetail(d)
      }
    } catch { /* pas de DB */ }
  }

  const selectedCmd = COMMANDS.find(c => c.value === cmd)

  return (
    <div className="space-y-8">

      {/* En-tête de page */}
      <div>
        <h1 className="text-3xl font-bold text-white">Diagnostic réseau</h1>
        <p className="text-slate-500 mt-1">Lancez une analyse MTR, speedtest ou looking glass vers n'importe quelle cible.</p>
      </div>

      {/* Carte formulaire */}
      <div className="rounded-2xl p-6 space-y-6"
        style={{ background: 'linear-gradient(135deg, #0f1929 0%, #111827 100%)', border: '1px solid rgba(255,255,255,0.07)' }}>

        {/* Sélecteur de commande — pills visuels */}
        <div>
          <label className={labelCls}>Commande</label>
          <div className="flex flex-wrap gap-2">
            {COMMANDS.map(c => (
              <button
                key={c.value}
                onClick={() => setCmd(c.value)}
                className={`flex items-center gap-2 px-4 py-2 rounded-xl text-sm font-semibold transition-all duration-150 ${
                  cmd === c.value
                    ? 'text-white shadow-lg shadow-blue-500/20'
                    : 'bg-slate-800/60 text-slate-400 border border-slate-700/60 hover:border-slate-600 hover:text-slate-200'
                }`}
                style={cmd === c.value ? { background: 'linear-gradient(135deg, #2563eb, #1d4ed8)', border: '1px solid #3b82f6' } : {}}
                title={c.desc}
              >
                <span>{c.icon}</span>
                <span>{c.label}</span>
              </button>
            ))}
          </div>
          {selectedCmd && (
            <p className="text-slate-500 text-sm mt-2 ml-1">{selectedCmd.desc}</p>
          )}
        </div>

        {/* Cible + options */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
          {needsTarget && (
            <div>
              <label className={labelCls}>Cible</label>
              <input
                type="text"
                value={target}
                onChange={e => setTarget(e.target.value)}
                placeholder="google.com ou 8.8.8.8"
                onKeyDown={e => e.key === 'Enter' && handleStart()}
                className={inputCls}
              />
            </div>
          )}

          {hasOptions && (
            <div className="flex flex-wrap gap-5 items-end">
              <div>
                <label className={labelCls}>Rounds MTR</label>
                <input
                  type="number" min={1} max={30} value={rounds}
                  onChange={e => setRounds(+e.target.value)}
                  className="bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 w-24 transition-all"
                />
              </div>
              {(cmd === 'diag' || cmd === 'aller') && (
                <label className="flex items-center gap-2.5 cursor-pointer select-none mb-0.5">
                  <div
                    onClick={() => setNoSpeedtest(v => !v)}
                    className={`w-10 h-6 rounded-full transition-all duration-200 relative ${noSpeedtest ? 'bg-blue-600' : 'bg-slate-700'}`}
                  >
                    <span className={`absolute top-1 w-4 h-4 bg-white rounded-full shadow transition-all duration-200 ${noSpeedtest ? 'left-5' : 'left-1'}`} />
                  </div>
                  <span className="text-sm text-slate-300">Sans speedtest <span className="text-slate-500">(plus rapide)</span></span>
                </label>
              )}
            </div>
          )}
        </div>

        {/* Options ECMP */}
        {hasEcmp && (
          <div className="flex flex-wrap gap-5 items-end">
            <div>
              <label className={labelCls}>Port destination</label>
              <input
                type="number" min={1} max={65535} value={port}
                onChange={e => setPort(+e.target.value)}
                className="bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 w-32 transition-all"
              />
            </div>
            <div>
              <label className={labelCls}>Nombre de flows</label>
              <input
                type="number" min={2} max={64} value={flows}
                onChange={e => setFlows(+e.target.value)}
                className="bg-slate-800/80 border border-slate-700 rounded-xl px-4 py-2.5 text-slate-100 w-28 transition-all"
              />
            </div>
          </div>
        )}

        {/* Erreur */}
        {error && (
          <div className="flex items-center gap-2 px-4 py-3 rounded-xl bg-red-500/10 border border-red-500/30 text-red-400 text-sm">
            ⚠ {error}
          </div>
        )}

        {/* Bouton */}
        <button
          onClick={handleStart}
          disabled={running}
          className="flex items-center gap-2.5 px-7 py-3 rounded-xl text-white font-semibold text-sm transition-all duration-150 disabled:opacity-40 disabled:cursor-not-allowed shadow-lg"
          style={running ? { background: '#1e3a5f' } : {
            background: 'linear-gradient(135deg, #2563eb, #1d4ed8)',
            boxShadow: '0 4px 20px rgba(37,99,235,0.35)',
          }}
        >
          {running ? (
            <>
              <span className="inline-block w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
              Diagnostic en cours…
            </>
          ) : (
            <>▶ Lancer le diagnostic</>
          )}
        </button>
      </div>

      {/* Terminal output */}
      {jobId && <TerminalOutput jobId={jobId} onDone={handleDone} />}

      {/* Graphiques hop-par-hop */}
      {detail && detail.aller.length > 0 && (
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
