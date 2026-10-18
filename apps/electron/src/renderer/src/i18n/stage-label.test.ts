import { describe, expect, it } from 'vitest'
import { stageLabel } from './index'

describe('stageLabel', () => {
  it('按 CLI 的稳定 key 出界面语言的文案（中 / 英各自独立于 CLI 进程语言）', () => {
    expect(stageLabel('zh', 'transcribing')).toBe('转写')
    expect(stageLabel('en', 'transcribing')).toBe('Transcribing')
    expect(stageLabel('zh', 'loading_aligner')).toBe('加载对齐模型')
    expect(stageLabel('en', 'loading_aligner')).toBe('Loading aligner model')
  })

  it('条件阶段的 key 也认得（人声分离 / 热词提取）', () => {
    expect(stageLabel('zh', 'separating')).toBe('人声分离')
    expect(stageLabel('en', 'rough_transcribing')).toBe('Extracting hotwords')
  })

  it('认不出的 key 退回 CLI 给的 label，不猜也不留空', () => {
    expect(stageLabel('zh', 'brand_new_stage', '新阶段')).toBe('新阶段')
    expect(stageLabel('zh', undefined, '转码音频')).toBe('转码音频')
  })

  it('两者都没有时给空串（渲染层会退到「进行中」文案）', () => {
    expect(stageLabel('zh', undefined, undefined)).toBe('')
  })
})
