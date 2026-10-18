import { AnimatePresence, motion } from 'framer-motion'
import { t, UiLang } from '../i18n'
import type { TaskSnapshot } from '../../../shared/types'

export function TaskList({
  lang,
  tasks,
  onStart,
  onCancel,
  onRemove,
  onOpen
}: {
  lang: UiLang
  tasks: TaskSnapshot[]
  onStart: (id: string) => void
  onCancel: (id: string) => void
  onRemove: (id: string) => void
  onOpen: (path: string) => void
}) {
  const s = t(lang)
  const label = (st: string) =>
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
        {tasks.map((task) => (
          <motion.div
            className="task-card"
            key={task.id}
            layout
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8, scale: 0.98 }}
            transition={{ duration: 0.2 }}
          >
            <div>
              <div className="name">
                {task.name}
                <span className={`badge ${task.status}`}>{label(task.status)}</span>
                {task.stage && task.status === 'running' && (
                  <span className="badge running">{task.stage}</span>
                )}
              </div>
              <div className="meta">{task.input}</div>
              {task.progress && task.status === 'running' && (
                <div className="meta">{task.progress}</div>
              )}
              {task.error && (
                <div className="meta" style={{ color: 'var(--danger)' }}>
                  {task.error}
                </div>
              )}
              {task.outputPath && (
                <div className="meta" style={{ color: 'var(--logo)' }}>
                  {task.outputPath}
                </div>
              )}
              {(task.status === 'running' || (task.progressPct != null && task.progressPct > 0)) && (
                <div className="progress" aria-hidden>
                  <i style={{ width: `${Math.min(100, task.progressPct || (task.status === 'done' ? 100 : 8))}%` }} />
                </div>
              )}
            </div>
            <div className="actions">
              {task.status === 'running' ? (
                <button className="btn btn-ghost btn-danger-ghost" onClick={() => onCancel(task.id)}>
                  {s.cancel}
                </button>
              ) : (
                <button
                  className="btn btn-primary"
                  disabled={task.status === 'running'}
                  onClick={() => onStart(task.id)}
                >
                  {s.start}
                </button>
              )}
              {task.outputPath && (
                <button className="btn btn-ghost" onClick={() => onOpen(task.outputPath!)}>
                  {s.openFolder}
                </button>
              )}
              <button
                className="btn btn-ghost btn-danger-ghost"
                disabled={task.status === 'running'}
                onClick={() => onRemove(task.id)}
              >
                {s.remove}
              </button>
            </div>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  )
}
