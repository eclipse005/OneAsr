import { describe, expect, it } from 'vitest'
import { visibleSteps } from './steps'

const base = {
  modelsReady: false,
  hasTasks: false,
  settingsOpen: false,
  cliOutdated: false
}

describe('tour steps', () => {
  it('模型未就绪时含下载步，空列表不含拖放区而含空态设置入口', () => {
    const ids = visibleSteps({ ...base, modelsReady: false, hasTasks: false }).map((s) => s.id)
    expect(ids).toContain('settings-download')
    expect(ids).toContain('status-models')
    expect(ids).not.toContain('toolbar-settings-cli')
    expect(ids).not.toContain('dropzone')
    expect(ids).toContain('empty-open-settings')
    expect(ids).not.toContain('task-lang')
  })

  it('CLI 过旧时走设置按钮，不装成模型状态，并跳过下载', () => {
    const ids = visibleSteps({ ...base, cliOutdated: true, modelsReady: false }).map((s) => s.id)
    expect(ids).toContain('toolbar-settings-cli')
    expect(ids).not.toContain('status-models')
    expect(ids).not.toContain('settings-download')
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

  it('仅点击推进的步骤不提供 next', () => {
    const tab = visibleSteps({ ...base, modelsReady: true }).find(
      (s) => s.id === 'settings-hotwords-tab'
    )
    expect(tab?.advance).toBe('click')
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
})
