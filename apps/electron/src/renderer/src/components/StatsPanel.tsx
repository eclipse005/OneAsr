import { motion } from 'framer-motion'
import { t, UiLang } from '../i18n'
import type { StatsSnapshot } from '../../../shared/types'

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
  const empty = stats.totalRuns === 0
  return (
    <div className="overlay center" onClick={onClose}>
      <motion.div
        className="panel"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: 8 }}
      >
        <div className="modal-header">
          <h3>{s.statsTitle}</h3>
          <div style={{ flex: 1 }} />
          <button className="btn btn-ghost" onClick={onClose}>
            {s.close}
          </button>
        </div>
        <div className="body">
          {empty ? (
            <p>{s.statsEmpty}</p>
          ) : (
            <>
              <div className="stats-grid">
                <div className="stat">
                  <div className="n">{stats.totalRuns}</div>
                  <div className="l">{s.totalRuns}</div>
                </div>
                <div className="stat">
                  <div className="n">{stats.successCount}</div>
                  <div className="l">{s.success}</div>
                </div>
                <div className="stat">
                  <div className="n">{stats.errorCount}</div>
                  <div className="l">{s.failures}</div>
                </div>
              </div>
              {stats.lastDurationMs != null && (
                <p>
                  {s.lastDuration}: {(stats.lastDurationMs / 1000).toFixed(1)}s
                </p>
              )}
              {stats.lastOutput && (
                <p style={{ wordBreak: 'break-all' }}>{stats.lastOutput}</p>
              )}
              <h4 style={{ color: 'var(--text)', marginTop: 16 }}>{s.recent}</h4>
              <ul style={{ paddingLeft: 18, margin: 0 }}>
                {stats.recent.map((r, i) => (
                  <li key={i}>
                    {r.name} · {r.status}
                    {r.durationMs != null ? ` · ${(r.durationMs / 1000).toFixed(1)}s` : ''}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      </motion.div>
    </div>
  )
}
