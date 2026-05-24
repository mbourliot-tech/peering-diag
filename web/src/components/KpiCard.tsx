interface Props {
  label:   string
  value:   string | number
  sub?:    string
  icon?:   string
  color?:  string  // CSS color pour la valeur
  accent?: string  // couleur de la barre de gauche
}

export function KpiCard({ label, value, sub, icon, color = '#f0f6fc', accent = '#388bfd' }: Props) {
  return (
    <div style={{
      background:   'var(--bg-card2)',
      border:       '1px solid var(--border)',
      boxShadow:    'var(--card-shadow)',
      borderRadius: 'var(--card-radius)',
      padding:      '1.25rem 1.5rem',
      borderLeft:   `3px solid ${accent}`,
      transition:   'transform 0.15s, box-shadow 0.15s',
    }}
    onMouseEnter={e => {
      ;(e.currentTarget as HTMLElement).style.transform = 'translateY(-2px)'
      ;(e.currentTarget as HTMLElement).style.boxShadow = '0 12px 40px rgba(0,0,0,0.6), 0 1px 0 rgba(255,255,255,0.07)'
    }}
    onMouseLeave={e => {
      ;(e.currentTarget as HTMLElement).style.transform = ''
      ;(e.currentTarget as HTMLElement).style.boxShadow = 'var(--card-shadow)'
    }}>
      <div style={{ color: 'var(--text-muted)', fontSize: 12, fontWeight: 500, letterSpacing: '0.05em', textTransform: 'uppercase', marginBottom: 8 }}>
        {icon && <span style={{ marginRight: 6 }}>{icon}</span>}{label}
      </div>
      <div style={{ fontSize: '2rem', fontWeight: 700, color, lineHeight: 1.1, fontVariantNumeric: 'tabular-nums' }}>
        {value}
      </div>
      {sub && <div style={{ color: 'var(--text-muted)', fontSize: 12, marginTop: 6 }}>{sub}</div>}
    </div>
  )
}
