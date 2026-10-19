import { describe, expect, it } from 'vitest'
import type { Settings } from '../shared/types'
import { defaultSettings, outputFlags } from './settings'

function withOutput(saveNextToSource: boolean, outputDir: string): Settings {
  return {
    ...defaultSettings('D:/app'),
    save_next_to_source: saveNextToSource,
    output_dir: outputDir
  }
}

describe('outputFlags', () => {
  it('保存到源文件旁：交还 to-source 旗标与兜底目录', () => {
    expect(outputFlags(withOutput(true, 'D:/out'))).toEqual([
      '--save-next-to-source',
      '--output-dir',
      'D:/out'
    ])
  })

  it('固定输出目录：只交还 --output-dir', () => {
    expect(outputFlags(withOutput(false, 'D:/out'))).toEqual(['--output-dir', 'D:/out'])
  })

  it('空目录不传，保留 CLI 的 {data-root}/output 默认', () => {
    expect(outputFlags(withOutput(false, ''))).toEqual([])
  })
})
