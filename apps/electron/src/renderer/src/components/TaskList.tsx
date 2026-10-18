import { t, UiLang } from '../i18n'
import type { TaskSnapshot } from '../../../shared/types'

export function TaskList({
  lang,
  tasks,
  onStart,
  onRemove,
  onOpen
}: {
  lang: UiLang
  tasks: TaskSnapshot[]
  onStart: (id: string) => void
  onRemove: (id: string) => void
  onOpen: (path: string) => void
}) {
  const s = t(lang)
  const label = (st: string) =>
    st === 'running' ? s.running : st === 'done' ? s.done : st === 'error' ? s.error : s.queued

  return (
    <div className="task-list">
      {tasks.map((task) => (
        <div className="task-card" key={task.id}>
          <div>
            <div className="name">
              {task.name}{' '}
              <span className={`badge ${task.status}`}>{label(task.status)}</span>
            </div>
            <div className="meta">{task.input}</div>
            {task.progress && <div className="meta">{task.progress}</div>}
            {task.error && (
              <div className="meta" style={{ color: 'var(--danger)' }}>
                {task.error}
              </div>
            )}
            {task.outputPath && (
              <div className="meta" style={{ color: 'var(--ok)' }}>
                {task.outputPath}
              </div>
            )}
          </div>
          <div className="actions">
            <button
              className="btn btn-primary"
              disabled={task.status === 'running'}
              onClick={() => onStart(task.id)}
            >
              {s.start}
            </button>
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
        </div>
      ))}
    </div>
  )
}
