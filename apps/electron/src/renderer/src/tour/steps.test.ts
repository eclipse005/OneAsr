import { describe, expect, it } from 'vitest'
import { visibleSteps } from './steps'

describe('tour steps', () => {
  it('模型未就绪时含下载步，空列表含拖放区', () => {
    const ids = visibleSteps({
      modelsReady: false,
      hasTasks: false,
      settingsOpen: false
    }).map((s) => s.id)
    expect(ids).toContain('settings-download')
    expect(ids).toContain('dropzone')
    expect(ids).not.toContain('task-lang')
  })

  it('模型就绪时跳过下载；有任务时讲行内芯片', () => {
    const ids = visibleSteps({
      modelsReady: true,
      hasTasks: true,
      settingsOpen: true
    }).map((s) => s.id)
    expect(ids).not.toContain('settings-download')
    expect(ids).not.toContain('dropzone')
    expect(ids).toContain('task-sep')
    expect(ids).toContain('task-transcript')
    expect(ids).toContain('task-start')
  })

  it('开始步不用 click 推进（避免误启 ASR）', () => {
    const start = visibleSteps({
      modelsReady: true,
      hasTasks: true,
      settingsOpen: false
    }).find((s) => s.id === 'task-start')
    expect(start?.advance).toBe('next')
  })
})
