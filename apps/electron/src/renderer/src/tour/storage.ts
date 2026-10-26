/**
 * 交互引导的完成状态只存在 localStorage，不写 Rust settings.json。
 * CLI / 配置文件不关心壳有没有走过引导。
 */

export const TOUR_STORAGE_KEY = 'oneasr.ui.tour.v1'

export type TourPrefs = {
  /** 走完了全部步骤 */
  completed: boolean
  /** 欢迎卡或中途点了跳过 */
  skipped: boolean
}

const DEFAULT: TourPrefs = { completed: false, skipped: false }

export function readTourPrefs(): TourPrefs {
  try {
    const raw = localStorage.getItem(TOUR_STORAGE_KEY)
    if (!raw) return { ...DEFAULT }
    const parsed = JSON.parse(raw) as Partial<TourPrefs>
    return {
      completed: !!parsed.completed,
      skipped: !!parsed.skipped
    }
  } catch {
    return { ...DEFAULT }
  }
}

export function writeTourPrefs(next: TourPrefs): void {
  try {
    localStorage.setItem(TOUR_STORAGE_KEY, JSON.stringify(next))
  } catch {
    // 隐私模式等写不了就放弃；下次启动再问一次欢迎卡
  }
}

export function markTourCompleted(): void {
  writeTourPrefs({ completed: true, skipped: false })
}

export function markTourSkipped(): void {
  writeTourPrefs({ completed: false, skipped: true })
}

/** 是否还该弹出首启欢迎卡 */
export function shouldShowWelcome(prefs: TourPrefs = readTourPrefs()): boolean {
  return !prefs.completed && !prefs.skipped
}
