import { t, UiLang } from '../i18n'
import type { ModelStatus } from '../../../shared/types'

export function StatusBar({ lang, models }: { lang: UiLang; models: ModelStatus | null }) {
  const s = t(lang)
  const ready = !!models?.ready
  return (
    <div className="statusbar">
      <span className={`dot ${ready ? 'ok' : ''}`} />
      <span>{ready ? s.modelsReady : s.modelsNotReady}</span>
      <span title={models?.asrPath}>
        {s.asr}: {models?.asrReady ? '✓' : '✗'}
      </span>
      <span title={models?.alignerPath}>
        {s.aligner}: {models?.alignerReady ? '✓' : '✗'}
      </span>
      <span title={models?.demucsPath}>
        {s.demucs}: {models?.demucsReady ? '✓' : '—'}
      </span>
    </div>
  )
}
