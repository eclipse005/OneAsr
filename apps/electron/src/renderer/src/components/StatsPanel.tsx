import { motion } from 'framer-motion'
import { formatDuration, heatTooltip, moreItems, t, taskStatusLabel, UiLang } from '../i18n'
import { useEscClose } from '../esc'
import { useSurfaceMotion } from '../motion'
import type { StatsSnapshot } from '../../../shared/types'

/**
 * 列表封顶：多出来的折成「还有 N 条」，放在卡片标题行里（不额外占高度）。
 * 面板在统一的 900×640 浮窗里铺满：30 天折线是唯一弹性区，多出来的高度给它；
 * 窗口压到最小时它先收到底，所以统计面板不出现滚动条。
 */
const LANGS_MAX = 4
const RECENT_MAX = 6

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

  const langs = stats.langs.slice(0, LANGS_MAX)
  const recent = stats.recentSession.slice(0, RECENT_MAX)
  const hasRecent = recent.length > 0 || !!stats.lastOutput

  const surface = useSurfaceMotion()
  useEscClose(onClose)

  return (
    <div className="overlay" onClick={onClose}>
      <motion.div
        className="modal-shell"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        {...surface}
      >
        <div className="modal-header" data-tour="stats-header">
          <h3>{s.statsTitle}</h3>
          <div style={{ flex: 1 }} />
          <button type="button" data-shot="close-modal" className="btn btn-ghost" onClick={onClose}>
            {s.close}
          </button>
        </div>
        <div className="modal-body stats-body" style={{ whiteSpace: 'normal' }}>
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

              <div className="chart-card spark-card">
                <h5>{s.mediaProcessed} · 30d</h5>
                <svg
                  className="spark"
                  width="100%"
                  viewBox={`0 0 ${w} ${h}`}
                  preserveAspectRatio="none"
                  role="img"
                  aria-label={s.sparklineAria}
                >
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
                <div className="heat" title={s.heatTitle}>
                  {stats.perDay.map((d) => (
                    <i
                      key={d.day}
                      className={heatLevel(d.mediaSec, d.errors)}
                      title={heatTooltip(lang, d.day, d.mediaSec, d.tasks, d.errors)}
                    />
                  ))}
                </div>
              </div>

              {(stats.langs.length > 0 || hasRecent) && (
                <div className="stats-row">
                  {stats.langs.length > 0 && (
                    <div className="chart-card">
                      <h5>
                        {s.langTitle}
                        {stats.langs.length > LANGS_MAX && (
                          <span className="more-note">
                            {moreItems(lang, stats.langs.length - LANGS_MAX)}
                          </span>
                        )}
                      </h5>
                      <div className="lang-list">
                        {langs.map((l) => (
                          <div className="bar-row" key={l.lang}>
                            <span className="lab">{l.lang}</span>
                            <div className="track">
                              <b style={{ width: `${(l.count / maxLang) * 100}%` }} />
                            </div>
                            <span className="num">{l.count}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {hasRecent && (
                    <div className="chart-card">
                      <h5>
                        {s.recent}
                        {stats.recentSession.length > RECENT_MAX && (
                          <span className="more-note">
                            {moreItems(lang, stats.recentSession.length - RECENT_MAX)}
                          </span>
                        )}
                      </h5>
                      {recent.length > 0 && (
                        <ul className="recent-list">
                          {recent.map((r, i) => (
                            <li key={i}>
                              <span className="recent-name" title={r.name}>
                                {r.name}
                              </span>
                              <span className={`recent-status ${r.status}`}>{taskStatusLabel(lang, r.status)}</span>
                              {r.durationMs != null && (
                                <span className="recent-ms">{(r.durationMs / 1000).toFixed(1)}s</span>
                              )}
                            </li>
                          ))}
                        </ul>
                      )}
                      {stats.lastOutput && (
                        <p className="last-output" title={stats.lastOutput}>
                          {stats.lastOutput}
                        </p>
                      )}
                    </div>
                  )}
                </div>
              )}
            </>
          )}
        </div>
      </motion.div>
    </div>
  )
}
