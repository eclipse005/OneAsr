import { t, UiLang } from '../i18n'
import type { ModelStatus, TaskSnapshot } from '../../../shared/types'

export function StatusBar({
  lang,
  models,
  tasks,
  message,
  onCopyMessage
}: {
  lang: UiLang
  models: ModelStatus | null
  tasks: TaskSnapshot[]
  /** 最近一条提示：toast 会消失，这里留 3~6 秒并且可点击复制（长路径/报错不至于看了就丢） */
  message?: { kind: string; text: string } | null
  onCopyMessage?: (text: string) => void
}) {
  const s = t(lang)
  const ready = !!models?.ready
  const running = tasks.filter((x) => x.status === 'running').length
  // 「排队」只算真正进了运行队列的（有 queueRank）；刚添加还没点开始的是「待处理」，行内有徽章
  const queued = tasks.filter((x) => x.status === 'queued' && x.queueRank != null).length
  const done = tasks.filter((x) => ['done', 'error', 'cancelled'].includes(x.status)).length
  return (
    <div className="statusbar">
      <span className={`dot ${ready ? 'ok' : ''}`} />
      <strong style={{ color: ready ? 'var(--logo)' : 'var(--warn)', fontWeight: 700 }}>
        {ready ? s.modelsReady : s.modelsNotReady}
      </strong>
      <span className="sep" />
      <span title={models?.asrPath}>
        {s.asr}: {models?.asrReady ? '✓' : '✗'} {models?.asrLabel || ''}
      </span>
      <span className="sep" />
      <span title={models?.alignerPath}>
        {s.aligner}: {models?.alignerReady ? '✓' : '✗'} {models?.alignerLabel || ''}
      </span>
      <span className="sep" />
      <span title={models?.demucsPath}>
        {s.demucs}: {models?.demucsReady ? '✓' : '—'}
      </span>
      <span className="sep" />
      <span title={models?.sensevoicePath}>
        {s.sensevoice}: {models?.sensevoiceReady ? '✓' : '—'}
      </span>
      <span className="sep" />
      <span>
        {s.backend}: {models?.backend || '—'}
      </span>
      {(running > 0 || queued > 0) && (
        <>
          <span className="sep" />
          <span>
            {s.progress} {Math.min(done + 1, tasks.length)}/{tasks.length} · {s.running} {running} ·{' '}
            {s.queued} {queued}
          </span>
        </>
      )}
      {message && (
        <>
          <span className="sep" />
          <button
            type="button"
            className={`status-chip ${message.kind}`}
            title={message.text}
            onClick={() => onCopyMessage?.(message.text)}
          >
            {message.text}
          </button>
        </>
      )}
    </div>
  )
}
