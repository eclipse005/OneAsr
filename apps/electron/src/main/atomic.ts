import fs from 'fs'
import path from 'path'

/**
 * 原子写：同目录临时文件 + rename。
 *
 * settings.json / hotword.json 是用户的配置；直接覆盖时崩溃或断电会留下
 * 半个 JSON，下次启动只能落默认值。rename 在同一文件系统上是原子的
 * （Windows 的 rename 会替换已存在的目标）。
 */
export function writeFileAtomicSync(file: string, data: string): void {
  const dir = path.dirname(file)
  fs.mkdirSync(dir, { recursive: true })
  const tmp = path.join(
    dir,
    `.${path.basename(file)}.${process.pid}.${Date.now()}.${Math.random().toString(16).slice(2)}.tmp`
  )
  fs.writeFileSync(tmp, data, 'utf8')
  try {
    fs.renameSync(tmp, file)
  } catch (error) {
    try {
      fs.unlinkSync(tmp)
    } catch {
      /* 清理失败不掩盖原始错误 */
    }
    throw error
  }
}
