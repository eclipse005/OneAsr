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
  const queued = tasks.filter((x) => x.status === 'queued').length
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
