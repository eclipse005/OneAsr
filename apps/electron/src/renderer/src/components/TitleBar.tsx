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
      <button className="win-btn" type="button" onClick={() => void window.oneasr.windowMinimize()}>
        ─
      </button>
      <button className="win-btn" type="button" onClick={() => void window.oneasr.windowMaximize()}>
        ▢
      </button>
      <button className="win-btn close" type="button" onClick={() => void window.oneasr.windowClose()}>
        ×
      </button>
    </div>
  )
}
