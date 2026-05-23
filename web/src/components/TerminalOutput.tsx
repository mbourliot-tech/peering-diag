import { useEffect, useRef, useState } from 'react'
import { streamJob } from '../api'

interface Props { jobId: string | null; onDone?: () => void }

// ── Parser de progression ─────────────────────────────────────────────────────

interface Progress {
  phase:        string   // label de la phase en cours
  phaseIndex:   number   // 0=init, 1=aller, 2=retour, 3=speedtest
  totalPhases:  number   // estimé selon la commande
  currentRound: number
  totalRounds:  number
  elapsed:      string
}

function parseProgress(lines: string[]): Progress {
  let phase = ''
  let phaseIndex = 0
  let totalPhases = 3
  let currentRound = 0
  let totalRounds = 0
  let elapsed = ''

  for (const line of lines) {
    // ── Détection des phases (bannières ASCII) ──
    if (/PHASE\s+1|CHEMIN\s+ALLER/i.test(line)) {
      phase = '📡 MTR aller'
      phaseIndex = 1
    }
    if (/PHASE\s+2|CHEMIN\s+RETOUR/i.test(line)) {
      phase = '↩ Chemin retour'
      phaseIndex = 2
    }
    if (/speedtest|SPEEDTEST|Speedtest|PHASE\s+3/i.test(line)) {
      phase = '⚡ Speedtest'
      phaseIndex = 3
    }
    if (/looking.glass|LG/i.test(line) && phaseIndex === 0) {
      phase = '🔍 Looking Glass'
      phaseIndex = 1
      totalPhases = 1
    }
    if (/ECMP/i.test(line) && phaseIndex === 0) {
      phase = '⑂ Exploration ECMP'
      phaseIndex = 1
      totalPhases = 1
    }
    if (/check.env|CHECK.ENV/i.test(line) && phaseIndex === 0) {
      phase = '🔧 Vérification'
      phaseIndex = 1
      totalPhases = 1
    }

    // ── Round progress depuis indicatif : "round 5/10" ──
    const roundMatch = line.match(/round\s+(\d+)\/(\d+)/i)
    if (roundMatch) {
      currentRound = parseInt(roundMatch[1])
      totalRounds  = parseInt(roundMatch[2])
    }

    // ── Elapsed time depuis indicatif : "[00:01:23]" ──
    const elapsedMatch = line.match(/\[(\d{1,2}:\d{2}(?::\d{2})?)\]/)
    if (elapsedMatch) elapsed = elapsedMatch[1]
  }

  return { phase, phaseIndex, totalPhases, currentRound, totalRounds, elapsed }
}

// ── Composant ─────────────────────────────────────────────────────────────────

export function TerminalOutput({ jobId, onDone }: Props) {
  const [lines,    setLines]   = useState<string[]>([])
  const [done,     setDone]    = useState(false)
  const [elapsed,  setElapsed] = useState(0)   // secondes depuis le démarrage
  const ref      = useRef<HTMLPreElement>(null)
  const timerRef = useRef<number | null>(null)

  useEffect(() => {
    if (!jobId) return
    setLines([])
    setDone(false)
    setElapsed(0)

    // Chrono côté client
    timerRef.current = window.setInterval(() => setElapsed(s => s + 1), 1000)

    const cleanup = streamJob(
      jobId,
      (line) => setLines(prev => [...prev, line]),
      () => {
        setDone(true)
        if (timerRef.current) window.clearInterval(timerRef.current)
        onDone?.()
      },
    )
    return () => {
      cleanup()
      if (timerRef.current) window.clearInterval(timerRef.current)
    }
  }, [jobId])

  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight
  }, [lines])

  if (!jobId) return null

  const prog = parseProgress(lines)

  // Formatage du chrono client
  const mm = String(Math.floor(elapsed / 60)).padStart(2, '0')
  const ss = String(elapsed % 60).padStart(2, '0')
  const chronoLabel = prog.elapsed || `${mm}:${ss}`

  // Progression rounds (0..1)
  const roundPct = prog.totalRounds > 0
    ? (prog.currentRound / prog.totalRounds) * 100
    : 0

  // Progression phases (0..1)
  const phasePct = prog.totalPhases > 0
    ? (prog.phaseIndex / prog.totalPhases) * 100
    : 0

  return (
    <div className="space-y-3">

      {/* ── Carte de progression ──────────────────────────────────────────── */}
      <div className="rounded-2xl p-5"
        style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.07)' }}>

        {/* Ligne supérieure : phase + chrono + statut */}
        <div className="flex items-center gap-4 flex-wrap">
          <div className="flex items-center gap-2.5 flex-1 min-w-0">
            {!done && <span className="inline-block w-2.5 h-2.5 rounded-full bg-blue-400 animate-pulse shrink-0" />}
            {done  && <span className="inline-block w-2.5 h-2.5 rounded-full bg-emerald-400 shrink-0" />}
            <span className={`font-semibold text-base ${done ? 'text-emerald-400' : 'text-white'}`}>
              {done
                ? '✔ Diagnostic terminé'
                : prog.phase || 'Initialisation…'}
            </span>
          </div>

          {/* Chrono */}
          <div className="flex items-center gap-2 text-slate-500 text-sm font-mono shrink-0">
            <span>⏱</span>
            <span>{chronoLabel}</span>
          </div>

          {/* Statut badge */}
          <span className={`px-3 py-1 rounded-lg text-xs font-semibold ${
            done
              ? 'bg-emerald-500/15 text-emerald-400 border border-emerald-500/30'
              : 'bg-blue-500/15 text-blue-400 border border-blue-500/30'
          }`}>
            {done ? 'Terminé' : 'En cours'}
          </span>
        </div>

        {/* Barre phases globale */}
        {!done && prog.phaseIndex > 0 && (
          <div className="mt-4 space-y-1.5">
            <div className="flex justify-between text-xs text-slate-500">
              <span>Phase {prog.phaseIndex}/{prog.totalPhases}</span>
              <span className="text-slate-600">{Math.round(phasePct)}%</span>
            </div>
            <div className="h-1.5 rounded-full overflow-hidden" style={{ background: '#1e3a5f' }}>
              <div
                className="h-full rounded-full transition-all duration-500"
                style={{ width: `${phasePct}%`, background: 'linear-gradient(90deg,#2563eb,#7c3aed)' }}
              />
            </div>
          </div>
        )}

        {/* Barre rounds MTR */}
        {!done && prog.totalRounds > 0 && (
          <div className="mt-3 space-y-1.5">
            <div className="flex justify-between text-xs text-slate-500">
              <span>Round MTR</span>
              <span className="font-mono">
                <span className="text-sky-400 font-bold">{prog.currentRound}</span>
                <span className="text-slate-600"> / {prog.totalRounds}</span>
              </span>
            </div>
            <div className="h-1.5 rounded-full overflow-hidden" style={{ background: '#1e3a5f' }}>
              <div
                className="h-full rounded-full transition-all duration-300"
                style={{ width: `${roundPct}%`, background: 'linear-gradient(90deg,#0ea5e9,#06b6d4)' }}
              />
            </div>
            {/* Pastilles rounds */}
            <div className="flex gap-1 mt-1 flex-wrap">
              {Array.from({ length: prog.totalRounds }, (_, i) => (
                <div
                  key={i}
                  className="h-1 flex-1 rounded-full min-w-0 transition-all duration-300"
                  style={{
                    background: i < prog.currentRound
                      ? '#0ea5e9'
                      : i === prog.currentRound
                        ? '#7c3aed'
                        : 'rgba(255,255,255,0.07)',
                  }}
                />
              ))}
            </div>
          </div>
        )}

        {/* Résumé ligne courante */}
        {!done && lines.length > 0 && (
          <p className="mt-3 text-xs text-slate-600 font-mono truncate">
            {lines[lines.length - 1]}
          </p>
        )}
      </div>

      {/* ── Fenêtre terminal ─────────────────────────────────────────────── */}
      <div className="rounded-2xl overflow-hidden shadow-2xl"
        style={{ border: '1px solid rgba(255,255,255,0.06)' }}>

        {/* Barre de titre */}
        <div className="flex items-center gap-2 px-4 py-3"
          style={{ background: '#161f2e', borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
          <span className="w-3 h-3 rounded-full bg-red-500/70" />
          <span className="w-3 h-3 rounded-full bg-yellow-500/70" />
          <span className="w-3 h-3 rounded-full bg-green-500/70" />
          <span className="ml-3 text-xs text-slate-500 font-mono flex-1">
            peering-diag · job {jobId?.slice(0, 8)}… · {lines.length} lignes
          </span>
        </div>

        {/* Sortie brute */}
        <pre
          ref={ref}
          className="font-mono text-sm p-5 overflow-y-auto whitespace-pre-wrap leading-relaxed"
          style={{ height: 'calc(100vh - 24rem)', minHeight: '420px', background: '#0d1117', color: '#4ade80' }}
        >
          {lines.join('\n') || ' '}
        </pre>
      </div>
    </div>
  )
}
