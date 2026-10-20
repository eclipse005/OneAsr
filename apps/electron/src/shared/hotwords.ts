/**
 * 解析一次输入 / 粘贴：**中文逗号（，）和英文逗号（,）都算分隔**，换行、回车、
 * 制表符同理；去首尾空白、丢空项、按不区分大小写去重。
 *
 * 与 core 的 `hotword::parse_word_list` 一样，顿号（、）不是分隔符，它会留在词条里
 * ——猜错分隔符等于静默改写用户输入，所以宁可让用户看见它没被拆开。制表符只在
 * 输入/粘贴这一层当分隔（表格粘贴），保存时会统一成换行，core 不会见到它。
 */
export function parseHotwordInput(raw: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const part of raw.split(/[,，\n\r\t]+/)) {
    const word = part.trim()
    if (!word) continue
    const key = word.toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push(word)
  }
  return out
}
