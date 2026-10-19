/**
 * 挑出来的文件怎么变成「媒体 + 挂好的文稿」。
 *
 * 与旧版同一条规则：只选一个媒体和一个文稿时直接配对；多文件时按文件名
 * （不区分大小写）找同名媒体。配不上的文稿不静默忽略。
 * 纯函数；渲染层也能用，所以不引入 Node 的 path。
 */

export interface PickedPair {
  mediaIndex: number
  transcript: string
}

export interface PairingResult {
  /** 按选中顺序排的媒体，每个建一条任务 */
  media: string[]
  /** 文稿 → 它挂到的媒体下标 */
  pairs: PickedPair[]
  /** 没找到同名媒体的文稿 */
  unpairedTranscripts: string[]
  /** 既不是媒体也不是文稿的文件 */
  skipped: string[]
}

/** 取文件名（两种分隔符都认；渲染层拿不到 path 模块）。 */
export function fileName(value: string): string {
  const i = Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\'))
  return i >= 0 ? value.slice(i + 1) : value
}

/** 小写扩展名，不带点。 */
export function fileExt(value: string): string {
  const base = fileName(value)
  const dot = base.lastIndexOf('.')
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : ''
}

/** 去扩展名的文件名，小写；`a.b.mp4` → `a.b`。 */
export function fileStem(value: string): string {
  const base = fileName(value)
  const dot = base.lastIndexOf('.')
  return (dot > 0 ? base.slice(0, dot) : base).toLowerCase()
}

function hasExt(list: string[], value: string): boolean {
  const ext = fileExt(value)
  return !!ext && list.some((x) => x.replace(/^\./, '').toLowerCase() === ext)
}

export function isTranscriptPath(value: string, transcriptExts: string[]): boolean {
  return hasExt(transcriptExts, value)
}

export function pairPicked(
  paths: string[],
  mediaExts: string[],
  transcriptExts: string[]
): PairingResult {
  // 状态还没加载时不要猜表：把全部当媒体，调用方会先挡下。
  const media = mediaExts.length || transcriptExts.length ? paths.filter((p) => hasExt(mediaExts, p)) : [...paths]
  const texts = transcriptExts.length ? paths.filter((p) => hasExt(transcriptExts, p)) : []
  const skipped = paths.filter((p) => !media.includes(p) && !texts.includes(p))
  const out: PairingResult = { media, pairs: [], unpairedTranscripts: [], skipped }

  if (media.length === 1 && texts.length === 1) {
    out.pairs.push({ mediaIndex: 0, transcript: texts[0] })
    return out
  }

  const claimed = new Set<number>()
  for (const transcript of texts) {
    const key = fileStem(transcript)
    const index = media.findIndex((m, i) => !claimed.has(i) && fileStem(m) === key)
    if (index >= 0) {
      claimed.add(index)
      out.pairs.push({ mediaIndex: index, transcript })
    } else {
      out.unpairedTranscripts.push(transcript)
    }
  }
  return out
}

/** 配好之后直接喂给 `addTasks` 的形状。 */
export function pairingItems(
  result: PairingResult
): Array<{ media: string; transcript?: string }> {
  const byIndex = new Map(result.pairs.map((p) => [p.mediaIndex, p.transcript]))
  return result.media.map((media, i) => ({ media, transcript: byIndex.get(i) }))
}
