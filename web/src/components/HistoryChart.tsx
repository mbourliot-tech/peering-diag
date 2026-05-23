import {
  ComposedChart, BarChart, Line, Bar, Cell, XAxis, YAxis, Tooltip,
  CartesianGrid, Legend, ResponsiveContainer, ReferenceArea,
} from 'recharts'
import type { RunJson, HourStatJson } from '../api'
import { fmtTs } from '../api'

// ─── Tendance RTT + perte dans le temps ───────────────────────────────────────

interface TrendProps { runs: RunJson[] }

export function TrendChart({ runs }: TrendProps) {
  const data = runs.map(r => ({
    ts:     fmtTs(r.timestamp).slice(5), // "MM-DD HH:MM"
    rtt:    r.avg_rtt_ms > 0 ? +r.avg_rtt_ms.toFixed(1) : null,
    loss:   +(Math.max(r.max_loss_aller, r.max_loss_retour)).toFixed(2),
    verdict: r.verdict,
    dl:     r.dl_mbps > 0 ? +r.dl_mbps.toFixed(0) : null,
  }))

  return (
    <div className="rounded-2xl p-5" style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.07)' }}>
      <h3 className="text-sm font-semibold text-slate-300 mb-3">Tendance RTT & Perte</h3>
      <ResponsiveContainer width="100%" height={240}>
        <ComposedChart data={data} margin={{ right: 20 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
          <XAxis dataKey="ts" tick={{ fill: '#94a3b8', fontSize: 10 }} interval="preserveStartEnd" />
          <YAxis yAxisId="rtt" tick={{ fill: '#38bdf8', fontSize: 11 }} label={{ value: 'ms', fill: '#38bdf8', angle: -90, position: 'insideLeft' }} />
          <YAxis yAxisId="loss" orientation="right" tick={{ fill: '#f59e0b', fontSize: 11 }} domain={[0, 'auto']} label={{ value: '%', fill: '#f59e0b', angle: 90, position: 'insideRight' }} />
          <Tooltip
            contentStyle={{ background: '#1e293b', border: '1px solid #334155', color: '#e2e8f0', fontSize: 12 }}
          />
          <Legend wrapperStyle={{ color: '#94a3b8', fontSize: 12 }} />
          {/* Zones colorées par verdict */}
          {data.map((d, i) => d.verdict !== 'Healthy' ? (
            <ReferenceArea
              key={i} yAxisId="rtt"
              x1={d.ts} x2={d.ts}
              fill={d.verdict === 'Faulty' ? '#ef444420' : '#f59e0b20'}
            />
          ) : null)}
          <Line yAxisId="rtt"  type="monotone" dataKey="rtt"  name="RTT moy (ms)" stroke="#38bdf8" dot={false} strokeWidth={2} connectNulls />
          <Bar  yAxisId="loss" dataKey="loss" name="Perte max (%)" fill="#f59e0b" opacity={0.5} />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  )
}

// ─── Pattern par heure ────────────────────────────────────────────────────────

interface HourProps { hours: HourStatJson[] }

export function HourChart({ hours }: HourProps) {
  // Compléter les 24 heures (y compris celles sans données)
  const all = Array.from({ length: 24 }, (_, h) => {
    const found = hours.find(x => x.hour === h)
    return found ?? { hour: h, total: 0, bad: 0, bad_pct: 0, avg_loss: 0, avg_rtt_ms: 0, avg_dl_mbps: 0 }
  })

  function barColor(bad_pct: number) {
    if (bad_pct >= 80) return '#ef4444'
    if (bad_pct >= 40) return '#f59e0b'
    if (bad_pct >  0)  return '#84cc16'
    return '#22c55e'
  }

  return (
    <div className="rounded-2xl p-5" style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.07)' }}>
      <h3 className="text-sm font-semibold text-slate-300 mb-3">Pattern par heure (% dégradé)</h3>
      <ResponsiveContainer width="100%" height={200}>
        <BarChart data={all} margin={{ right: 10 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
          <XAxis dataKey="hour" tickFormatter={h => `${h}h`} tick={{ fill: '#94a3b8', fontSize: 10 }} />
          <YAxis domain={[0, 100]} tick={{ fill: '#94a3b8', fontSize: 11 }} tickFormatter={v => `${v}%`} />
          <Tooltip
            contentStyle={{ background: '#1e293b', border: '1px solid #334155', color: '#e2e8f0', fontSize: 12 }}
            formatter={(value, _name, entry) => {
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              const p = (entry as any).payload
              return [`${Number(value).toFixed(0)}% dégradé (${p.bad}/${p.total} runs)`, `${p.hour}h`]
            }}
          />
          <Bar dataKey="bad_pct" name="% dégradé" radius={2}>
            {all.map((d, i) => (
              <Cell key={i} fill={barColor(d.bad_pct)} />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}
