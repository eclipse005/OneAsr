import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

/**
 * 文稿面板只放**数据**（文件名、行数、语速、状态），解释性文字一律进指南。
 *
 * 面板是干活的地方：左边摘要、右边正文、下面两个按钮，中间夹一段说明只会
 * 让人以为漏了什么。用户在 2026-10-19 明确说过：说明文字都应该放到指南里。
 *
 * 组件没有 DOM 运行时（vitest 里跑不了 React 树），所以照本仓库
 * `styles/*.test.ts` 的做法直读源码，把这条规矩钉住。
 */
const panel = readFileSync(new URL('./TranscriptPanel.tsx', import.meta.url), 'utf8')
const guide = readFileSync(new URL('./GuidePanel.tsx', import.meta.url), 'utf8')

describe('文稿面板的文案分工', () => {
  it('面板里不出现解释性文案', () => {
    // 这两条已经搬进指南；面板再引用它们，字典类型就会编译不过。
    // 这里把「不许回来」写成显式的规矩，免得有人顺手又加回来。
    expect(panel).not.toMatch(/transcriptEditorHint/)
    expect(panel).not.toMatch(/transcriptUseRowStart/)
    // 面板底部不再挂说明条（那是搬走的两句所在的位置）
    expect(panel).not.toMatch(/className="hint"/)
  })

  it('说明文字在指南的「文稿」这一页里', () => {
    expect(guide).toMatch(/id: 'transcript'/)
    for (const key of [
      'guideTranscriptPair',
      'guideTranscriptPanel',
      'guideTranscriptEdit',
      'guideTranscriptSplit',
      'guideTranscriptRealign',
      'guideTranscriptTimed'
    ]) {
      expect(guide, `指南的文稿页要有 ${key}`).toContain(key)
    }
  })

  it('面板有行号槽：号只画在字幕行的第一行上', () => {
    // 行号是这次的核心：软换行之后要能分清「一条长字幕」和「两条字幕」。
    expect(panel).toMatch(/transcript-gutter/)
    // 号的来源是「字幕行」判据，不是物理行
    expect(panel).toMatch(/transcriptLineMarks/)
  })
})
