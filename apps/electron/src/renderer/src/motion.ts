/**
 * 动效只有一套曲线、三档时长。组件里不要再写别的 ease。
 *
 * - micro 120ms：颜色、开关、按钮
 * - layer 160ms：菜单、确认条、状态胶囊（透明度 + 4px）
 * - surface 220ms：浮窗、任务行
 *
 * 退出比进入短，并沿进来的方向收回。系统开了「减少动态效果」时只留透明度。
 */
import { useReducedMotion, type Transition } from 'framer-motion'

export const EASE_OUT = [0.16, 1, 0.3, 1] as const

const SURFACE_MS = 0.22
const SURFACE_EXIT_MS = 0.14
const LAYER_MS = 0.16
const LAYER_EXIT_MS = 0.1
const PANE_MS = 0.12

type Tween = Transition & { ease?: readonly [number, number, number, number] }

function useReduce(): boolean {
  return useReducedMotion() === true
}

function fade(duration: number): {
  initial: { opacity: number }
  animate: { opacity: number }
  exit: { opacity: number; transition: Tween }
  transition: Tween
} {
  return {
    initial: { opacity: 0 },
    animate: { opacity: 1 },
    exit: { opacity: 0, transition: { duration: Math.min(duration, 0.08) } },
    transition: { duration }
  }
}

/** 浮窗：从下方 12px 淡入，退出收回下方并略缩短。 */
export function useSurfaceMotion() {
  if (useReduce()) return fade(0.08)
  return {
    initial: { opacity: 0, y: 12, scale: 0.98 },
    animate: { opacity: 1, y: 0, scale: 1 },
    exit: {
      opacity: 0,
      y: 10,
      scale: 0.99,
      transition: { duration: SURFACE_EXIT_MS, ease: EASE_OUT }
    },
    transition: { duration: SURFACE_MS, ease: EASE_OUT } satisfies Tween
  }
}

/** 任务行：新行从下方进来，删掉时沿原方向沉回去，其余行补位。 */
export function useRowMotion() {
  if (useReduce()) return fade(0.08)
  return {
    layout: true as const,
    initial: { opacity: 0, y: 8 },
    animate: { opacity: 1, y: 0 },
    exit: {
      opacity: 0,
      y: 6,
      transition: { duration: SURFACE_EXIT_MS, ease: EASE_OUT }
    },
    transition: { duration: SURFACE_MS, ease: EASE_OUT } satisfies Tween
  }
}

/** 菜单、确认条、状态胶囊。 */
export function useLayerMotion() {
  if (useReduce()) return fade(0.08)
  return {
    initial: { opacity: 0, y: 4 },
    animate: { opacity: 1, y: 0 },
    exit: {
      opacity: 0,
      y: 4,
      transition: { duration: LAYER_EXIT_MS, ease: EASE_OUT }
    },
    transition: { duration: LAYER_MS, ease: EASE_OUT } satisfies Tween
  }
}

/** 设置换页：只交叉淡化，不左右滑。 */
export function usePaneMotion() {
  if (useReduce()) return fade(0.01)
  return {
    initial: { opacity: 0 },
    animate: { opacity: 1 },
    exit: { opacity: 0, transition: { duration: 0.08, ease: EASE_OUT } },
    transition: { duration: PANE_MS, ease: EASE_OUT } satisfies Tween
  }
}
