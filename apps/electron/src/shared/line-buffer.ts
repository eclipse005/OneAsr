/**
 * 把任意切分的字节流拼回整行。
 *
 * 子进程的 `data` 事件不保证按行到达：一行 JSON 完全可能被拆成两个 chunk。
 * 直接对每个 chunk `split('\n')` 会把半行当坏 JSON 丢掉 —— 对 transcribe
 * 来说，丢掉 `done` 事件会让一次成功运行被判成「没有产物」。
 */
export class LineBuffer {
  private rest = ''

  push(chunk: string, onLine: (line: string) => void): void {
    this.rest += chunk
    const lines = this.rest.split(/\r?\n/)
    // 最后一段没有换行结尾：留到下一次（或者 flush）。
    this.rest = lines.pop() ?? ''
    for (const line of lines) {
      if (line.trim()) onLine(line)
    }
  }

  /** 进程关闭时把没有换行结尾的最后一行也交出去。 */
  flush(onLine: (line: string) => void): void {
    const line = this.rest
    this.rest = ''
    if (line.trim()) onLine(line)
  }
}
