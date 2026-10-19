import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

/**
 * 「用户拖进来的文件不许因为表没到手就被丢掉」这条规矩。
 *
 * 扩展名表来自 CLI 的 `status --json`，而一次 status 要 spawn 一次 CLI、
 * 再去 stat 模型目录 —— 磁盘慢时得好几秒。早先的实现在这几秒里直接
 * `return`，把用户的选择整个丢了，只留一句「正在读取模型状态…」。
 * 而这个窗口恰好是启动后最容易拖文件的时候（模型还在探，界面已经能拖）。
 *
 * 组件没法在 vitest 里直接跑（没有 DOM 运行时），所以照本仓库
 * `styles/*.test.ts` 的做法直读源码，盯住那条分支不许回来。
 */
const appSource = readFileSync(new URL('./App.tsx', import.meta.url), 'utf8')

describe('加文件的策略', () => {
  it('表没到手时等它，而不是把这次选择丢掉', () => {
    // 必须有「等一次 status」的写法
    expect(
      appSource,
      'addFiles 应当在表没到手时去拿（status ?? await refreshModels()）'
    ).toMatch(/status\s*\?\?\s*\(\s*await\s+refreshModels\(\)\s*\)/)

    // 不许再有「表没到手就直接 return」那种写法。
    // 断骨锚点：把 App.tsx 改回 `if (!status) { notify(...); return }`，
    // 下面这条断言立刻变红。
    const bailOut = /if\s*\(\s*!status\s*\)\s*\{[^}]*\breturn\b/.exec(appSource)
    expect(
      bailOut,
      'addFiles 不许因为 status 没到手就把用户的输入丢掉'
    ).toBeNull()
  })

  it('status 查询失败才提示（那时是真的没有表了）', () => {
    // 拿不到表的时候要说一句，但那条提示只该出现在「查过之后还是 null」的分支上。
    const warn = /if\s*\(\s*!st\s*\)\s*\{[^}]*notify\('warn',\s*s\.modelsLoading\)/.exec(appSource)
    expect(warn, '查不到表时要提示 modelsLoading').not.toBeNull()
  })
})
