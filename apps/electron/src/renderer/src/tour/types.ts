export type TourPlacement = 'top' | 'bottom' | 'left' | 'right' | 'auto'

/** 怎么推进下一步 */
export type TourAdvance =
  /** 用户必须点高亮控件（真·走点击路径）；气泡不显示「下一步」 */
  | 'click'
  /** 气泡上的「下一步」；不强迫点目标（避免误启转写 / 弹文件框） */
  | 'next'
  /** 点目标或点下一步都行；文案需说明可「下一步」跳过 */
  | 'click-or-next'

export type TourStepId =
  | 'status-models'
  | 'settings-models'
  | 'settings-hotwords'
  | 'settings-output'
  | 'add-or-drop'
  | 'task-row'
  | 'done'

export type TourContext = {
  modelsReady: boolean
  hasTasks: boolean
  settingsOpen: boolean
}

export type TourStep = {
  id: TourStepId
  /**
   * CSS selector；通常是 [data-tour="…"]。
   * null / 空 = 无高亮洞（收束步）。
   * 也可按 ctx 动态选择（空列表就绪高亮拖放区等）。
   */
  selector: string | null | ((ctx: TourContext) => string | null)
  /** i18n key under tour*；可按 ctx 区分 missing / outdated */
  titleKey: string | ((ctx: TourContext) => string)
  bodyKey: string | ((ctx: TourContext) => string)
  /** @deprecated 讲解卡已固定右下角，此字段忽略 */
  placement?: TourPlacement
  advance: TourAdvance
  /** 步骤是否出现；默认 true */
  when?: (ctx: TourContext) => boolean
  /**
   * 进入步骤前调用（例如确保设置已开到某页）。
   * 返回 false 表示这次跳过（例如等不到 DOM）。
   */
  before?: (ctx: TourContext, api: TourStepApi) => void | boolean | Promise<void | boolean>
  /** 高亮前等这个选择器出现（默认用 selector） */
  waitFor?: string | ((ctx: TourContext) => string | undefined)
  waitTimeoutMs?: number
  /** 气泡上额外显示「跳过下载等待」之类 */
  allowSkipWait?: boolean | ((ctx: TourContext) => boolean)
}

export type TourStepApi = {
  openSettings: (tab?: 'general' | 'output' | 'models' | 'hotwords' | 'compute') => Promise<void>
  closeSettings: () => void
  closeGuide: () => void
  openStats: () => Promise<void>
  closeStats: () => void
}

export function resolveTourSelector(
  selector: TourStep['selector'],
  ctx: TourContext
): string | null {
  return typeof selector === 'function' ? selector(ctx) : selector
}

export function resolveTourWaitFor(step: TourStep, ctx: TourContext): string | null {
  if (step.waitFor) {
    return typeof step.waitFor === 'function' ? step.waitFor(ctx) ?? null : step.waitFor
  }
  return resolveTourSelector(step.selector, ctx)
}

export function resolveTourAllowSkipWait(step: TourStep, ctx: TourContext): boolean {
  return typeof step.allowSkipWait === 'function' ? !!step.allowSkipWait(ctx) : !!step.allowSkipWait
}
