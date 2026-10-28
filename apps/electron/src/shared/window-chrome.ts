/**
 * 窗口 chrome 真源：标题栏高度 / 底色 / WCO 符号色，以及 Win/Linux 的 titleBarOverlay 选项。
 *
 * 内容 UI 跨平台统一；窗口按钮一律系统画（不自绘 min/max/close）：
 * - Win/Linux：titleBarStyle hidden + titleBarOverlay
 * - macOS：titleBarStyle hidden + trafficLightPosition（主进程 windowChrome 里）
 *
 * overlay.color 必须透明（#00000000）：若设成不透明 #ffffff，系统会在右上角再铺一块
 * 原生白矩形，和渲染进程 .titlebar { background } 叠两层，色差/抗锯齿一偏就会
 * 「发亮」。透明后只剩 CSS 标题栏底色可见，symbolColor 仍着色系统按钮字形。
 * （同思路见 hermes-agent 等。）
 */

export const TITLEBAR_HEIGHT = 44

/** 与 CSS --titlebar-bg 对齐；主进程 overlay 不再用这个做不透明底板。 */
export const TITLEBAR_BG = '#ffffff'

/** WCO 按钮字形色（slate）；深浅主题都靠这一套浅色 chrome。 */
export const TITLEBAR_SYMBOL = '#475569'

export type TitleBarOverlayOptions = {
  color: string
  symbolColor: string
  height: number
}

/** Win/Linux Window Controls Overlay：透明底板 + 共享高度/符号色。 */
export function titleBarOverlayOptions(): TitleBarOverlayOptions {
  return {
    color: '#00000000',
    symbolColor: TITLEBAR_SYMBOL,
    height: TITLEBAR_HEIGHT
  }
}
