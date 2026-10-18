import { t, UiLang } from '../i18n'
import type { ModelStatus, TaskSnapshot } from '../../../shared/types'

export function StatusBar({
  lang,
  models,
  tasks
}: {
  lang: UiLang
  models: ModelStatus | null
  tasks: TaskSnapshot[]
}) {
  const s = t(lang)
  const ready = !!models?.ready
  const running = tasks.filter((x) => x.status === 'running').length
  // 「排队」只算真正进了运行队列的（有 queueRank）；刚添加还没点开始的是「待处理」，行内有徽章
  const queued = tasks.filter((x) => x.status === 'queued' && x.queueRank != null).length
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
            {s.running} {running} · {s.queued} {queued}
          </span>
        </>
      )}
    </div>
  )
}
