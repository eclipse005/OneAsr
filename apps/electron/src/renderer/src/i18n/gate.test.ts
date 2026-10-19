import { describe, expect, it } from 'vitest'
import * as ts from 'typescript'
import { en, zh } from './index'

const CJK = /[\u3400-\u9fff\uf900-\ufaff]/
const PLACEHOLDER = /\{([a-zA-Z_][a-zA-Z0-9_]*)\}/g

function placeholders(value: string): string[] {
  return (value.match(PLACEHOLDER) ?? []).map((match) => match.slice(1, -1)).sort()
}

function cjkStringsIn(file: string, source: string): string[] {
  const kind = file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, kind)
  const found: string[] = []

  const visit = (node: ts.Node): void => {
    let text: string | undefined
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      text = node.text
    } else if (ts.isJsxText(node)) {
      text = node.text
    } else if (
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node)
    ) {
      text = node.text
    }
    if (text && CJK.test(text)) {
      const { line } = sf.getLineAndCharacterOfPosition(node.getStart(sf))
      found.push(`${file}:${line + 1}: ${JSON.stringify(text.trim().slice(0, 80))}`)
    }
    ts.forEachChild(node, visit)
  }

  visit(sf)
  return found
}

describe('渲染层双语门禁', () => {
  it('zh / en 的键集合完全一致', () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort())
  })

  it('两侧都没有空文案', () => {
    for (const [lang, dict] of [
      ['zh', zh],
      ['en', en]
    ] as const) {
      for (const [key, value] of Object.entries(dict)) {
        expect(value.trim(), `${lang}.${key}`).not.toBe('')
      }
    }
  })

  it('同一文案两侧的占位符成对', () => {
    for (const key of Object.keys(zh) as Array<keyof typeof zh>) {
      expect(placeholders(en[key]), key).toEqual(placeholders(zh[key]))
    }
  })

  it('AST 扫描器能抓到文案、放过注释', () => {
    const source = [
      'const a = <div>确定</div>',
      'const b = "取消"',
      '// 注释里的中文不是界面文案',
      'const c = "OK"'
    ].join('\n')
    expect(cjkStringsIn('fixture.tsx', source)).toHaveLength(2)
  })

  it('渲染层源码里没有硬编码中文（注释与 i18n 表除外）', () => {
    // 读原始源码而不是打包产物：注释不在 AST 里，天然不会误报；
    // i18n 目录是文案本体，测试文件里则是夹具。
    const sources = import.meta.glob(
      ['../**/*.{ts,tsx}', '!../i18n/**', '!../**/*.test.{ts,tsx}', '!../**/*.d.ts'],
      {
        query: '?raw',
        import: 'default',
        eager: true
      }
    ) as Record<string, string>
    const files = Object.entries(sources)
    expect(files.length).toBeGreaterThan(0)

    const violations = files.flatMap(([file, source]) => cjkStringsIn(file, source))
    expect(violations).toEqual([])
  })
})
