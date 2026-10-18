import { t, UiLang } from '../i18n'

export function TitleBar({ lang }: { lang: UiLang }) {
  const s = t(lang)
  return (
    <div className="titlebar">
      <div className="brand">
        <div className="logo" />
        <span>{s.appName}</span>
      </div>
      <div className="spacer" />
      <button className="win-btn" onClick={() => window.oneasr.windowMinimize()} title="—">
        ─
      </button>
      <button className="win-btn" onClick={() => window.oneasr.windowMaximize()} title="□">
        ▢
      </button>
      <button className="win-btn close" onClick={() => window.oneasr.windowClose()} title="×">
        ×
      </button>
    </div>
  )
}
