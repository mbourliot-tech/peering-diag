// Types
export interface RunJson {
  id: number
  timestamp: string
  target: string
  verdict: 'Healthy' | 'Degraded' | 'Faulty' | string
  finding: string
  max_loss_aller: number
  max_loss_retour: number
  avg_rtt_ms: number
  dl_mbps: number
}

export interface HopDetailJson {
  ttl: number
  ip: string | null
  asn: number | null
  as_name: string | null
  loss_pct: number | null
  avg_ms: number
  min_ms: number
  max_ms: number
  jitter_ms: number
  ratelimit: boolean
}

export interface RunDetailJson {
  id: number
  timestamp: string
  target: string
  aller: HopDetailJson[]
  retour: HopDetailJson[]
}

export interface HourStatJson {
  hour: number
  total: number
  bad: number
  bad_pct: number
  avg_loss: number
  avg_rtt_ms: number
  avg_dl_mbps: number
}

export interface JobInfo {
  id: string
  command: string
  status: 'running' | 'done' | 'failed'
}

export interface WatchSeriesJson {
  id: number
  started_at: string
  target: string
  interval_s: number
  run_count: number
  last_verdict: string | null
  job_id: string | null
}

// API helpers
const BASE = '/api'

export async function startJob(command: string, args: Record<string, unknown> = {}): Promise<string> {
  const res = await fetch(`${BASE}/jobs`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args }),
  })
  if (!res.ok) throw new Error(await res.text())
  const data = await res.json()
  return data.job_id
}

export async function getJobStatus(id: string): Promise<JobInfo> {
  const res = await fetch(`${BASE}/jobs/${id}`)
  if (!res.ok) throw new Error('Job introuvable')
  return res.json()
}

export async function stopJob(id: string): Promise<void> {
  await fetch(`${BASE}/jobs/${id}`, { method: 'DELETE' })
}

const SSE_RETRY_DELAYS = [1_000, 2_000, 4_000, 8_000, 16_000] // ms

export function streamJob(
  id: string,
  onLine: (line: string) => void,
  onDone: () => void,
): () => void {
  let es:           EventSource | null = null
  let retryTimer:   ReturnType<typeof setTimeout> | null = null
  let stopped       = false
  let attempt       = 0
  let linesReceived = 0   // lignes déjà transmises à onLine

  function connect() {
    if (stopped) return

    // Sur reconnexion, le serveur rejoue tout le buffer.
    // On saute les lignes déjà vues pour éviter les doublons.
    let skipRemaining = linesReceived

    es = new EventSource(`${BASE}/jobs/${id}/stream`)

    es.onmessage = (e) => {
      attempt = 0                      // réinitialise le backoff sur message reçu
      if (skipRemaining > 0) { skipRemaining--; return }
      linesReceived++
      onLine(e.data)
    }

    es.addEventListener('done', () => {
      stopped = true
      es?.close()
      onDone()
    })

    es.onerror = () => {
      es?.close()
      es = null
      if (stopped) return

      if (attempt >= SSE_RETRY_DELAYS.length) {
        // Abandon après toutes les tentatives
        stopped = true
        onDone()
        return
      }
      const delay = SSE_RETRY_DELAYS[attempt++]
      retryTimer = setTimeout(connect, delay)
    }
  }

  connect()

  return () => {
    stopped = true
    if (retryTimer) clearTimeout(retryTimer)
    es?.close()
    es = null
  }
}

export async function fetchTargets(): Promise<string[]> {
  const res = await fetch(`${BASE}/history/targets`)
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export async function fetchHistory(params: { target?: string; last?: number; since?: string } = {}): Promise<RunJson[]> {
  const qs = new URLSearchParams()
  if (params.target) qs.set('target', params.target)
  if (params.last)   qs.set('last', String(params.last))
  if (params.since)  qs.set('since', params.since)
  const res = await fetch(`${BASE}/history?${qs}`)
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export async function fetchByHour(target?: string): Promise<HourStatJson[]> {
  const qs = target ? `?target=${encodeURIComponent(target)}` : ''
  const res = await fetch(`${BASE}/history/by-hour${qs}`)
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export async function fetchRunDetail(id: number): Promise<RunDetailJson> {
  const res = await fetch(`${BASE}/history/run/${id}`)
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export interface MapHopJson {
  ttl:       number
  ip:        string | null
  asn:       number | null
  as_name:   string | null
  lat:       number | null
  lon:       number | null
  city:      string | null
  loss_pct:  number | null
  avg_ms:    number
  ratelimit: boolean
}

export interface MapRunJson {
  id:        number
  timestamp: string
  target:    string
  aller:     MapHopJson[]
  retour:    MapHopJson[]
}

export async function fetchRunMap(id: number): Promise<MapRunJson> {
  const res = await fetch(`${BASE}/history/run/${id}/map`)
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export async function fetchWatchList(): Promise<WatchSeriesJson[]> {
  const res = await fetch(`${BASE}/watch`)
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export async function startWatch(target: string, interval: number, noSpeedtest: boolean): Promise<string> {
  const res = await fetch(`${BASE}/watch`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ target, interval, no_speedtest: noSpeedtest }),
  })
  if (!res.ok) throw new Error(await res.text())
  const data = await res.json()
  return data.job_id
}

export async function stopWatch(jobId: string): Promise<void> {
  const res = await fetch(`${BASE}/watch/${jobId}`, { method: 'DELETE' })
  if (!res.ok) throw new Error(await res.text())
}

export function fmtTs(ts: string): string {
  return ts.length >= 16 ? ts.slice(0, 16).replace('T', ' ') : ts
}

// ─── DB Maintenance ───────────────────────────────────────────────────────────

export interface DbStatsJson {
  run_count:          number
  hop_count:          number
  speedtest_count:    number
  watch_series_count: number
  oldest_run:         string | null
  newest_run:         string | null
  db_size_bytes:      number
  human_size:         string
}

export async function fetchDbStats(): Promise<DbStatsJson> {
  const res = await fetch(`${BASE}/db/stats`)
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export async function vacuumDb(): Promise<{ message: string }> {
  const res = await fetch(`${BASE}/db/vacuum`, { method: 'POST' })
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}

export async function purgeDb(opts: { older_than_days?: number; keep_last?: number }): Promise<{ deleted: number }> {
  const res = await fetch(`${BASE}/db/purge`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(opts),
  })
  if (!res.ok) throw new Error(await res.text())
  return res.json()
}
