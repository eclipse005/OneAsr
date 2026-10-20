import { describe, expect, it } from 'vitest'
import { parseHotwordInput } from './hotwords'

describe('parseHotwordInput', () => {
  it('中文逗号和英文逗号都算分隔', () => {
    expect(parseHotwordInput('甲,乙')).toEqual(['甲', '乙'])
    expect(parseHotwordInput('甲，乙')).toEqual(['甲', '乙'])
    expect(parseHotwordInput('甲, 乙，丙')).toEqual(['甲', '乙', '丙'])
  })

  it('换行、回车、制表符也当分隔（粘贴多行/表格）', () => {
    expect(parseHotwordInput('甲\n乙\r\n丙\t丁')).toEqual(['甲', '乙', '丙', '丁'])
  })

  it('去首尾空白、丢空项，按不区分大小写去重', () => {
    expect(parseHotwordInput('  , 甲 ,, 乙 , ')).toEqual(['甲', '乙'])
    expect(parseHotwordInput('OneAsr\noneasr\nONEASR')).toEqual(['OneAsr'])
  })

  it('顿号不是逗号：不拆开，整条留作一个词', () => {
    // 断骨锚点：把顿号加进分隔集合后，这条立刻变红。core 的词表解析同样不拆顿号。
    expect(parseHotwordInput('甲、乙')).toEqual(['甲、乙'])
  })

  it('空输入 / 纯空白返回空表', () => {
    expect(parseHotwordInput('')).toEqual([])
    expect(parseHotwordInput('  \n\t  ')).toEqual([])
  })
})
