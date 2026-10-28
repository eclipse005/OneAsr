import { describe, expect, it } from 'vitest'
import { visibleSteps } from './steps'
import { resolveTourAllowSkipWait, resolveTourSelector } from './types'

const base = {
  modelsReady: false,
  hasTasks: false,
  settingsOpen: false
}

describe('tour steps', () => {
  it('就绪与未就绪同一课表：约 6 步，未就绪不另开 empty-open-settings / 独立下载步', () => {
    const notReady = visibleSteps({ ...base, modelsReady: false, hasTasks: false }).map((s) => s.id)
    const ready = visibleSteps({ ...base, modelsReady: true, hasTasks: false }).map((s) => s.id)
    expect(notReady).toEqual([
      'status-models',
      'settings-models',
      'settings-hotwords',
      'settings-output',
      'add-or-drop',
      'done'
    ])
    expect(ready).toEqual(notReady)
    expect(notReady).not.toContain('settings-download')
    expect(notReady).not.toContain('empty-open-settings')
    expect(notReady).not.toContain('settings-close')
    expect(notReady).not.toContain('dropzone')
    expect(notReady).not.toContain('toolbar-stats')
    expect(notReady).not.toContain('toolbar-guide')
  })

  it('状态栏步始终 click 推进（与 onOpenModels 一致）', () => {
    for (const modelsReady of [false, true]) {
      const status = visibleSteps({ ...base, modelsReady }).find((s) => s.id === 'status-models')
      expect(status?.advance).toBe('click')
    }
  })

  it('未就绪时模型步高亮下载并允许跳过等待；就绪时高亮面板', () => {
    const notReady = visibleSteps({ ...base, modelsReady: false }).find(
      (s) => s.id === 'settings-models'
    )!
    const ready = visibleSteps({ ...base, modelsReady: true }).find(
      (s) => s.id === 'settings-models'
    )!
    expect(resolveTourSelector(notReady.selector, { ...base, modelsReady: false })).toBe(
      '[data-tour="model-download-asr"]'
    )
    expect(resolveTourAllowSkipWait(notReady, { ...base, modelsReady: false })).toBe(true)
    expect(resolveTourSelector(ready.selector, { ...base, modelsReady: true })).toBe(
      '[data-tour="settings-pane-models"]'
    )
    expect(resolveTourAllowSkipWait(ready, { ...base, modelsReady: true })).toBe(false)
  })

  it('空列表且模型就绪时添加步高亮拖放区，否则高亮工具栏添加', () => {
    const emptyReady = visibleSteps({ ...base, modelsReady: true, hasTasks: false }).find(
      (s) => s.id === 'add-or-drop'
    )!
    const emptyNotReady = visibleSteps({ ...base, modelsReady: false, hasTasks: false }).find(
      (s) => s.id === 'add-or-drop'
    )!
    const withTasks = visibleSteps({ ...base, modelsReady: true, hasTasks: true }).find(
      (s) => s.id === 'add-or-drop'
    )!
    expect(
      resolveTourSelector(emptyReady.selector, { ...base, modelsReady: true, hasTasks: false })
    ).toBe('[data-tour="dropzone"]')
    expect(
      resolveTourSelector(emptyNotReady.selector, { ...base, modelsReady: false, hasTasks: false })
    ).toBe('[data-tour="toolbar-add"]')
    expect(
      resolveTourSelector(withTasks.selector, { ...base, modelsReady: true, hasTasks: true })
    ).toBe('[data-tour="toolbar-add"]')
  })

  it('有任务时多一步 task-row，且仅 next 推进', () => {
    const ids = visibleSteps({
      ...base,
      modelsReady: true,
      hasTasks: true
    }).map((s) => s.id)
    expect(ids).toEqual([
      'status-models',
      'settings-models',
      'settings-hotwords',
      'settings-output',
      'add-or-drop',
      'task-row',
      'done'
    ])
    const row = visibleSteps({ ...base, modelsReady: true, hasTasks: true }).find(
      (s) => s.id === 'task-row'
    )
    expect(row?.advance).toBe('next')
    expect(row?.selector).toBe('[data-tour="task-row"]')
    for (const gone of [
      'task-lang',
      'task-hotwords',
      'task-sep',
      'task-transcript',
      'task-start',
      'task-output'
    ]) {
      expect(ids).not.toContain(gone)
    }
  })

  it('引导里没有 CLI 状态分支：引擎问题不上界面，用户治不了', () => {
    const ids = visibleSteps({ ...base, modelsReady: true }).map((s) => s.id)
    expect(ids.some((id) => id.includes('cli'))).toBe(false)
  })

  it('模型/热词/输出面板步仍在引导路径中', () => {
    const ids = visibleSteps({ ...base, modelsReady: true }).map((s) => s.id)
    for (const id of ['settings-models', 'settings-hotwords', 'settings-output'] as const) {
      expect(ids).toContain(id)
    }
  })

  it('收束步无高亮选择器', () => {
    const done = visibleSteps({ ...base, modelsReady: true }).find((s) => s.id === 'done')
    expect(done?.selector).toBeNull()
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

  it('添加步 before 会关闭设置与统计', async () => {
    const add = visibleSteps({ ...base, modelsReady: true }).find((s) => s.id === 'add-or-drop')!
    let closedSettings = false
    let closedStats = false
    await add.before!(
      { ...base, modelsReady: true },
      {
        openSettings: async () => {},
        closeSettings: () => {
          closedSettings = true
        },
        closeGuide: () => {},
        openStats: async () => {},
        closeStats: () => {
          closedStats = true
        }
      }
    )
    expect(closedSettings).toBe(true)
    expect(closedStats).toBe(true)
  })
})
