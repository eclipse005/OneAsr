import type { TourStep } from './types'

/**
 * 完整产品引导步骤。条件路径在 when / before 里分叉：
 * - 模型未就绪：从状态栏点进设置 → 模型，可跳过下载等待
 * - 模型已就绪：状态栏只讲解，不强迫开设置
 * - 空列表：讲拖放区；有任务：讲行内芯片（不点「开始」以免真跑 ASR）
 */
export const TOUR_STEPS: TourStep[] = [
  {
    id: 'status-models',
    selector: '[data-tour="status-models"]',
    titleKey: 'tourStatusTitle',
    bodyKey: 'tourStatusBody',
    placement: 'top',
    // 未就绪：点状态栏进设置；已就绪：下一步即可
    advance: 'click-or-next',
    before: async (ctx, api) => {
      api.closeGuide()
      if (!ctx.modelsReady && !ctx.settingsOpen) {
        // 用户点状态栏会自己开；这里不抢先打开，保留真实点击路径
      }
      return true
    }
  },
  {
    id: 'settings-models',
    selector: '[data-tour="settings-pane-models"]',
    titleKey: 'tourModelsTitle',
    bodyKey: 'tourModelsBody',
    placement: 'left',
    advance: 'next',
    waitFor: '[data-tour="settings-pane-models"]',
    before: async (ctx, api) => {
      if (!ctx.settingsOpen) {
        await api.openSettings('models')
      } else {
        // 已开着但可能不在 models：再推一次
        await api.openSettings('models')
      }
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
    when: (ctx) => !ctx.modelsReady,
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
    advance: 'click',
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
    waitFor: '[data-tour="settings-pane-hotwords"]'
  },
  {
    id: 'settings-output-tab',
    selector: '[data-tour="settings-tab-output"]',
    titleKey: 'tourOutputTabTitle',
    bodyKey: 'tourOutputTabBody',
    placement: 'right',
    advance: 'click'
  },
  {
    id: 'settings-output',
    selector: '[data-tour="settings-pane-output"]',
    titleKey: 'tourOutputTitle',
    bodyKey: 'tourOutputBody',
    placement: 'left',
    advance: 'next',
    waitFor: '[data-tour="settings-pane-output"]'
  },
  {
    id: 'settings-close',
    selector: '[data-tour="settings-close"]',
    titleKey: 'tourSettingsCloseTitle',
    bodyKey: 'tourSettingsCloseBody',
    placement: 'bottom',
    // 只用「下一步」：点 X 若有未保存会先弹确认，跟引导抢焦点
    advance: 'next',
    waitFor: '[data-tour="settings-close"]',
    before: async (ctx, api) => {
      if (!document.querySelector('[data-tour="settings-close"]')) {
        // 设置已关就跳过这一步
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
    // 不强迫点添加（会弹系统文件框）；讲解 + 下一步
    advance: 'next',
    before: async (_ctx, api) => {
      api.closeSettings()
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
    when: (ctx) => !ctx.hasTasks
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
    selector: '[data-tour="toolbar-stats"]',
    titleKey: 'tourStatsTitle',
    bodyKey: 'tourStatsBody',
    placement: 'bottom',
    advance: 'next'
  },
  {
    id: 'toolbar-guide',
    selector: '[data-tour="toolbar-guide"]',
    titleKey: 'tourGuideReopenTitle',
    bodyKey: 'tourGuideReopenBody',
    placement: 'bottom',
    advance: 'next'
  },
  {
    id: 'done',
    selector: '[data-tour="toolbar-guide"]',
    titleKey: 'tourDoneTitle',
    bodyKey: 'tourDoneBody',
    placement: 'bottom',
    advance: 'next'
  }
]

export function visibleSteps(ctx: {
  modelsReady: boolean
  hasTasks: boolean
  settingsOpen: boolean
}): TourStep[] {
  return TOUR_STEPS.filter((step) => (step.when ? step.when(ctx) : true))
}
