import type { TourContext, TourStep } from './types'

/**
 * 完整产品引导步骤。条件路径在 when / before 里分叉：
 * - CLI 过旧：工具栏「设置」进模型页（勿把橙色 CLI 条当模型状态）
 * - 模型未就绪：状态栏 → 设置 → 模型；可跳过下载等待
 * - 模型已就绪：状态栏只讲解；跳过下载与空态「打开设置」
 * - 空列表且模型就绪：讲拖放区；未就绪：跳过拖放（画布不可投）
 * - 有任务：讲行内芯片（不点「开始」以免真跑 ASR）
 */
export const TOUR_STEPS: TourStep[] = [
  {
    id: 'status-models',
    selector: '[data-tour="status-models"]',
    titleKey: 'tourStatusTitle',
    bodyKey: 'tourStatusBody',
    placement: 'top',
    // 未就绪：点状态栏进设置；已就绪：下一步即可（文案说明可跳过）
    advance: 'click-or-next',
    when: (ctx) => !ctx.cliOutdated && !ctx.cliMissing,
    before: async (_ctx, api) => {
      api.closeGuide()
      return true
    }
  },
  {
    id: 'toolbar-settings-cli',
    selector: '[data-tour="toolbar-settings"]',
    titleKey: (ctx) => (ctx.cliMissing ? 'tourCliMissingTitle' : 'tourCliTitle'),
    bodyKey: (ctx) => (ctx.cliMissing ? 'tourCliMissingBody' : 'tourCliBody'),
    placement: 'bottom',
    advance: 'click-or-next',
    when: (ctx) => ctx.cliOutdated || ctx.cliMissing,
    before: async (_ctx, api) => {
      api.closeGuide()
      return true
    }
  },
  {
    id: 'settings-models',
    selector: '[data-tour="settings-pane-models"]',
    titleKey: 'tourModelsTitle',
    bodyKey: 'tourModelsBody',
    // 气泡放在面板左侧（盖住导航侧），避免挡住模型内容与 CLI 横幅
    placement: 'left',
    advance: 'next',
    waitFor: '[data-tour="settings-pane-models"]',
    before: async (_ctx, api) => {
      // 必须赢过工具栏 Settings 默认 openSettings('general') 的竞态：
      // 进入本步时强制打开 models，并确认 pane 出现后再亮气泡。
      await api.openSettings('models')
      return true
    }
  },
  {
    id: 'settings-download',
    selector: '[data-tour="model-download-asr"]',
    titleKey: 'tourDownloadTitle',
    bodyKey: 'tourDownloadBody',
    placement: 'left',
    advance: 'next',
    allowSkipWait: true,
    // CLI 过旧时 status 也会 Missing，下载无效；只在真缺权重时讲下载
    when: (ctx) => !ctx.modelsReady && !ctx.cliOutdated && !ctx.cliMissing,
    waitFor: '[data-tour="model-download-asr"]',
    before: async (_ctx, api) => {
      await api.openSettings('models')
      return true
    }
  },
  {
    id: 'settings-hotwords-tab',
    selector: '[data-tour="settings-tab-hotwords"]',
    titleKey: 'tourHotwordsTabTitle',
    bodyKey: 'tourHotwordsTabBody',
    placement: 'right',
    // click-or-next：Esc 关掉设置后仍可「跳过此步」，不会卡死无 Next 的 click-only
    advance: 'click-or-next',
    before: async (_ctx, api) => {
      await api.openSettings('models')
      return true
    }
  },
  {
    id: 'settings-hotwords',
    selector: '[data-tour="settings-pane-hotwords"]',
    titleKey: 'tourHotwordsTitle',
    bodyKey: 'tourHotwordsBody',
    placement: 'left',
    advance: 'next',
    waitFor: '[data-tour="settings-pane-hotwords"]',
    // 点标签或「跳过此步」都要先切到热词页，再等 pane；否则 waitFor 超时会误跳到 output-tab
    before: async (_ctx, api) => {
      await api.openSettings('hotwords')
      return true
    }
  },
  {
    id: 'settings-output-tab',
    selector: '[data-tour="settings-tab-output"]',
    titleKey: 'tourOutputTabTitle',
    bodyKey: 'tourOutputTabBody',
    placement: 'right',
    advance: 'click-or-next'
  },
  {
    id: 'settings-output',
    selector: '[data-tour="settings-pane-output"]',
    titleKey: 'tourOutputTitle',
    bodyKey: 'tourOutputBody',
    placement: 'left',
    advance: 'next',
    waitFor: '[data-tour="settings-pane-output"]',
    before: async (_ctx, api) => {
      await api.openSettings('output')
      return true
    }
  },
  {
    id: 'settings-close',
    selector: '[data-tour="settings-close"]',
    titleKey: 'tourSettingsCloseTitle',
    bodyKey: 'tourSettingsCloseBody',
    placement: 'bottom',
    // 点关闭或「下一步」；设置真正关掉时引擎会立刻推进（见 TourController）
    advance: 'click-or-next',
    waitFor: '[data-tour="settings-close"]',
    before: async (ctx) => {
      if (!ctx.settingsOpen || !document.querySelector('[data-tour="settings-close"]')) {
        return false
      }
      return true
    }
  },
  {
    id: 'toolbar-add',
    selector: '[data-tour="toolbar-add"]',
    titleKey: 'tourAddTitle',
    bodyKey: 'tourAddBody',
    placement: 'bottom',
    // 不强迫点添加（会弹系统文件框）；讲解 + 下一步跳过
    advance: 'next',
    before: async (_ctx, api) => {
      api.closeSettings()
      api.closeStats()
      return true
    }
  },
  {
    id: 'dropzone',
    selector: '[data-tour="dropzone"]',
    titleKey: 'tourDropzoneTitle',
    bodyKey: 'tourDropzoneBody',
    placement: 'bottom',
    advance: 'next',
    // 模型未就绪时空态不可投，勿高亮整块画布
    when: (ctx) => !ctx.hasTasks && ctx.modelsReady
  },
  {
    id: 'empty-open-settings',
    selector: '[data-tour="empty-open-settings"]',
    titleKey: 'tourEmptySettingsTitle',
    bodyKey: 'tourEmptySettingsBody',
    placement: 'top',
    advance: 'click-or-next',
    when: (ctx) => !ctx.hasTasks && !ctx.modelsReady,
    before: async (ctx) => {
      // 刚从设置出来且仍未就绪：讲空态上的「打开设置」入口
      return !ctx.settingsOpen
    }
  },
  {
    id: 'task-lang',
    selector: '[data-tour="task-lang"]',
    titleKey: 'tourTaskLangTitle',
    bodyKey: 'tourTaskLangBody',
    placement: 'bottom',
    advance: 'next',
    when: (ctx) => ctx.hasTasks
  },
  {
    id: 'task-hotwords',
    selector: '[data-tour="task-hotwords"]',
    titleKey: 'tourTaskHotwordsTitle',
    bodyKey: 'tourTaskHotwordsBody',
    placement: 'bottom',
    advance: 'next',
    when: (ctx) => ctx.hasTasks
  },
  {
    id: 'task-sep',
    selector: '[data-tour="task-sep"]',
    titleKey: 'tourTaskSepTitle',
    bodyKey: 'tourTaskSepBody',
    placement: 'bottom',
    advance: 'next',
    when: (ctx) => ctx.hasTasks
  },
  {
    id: 'task-transcript',
    selector: '[data-tour="task-transcript"]',
    titleKey: 'tourTaskTranscriptTitle',
    bodyKey: 'tourTaskTranscriptBody',
    placement: 'bottom',
    advance: 'next',
    when: (ctx) => ctx.hasTasks
  },
  {
    id: 'task-start',
    selector: '[data-tour="task-start"]',
    titleKey: 'tourTaskStartTitle',
    bodyKey: 'tourTaskStartBody',
    placement: 'left',
    // 绝不用 click 推进——那会真的开跑 ASR
    advance: 'next',
    when: (ctx) => ctx.hasTasks
  },
  {
    id: 'task-output',
    selector: '[data-tour="task-output"]',
    titleKey: 'tourTaskOutputTitle',
    bodyKey: 'tourTaskOutputBody',
    placement: 'left',
    advance: 'next',
    when: (ctx) => ctx.hasTasks
  },
  {
    id: 'toolbar-stats',
    // 面板盖住工具栏按钮时洞会落空；高亮已打开的统计面板标题栏
    selector: '[data-tour="stats-header"]',
    titleKey: 'tourStatsTitle',
    bodyKey: 'tourStatsBody',
    placement: 'bottom',
    advance: 'next',
    waitFor: '[data-tour="stats-header"]',
    before: async (_ctx, api) => {
      await api.openStats()
      return true
    }
  },
  {
    id: 'toolbar-guide',
    selector: '[data-tour="toolbar-guide"]',
    titleKey: 'tourGuideReopenTitle',
    bodyKey: 'tourGuideReopenBody',
    placement: 'bottom',
    advance: 'next',
    before: async (_ctx, api) => {
      api.closeStats()
      return true
    }
  },
  {
    id: 'done',
    selector: null,
    titleKey: 'tourDoneTitle',
    bodyKey: 'tourDoneBody',
    placement: 'auto',
    advance: 'next'
  }
]

export function visibleSteps(ctx: TourContext): TourStep[] {
  return TOUR_STEPS.filter((step) => (step.when ? step.when(ctx) : true))
}
