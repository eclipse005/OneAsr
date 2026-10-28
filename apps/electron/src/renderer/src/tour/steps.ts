import type { TourContext, TourStep } from './types'

/**
 * 产品引导约 6 步；就绪与未就绪同一课表，仅个别文案 / 高亮 / skip-wait 分叉：
 * 1. status-models — 必点状态栏进「设置 → 模型」
 * 2. settings-models — 模型页；未就绪时同步讲下载（可跳过等待）
 * 3. settings-hotwords — 热词页（before 直接打开）
 * 4. settings-output — 输出页（before 直接打开）
 * 5. add-or-drop — 添加 / 拖放合并；设置与统计在 before 里关掉
 * 6. done — 收束（指南可重开 + 工具栏统计）
 * 有任务时在添加后多一步 task-row（行内芯片总览；开始仅 next，不点跑 ASR）
 */
export const TOUR_STEPS: TourStep[] = [
  {
    id: 'status-models',
    selector: '[data-tour="status-models"]',
    titleKey: 'tourStatusTitle',
    bodyKey: (ctx) => (ctx.modelsReady ? 'tourStatusBodyReady' : 'tourStatusBody'),
    placement: 'top',
    // 就绪与未就绪都必点：与产品 onOpenModels 一致，进设置→模型
    advance: 'click',
    before: async (_ctx, api) => {
      api.closeGuide()
      return true
    }
  },
  {
    id: 'settings-models',
    // 未就绪：高亮下载控件，同页讲模型+下载；就绪：高亮模型面板
    selector: (ctx) =>
      ctx.modelsReady
        ? '[data-tour="settings-pane-models"]'
        : '[data-tour="model-download-asr"]',
    titleKey: 'tourModelsTitle',
    bodyKey: (ctx) => (ctx.modelsReady ? 'tourModelsBody' : 'tourModelsBodyNeedDownload'),
    placement: 'left',
    advance: 'next',
    waitFor: (ctx) =>
      ctx.modelsReady
        ? '[data-tour="settings-pane-models"]'
        : '[data-tour="model-download-asr"]',
    allowSkipWait: (ctx) => !ctx.modelsReady,
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
    before: async (_ctx, api) => {
      await api.openSettings('hotwords')
      return true
    }
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
    id: 'add-or-drop',
    // 空列表且模型就绪：高亮拖放区；否则高亮工具栏添加（未就绪画布不可投）
    selector: (ctx) =>
      !ctx.hasTasks && ctx.modelsReady
        ? '[data-tour="dropzone"]'
        : '[data-tour="toolbar-add"]',
    titleKey: 'tourAddTitle',
    bodyKey: 'tourAddBody',
    placement: 'bottom',
    advance: 'next',
    before: async (_ctx, api) => {
      api.closeSettings()
      api.closeStats()
      return true
    }
  },
  {
    id: 'task-row',
    selector: '[data-tour="task-row"]',
    titleKey: 'tourTaskRowTitle',
    bodyKey: 'tourTaskRowBody',
    placement: 'bottom',
    // 绝不用 click 推进——会点到「开始」真跑 ASR
    advance: 'next',
    when: (ctx) => ctx.hasTasks,
    waitFor: '[data-tour="task-row"]'
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
