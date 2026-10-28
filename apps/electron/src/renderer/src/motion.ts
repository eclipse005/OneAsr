/**
 * 动效只有一套曲线、几档时长。组件里不要再写别的 ease。
 *
 * - micro 120ms：颜色、开关、按钮
 * - layer 160ms：菜单、确认条、状态胶囊（透明度 + 4px）
 * - surface 220ms：浮窗、任务行
 * - overlay 180ms：遮罩淡入淡出（比 surface 略短，避免拖沓）
 *
 * 退出比进入短，并沿进来的方向收回。系统开了「减少动态效果」时只留透明度。
 */
import { useReducedMotion, type Transition, type Variants } from 'framer-motion'

export const EASE_OUT = [0.16, 1, 0.3, 1] as const

/** Durations in seconds — keep in sync with tokens.css --dur-* */
export const DUR = {
  micro: 0.12,
  layer: 0.16,
  layerExit: 0.1,
  surface: 0.22,
  surfaceExit: 0.14,
  overlay: 0.18,
  overlayExit: 0.12,
  pane: 0.12,
  reduced: 0.08
} as const

type Tween = Transition & { ease?: readonly [number, number, number, number] }

function useReduce(): boolean {
  return useReducedMotion() === true
}

function fadeOnly(duration: number, exitDuration = Math.min(duration, 0.08)): {
  initial: { opacity: number }
  animate: { opacity: number }
  exit: { opacity: number; transition: Tween }
  transition: Tween
} {
  return {
    initial: { opacity: 0 },
    animate: { opacity: 1 },
    exit: { opacity: 0, transition: { duration: exitDuration, ease: EASE_OUT } },
    transition: { duration, ease: EASE_OUT }
  }
}

/* ── Static variants (for TourOverlay / one-off spreads; prefer hooks in components) ── */

export const fadeVariants: Variants = {
  initial: { opacity: 0 },
  animate: { opacity: 1 },
  exit: { opacity: 0 }
}

export const fadeUpVariants: Variants = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: 6 }
}

export const scaleInSoftVariants: Variants = {
  initial: { opacity: 0, y: 12, scale: 0.98 },
  animate: { opacity: 1, y: 0, scale: 1 },
  exit: { opacity: 0, y: 10, scale: 0.99 }
}

export const overlayFadeVariants: Variants = {
  initial: { opacity: 0 },
  animate: { opacity: 1 },
  exit: { opacity: 0 }
}

/** Drawer-style: soft vertical settle (same language as surface; not a side slide). */
export const panelSlideVariants: Variants = {
  initial: { opacity: 0, y: 10 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: 8 }
}

/* ── Hooks ── */

/** 遮罩：只淡入淡出。必须挂在 AnimatePresence 的根 motion 上，退出才收得住。 */
export function useOverlayMotion() {
  if (useReduce()) return fadeOnly(DUR.reduced, 0.05)
  return {
    initial: { opacity: 0 },
    animate: { opacity: 1 },
    exit: {
      opacity: 0,
      transition: { duration: DUR.overlayExit, ease: EASE_OUT }
    },
    transition: { duration: DUR.overlay, ease: EASE_OUT } satisfies Tween
  }
}

/** 浮窗：从下方 12px 淡入并略缩放，退出收回下方并略缩短。 */
export function useSurfaceMotion() {
  if (useReduce()) return fadeOnly(DUR.reduced)
  return {
    initial: { opacity: 0, y: 12, scale: 0.98 },
    animate: { opacity: 1, y: 0, scale: 1 },
    exit: {
      opacity: 0,
      y: 10,
      scale: 0.99,
      transition: { duration: DUR.surfaceExit, ease: EASE_OUT }
    },
    transition: { duration: DUR.surface, ease: EASE_OUT } satisfies Tween
  }
}

/** 空态 / 全幅提示：只淡入上移，不要缩放（整页缩放会晃）。 */
export function useEmptyMotion() {
  if (useReduce()) return fadeOnly(DUR.reduced)
  return {
    initial: { opacity: 0, y: 8 },
    animate: { opacity: 1, y: 0 },
    exit: {
      opacity: 0,
      y: 4,
      transition: { duration: DUR.layerExit, ease: EASE_OUT }
    },
    transition: { duration: DUR.surface, ease: EASE_OUT } satisfies Tween
  }
}

/** 任务行：新行从下方进来，删掉时沿原方向沉回去，其余行补位。 */
export function useRowMotion() {
  if (useReduce()) return fadeOnly(DUR.reduced)
  return {
    layout: true as const,
    initial: { opacity: 0, y: 8 },
    animate: { opacity: 1, y: 0 },
    exit: {
      opacity: 0,
      y: 6,
      transition: { duration: DUR.surfaceExit, ease: EASE_OUT }
    },
    transition: { duration: DUR.surface, ease: EASE_OUT } satisfies Tween
  }
}

/** 菜单、确认条、状态胶囊。 */
export function useLayerMotion() {
  if (useReduce()) return fadeOnly(DUR.reduced)
  return {
    initial: { opacity: 0, y: 4 },
    animate: { opacity: 1, y: 0 },
    exit: {
      opacity: 0,
      y: 4,
      transition: { duration: DUR.layerExit, ease: EASE_OUT }
    },
    transition: { duration: DUR.layer, ease: EASE_OUT } satisfies Tween
  }
}

/** 设置换页：只交叉淡化，不左右滑。 */
export function usePaneMotion() {
  if (useReduce()) return fadeOnly(0.01)
  return {
    initial: { opacity: 0 },
    animate: { opacity: 1 },
    exit: { opacity: 0, transition: { duration: 0.08, ease: EASE_OUT } },
    transition: { duration: DUR.pane, ease: EASE_OUT } satisfies Tween
  }
}
