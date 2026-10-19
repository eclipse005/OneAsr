import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

/**
 * 任务卡片「元信息行」的布局门禁。
 *
 * 这一行的形状是：文件名一行，下面是「大小 · 时长 · 耗时 · 状态」。
 * 状态（跑着的时候是阶段名，跑完是「完成」，失败/取消/排队同理）就排在
 * 文件名下面这一行；右列只留参数 chip 和动作按钮，**不再有状态槽**。
 *
 * 之前状态占着右列一个 88px 的定宽槽，右边空一大块、四个参数 chip 还得
 * 整体往左让（用户 2026-10-19 指出：状态该在文件名下面，右边别再空一块）。
 * 同一轮反馈还去掉了行里那个多余的「打开字幕」入口——右边的「打开输出」
 * 按钮已经能做这件事，不该在元信息里再塞第二个入口。
 *
 * 纯 CSS 的布局没法在 vitest 里真渲染，所以这里锁住**产生那些洞的那几条机制**：
 * 谁把状态塞回右列、或把那个入口加回来，测试就红。
 *
 * CSS 走 fs 直读而不是 `?raw` 导入：vitest 默认把 CSS 处理成空串
 * （css:false 的桩），`?raw` 拿到的也是空串，规则一条都匹配不上。
 */
const appCss = readFileSync(new URL('./app.css', import.meta.url), 'utf8')
const taskListSource = readFileSync(new URL('../components/TaskList.tsx', import.meta.url), 'utf8')
const guideSource = readFileSync(new URL('../components/GuidePanel.tsx', import.meta.url), 'utf8')

/** 取一条规则的花括号内容（选择器里的 . 和 : 都要转义） */
function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const m = new RegExp(`${escaped}\\s*\\{([^}]*)\\}`).exec(css)
  if (!m) throw new Error(`规则没找到：${selector}`)
  return m[1]
}

describe('任务卡片的元信息行', () => {
  it('状态排在文件名下面这一行（排在大小·时长、耗时的后面）', () => {
    const meta = taskListSource.indexOf('className="meta-line"')
    expect(meta, 'TaskList.tsx 里应当还有 meta-line').toBeGreaterThan(-1)
    const nameMeta = taskListSource.indexOf('className="name-meta"')
    const status = taskListSource.indexOf('className="badge running"')
    expect(nameMeta).toBeGreaterThan(meta)
    expect(status, 'meta-line 里应当有状态药丸').toBeGreaterThan(nameMeta)
    // 右列（.controls）在状态之后：状态留在左列，右列只剩 chip 与动作按钮
    expect(taskListSource.indexOf('className="controls"')).toBeGreaterThan(status)
  })

  it('右列不再有状态槽（撤掉之后四个参数 chip 整体右移）', () => {
    expect(taskListSource, '右列不该再留 badge-slot').not.toContain('badge-slot')
  })

  it('行里不再有第二个「打开字幕」入口（右边已有「打开输出」按钮）', () => {
    expect(taskListSource, '不该再有 row-output 入口').not.toContain('row-output')
    expect(appCss, 'app.css 里也不该再有 .row-output 规则').not.toContain('.row-output')
  })

  it('状态不靠 margin-left:auto 顶到左列右端', () => {
    const body = ruleBody(appCss, '.task-card .meta-line .badge')
    expect(body, '状态不该再靠 auto 推到列尾').not.toMatch(/margin-left:\s*auto/)
  })

  it('状态是透明底的行内文字（跟旁边的灰字元信息排成一行，不是一块灰药丸）', () => {
    const body = ruleBody(appCss, '.task-card .meta-line .badge')
    expect(body).toMatch(/background:\s*transparent/)
    expect(body).toMatch(/padding:\s*0/)
  })

  it('状态可以收缩并省略（长阶段名不会把整行撑歪）', () => {
    const body = ruleBody(appCss, '.task-card .meta-line .badge')
    expect(body).toMatch(/min-width:\s*0/)
    expect(body).toMatch(/overflow:\s*hidden/)
    const inner = ruleBody(appCss, '.task-card .meta-line .badge > span')
    expect(inner).toMatch(/text-overflow:\s*ellipsis/)
  })

  it('状态文字留了裁切余量（不然中日韩粗体字最后一个字会被削掉一竖）', () => {
    // overflow:hidden 正好切在字宽边界上，而中日韩字形的墨迹会画到字宽之外一点
    // （实测「待处理」的「理」右竖被切掉 0.5px，放大截图才看得出来）。
    // 内层 span 留一点右余量，字就不会被切；药丸底色透明，多出来的看不出来。
    const inner = ruleBody(appCss, '.task-card .meta-line .badge > span')
    const pad = /padding-right:\s*(\d+(?:\.\d+)?)px/.exec(inner)
    expect(pad, '状态文字要有右裁切余量').not.toBeNull()
    expect(Number(pad![1]), '余量至少 1px').toBeGreaterThanOrEqual(1)
  })

  it('元信息本身可以被挤（挤不下时先省略它，状态那一句要保持完整）', () => {
    // 元信息必须**可收缩**：右列那四个 chip 加三个动作要 508px，窗口窄的时候
    // 若元信息不让位，它会直接溢出到右列上（实测 metaLine.scrollW 81 > clientW 10）。
    for (const selector of ['.task-card .name-meta', '.task-card .meta-line > .timing-chip']) {
      const body = ruleBody(appCss, selector)
      expect(body, `${selector} 该能被挤`).toMatch(/flex:\s*0\s+1\s+auto/)
      expect(body, `${selector} 省略号要留着`).toMatch(/text-overflow:\s*ellipsis/)
    }
  })

  it('状态不参与收缩（窗口再窄也是先省略元信息，状态那一句保持完整）', () => {
    const body = ruleBody(appCss, '.task-card .meta-line .badge')
    expect(body, '状态不该被挤扁').toMatch(/flex:\s*0\s+0\s+auto/)
    // 但特别长的阶段名不能把右列顶开：最多占这一行四成，超出部分自己省略
    expect(body).toMatch(/max-width:\s*\d+%/)
  })

  it('左列保底宽度够放下「大小·时长 · 耗时 · 状态」一整行', () => {
    const body = ruleBody(appCss, '.task-card')
    const m = /grid-template-columns:\s*minmax\((\d+)px,\s*1fr\)/.exec(body)
    expect(m, '.task-card 应当给左列保底宽度').not.toBeNull()
    expect(Number(m![1]), '左列保底至少要 240px，否则状态会被裁').toBeGreaterThanOrEqual(240)
  })

  it('元信息之间留有间距（状态不会跟耗时粘在一起）', () => {
    const body = ruleBody(appCss, '.task-card .meta-line')
    const gap = /gap:\s*([\d.]+)px/.exec(body)
    expect(gap, '.meta-line 应当有 gap').not.toBeNull()
    expect(Number(gap![1])).toBeGreaterThan(0)
  })

  it('状态药丸在任务卡片里按状态上色（不再只挂在 .controls 下）', () => {
    for (const state of ['running', 'done', 'error', 'cancelled']) {
      const body = ruleBody(appCss, `.task-card .badge.${state}`)
      expect(body, `.badge.${state} 应当有颜色`).toMatch(/color:\s*var\(|color:\s*#/)
    }
  })

  it('使用指南里的示意行跟真实行一致：状态在左列，chip 排一排', () => {
    // 示意行照抄真实卡片的排布：状态在文件名下面那一行，右列不再有状态槽
    expect(guideSource, '指南里不该再留 badge-slot').not.toContain('badge-slot')
    const meta = guideSource.indexOf('className="meta-line"')
    const badge = guideSource.indexOf('className="badge queued"')
    const controls = guideSource.indexOf('className="controls"')
    expect(meta).toBeGreaterThan(-1)
    expect(badge, '指南的示意行里状态应当排在 meta-line 里').toBeGreaterThan(meta)
    expect(badge, '状态要在右列之前').toBeLessThan(controls)
    // 四个 chip 要排得下一行：示意卡不能守真实卡片的 240px 保底（指南面板窄）
    const body = ruleBody(appCss, '.guide-demo')
    const m = /grid-template-columns:\s*minmax\((\d+)px,\s*1fr\)/.exec(body)
    expect(m, '.guide-demo 应当有自己的列宽').not.toBeNull()
    expect(Number(m![1]), '示意卡的左列保底要比真实卡片小，否则 chip 会换行').toBeLessThan(240)
  })
})
