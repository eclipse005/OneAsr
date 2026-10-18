import { t, UiLang } from '../i18n'

/**
 * 标题栏（窗口内容延伸到标题栏区域，按钮由系统画，这里只放品牌）。
 *
 * 让位规则在 CSS 的 .titlebar.platform-* 里：
 *   Windows / Linux → 右上角 Window Controls Overlay，留 3 × 46px
 *   macOS           → 左上角红绿灯，左侧留 78px
 *
 * 注：早期版本在这里自绘过 min/max/close（用仓库的 win-*.svg）。那套只在
 * 拿不到 overlay 时才需要；若将来某个 Linux 桌面环境不显示 overlay 按钮，
 * 可在 git 历史里找回那段自绘代码，并把 windowChrome() 退回 { frame: false }。
 */
export function TitleBar({ lang }: { lang: UiLang }) {
  const s = t(lang)
  return (
    <div className={`titlebar platform-${window.oneasr.platform}`}>
      <div className="brand">
        <span className="logo">
          <span className="logo-mark" />
        </span>
        <span>{s.appName}</span>
      </div>
      <div className="spacer" />
    </div>
  )
}
