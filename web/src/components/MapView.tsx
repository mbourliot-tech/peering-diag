import { useEffect, useState } from 'react'
import { MapContainer, TileLayer, CircleMarker, Polyline, Popup } from 'react-leaflet'
import 'leaflet/dist/leaflet.css'
import { fetchRunMap, type MapHopJson, type MapRunJson } from '../api'

const TILE_URL = 'https://{s}.basemaps.cartocdn.com/rastertiles/voyager/{z}/{x}/{y}{r}.png'
const TILE_ATTR = '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors © <a href="https://carto.com/attributions">CARTO</a>'

function hopColor(loss: number | null, ratelimit: boolean): string {
  if (ratelimit) return '#475569'
  if (loss === null) return '#94a3b8'
  if (loss > 5) return '#ef4444'
  if (loss > 0) return '#f59e0b'
  return '#10b981'
}

function HopPopup({ hop, dir }: { hop: MapHopJson; dir: 'aller' | 'retour' }) {
  return (
    <div style={{ minWidth: 160, fontFamily: 'monospace', fontSize: 12, color: '#1e293b' }}>
      <div style={{ fontWeight: 700, marginBottom: 4 }}>Hop {hop.ttl} — {dir}</div>
      <div>{hop.ip ?? '*'}</div>
      {hop.asn && <div>AS{hop.asn} {hop.as_name}</div>}
      {hop.city && <div style={{ color: '#475569' }}>{hop.city}</div>}
      <div style={{ marginTop: 4 }}>RTT : {hop.avg_ms.toFixed(1)} ms</div>
      {hop.loss_pct != null && (
        <div style={{ color: hop.loss_pct > 5 ? '#ef4444' : hop.loss_pct > 0 ? '#f59e0b' : '#10b981' }}>
          Perte : {hop.loss_pct.toFixed(1)}%
        </div>
      )}
    </div>
  )
}

// Détecte si deux chemins ont des points en commun (même lat/lon à ε près)
function pathsOverlap(a: [number, number][], b: [number, number][]): boolean {
  const EPS = 0.01
  for (const [la, loa] of a) {
    for (const [lb, lob] of b) {
      if (Math.abs(la - lb) < EPS && Math.abs(loa - lob) < EPS) return true
    }
  }
  return false
}

// Décale légèrement un chemin perpendiculairement (offset en degrés lat/lon)
function offsetPath(path: [number, number][], dlat: number, dlon: number): [number, number][] {
  return path.map(([lat, lon]) => [lat + dlat, lon + dlon])
}

function LayerToggle({ label, color, active, onToggle }: { label: string; color: string; active: boolean; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      style={{
        display:      'flex',
        alignItems:   'center',
        gap:          8,
        padding:      '5px 12px',
        borderRadius: 8,
        border:       `1px solid ${active ? color + '60' : 'rgba(255,255,255,0.1)'}`,
        background:   active ? color + '18' : 'rgba(255,255,255,0.04)',
        color:        active ? color : '#64748b',
        fontSize:     12,
        fontWeight:   600,
        cursor:       'pointer',
        transition:   'all 0.15s',
      }}>
      <span style={{
        display: 'inline-block', width: 20, height: 3, borderRadius: 2,
        background: active ? color : '#334155',
        transition: 'background 0.15s',
      }} />
      {label}
      <span style={{ fontSize: 10, opacity: 0.7 }}>{active ? '●' : '○'}</span>
    </button>
  )
}

interface Props { runId: number }

export function MapView({ runId }: Props) {
  const [data,       setData]       = useState<MapRunJson | null>(null)
  const [loading,    setLoading]    = useState(true)
  const [error,      setError]      = useState('')
  const [showAller,  setShowAller]  = useState(true)
  const [showRetour, setShowRetour] = useState(true)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setError('')
    setData(null)
    fetchRunMap(runId)
      .then(d  => { if (!cancelled) setData(d) })
      .catch(e => { if (!cancelled) setError(String(e)) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [runId])

  if (loading) {
    return (
      <div className="flex items-center gap-3 py-8 text-slate-400 text-sm">
        <span className="inline-block w-4 h-4 border-2 border-slate-600 border-t-blue-400 rounded-full animate-spin" />
        Géolocalisation des hops…
      </div>
    )
  }
  if (error) {
    return (
      <div className="py-4 px-4 rounded-xl bg-red-500/10 border border-red-500/20 text-red-400 text-sm">⚠ {error}</div>
    )
  }
  if (!data) return null

  const allerGeo  = data.aller.filter(h  => h.lat != null && h.lon != null)
  const retourGeo = data.retour.filter(h => h.lat != null && h.lon != null)
  const allerPath  = allerGeo.map(h  => [h.lat!, h.lon!] as [number, number])
  const retourPath = retourGeo.map(h => [h.lat!, h.lon!] as [number, number])

  // Décalage si les deux chemins se superposent
  const overlap    = showAller && showRetour && pathsOverlap(allerPath, retourPath)
  const retourDraw = overlap ? offsetPath(retourPath, 0.004, 0.004) : retourPath

  const allGeo = [...allerGeo, ...retourGeo]
  const center: [number, number] = allGeo.length > 0
    ? [allGeo[0].lat!, allGeo[0].lon!]
    : [48.85, 2.35]

  const totalHops  = data.aller.length + data.retour.length
  const mappedHops = allerGeo.length + retourGeo.length

  return (
    <div className="space-y-3">

      {/* Contrôles */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap',
        padding: '10px 14px', borderRadius: 10,
        background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.07)',
      }}>
        <span style={{ fontSize: 12, color: '#64748b', fontWeight: 500 }}>Afficher :</span>

        <LayerToggle label={`Aller (${allerGeo.length} hops)`}  color="#3b82f6" active={showAller}  onToggle={() => setShowAller(v => !v)} />
        {retourGeo.length > 0 && (
          <LayerToggle label={`Retour (${retourGeo.length} hops)`} color="#f97316" active={showRetour} onToggle={() => setShowRetour(v => !v)} />
        )}

        {overlap && (
          <span style={{ marginLeft: 'auto', fontSize: 11, color: '#f59e0b', display: 'flex', alignItems: 'center', gap: 5 }}>
            ⚠ Chemins superposés — décalage appliqué
          </span>
        )}
      </div>

      {/* Carte */}
      <div style={{ height: 420, borderRadius: 12, overflow: 'hidden', border: '1px solid rgba(255,255,255,0.07)' }}>
        <MapContainer center={center} zoom={4} style={{ height: '100%', width: '100%' }} scrollWheelZoom>
          <TileLayer url={TILE_URL} attribution={TILE_ATTR} />

          {showAller  && allerPath.length  > 1 && <Polyline positions={allerPath}  color="#3b82f6" weight={3}   opacity={0.85} />}
          {showRetour && retourDraw.length  > 1 && <Polyline positions={retourDraw} color="#f97316" weight={2.5} opacity={0.8} dashArray="7,4" />}

          {showAller && allerGeo.map(hop => (
            <CircleMarker key={`a-${hop.ttl}`} center={[hop.lat!, hop.lon!]} radius={7}
              pathOptions={{ color: '#1d4ed8', fillColor: hopColor(hop.loss_pct, hop.ratelimit), fillOpacity: 0.9, weight: 1.5 }}>
              <Popup><HopPopup hop={hop} dir="aller" /></Popup>
            </CircleMarker>
          ))}

          {showRetour && retourGeo.map((hop) => {
            const pos: [number, number] = overlap
              ? [hop.lat! + 0.004, hop.lon! + 0.004]
              : [hop.lat!, hop.lon!]
            return (
              <CircleMarker key={`r-${hop.ttl}`} center={pos} radius={5}
                pathOptions={{ color: '#c2410c', fillColor: '#f97316', fillOpacity: 0.8, weight: 1.5 }}>
                <Popup><HopPopup hop={hop} dir="retour" /></Popup>
              </CircleMarker>
            )
          })}
        </MapContainer>
      </div>

      {/* Légende */}
      <div className="flex flex-wrap items-center gap-4 text-xs text-slate-400 px-1">
        <div className="flex items-center gap-1.5">
          <span style={{ display: 'inline-block', width: 20, height: 3, background: '#3b82f6', borderRadius: 2 }} />
          Aller
        </div>
        <div className="flex items-center gap-1.5">
          <span style={{ display: 'inline-block', width: 20, height: 3, background: '#f97316', borderRadius: 2 }} />
          Retour
        </div>
        <div className="flex items-center gap-1.5">
          <span style={{ display: 'inline-block', width: 10, height: 10, background: '#10b981', borderRadius: '50%' }} />Sain
        </div>
        <div className="flex items-center gap-1.5">
          <span style={{ display: 'inline-block', width: 10, height: 10, background: '#f59e0b', borderRadius: '50%' }} />Perte partielle
        </div>
        <div className="flex items-center gap-1.5">
          <span style={{ display: 'inline-block', width: 10, height: 10, background: '#ef4444', borderRadius: '50%' }} />Perte &gt;5%
        </div>
        <div className="ml-auto text-slate-500">
          {mappedHops}/{totalHops} hops géolocalisés
        </div>
      </div>
    </div>
  )
}
