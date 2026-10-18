/**
 * 动效 tokens：全应用一套时长与曲线，别在每个组件里各写一份 spring。
 *
 * 取值对齐 GPUI 版的体感（设置抽屉 0.34s ease-out、行淡入 0.30s、墓碑 0.22s），
 * 但浮窗因为是"从中间长出来"，改用更短的 0.22s + 前快后慢的曲线。
 */
const EASE_OUT = [0.16, 1, 0.3, 1] as const

/** 浮窗（设置/统计/指南/欢迎）：淡入 + 轻微上浮，退出更快 */
export const panelMotion = {
  initial: { opacity: 0, y: 14, scale: 0.985 },
  animate: { opacity: 1, y: 0, scale: 1 },
  exit: { opacity: 0, y: 10, scale: 0.99 },
  transition: { duration: 0.22, ease: EASE_OUT }
}

/** 任务行：新增就地淡入、删除原位淡出（列表不跳） */
export const rowMotion = {
  layout: true,
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -6, scale: 0.99 },
  transition: { duration: 0.2, ease: EASE_OUT }
}

