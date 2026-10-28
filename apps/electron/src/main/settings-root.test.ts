import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, describe, expect, it } from 'vitest'
import { chooseDevAppRoot, findCli, hasCliBinary, resolveAppRoot, resolveCli } from './settings'

const temps: string[] = []

function tmpDir(name: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `oneasr-${name}-`))
  temps.push(dir)
  return dir
}

afterEach(() => {
  for (const dir of temps.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

describe('chooseDevAppRoot', () => {
  it('仓库只有 ffmpeg、旁路 oneasr-runtime 有 CLI 时选 runtime', () => {
    const base = tmpDir('layout')
    const repo = path.join(base, 'OneAsr')
    const runtime = path.join(base, 'oneasr-runtime')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.mkdirSync(runtime, { recursive: true })
    fs.writeFileSync(path.join(runtime, 'oneasr-cli'), '')
    fs.chmodSync(path.join(runtime, 'oneasr-cli'), 0o755)

    const picked = chooseDevAppRoot([repo])
    expect(picked).toBe(runtime)
    expect(hasCliBinary(runtime)).toBe(true)
    expect(hasCliBinary(repo)).toBe(false)
  })

  it('仓库本身有 CLI 时仍选仓库', () => {
    const repo = tmpDir('repo-cli')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.writeFileSync(path.join(repo, 'oneasr-cli'), '')
    expect(chooseDevAppRoot([repo])).toBe(repo)
  })

  it('都没有 CLI 时回退到第一个 ffmpeg 根', () => {
    const repo = tmpDir('ffmpeg-only')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    expect(chooseDevAppRoot([repo], [])).toBe(repo)
  })

  it('CLI 只在 target/release|debug 下也算数（cargo build 后不必拷到根）', () => {
    // 这正是 run-with-runtime.mjs 当年翻车的形状：它只看 <root>/oneasr-cli，
    // 于是把“仓库根只有 bin/ffmpeg、CLI 在 target/debug”误判成“没有 CLI”，
    // 回退到空目录 oneasr-runtime 并当作 ONEASR_APP_ROOT 塞给主进程，
    // 表现就是状态栏一直“未找到引擎”。
    const repo = tmpDir('repo-target-cli')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.mkdirSync(path.join(repo, 'target', 'debug'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'target', 'debug', 'oneasr-cli'), '')

    expect(hasCliBinary(repo)).toBe(true)
    expect(chooseDevAppRoot([repo])).toBe(repo)
  })

  it('仓库根有 ffmpeg + CLI 只在 target/release 时不会被空 runtime 抢走', () => {
    const base = tmpDir('target-vs-runtime')
    const repo = path.join(base, 'OneAsr')
    const runtime = path.join(base, 'oneasr-runtime')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.mkdirSync(path.join(repo, 'target', 'release'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'target', 'release', 'oneasr-cli'), '')
    // 旁路 runtime 存在但只有日志，没有 CLI
    fs.mkdirSync(runtime, { recursive: true })
    fs.writeFileSync(path.join(runtime, 'oneasr-error.log'), '')

    expect(hasCliBinary(runtime)).toBe(false)
    expect(chooseDevAppRoot([repo])).toBe(repo)
  })

  it('extraRoots 可补上 /workspace/oneasr-runtime 一类路径', () => {
    const repo = tmpDir('repo')
    const runtime = tmpDir('runtime-extra')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.writeFileSync(path.join(runtime, 'oneasr-cli'), '')
    expect(chooseDevAppRoot([repo], [runtime])).toBe(runtime)
  })
})

describe('resolveAppRoot：app root 的唯一决策点', () => {
  it('env 指向没 CLI 的目录 -> 忽略它，回到探测（附带笔记）', () => {
    // 这就是用户撞上的那个 bug 的形状：某个过期的 ONEASR_APP_ROOT 指向空目录。
    // 以前 env 无条件优先 -> 整个 app 掉进“找不到引擎”。
    const base = tmpDir('env-bogus')
    const empty = path.join(base, 'empty-runtime')
    const repo = path.join(base, 'OneAsr')
    fs.mkdirSync(path.join(empty), { recursive: true })
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    fs.mkdirSync(path.join(repo, 'target', 'debug'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'target', 'debug', 'oneasr-cli'), '')

    const d = resolveAppRoot({ envRoot: empty, startDir: path.join(repo, 'target', 'debug') })
    expect(d.source).toBe('detected')
    expect(d.root).toBe(repo)
    expect(d.notes.join(' ')).toContain('没有 oneasr-cli')
  })

  it('env 里有 CLI -> 当做显式选择，不探测', () => {
    const runtime = tmpDir('env-ok')
    fs.writeFileSync(path.join(runtime, 'oneasr-cli'), '')
    const d = resolveAppRoot({ envRoot: runtime, startDir: tmpDir('elsewhere') })
    expect(d.source).toBe('env')
    expect(d.root).toBe(runtime)
  })

  it('打包后 resources/runtime 是唯一答案，env 不能改写', () => {
    const runtime = tmpDir('packaged')
    fs.writeFileSync(path.join(runtime, 'oneasr-cli'), '')
    const d = resolveAppRoot({ envRoot: tmpDir('stale-env'), packagedRoot: runtime })
    expect(d.source).toBe('packaged')
    expect(d.root).toBe(runtime)
  })

  it('完全无线索时不编造路径，并说明原因', () => {
    // Nest deeper than walkFfmpegRoots(maxLevels=6) so we never reach `/`.
    // Otherwise system `/bin/ffmpeg` false-positives as an app root (source=detected).
    const base = tmpDir('no-clues')
    const lonely = path.join(base, 'a', 'b', 'c', 'd', 'e', 'f')
    fs.mkdirSync(lonely, { recursive: true })
    const d = resolveAppRoot({ startDir: lonely })
    expect(d.source).toBe('fallback')
    expect(d.notes.join(' ')).toContain('没找到')
    // 不允许退回任何写死的开发机路径
    expect(d.root).not.toBe('/workspace/oneasr-runtime')
  })

  it('有 ffmpeg 但没 CLI：仍选中该根，但注明未编译', () => {
    const repo = tmpDir('no-cli')
    fs.mkdirSync(path.join(repo, 'bin'), { recursive: true })
    fs.writeFileSync(path.join(repo, 'bin', 'ffmpeg'), '')
    const d = resolveAppRoot({ startDir: repo })
    expect(d.root).toBe(repo)
    expect(d.notes.join(' ')).toContain('没有 oneasr-cli')
  })
})

describe('findCli / resolveCli：“找不到”是 null，不是假路径', () => {
  it('没有 CLI 时 resolveCli 返回 null', () => {
    const empty = tmpDir('empty')
    expect(findCli(empty)).toBeNull()
    expect(resolveCli(empty)).toBeNull()
  })

  it('根目录与 target/release|debug 用同一份候选规则', () => {
    const root = tmpDir('candidates')
    fs.mkdirSync(path.join(root, 'target', 'release'), { recursive: true })
    const release = path.join(root, 'target', 'release', 'oneasr-cli')
    fs.writeFileSync(release, '')
    expect(findCli(root)).toBe(release)
    expect(hasCliBinary(root)).toBe(true)
    // 根目录同名文件优先（部署时把 release 二进制拷到根就是想要的那个）
    const top = path.join(root, 'oneasr-cli')
    fs.writeFileSync(top, '')
    expect(findCli(root)).toBe(top)
  })
})

/**
 * 结构护栏：app root 只许在 `settings.ts` 的 `resolveAppRoot()` 里决策。
 *
 * 历史：`scripts/run-with-runtime.mjs` 又写了一份「只看根目录」的探测，
 * 把结果当 `ONEASR_APP_ROOT` 塞给主进程 —— 而 env 变量在 `DEFAULT_APP_ROOT`
 * 里无校验优先，于是正确逻辑被整体遮掉，状态栏一直「未找到引擎」。
 * 两份探测不一致时，用户看到的是一份自己无法处理的错误。
 */
describe('npm 脚本不预先写死 app root', () => {
  const pkg = JSON.parse(
    fs.readFileSync(new URL('../../package.json', import.meta.url), 'utf8')
  ) as { scripts: Record<string, string> }

  for (const name of ['dev', 'start', 'preview']) {
    it(`npm run ${name} 不包一层探测脚本`, () => {
      const script = pkg.scripts[name]!
      expect(script).not.toContain('ONEASR_APP_ROOT')
      expect(script).not.toContain('run-with-runtime')
    })
  }
})
