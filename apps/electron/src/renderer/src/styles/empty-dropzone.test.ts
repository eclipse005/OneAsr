import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

/**
 * 空态落点区（.dropzone）的边框纪律。
 *
 * 没有任务时，那一大圈虚线框纯属噪声——背景渐变加居中的提示已经说清「往这儿放」
 * （用户 2026-10-19 指出「这个边框完全没必要」）。
 * 但**拖文件进来时还得有落点提示**：所以边框位留着（平时透明），
 * 拖放时才亮出强调色虚线。
 *
 * CSS 走 fs 直读：vitest 默认把 CSS 处理成空串（css:false 的桩），
 * `?raw` 拿到的也是空串，规则一条都匹配不上。
 */
const appCss = readFileSync(new URL('./app.css', import.meta.url), 'utf8')

/** 取一条规则的花括号内容（选择器里的 . 和 : 都要转义） */
function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const m = new RegExp(`${escaped}\\s*\\{([^}]*)\\}`).exec(css)
  if (!m) throw new Error(`规则没找到：${selector}`)
  return m[1]
}

describe('空态落点区', () => {
  it('平时没有可见边框（虚线是透明的，不占地方也不画灰框）', () => {
    const body = ruleBody(appCss, '.dropzone')
    const border = /border:\s*([^;]+);/.exec(body)
    expect(border, '.dropzone 应当声明边框位').not.toBeNull()
    expect(border![1], '空态的边框要透明').toMatch(/transparent/)
    // 别把强调色直接挂上去：那是拖进来时才亮的状态
    expect(body, '空态不该直接画可见边框').not.toMatch(/border:\s*[^;]*var\(--(?!line)[a-z-]+\)\s*;?\s*$/)
  })

  it('拖文件进来时才亮出强调色虚线框', () => {
    const body = ruleBody(appCss, '.dropzone.active')
    expect(body, '.active 应当亮出边框').toMatch(/border-color:\s*var\(--accent\)/)
  })

  it('边框位一直留着，尺寸不会因为去掉边框而跳动', () => {
    const body = ruleBody(appCss, '.dropzone')
    expect(body, '边框位要留着（transparent 也算）').toMatch(/border:\s*[\d.]+px\s+\w+\s+transparent/)
  })

  it('空态拖放时不叠两层框（落点区自己亮，窗口级那层让位）', () => {
    const body = ruleBody(appCss, '.main.dragging:has(.dropzone.active)::after')
    expect(body, '窗口级的虚线覆盖层在空态要让位').toMatch(/display:\s*none/)
    // 有任务时那层还得留着：列表区没有落点区，得靠它提示
    const overlay = ruleBody(appCss, '.main.dragging::after')
    expect(overlay, '窗口级覆盖层本身要留着').toMatch(/border:\s*\d+px\s+dashed/)
  })

  it('空态铺满整个内容区：四周不留一圈边、也不要圆角', () => {
    // 留白一圈的话，落点区就像一张浮在灰底上的卡片（用户 2026-10-19 指出
    // 「不要周围留白」）。铺满之后圆角也得去掉——贴着窗口边还带圆角会缺一块。
    const main = ruleBody(appCss, '.main:has(.dropzone)')
    expect(main, '空态时 .main 的内边距要归零').toMatch(/padding:\s*0/)
    const dz = ruleBody(appCss, '.main:has(.dropzone) .dropzone')
    expect(dz, '铺满后不要圆角').toMatch(/border-radius:\s*0/)
  })
})
