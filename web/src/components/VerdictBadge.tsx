interface Props { verdict: string; size?: 'sm' | 'md' }

export function VerdictBadge({ verdict, size = 'md' }: Props) {
  const styles: Record<string, { bg: string; border: string; color: string }> = {
    Healthy:  { bg: 'rgba(16,185,129,0.1)',  border: 'rgba(16,185,129,0.35)', color: '#34d399' },
    Degraded: { bg: 'rgba(245,158,11,0.1)',  border: 'rgba(245,158,11,0.35)', color: '#fbbf24' },
    Faulty:   { bg: 'rgba(239,68,68,0.1)',   border: 'rgba(239,68,68,0.35)',  color: '#f87171' },
  }
  const s = styles[verdict] ?? { bg: 'rgba(100,116,139,0.1)', border: 'rgba(100,116,139,0.3)', color: '#94a3b8' }

  const label = { Healthy: '✔ SAIN', Degraded: '⚠ DÉGRADÉ', Faulty: '✖ FAULTY' }[verdict] ?? verdict
  const px = size === 'sm' ? 'px-2.5 py-0.5 text-xs' : 'px-3.5 py-1 text-sm'

  return (
    <span
      className={`${px} rounded-lg font-mono font-bold tracking-wide`}
      style={{ background: s.bg, border: `1px solid ${s.border}`, color: s.color }}
    >
      {label}
    </span>
  )
}
