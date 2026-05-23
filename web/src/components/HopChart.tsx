import {
  BarChart, Bar, XAxis, YAxis, Tooltip, Cell, ResponsiveContainer,
} from 'recharts'
import type { HopDetailJson } from '../api'

interface Props { hops: HopDetailJson[] }

function lossColor(loss: number | null, ratelimit: boolean) {
  if (ratelimit) return '#64748b'
  if (loss === null) return '#64748b'
  if (loss > 5) return '#ef4444'
  if (loss > 0) return '#f59e0b'
  return '#22c55e'
}

function rttColor(avg: number) {
  if (avg > 150) return '#ef4444'
  if (avg > 50)  return '#f59e0b'
  return '#38bdf8'
}

export function HopChart({ hops }: Props) {
  const data = hops.map(h => ({
    name: h.ip ? `${h.ttl} — ${h.ip}` : `${h.ttl} — *`,
    label: h.as_name ? `AS${h.asn} ${h.as_name}` : '',
    avg:   h.ratelimit ? 0 : h.avg_ms,
    max:   h.ratelimit ? 0 : h.max_ms,
    loss:  h.ratelimit ? null : h.loss_pct,
    ratelimit: h.ratelimit,
  }))

  return (
    <div className="grid grid-cols-1 xl:grid-cols-2 gap-4 mt-4">
      {/* RTT par hop */}
      <div className="rounded-2xl p-5" style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.07)' }}>
        <h3 className="text-sm font-semibold text-slate-300 mb-3">RTT par hop (ms)</h3>
        <ResponsiveContainer width="100%" height={Math.max(200, hops.length * 36)}>
          <BarChart data={data} layout="vertical" margin={{ left: 170, right: 20 }}>
            <XAxis type="number" tick={{ fill: '#94a3b8', fontSize: 12 }} />
            <YAxis
              type="category" dataKey="name"
              tick={{ fill: '#cbd5e1', fontSize: 13 }}
              width={170}
            />
            <Tooltip
              contentStyle={{ background: '#1e293b', border: '1px solid #334155', color: '#e2e8f0' }}
              formatter={(value, _name, entry) => {
                const v = Number(value)
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                const p = (entry as any).payload
                return [`moy ${v.toFixed(1)} ms | max ${p.max.toFixed(1)} ms${p.label ? ` | ${p.label}` : ''}`, 'RTT']
              }}
            />
            <Bar dataKey="avg" name="RTT moy" radius={2}>
              {data.map((d, i) => <Cell key={i} fill={rttColor(d.avg)} />)}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>

      {/* Perte par hop */}
      <div className="rounded-2xl p-5" style={{ background: '#0f1929', border: '1px solid rgba(255,255,255,0.07)' }}>
        <h3 className="text-sm font-semibold text-slate-300 mb-3">Perte par hop (%)</h3>
        <ResponsiveContainer width="100%" height={Math.max(200, hops.length * 36)}>
          <BarChart data={data} layout="vertical" margin={{ left: 170, right: 20 }}>
            <XAxis type="number" domain={[0, 100]} tick={{ fill: '#94a3b8', fontSize: 12 }} />
            <YAxis
              type="category" dataKey="name"
              tick={{ fill: '#cbd5e1', fontSize: 13 }}
              width={170}
            />
            <Tooltip
              contentStyle={{ background: '#1e293b', border: '1px solid #334155', color: '#e2e8f0' }}
              formatter={(value, _name, entry) => {
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                const p = (entry as any).payload
                return [p.ratelimit ? 'rate-limited (normal)' : `${Number(value).toFixed(1)} %`, 'Perte']
              }}
            />
            <Bar dataKey="loss" name="Perte %" radius={2} minPointSize={2}>
              {data.map((d, i) => <Cell key={i} fill={lossColor(d.loss, d.ratelimit)} />)}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
        <p className="text-xs text-slate-500 mt-2">Gris = rate-limited (pas une vraie perte)</p>
      </div>
    </div>
  )
}
