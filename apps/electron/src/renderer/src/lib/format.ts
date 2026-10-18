/** 纯展示格式化。壳里所有"把数字/路径变成字符串"的地方都从这里取，别各写一份。 */

/** 字节 → 自适应单位（列表里的「尺寸」）。 */
export function formatSize(bytes?: number): string | undefined {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return undefined
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${Math.max(1, Math.round(bytes / 1024))} KB`
}

/** 字节 → GiB 文本（下载进度的「已下 / 总量」，两位小数够看）。 */
export function gib(bytes?: number): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return '0 GB'
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`
}

/** 路径 → 末段名字（只用于显示；完整路径放 title）。 */
export function dirName(p?: string): string {
  if (!p) return ''
  const parts = p.split(/[\\/]/)
  return parts[parts.length - 1] ?? p
}
