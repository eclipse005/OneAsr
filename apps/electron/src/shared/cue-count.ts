import path from 'path'

/**
 * 统计一份主产物里的「条数」。
 *
 * SRT 数字幕条（箭头行），ASS 数 Dialogue 行，TXT 数非空行。别再用
 * “非空且不是时间轴”那种混合口径：它对 ASS 和 TXT 都会算错。
 */
export function countCues(filePath: string, text: string): number {
  const lines = text.split(/\r?\n/)
  const ext = path.extname(filePath).toLowerCase()
  if (ext === '.srt') return lines.filter((l) => l.includes('-->')).length
  if (ext === '.ass') return lines.filter((l) => l.startsWith('Dialogue:')).length
  return lines.filter((l) => l.trim()).length
}
