import { describe, expect, it } from 'vitest'
import { visibleSteps } from './steps'

const base = {
  modelsReady: false,
  hasTasks: false,
  settingsOpen: false
}

describe('tour steps', () => {
  it('模型未就绪时含下载步，空列表不含拖放区而含空态设置入口', () => {
    const ids = visibleSteps({ ...base, modelsReady: false, hasTasks: false }).map((s) => s.id)
    expect(ids).toContain('settings-download')
    expect(ids).toContain('status-models')
    expect(ids).not.toContain('dropzone')
    expect(ids).toContain('empty-open-settings')
    expect(ids).not.toContain('task-lang')
  })

  it('模型就绪时跳过下载与空态设置；有任务时讲行内芯片', () => {
    const ids = visibleSteps({
      ...base,
      modelsReady: true,
      hasTasks: true,
      settingsOpen: true
    }).map((s) => s.id)
    expect(ids).not.toContain('settings-download')
    expect(ids).not.toContain('dropzone')
    expect(ids).not.toContain('empty-open-settings')
    expect(ids).toContain('task-sep')
    expect(ids).toContain('task-transcript')
    expect(ids).toContain('task-start')
  })

  it('模型就绪且空列表时讲拖放区', () => {
    const ids = visibleSteps({ ...base, modelsReady: true, hasTasks: false }).map((s) => s.id)
    expect(ids).toContain('dropzone')
    expect(ids).not.toContain('empty-open-settings')
  })

  it('开始步不用 click 推进（避免误启 ASR）', () => {
    const start = visibleSteps({
      ...base,
      modelsReady: true,
      hasTasks: true
    }).find((s) => s.id === 'task-start')
    expect(start?.advance).toBe('next')
  })

  it('设置标签步为 click-or-next，避免 Esc 关设置后卡死', () => {
    const ids = visibleSteps({ ...base, modelsReady: true })
    const hot = ids.find((s) => s.id === 'settings-hotwords-tab')
    const out = ids.find((s) => s.id === 'settings-output-tab')
    expect(hot?.advance).toBe('click-or-next')
    expect(out?.advance).toBe('click-or-next')
  })

  it('引导里没有 CLI 状态分支：引擎问题不上界面，用户治不了', () => {
    // 历史：这里曾有 toolbar-settings-cli 一步专门讲「引擎 CLI 过旧」。
    // 那条警告的判据是正则匹配本地化的人读文案，中文界面下永远误报，
    // 而用户也无法处理 —— 整条链路已删除，引导只剩产品本身的使用路径。
    const ids = visibleSteps({ ...base, modelsReady: true }).map((s) => s.id)
    expect(ids.some((id) => id.includes('cli'))).toBe(false)
  })

  it('模型/热词/输出面板步仍在引导路径中（讲解卡已统一钉右下角，placement 不再约束）', () => {
    const ids = visibleSteps({ ...base, modelsReady: true }).map((s) => s.id)
    for (const id of ['settings-models', 'settings-hotwords', 'settings-output'] as const) {
      expect(ids).toContain(id)
    }
  })

  it('统计步高亮面板标题栏而非被挡住的工具栏按钮', () => {
    const stats = visibleSteps({ ...base, modelsReady: true }).find((s) => s.id === 'toolbar-stats')
    expect(stats?.selector).toBe('[data-tour="stats-header"]')
    expect(stats?.waitFor).toBe('[data-tour="stats-header"]')
  })

  it('收束步无高亮选择器', () => {
    const done = visibleSteps({ ...base, modelsReady: true }).find((s) => s.id === 'done')
    expect(done?.selector).toBeNull()
  })

  it('关闭设置可 click-or-next，便于监听 overlay 关闭推进', () => {
    const close = visibleSteps({ ...base, modelsReady: true }).find(
      (s) => s.id === 'settings-close'
    )
    expect(close?.advance).toBe('click-or-next')
  })

  it('热词/输出内容步 before 会打开对应设置页，避免 waitFor 超时跳步', async () => {
    const ids = visibleSteps({ ...base, modelsReady: true })
    const hot = ids.find((s) => s.id === 'settings-hotwords')
    const out = ids.find((s) => s.id === 'settings-output')
    expect(hot?.waitFor).toBe('[data-tour="settings-pane-hotwords"]')
    expect(out?.waitFor).toBe('[data-tour="settings-pane-output"]')

    const opened: string[] = []
    const api = {
      openSettings: async (tab?: 'general' | 'output' | 'models' | 'hotwords' | 'compute') => {
        opened.push(tab ?? 'general')
      },
      closeSettings: () => {},
      closeGuide: () => {},
      openStats: async () => {},
      closeStats: () => {}
    }
    const ctx = { ...base, modelsReady: true, settingsOpen: true }
    await hot!.before!(ctx, api)
    await out!.before!(ctx, api)
    expect(opened).toEqual(['hotwords', 'output'])
  })

})
