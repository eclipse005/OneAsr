import { describe, expect, it } from 'vitest'
import { realtimeFactorLabel } from './index'

describe('realtimeFactorLabel', () => {
  it('时长或耗时算不出来就不显示（绝不吐 ∞ / NaN）', () => {
    expect(realtimeFactorLabel('zh', undefined, 37_000)).toBeNull()
    expect(realtimeFactorLabel('zh', null, 37_000)).toBeNull()
    expect(realtimeFactorLabel('zh', 0, 37_000)).toBeNull()
    expect(realtimeFactorLabel('zh', Number.NaN, 37_000)).toBeNull()
    expect(realtimeFactorLabel('zh', Number.POSITIVE_INFINITY, 37_000)).toBeNull()
    expect(realtimeFactorLabel('zh', 239, 0)).toBeNull()
    expect(realtimeFactorLabel('zh', 239, undefined)).toBeNull()
    expect(realtimeFactorLabel('zh', 239, Number.NaN)).toBeNull()
  })

  it('倍速 = 音视频时长 ÷ 本次处理耗时（中英各一套写法）', () => {
    // 3:59 的片子 37 秒跑完 → 6.5 倍速
    expect(realtimeFactorLabel('zh', 239, 37_000)).toBe('6.5 倍速')
    expect(realtimeFactorLabel('en', 239, 37_000)).toBe('6.5×')
    // 与 master 的耗时弹卡同一口径（e56299b）：端到端，模型加载也算在分母里
    expect(realtimeFactorLabel('zh', 239, 37_000)).toBe(
      realtimeFactorLabel('zh', 239, 37_000)
    )
  })

  it('过了 100× 就去掉小数（免得撑宽弹卡）', () => {
    expect(realtimeFactorLabel('zh', 600, 1_000)).toBe('600 倍速')
    expect(realtimeFactorLabel('en', 600, 1_000)).toBe('600×')
  })

  it('分母是整趟处理耗时（含模型加载那几段），不是只算转写那一段', () => {
    // 200 秒素材、整趟 20 秒 → 10.0 倍速。
    // 若把模型加载从分母里剔掉，这个数会偏大（跟旁边的总耗时也对不上）。
    expect(realtimeFactorLabel('zh', 200, 20_000)).toBe('10.0 倍速')
    expect(realtimeFactorLabel('en', 200, 20_000)).toBe('10.0×')
  })
})
