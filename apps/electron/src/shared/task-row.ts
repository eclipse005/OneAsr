import type { TaskStatus } from './types'

/**
 * 打轴时行内控件也不收起来。热词对对齐无效，所以禁用；
 * 语言会传给对齐器，人声分离也会跑，这两项保持可用。
 */
export function rowChipState(hasTranscript: boolean): {
  showLanguage: boolean
  showHotwords: boolean
  languageEnabled: boolean
  hotwordsEnabled: boolean
} {
  return {
    showLanguage: true,
    showHotwords: true,
    languageEnabled: true,
    hotwordsEnabled: !hasTranscript
  }
}

/**
 * 外面那一颗开始键要跑的流程。
 * 图标按「再来一次」和「从零开始」分两族：
 * 重新转录 / 重试 / 重试打轴 / 重新打轴 都是「再来一遍」，用圆圈箭头（↻）；
 * 开始 / 开始打轴 是第一次跑，用播放三角。
 */
export type RowStartKind =
  | 'blocked'
  | 'align'
  | 'realign'
  | 'retry-align'
  | 'start'
  | 'retry'
  | 'retranscribe'

/** 行内那颗主按钮的图标：'retry' 是圆圈箭头，'play' 是播放三角。 */
export function rowStartIcon(kind: RowStartKind): 'play' | 'retry' {
  switch (kind) {
    case 'retranscribe':
    case 'retry':
    case 'retry-align':
    case 'realign':
      return 'retry'
    case 'blocked':
    case 'align':
    case 'start':
      return 'play'
  }
}

export function rowStartKind(input: {
  hasTranscript: boolean
  status: TaskStatus
  hasOutput: boolean
  needsRealign: boolean
  timedOutputEnabled: boolean
}): RowStartKind {
  if (input.hasTranscript && !input.timedOutputEnabled) return 'blocked'
  if (input.hasTranscript) {
    if (input.status === 'error') return 'retry-align'
    if (input.needsRealign || (input.status === 'done' && input.hasOutput)) return 'realign'
    return 'align'
  }
  if (input.status === 'error') return 'retry'
  if (input.status === 'done' && input.hasOutput) return 'retranscribe'
  return 'start'
}
