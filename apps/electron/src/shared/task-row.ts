import type { TaskStatus } from './types'
import { supportsHotwords } from './language'

/** 行内组名：没起名时退回占位名，绝不把胶囊渲染成空白。 */
export function hotwordGroupLabel(
  name: string | null | undefined,
  placeholder: string
): string {
  return (name ?? '').trim() || placeholder
}

/** 热词为什么不能选：打轴不用，或这行语言不支持。 */
export type HotwordsDisabledReason = 'align' | 'language' | null

export interface RowChipState {
  showLanguage: boolean
  showHotwords: boolean
  languageEnabled: boolean
  hotwordsEnabled: boolean
  hotwordsDisabledReason: HotwordsDisabledReason
}

/**
 * 打轴时行内控件也不收起来。热词对对齐无效，所以禁用；
 * 语言会传给对齐器，人声分离也会跑，这两项保持可用。
 *
 * 语言不是中文 / 英文时热词同样禁用——这一栏固定显示「无热词」，
 * 不能挂一个管线永远不会读取的组。两个原因同时成立时按打轴算
 * （对齐连中文热词也不用）。
 */
export function rowChipState(hasTranscript: boolean, language: string): RowChipState {
  const languageOk = supportsHotwords(language)
  return {
    showLanguage: true,
    showHotwords: true,
    languageEnabled: true,
    hotwordsEnabled: !hasTranscript && languageOk,
    hotwordsDisabledReason: hasTranscript ? 'align' : languageOk ? null : 'language'
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
