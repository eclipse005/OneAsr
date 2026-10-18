import { AnimatePresence, motion } from 'framer-motion'
import { useState } from 'react'
import { LANG_OPTIONS, formatDuration, queueRank, t, UiLang } from '../i18n'
import type { HotwordGroup, TaskSnapshot } from '../../../shared/types'

/** 文件大小：GB / MB / KB 自适应。 */
function formatSize(bytes?: number): string | undefined {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return undefined
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${Math.max(1, Math.round(bytes / 1024))} KB`
}

function mediaKind(name: string): 'video' | 'audio' {
  return /\.(mp4|mkv|mov|webm|avi|flv|ts|m4v)$/i.test(name) ? 'video' : 'audio'
}

export function TaskList({
  lang,
  tasks,
  hotwordGroups,
  defaultLanguage,
  onStart,
  onCancel,
  onRemove,
  onOpen,
  onSetLanguage,
  onSetVocalSeparation,
  onSetHotwordGroup,
  onCopyError
}: {
  lang: UiLang
  tasks: TaskSnapshot[]
  hotwordGroups: HotwordGroup[]
  /** 设置里的识别语言：行内没单独选过时用它 */
  defaultLanguage: string
  onStart: (id: string) => void
  onCancel: (id: string) => void
  onRemove: (id: string) => void
  onOpen: (path: string) => void
  onSetLanguage: (id: string, language: string) => void
  onSetVocalSeparation: (id: string, on: boolean) => void
  onSetHotwordGroup: (id: string, groupId: string | null) => void
  onCopyError: (text: string) => void
}) {
  const s = t(lang)
  const [menu, setMenu] = useState<string | null>(null)

  const label = (st: string): string =>
    st === 'running'
      ? s.running
      : st === 'done'
        ? s.done
        : st === 'error'
          ? s.error
          : st === 'cancelled'
            ? s.cancelled
            : s.queued

  return (
    <div className="task-list">
      <AnimatePresence initial={false}>
        {tasks.map((task) => {
          // 运行中锁定行内操作（与 GPUI 的 locks_row_actions 一致）
          const locked = task.status === 'running'
          const doneWithOutput = task.status === 'done' && !!task.outputPath
          const language = task.language || defaultLanguage
          const langLabel = LANG_OPTIONS.find((o) => o.id === language)?.label ?? language
          const boundGroup = hotwordGroups.find((g) => g.id === task.hotwordGroupId)
          const size = formatSize(task.sizeBytes)
          const duration = task.durationSec != null ? formatDuration(task.durationSec) : undefined
          const meta = [size, duration].filter(Boolean).join('  ·  ')
          const timingMs =
            task.totalMs ?? (task.stageTimings?.reduce((a, b) => a + b.ms, 0) || undefined)

          return (
            <motion.div
              className={`task-card status-${task.status}`}
              key={task.id}
              layout
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -8, scale: 0.98 }}
              transition={{ duration: 0.2 }}
            >
              <div>
                <div className="name">
                  <span className={`media-icon ${mediaKind(task.name)}`} aria-hidden />
                  {task.name}
                  {/* 跑的时候药丸直接写阶段名（跟 GPUI 一样，不画进度条——百分比是拿阶段序号
                      估的，本来也不代表真实进度） */}
                  {task.status === 'running' ? (
                    <span className="badge running">{task.stage || s.running}</span>
                  ) : (
                    <span className={`badge ${task.status}`}>
                      {task.status === 'queued' && task.queueRank == null
                        ? s.pending
                        : label(task.status)}
                    </span>
                  )}
                  {task.status === 'queued' && task.queueRank != null && (
                    <span className="badge rank">{queueRank(lang, task.queueRank)}</span>
                  )}
                </div>
                {meta && <div className="row-meta">{meta}</div>}
                <div className="meta">{task.input}</div>

                <div className="row-chips">
                  <div className="chip-wrap">
                    <button
                      type="button"
                      className={`row-chip ${menu === `${task.id}:lang` ? 'open' : ''}`}
                      disabled={locked}
                      title={s.recognitionLanguage}
                      onClick={() => setMenu(menu === `${task.id}:lang` ? null : `${task.id}:lang`)}
                    >
                      {langLabel}
                      <span className="caret">▾</span>
                    </button>
                    {menu === `${task.id}:lang` && (
                      <div className="chip-menu">
                        {LANG_OPTIONS.map((o) => (
                          <button
                            type="button"
                            key={o.id}
                            className={`chip-menu-item ${o.id === language ? 'active' : ''}`}
                            onClick={() => {
                              onSetLanguage(task.id, o.id)
                              setMenu(null)
                            }}
                          >
                            {o.label}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>

                  <div className="chip-wrap">
                    <button
                      type="button"
                      className={`row-chip ${menu === `${task.id}:hw` ? 'open' : ''} ${
                        boundGroup ? '' : 'muted'
                      }`}
                      disabled={locked}
                      title={s.hotwords}
                      onClick={() => setMenu(menu === `${task.id}:hw` ? null : `${task.id}:hw`)}
                    >
                      <span className="ellipsis">{boundGroup ? boundGroup.name : s.hotwordNone}</span>
                      <span className="caret">▾</span>
                    </button>
                    {menu === `${task.id}:hw` && (
                      <div className="chip-menu">
                        <button
                          type="button"
                          className={`chip-menu-item ${task.hotwordGroupId ? '' : 'active'}`}
                          onClick={() => {
                            onSetHotwordGroup(task.id, null)
                            setMenu(null)
                          }}
                        >
                          {s.hotwordNone}
                        </button>
                        {hotwordGroups.map((g) => (
                          <button
                            type="button"
                            key={g.id}
                            className={`chip-menu-item ${g.id === task.hotwordGroupId ? 'active' : ''}`}
                            onClick={() => {
                              onSetHotwordGroup(task.id, g.id)
                              setMenu(null)
                            }}
                          >
                            {g.name}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>

                  <button
                    type="button"
                    // 类名别叫 toggle：全局 .toggle 是开关组件的样式（带 ::after 旋钮），会撞车
                    className={`row-chip sep-chip ${task.vocalSeparation ? 'is-on' : ''}`}
                    disabled={locked}
                    title={s.vocalSep}
                    onClick={() => onSetVocalSeparation(task.id, !task.vocalSeparation)}
                  >
                    {s.vocalSep}
                  </button>

                  {task.stageTimings && task.stageTimings.length > 0 && (
                    <span className="row-chip timing-chip">
                      {s.timing}
                      {timingMs != null ? ` ${formatDuration(timingMs / 1000)}` : ''}
                      <span className="timing-pop">
                        {task.stageTimings.map((t2) => (
                          <span className="timing-line" key={t2.stage}>
                            <i>{t2.stage}</i>
                            <b>{formatDuration(t2.ms / 1000)}</b>
                          </span>
                        ))}
                      </span>
                    </span>
                  )}
                </div>


                {task.error && (
                  <div className="row-error">
                    <span className="err-text">{task.error}</span>
                    <button
                      type="button"
                      className="btn-mini"
                      onClick={() => onCopyError(task.error!)}
                    >
                      {s.copyError}
                    </button>
                  </div>
                )}
              </div>

              <div className="actions">
                {task.status === 'running' ? (
                  <button className="btn btn-ghost btn-danger-ghost" onClick={() => onCancel(task.id)}>
                    {s.cancel}
                  </button>
                ) : (
                  <button className="btn btn-primary" onClick={() => onStart(task.id)}>
                    {task.status === 'error' ? s.retry : s.start}
                  </button>
                )}
                {doneWithOutput && (
                  <button className="btn btn-ghost" onClick={() => onOpen(task.outputPath!)}>
                    {s.openOutput}
                  </button>
                )}
                <button
                  className="btn btn-ghost btn-danger-ghost"
                  disabled={locked}
                  onClick={() => onRemove(task.id)}
                >
                  {s.remove}
                </button>
              </div>
            </motion.div>
          )
        })}
      </AnimatePresence>
    </div>
  )
}
