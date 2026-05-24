import { useState, useEffect } from 'react'
import { streamJob } from '../api'

export interface Progress {
  phase:        string
  phaseIndex:   number
  totalPhases:  number
  currentRound: number
  totalRounds:  number
  elapsed:      string
}

export function parseProgress(lines: string[]): Progress {
  let phase = '', phaseIndex = 0, totalPhases = 3
  let currentRound = 0, totalRounds = 0, elapsed = ''

  for (const line of lines) {
    if (/PHASE\s+1|CHEMIN\s+ALLER/i.test(line))  { phase = '📡 MTR aller';        phaseIndex = 1 }
    if (/PHASE\s+2|CHEMIN\s+RETOUR/i.test(line)) { phase = '↩ Chemin retour';     phaseIndex = 2 }
    if (/speedtest|SPEEDTEST|Speedtest/i.test(line)) { phase = '⚡ Speedtest';     phaseIndex = 3 }
    if (/looking.glass|LG/i.test(line) && phaseIndex === 0) { phase = '🔍 Looking Glass'; phaseIndex = 1; totalPhases = 1 }
    if (/ECMP/i.test(line) && phaseIndex === 0)  { phase = '⑂ Exploration ECMP'; phaseIndex = 1; totalPhases = 1 }
    if (/check.env/i.test(line) && phaseIndex === 0) { phase = '🔧 Vérification'; phaseIndex = 1; totalPhases = 1 }

    const rm = line.match(/round\s+(\d+)\/(\d+)/i)
    if (rm) { currentRound = parseInt(rm[1]); totalRounds = parseInt(rm[2]) }

    const em = line.match(/\[(\d{1,2}:\d{2}(?::\d{2})?)\]/)
    if (em) elapsed = em[1]
  }

  return { phase, phaseIndex, totalPhases, currentRound, totalRounds, elapsed }
}

export function useJobStream(jobId: string | null, onDone?: () => void) {
  const [lines,   setLines]   = useState<string[]>([])
  const [done,    setDone]    = useState(false)
  const [elapsed, setElapsed] = useState(0)

  useEffect(() => {
    if (!jobId) return
    setLines([])
    setDone(false)
    setElapsed(0)

    const timer = window.setInterval(() => setElapsed(s => s + 1), 1000)
    const cleanup = streamJob(
      jobId,
      line => setLines(prev => [...prev, line]),
      () => { setDone(true); clearInterval(timer); onDone?.() },
    )
    return () => { cleanup(); clearInterval(timer) }
  }, [jobId])

  return { lines, done, elapsed }
}
