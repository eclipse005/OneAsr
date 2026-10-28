import fs from 'fs'
import { describe, expect, it } from 'vitest'

/**
 * 这一组测试守的是一条设计不变式：**壳不许解析 CLI 的人读文案**，
 * 也不许为了“探测能力”去猜。
 *
 * 来历（同一形状翻车两次）：
 * 1. 早先用正则匹配 `unknown command: status` / `missing required --input`
 *    来判断 CLI 支持哪些子命令；
 * 2. 而 CLI 的报错文案随 `ONEASR_UI_LANG` 本地化 —— 中文界面下输出是
 *    「缺少必需参数 --input」，英文正则永远匹配不上，于是状态栏持续误报
 *    「CLI 过旧，请更新引擎」，而用户的二进制完全正常；
 * 3. 「未知命令」和「缺参」都是 exit 2，靠退出码也分不开 ——
 *    这条路径先天不可能稳。
 *
 * 结论：能力探测整套删掉。它产的 status/transcript 两个能力位从来没有消费者，
 * 唯一作用就是点亮一条用户治不了的警告。真实调用点（runCliJson）本来就会把
 * 真错误带回来；版本一致性用机器可读的 `status --json` 的 `version` 字段。
 */
describe('壳不解析 CLI 的人读文案', () => {
  /** 去掉注释再扫：注释里提到旧实现不算违规 */
  function codeOnly(source: string): string {
    return source
      .replace(/\/\*[\s\S]*?\*\//g, '') // 块注释
      .replace(/(^|[^:])\/\/.*$/gm, '$1') // 行注释
  }

  const source = codeOnly(fs.readFileSync(new URL('./cli.ts', import.meta.url), 'utf8'))

  it('cli.ts 的代码里没有针对 CLI 输出文案的正则', () => {
    // 这些都是「人类读的句子」里的片段；任何一条出现在代码里，都说明又在读文案
    const forbidden = [/missing required/i, /unknown command/i, /not found/i]
    for (const re of forbidden) {
      expect(source).not.toMatch(re)
    }
  })

  it('不再导出任何“按文案判能力”或“探测能力”的函数', async () => {
    const mod = (await import('./cli')) as Record<string, unknown>
    for (const gone of [
      'transcriptCapabilityFromOutput',
      'statusCapabilityFromOutput',
      'probeCliCapabilities',
      'probeCliReachable',
      'isCliUnreachable',
      'runCliCapture'
    ]) {
      expect(mod[gone], gone).toBeUndefined()
    }
  })

  it('壳只留一个入口：runCliJson（读 JSON，失败就报错）', async () => {
    const mod = (await import('./cli')) as Record<string, unknown>
    expect(typeof mod.runCliJson).toBe('function')
    expect(typeof mod.cliEnv).toBe('function')
  })
})
