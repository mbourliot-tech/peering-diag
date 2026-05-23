import { useState } from 'react'
import { startJob } from '../api'
import { TerminalOutput } from '../components/TerminalOutput'

const CHECKS = [
  { icon: '🔒', label: 'RAW ICMP',  desc: 'Sockets RAW ICMP (admin/root requis)' },
  { icon: '⚡', label: 'mtr',       desc: 'Traceroute avec statistiques par hop' },
  { icon: '🌐', label: 'curl',      desc: 'Requêtes HTTP / accès internet' },
  { icon: '📡', label: 'iperf3',    desc: 'Test de débit réseau' },
]

export function CheckEnvPage() {
  const [jobId,   setJobId]   = useState<string | null>(null)
  const [running, setRunning] = useState(false)
  const [done,    setDone]    = useState(false)
  const [error,   setError]   = useState('')

  async function handleCheck() {
    setError('')
    setDone(false)
    try {
      setRunning(true)
      const id = await startJob('check-env', {})
      setJobId(id)
    } catch (e) {
      setError(String(e))
      setRunning(false)
    }
  }

  return (
    <div className="space-y-8">

      {/* En-tête */}
      <div>
        <h1 className="text-3xl font-bold text-white">Vérification de l'environnement</h1>
        <p className="text-slate-500 mt-1">Vérifie que tous les outils nécessaires sont disponibles sur ce système.</p>
      </div>

      {/* Outils vérifiés */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        {CHECKS.map(c => (
          <div key={c.label} className="rounded-2xl p-5"
            style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.07)' }}>
            <div className="text-2xl mb-2">{c.icon}</div>
            <div className="text-white font-bold font-mono">{c.label}</div>
            <div className="text-slate-500 text-xs mt-1">{c.desc}</div>
          </div>
        ))}
      </div>

      {/* Bouton */}
      <div className="rounded-2xl p-6 space-y-4"
        style={{ background: 'linear-gradient(135deg,#0f1929,#111827)', border: '1px solid rgba(255,255,255,0.07)' }}>
        {error && <div className="px-4 py-3 rounded-xl bg-red-500/10 border border-red-500/30 text-red-400 text-sm">⚠ {error}</div>}
        <button
          onClick={handleCheck}
          disabled={running}
          className="flex items-center gap-2.5 px-7 py-3 rounded-xl text-white font-semibold text-sm disabled:opacity-40 shadow-lg"
          style={{ background: 'linear-gradient(135deg,#2563eb,#1d4ed8)', boxShadow: '0 4px 20px rgba(37,99,235,0.35)' }}>
          {running ? (
            <><span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />Vérification…</>
          ) : (
            <>🔍 Lancer la vérification</>
          )}
        </button>
      </div>

      {jobId && <TerminalOutput jobId={jobId} onDone={() => { setRunning(false); setDone(true) }} />}

      {done && (
        <div className="rounded-2xl p-5 flex items-center gap-4"
          style={{ background: '#0c1a14', border: '1px solid rgba(34,197,94,0.25)' }}>
          <span className="text-3xl">✅</span>
          <div>
            <div className="text-emerald-400 font-semibold">Vérification terminée</div>
            <div className="text-slate-500 text-sm mt-0.5">Consultez la sortie ci-dessus pour les détails.</div>
          </div>
        </div>
      )}
    </div>
  )
}
