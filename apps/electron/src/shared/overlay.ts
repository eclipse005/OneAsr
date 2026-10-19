/**
 * 浮窗互斥：同一时刻只开一个。
 *
 * 以前这里是三个独立的布尔量（settingsOpen / statsOpen / guideOpen），
 * 于是「打开新的」不会「关掉旧的」——点统计再点指南，两个浮窗会同时开着，
 * 叠在一起互相穿透（2026-10-19 实测：统计页的 KPI、折线图、热力图全透在
 * 指南正文上，两边文字都读不了）。根因不是漏写了一次 `setStatsOpen(false)`，
 * 而是**状态本身能表达「两个同时开」**。
 *
 * 现在只留一个 kind：开新的必然顶掉旧的，结构上就不可能再叠起来。
 */

/** 当前开着的浮窗。`none` = 都没开。 */
export type OverlayKind = 'none' | 'settings' | 'stats' | 'guide'

/**
 * 打开一个浮窗：当前开着的那个（如果有）一律关掉。
 *
 * 「同时只开一个」写在这一处，而不是散在三个按钮的 onClick 里各写一遍
 * `setXOpen(false)` —— 散着写就是上次漏掉的那三个地方。
 *
 * 断骨锚点：把 `return want` 换成 `return current`（= 不顶掉前一个），
 * `overlay.test.ts` 的「打开指南会把统计关掉」立刻变红。
 */
export function openOverlay(
  current: OverlayKind,
  want: Exclude<OverlayKind, 'none'>
): OverlayKind {
  if (current === want) return current
  return want
}

/**
 * 关掉浮窗。`which` 省略 = 关掉当前开着的那个；`which` 与当前不一致时
 * 什么也不做（别把别的浮窗顺手关了）。
 */
export function closeOverlay(current: OverlayKind, which?: OverlayKind): OverlayKind {
  if (which === undefined || which === current) return 'none'
  return current
}
