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
  | 'toolbar-settings-cli'
  | 'settings-models'
  | 'settings-download'
  | 'settings-hotwords-tab'
  | 'settings-hotwords'
  | 'settings-output-tab'
  | 'settings-output'
  | 'settings-close'
  | 'toolbar-add'
  | 'dropzone'
  | 'empty-open-settings'
  | 'task-lang'
  | 'task-hotwords'
  | 'task-sep'
  | 'task-transcript'
  | 'task-start'
  | 'task-output'
  | 'toolbar-stats'
  | 'toolbar-guide'
  | 'done'

export type TourContext = {
  modelsReady: boolean
  hasTasks: boolean
  settingsOpen: boolean
  /** 状态栏显示「CLI 过旧」时为 true，勿把橙色条当模型就绪 */
  cliOutdated: boolean
}

export type TourStep = {
  id: TourStepId
  /**
   * CSS selector；通常是 [data-tour="…"]。
   * null / 空 = 无高亮洞（收束步）。
   */
  selector: string | null
  /** i18n key suffix under tour* */
  titleKey: string
  bodyKey: string
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
  waitFor?: string
  waitTimeoutMs?: number
  /** 气泡上额外显示「跳过下载等待」之类 */
  allowSkipWait?: boolean
}

export type TourStepApi = {
  openSettings: (tab?: 'general' | 'output' | 'models' | 'hotwords' | 'compute') => Promise<void>
  closeSettings: () => void
  closeGuide: () => void
  openStats: () => Promise<void>
  closeStats: () => void
}
