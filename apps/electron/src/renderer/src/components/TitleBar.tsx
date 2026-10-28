import { t, UiLang } from '../i18n'

/**
 * 标题栏（窗口内容延伸到标题栏区域，按钮由系统画，这里只放品牌）。
 *
 * 源头设计（见 shared/window-chrome + tokens --titlebar-bg）：
 * - 主进程 Win/Linux：titleBarOverlay.color 透明，底色只由本组件 CSS 画，避免原生白块发亮
 * - 让位用 CSS env(titlebar-area-x/width/height)，不再猜 138px；macOS 无 env 时
 *   .titlebar.platform-darwin .titlebar-safe 用 padding-left 给红绿灯兜底
 * - 不自绘 min/max/close。若将来某桌面环境不显示 overlay，可在 git 历史里找回自绘，
 *   并把 windowChrome() 退回 { frame: false }。
 */
export function TitleBar({ lang }: { lang: UiLang }) {
  const s = t(lang)
  return (
    <div className={`titlebar platform-${window.oneasr.platform}`}>
      <div className="titlebar-safe">
        <div className="brand">
          <span className="logo">
            <span className="logo-mark" />
          </span>
          <span>{s.appName}</span>
        </div>
        <div className="spacer" />
      </div>
    </div>
  )
}
