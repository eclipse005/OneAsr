/**
 * 热词只接中文与英文——与 core 的 `lang::supports_hotwords` 同一判据。
 *
 * 这里不复制整套语言表：行内存的是 `zh` / `en` 这类 id，Rust 侧还认
 * `Chinese` / `English` / `cmn` / `mandarin` 这些写法，所以顺手接住，
 * 免得老任务或手改配置里的标签被误判成「不支持」。
 */
const HOTWORD_LANGUAGES = new Set(['zh', 'chinese', 'cmn', 'mandarin', 'en', 'english'])

/** 这个语言能不能用热词（`zh` / `en` 及其别名）。 */
export function supportsHotwords(language: string | null | undefined): boolean {
  const head = (language ?? '').trim().toLowerCase().split(/[-_]/)[0]
  return HOTWORD_LANGUAGES.has(head)
}

/**
 * 任务的词库绑定：语言不支持热词时**一律清成「无」**。
 *
 * 「不支持」不是「先挂着、以后可能生效」——留着 id 只会让界面显示无热词、
 * 任务里却还挂着一个永远不会被管线读取的组。所有写 `hotwordGroupId` 的路径
 * 都走这里，避免只改一处、漏掉新建 / 换语言 / 应用到全部行。
 */
export function hotwordGroupForLanguage(
  language: string | null | undefined,
  groupId: string | null | undefined
): string | null {
  return supportsHotwords(language) ? (groupId ?? null) : null
}
