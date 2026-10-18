import { motion } from 'framer-motion'
import { formatDuration, t, UiLang } from '../i18n'
import type { StatsSnapshot } from '../../../shared/types'

function heatLevel(mediaSec: number, errors: number): string {
  if (errors > 0 && mediaSec <= 0) return 'err'
  if (mediaSec <= 0) return ''
  if (mediaSec < 60) return 'l1'
  if (mediaSec < 300) return 'l2'
  if (mediaSec < 1200) return 'l3'
  return 'l4'
}

export function StatsPanel({
  lang,
  stats,
  onClose
}: {
  lang: UiLang
  stats: StatsSnapshot
  onClose: () => void
}) {
  const s = t(lang)
  const empty = stats.tasksTotal === 0
  const maxLang = Math.max(1, ...stats.langs.map((l) => l.count))

  // Simple SVG sparkline of last 30 days media
  const spark = stats.perDay.slice(-30)
  const maxM = Math.max(1, ...spark.map((d) => d.mediaSec))
  const w = 560
  const h = 72
  const points = spark
    .map((d, i) => {
      const x = (i / Math.max(1, spark.length - 1)) * (w - 8) + 4
      const y = h - 6 - (d.mediaSec / maxM) * (h - 16)
      return `${x},${y}`
    })
    .join(' ')

  return (
    <div className="overlay" onClick={onClose}>
      <motion.div
        className="modal-shell wide"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 18, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 10, scale: 0.97 }}
        transition={{ type: 'spring', stiffness: 380, damping: 32 }}
      >
        <div className="modal-header">
          <h3>{s.statsTitle}</h3>
          <div style={{ flex: 1 }} />
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            {s.close}
          </button>
        </div>
        <div className="modal-body" style={{ whiteSpace: 'normal' }}>
          {empty ? (
            <p>{s.statsEmpty}</p>
          ) : (
            <>
              <div className="stats-grid">
                <div className="stat">
                  <div className="n">{stats.tasksTotal}</div>
                  <div className="l">{s.totalRuns}</div>
                </div>
                <div className="stat">
                  <div className="n">{stats.tasksOk}</div>
                  <div className="l">{s.success}</div>
                </div>
                <div className="stat">
                  <div className="n">{stats.tasksErr}</div>
                  <div className="l">{s.failures}</div>
                </div>
                <div className="stat">
                  <div className="n">{stats.cues}</div>
                  <div className="l">{s.cues}</div>
                </div>
                <div className="stat">
                  <div className="n">{formatDuration(stats.mediaSec)}</div>
                  <div className="l">{s.mediaProcessed}</div>
                </div>
                <div className="stat">
                  <div className="n">{formatDuration(stats.savedSec)}</div>
                  <div className="l">{s.savedTime}</div>
                </div>
                <div className="stat">
                  <div className="n">
                    {stats.avgRealtime != null ? `${stats.avgRealtime.toFixed(2)}×` : '—'}
                  </div>
                  <div className="l">{s.avgSpeed}</div>
                </div>
                <div className="stat">
                  <div className="n">
                    {stats.lastDurationMs != null
                      ? formatDuration(stats.lastDurationMs / 1000)
                      : '—'}
                  </div>
                  <div className="l">{s.lastDuration}</div>
                </div>
              </div>

              <div className="chart-card">
                <h5>{s.mediaProcessed} · 30d</h5>
                <svg width="100%" viewBox={`0 0 ${w} ${h}`} role="img" aria-label="media sparkline">
                  <polyline
                    fill="none"
                    stroke="#0d9488"
                    strokeWidth="2.5"
                    strokeLinejoin="round"
                    strokeLinecap="round"
                    points={points}
                  />
                  <polyline
                    fill="rgba(13,148,136,0.12)"
                    stroke="none"
                    points={`4,${h - 4} ${points} ${w - 4},${h - 4}`}
                  />
                </svg>
              </div>

              <div className="chart-card">
                <h5>{s.heatTitle}</h5>
                <div className="heat" title="media seconds / day">
                  {stats.perDay.map((d) => (
                    <i
                      key={d.day}
                      className={heatLevel(d.mediaSec, d.errors)}
                      title={`${d.day}: ${formatDuration(d.mediaSec)} · ${d.tasks} ok · ${d.errors} err`}
                    />
                  ))}
                </div>
              </div>

              {stats.langs.length > 0 && (
                <div className="chart-card">
                  <h5>{s.langTitle}</h5>
                  {stats.langs.map((l) => (
                    <div className="bar-row" key={l.lang}>
                      <span className="lab">{l.lang}</span>
                      <div className="track">
                        <b style={{ width: `${(l.count / maxLang) * 100}%` }} />
                      </div>
                      <span style={{ width: 28, textAlign: 'right', color: 'var(--muted)' }}>
                        {l.count}
                      </span>
                    </div>
                  ))}
                </div>
              )}

              {stats.lastOutput && (
                <p style={{ wordBreak: 'break-all', fontSize: '0.82rem' }}>{stats.lastOutput}</p>
              )}

              {stats.recentSession.length > 0 && (
                <>
                  <h4 style={{ color: 'var(--text)', margin: '8px 0' }}>{s.recent}</h4>
                  <ul style={{ paddingLeft: 18, margin: 0, color: 'var(--muted)' }}>
                    {stats.recentSession.map((r, i) => (
                      <li key={i}>
                        {r.name} · {r.status}
                        {r.durationMs != null ? ` · ${(r.durationMs / 1000).toFixed(1)}s` : ''}
                      </li>
                    ))}
                  </ul>
                </>
              )}
            </>
          )}
        </div>
      </motion.div>
    </div>
  )
}
