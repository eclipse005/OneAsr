/**
 * 文稿的「行」有两个含义，混在一起就没法看了：
 *
 * - **物理行**：编辑器里的一行文本。用户敲回车就多一行。
 * - **字幕行**：一条字幕。文稿匹配里「一行 = 一条字幕」，最终每条字幕
 *   一个 cue。
 *
 * 差别在两处：一条太长的字幕在屏幕上会**软换行**（视觉上占好几行，但它还是
 * 一条）；空行不是字幕（core 的 `text_stats` 也只数非空行）。
 *
 * 面板里的行号必须跟 core 的口径一致 —— 行号就是字幕号，用户拿它对最终 SRT
 * 的序号。判据只有这一处，壳不自己数。
 */

/** 一条字幕在文稿里的位置。 */
export interface TranscriptLineMark {
  /** 物理行下标（0 起，含空行） */
  row: number
  /** 字幕号（1 起） */
  no: number
}

/** 文稿的物理行（保留空行：它们在编辑器里确实占一行）。 */
export function transcriptRows(text: string): string[] {
  return text.split('\n')
}

/**
 * 哪些物理行是「一条字幕」，以及它的字幕号。
 *
 * 空行跳过（不是字幕）；其余按出现顺序编号。
 */
export function transcriptLineMarks(text: string): TranscriptLineMark[] {
  const marks: TranscriptLineMark[] = []
  let no = 0
  transcriptRows(text).forEach((row, index) => {
    // 与 core 的 text_stats 同口径：整行都是空白就算空行
    if (!row.trim()) return
    no += 1
    marks.push({ row: index, no })
  })
  return marks
}
