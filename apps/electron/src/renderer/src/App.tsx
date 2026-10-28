import { AnimatePresence, motion } from 'framer-motion'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { GuidePanel } from './components/GuidePanel'
import { SettingsDrawer, type SettingsTab } from './components/SettingsDrawer'
import { StatsPanel } from './components/StatsPanel'
import { StatusBar } from './components/StatusBar'
import { TaskList } from './components/TaskList'
import { ConfirmDialog } from './components/ConfirmDialog'
import { TitleBar } from './components/TitleBar'
import { TranscriptPanel } from './components/TranscriptPanel'
import { GuideMenu, TourProvider, useTour } from './tour'
import { useEscClose } from './esc'
import {
  confirmClearTasks,
  resolveUiLang,
  t,
  transcriptsPaired,
  transcriptsUnpaired,
  unsupportedCount
} from './i18n'
import { useSurfaceMotion } from './motion'
import { isBulkStartCandidate } from '../../shared/start-policy'
import { closeOverlay, openOverlay, type OverlayKind } from '../../shared/overlay'
import { fileName, pairPicked, pairingItems } from './lib/pairing'
import type {
  HotwordStore,
  DownloadState,
  StatusSnapshot,
  Settings,
  StartResult,
  StatsSnapshot,
  TaskSnapshot,
  TranscriptInfo
} from '../../shared/types'

function deepEq(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}


export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [draft, setDraft] = useState<Settings | null>(null)
  /** CLI 的 status --json 快照：就绪、门禁、catalog 全在里面 */
  const [status, setStatus] = useState<StatusSnapshot | null>(null)
  const [statusAt, setStatusAt] = useState(0)
  const [tasks, setTasks] = useState<TaskSnapshot[]>([])
  const [stats, setStats] = useState<StatsSnapshot | null>(null)
  const [hotwords, setHotwords] = useState<HotwordStore | null>(null)
  const [hotwordDraft, setHotwordDraft] = useState<HotwordStore | null>(null)
  const [settingsBaseline, setSettingsBaseline] = useState<Settings | null>(null)
  const [hotwordsBaseline, setHotwordsBaseline] = useState<HotwordStore | null>(null)
  /**
   * 浮窗互斥：同一时刻只开一个（规则见 shared/overlay.ts）。
   *
   * 以前是三个独立布尔量，打开新的不会关掉旧的——统计与指南能同时开着，
   * 两个浮窗叠在一起互相穿透。现在只留一个 kind，结构上就叠不起来了。
   */
  const [overlay, setOverlay] = useState<OverlayKind>('none')
  const settingsOpen = overlay === 'settings'
  const statsOpen = overlay === 'stats'
  const guideOpen = overlay === 'guide'
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('general')
  const [dragging, setDragging] = useState(false)
  /** dragenter/leave 会从子元素冒泡：用计数判断指针是否真的离开了窗口。 */
  const dragDepth = useRef(0)
  /** 当前打开文稿面板的任务；正文只放在主进程，面板打开时再取。 */
  const [transcriptId, setTranscriptId] = useState<string | null>(null)
  const [transcriptText, setTranscriptText] = useState('')
  const [transcriptInfo, setTranscriptInfo] = useState<TranscriptInfo | null>(null)
  /**
   * 提示只有**一条通道**：底部状态栏右侧那枚可点的消息片。
   *
   * 之前是 toast（右下角浮层）+ 状态栏各来一份，同一个「已复制」显示两遍；
   * 而且 toast 浮在列表上方会挡住内容。状态栏在最下方、不挡东西、还支持点击复制，
   * 留它就够了。
   */
  const [statusMsg, setStatusMsg] = useState<{ kind: string; text: string } | null>(null)
  const statusTimer = useRef<number | undefined>(undefined)
  /** 正在飞的那次 status 查询：并发调用共享它（见 refreshModels） */
  const statusQuery = useRef<Promise<StatusSnapshot | null> | null>(null)
  /** 正在开系统文件对话框（防重入） */
  const [picking, setPicking] = useState(false)
  /** 清空会丢掉整表，先在按钮旁边确认。单行移除不确认。 */
  const [clearOpen, setClearOpen] = useState(false)
  /** 设置刚保存成功：主按钮短暂显示「已保存」，反馈留在刚才点的地方 */
  const [settingsAck, setSettingsAck] = useState(false)
  const ackTimer = useRef<number | undefined>(undefined)

  const [downloads, setDownloads] = useState<Record<string, DownloadState>>({})
  const notifiedDownloads = useRef(new Set<string>())

  const lang = useMemo(
    () => resolveUiLang(draft?.ui_language || settings?.ui_language || 'system'),
    [draft?.ui_language, settings?.ui_language]
  )
  const s = t(lang)
  // 就绪与否 = Rust 的 can_start（壳不再自己判断目录够不够）
  const modelsReady = !!status?.can_start.ok
  const dirty = !!(
    settingsBaseline &&
    draft &&
    hotwordsBaseline &&
    hotwordDraft &&
    (!deepEq(settingsBaseline, draft) || !deepEq(hotwordsBaseline, hotwordDraft))
  )

  const openSettingsFn = useCallback(
    async (tab: SettingsTab = 'general') => {
      const savedSettings = settings || (await window.oneasr.getSettings())
      const savedHw = hotwords || (await window.oneasr.getHotwords())
      /**
       * 模型目录显示 **CLI 解出来的那个**，不是配置里存的原样。
       *
       * 配置里可能是相对写法（`"Qwen3-ForcedAligner-0.6B-hf"` 这样的选中目录），
       * 跟旁边两个全路径的输入框摆在一起看着像坏值。壳**不自己拼路径**：
       * 这里用的就是 `status --json` 里 CLI 已经算好的 `models.*.dir`，显示出来而已。
       *
       * 只改这四个「派生字段」，不动 `asr_dirs` / `aligner_dirs` 那两张按模型记
       * 目录的表——表里存的是用户的「选中目录」，塞进布局默认路径等于把这台机器的
       * 绝对路径钉死进配置（应用挪走就废）。副作用是好的：目录成了绝对路径之后，
       * 切换对齐引擎时推出来的目录（`replace(/[^/]+$/, id)`）也仍然是绝对路径。
       */
      const st = status ?? (await window.oneasr.getStatus())
      const shown: Settings = { ...savedSettings }
      if (st) {
        const resolved: Record<string, string | undefined> = {
          asr_model_dir: st.models.asr.dir,
          aligner_model_dir: st.models.aligner.dir,
          demucs_model_dir: st.models.demucs.dir,
          sensevoice_model_dir: st.models.sensevoice.dir
        }
        for (const [key, value] of Object.entries(resolved)) {
          if (value) (shown as unknown as Record<string, unknown>)[key] = value
        }
      }
      setSettings(shown)
      setHotwords(savedHw)
      const sSnap = structuredClone(shown)
      const hSnap = structuredClone(savedHw)
      setSettingsBaseline(sSnap)
      setHotwordsBaseline(hSnap)
      setDraft(structuredClone(sSnap))
      setHotwordDraft(structuredClone(hSnap))
      setSettingsTab(tab)
      setOverlay((cur) => openOverlay(cur, 'settings'))
    },
    [settings, hotwords, status]
  )

  useEffect(() => {
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en'
  }, [lang])

  useEffect(() => {
    const api = {
      setUiLang: (code: string) => {
        // 同时改 draft 与 settings：打开设置会从 settings 克隆 draft，
        // 只改 draft 会被冲掉，截图脚本的 en 集就会全变回中文。
        setDraft((d) => (d ? { ...d, ui_language: code } : d))
        setSettings((s) => (s ? { ...s, ui_language: code } : s))
      },
      open: async (name: 'settings' | 'hotwords' | 'stats' | 'guide' | 'none') => {
        if (name === 'stats') {
          setStats(await window.oneasr.getStats())
        }
        if (name === 'hotwords' || name === 'settings') {
          // 走**和工具栏按钮同一条路**（openSettingsFn）。以前这里另抄了一份
          // 加载逻辑，结果跟真实按钮脱节：按钮那边显示 CLI 解出来的模型全路径，
          // 这个钩子显示配置里的裸名字，自动化脚本两头看到的不一样。
          await openSettingsFn(name === 'hotwords' ? 'hotwords' : 'general')
          return
        }
        if (name === 'none') {
          setOverlay((cur) => closeOverlay(cur))
          return
        }
        setOverlay((cur) => openOverlay(cur, name))
      }
    }
    ;(window as unknown as { __oneasrShotApi?: typeof api }).__oneasrShotApi = api
    return () => {
      delete (window as unknown as { __oneasrShotApi?: typeof api }).__oneasrShotApi
    }
  }, [openSettingsFn])

  const dropzoneMotion = useSurfaceMotion()
  useEscClose(() => setClearOpen(false), clearOpen)

  const notify = useCallback((kind: 'info' | 'success' | 'warn' | 'error', message: string) => {
    setStatusMsg({ kind, text: message })
    if (statusTimer.current) window.clearTimeout(statusTimer.current)
    // 没有 toast 了，报错/警告多留一会儿（8s），普通提示 4s
    statusTimer.current = window.setTimeout(
      () => setStatusMsg(null),
      kind === 'warn' || kind === 'error' ? 8000 : 4000
    )
  }, [])

  /**
   * 拉一次 CLI 的 status 快照。
   *
   * **返回快照本身**（不只是塞进 state）：调用方常常要马上用那张表
   * （比如把拖进来的文件按扩展名分流成媒体 / 文稿），等下一次渲染再读
   * state 会读到旧值。
   *
   * 并发调用共享同一次查询：一次 status 要 spawn 一次 CLI 并去 stat 模型
   * 目录，用户连着拖几个文件时不该并排跑好几个。
   */
  const refreshModels = useCallback(async (): Promise<StatusSnapshot | null> => {
    if (!statusQuery.current) {
      statusQuery.current = window.oneasr
        .getStatus()
        .catch(() => null)
        .finally(() => {
          statusQuery.current = null
        })
    }
    const pending = statusQuery.current
    const next = await pending
    setStatus(next)
    setStatusAt(Date.now())
    return next
  }, [])

  const refreshStats = useCallback(async () => {
    setStats(await window.oneasr.getStats())
  }, [])

  useEffect(() => {
    void (async () => {
      const loaded = await window.oneasr.getSettings()
      setSettings(loaded)
      setDraft(loaded)
      setTasks(await window.oneasr.listTasks())
      setHotwords(await window.oneasr.getHotwords())
      // 不再探测“CLI 是否过旧”：那靠正则匹配本地化的人读文案，中文界面下
      // 必然误判（已连续误报两次），而它只用来点亮一条用户治不了的警告。
      // 引擎的真实问题（缺失 / 跑挂）由 runCliJson 的失败与日志负责。
      await refreshModels()
      await refreshStats()
    })()
    const offTasks = window.oneasr.onTasksUpdated((next) => {
      setTasks(next)
    })
    const offStats = window.oneasr.onStatsUpdated((next) => setStats(next))
    const offDownload = window.oneasr.onDownloadUpdated((state) =>
      setDownloads((prev) => ({ ...prev, [state.model]: state }))
    )
    return () => {
      offTasks()
      offStats()
      offDownload()
    }
  }, [refreshModels, refreshStats])

  // 多个模型可同时下载：每个模型各自进入终态时刷新一次清单并提示。
  useEffect(() => {
    let refresh = false
    for (const state of Object.values(downloads)) {
      if (state.state !== 'done' && state.state !== 'error') {
        // 重试/重新下载会先回到进行中，清掉上一轮的提示标记
        notifiedDownloads.current.delete(state.model)
        continue
      }
      const sig = `${state.model}:${state.state}:${state.message ?? ''}`
      if (notifiedDownloads.current.has(sig)) continue
      notifiedDownloads.current.add(sig)
      if (state.state === 'done') notify('success', s.downloadDone)
      else if (state.message === 'cancelled') notify('info', s.cancelled)
      else if (state.message) notify('error', `${s.downloadFailed}：${state.message}`)
      else notify('error', s.downloadFailed)
      refresh = true
    }
    if (refresh) void refreshModels()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [downloads])

  /**
   * 启动被门禁挡下时把原因弹出来。**文案是 Rust 给的**（status --json 的
   * can_start.reason，双语），壳不再自己拼"模型未就绪/热词组为空"。
   */
  const reportStart = useCallback(
    (r: StartResult) => {
      if (r.ok) return
      notify('warn', r.reason ?? s.blockNothing)
    },
    [s, notify]
  )

  const openTranscript = async (id: string) => {
    const r = await window.oneasr.getTranscript(id)
    if (!r.ok || r.text == null || !r.info) {
      notify('error', r.error || s.transcriptReadFailed)
      return
    }
    // 文稿面板也是一层浮窗：开它之前把别的浮窗收掉，别再叠出两个。
    setOverlay((cur) => closeOverlay(cur))
    setTranscriptId(id)
    setTranscriptText(r.text)
    setTranscriptInfo(r.info)
  }

  const pickTranscriptFor = async (id: string) => {
    const r = await window.oneasr.pickTranscript()
    if (!r.ok) {
      notify('error', `${s.dialogFailed}: ${r.error || ''}`)
      return
    }
    if (!r.path) return
    const next = await window.oneasr.setTaskTranscript(id, r.path)
    setTasks(next)
    const info = next.find((t) => t.id === id)?.transcript
    if (info?.error) {
      notify('error', info.error)
      return
    }
    await openTranscript(id)
  }

  const attachTranscript = async (id: string, path: string) => {
    const next = await window.oneasr.setTaskTranscript(id, path)
    setTasks(next)
    const info = next.find((t) => t.id === id)?.transcript
    if (info?.error) {
      notify('error', info.error)
      return
    }
    await openTranscript(id)
  }

  const saveTranscript = async (text: string): Promise<boolean> => {
    if (!transcriptId) return false
    const r = await window.oneasr.saveTaskTranscript(transcriptId, text)
    if (!r.ok) {
      notify('error', r.error || s.transcriptSaveFailed)
      return false
    }
    if (r.tasks) setTasks(r.tasks)
    if (r.text != null) setTranscriptText(r.text)
    if (r.info) setTranscriptInfo(r.info)
    notify('success', s.transcriptSaved)
    return true
  }

  const splitTranscript = async (text: string) => {
    if (!transcriptId) return
    const id = transcriptId
    // 先把面板里的当前草稿落成暂存，再交给 core 断句；源文件一个字都不动。
    const saved = await window.oneasr.saveTaskTranscript(id, text)
    if (!saved.ok) {
      notify('error', saved.error || s.transcriptSaveFailed)
      return
    }
    if (saved.tasks) setTasks(saved.tasks)
    const r = await window.oneasr.splitTaskTranscript(id)
    if (!r.ok) {
      notify('error', r.error || s.transcriptSplitFailed)
      return
    }
    if (r.tasks) setTasks(r.tasks)
    if (r.text != null) setTranscriptText(r.text)
    if (r.info) setTranscriptInfo(r.info)
    notify('success', s.transcriptSplitDone)
  }

  const rereadTranscript = async () => {
    if (!transcriptId) return
    const r = await window.oneasr.rereadTaskTranscript(transcriptId)
    if (!r.ok) {
      notify('error', r.error || s.transcriptReadFailed)
      return
    }
    if (r.tasks) setTasks(r.tasks)
    if (r.text != null) setTranscriptText(r.text)
    if (r.info) setTranscriptInfo(r.info)
    notify('success', s.transcriptRereadDone)
  }

  const removeTranscript = async () => {
    if (!transcriptId) return
    const id = transcriptId
    const next = await window.oneasr.removeTaskTranscript(id)
    setTasks(next)
    setTranscriptId(null)
    setTranscriptInfo(null)
    notify('success', s.transcriptRemoved)
  }

  const saveAllSettings = async () => {
    if (!draft || !hotwordDraft) return
    const saved = await window.oneasr.saveSettings(draft)
    const savedHw = await window.oneasr.saveHotwords(hotwordDraft)
    setSettings(saved)
    setDraft(saved)
    setHotwords(savedHw)
    setHotwordDraft(structuredClone(savedHw))
    setSettingsBaseline(structuredClone(saved))
    setHotwordsBaseline(structuredClone(savedHw))
    await refreshModels()
    setSettingsAck(true)
    if (ackTimer.current) window.clearTimeout(ackTimer.current)
    ackTimer.current = window.setTimeout(() => setSettingsAck(false), 1400)
    notify('success', s.saved)
  }


  const addFiles = async (paths?: string[]) => {
    if (picking) return
    let list = paths
    if (!list) {
      // 系统对话框是异步的：不加锁的话连点会开好几个窗口
      setPicking(true)
      try {
        const r = await window.oneasr.pickFiles()
        if (!r.ok) {
          notify('error', `${s.dialogFailed}: ${r.error || ''}`)
          return
        }
        list = r.paths
      } finally {
        setPicking(false)
      }
      if (!list.length) {
        notify('info', s.noFiles)
        return
      }
    }
    // 拖进来的可能什么都有：媒体建任务，文稿按同名配对；其余报一次数——不静默吞掉。
    // 两张扩展名表都来自 CLI（core 是唯一真相）。
    //
    // 表还没到手就**等它**：一次状态查询慢几秒，不该把用户刚拖进来的文件丢掉。
    // （旧实现在这里直接 return，只留一句「正在读取模型状态…」，选择整个没了。）
    const st = status ?? (await refreshModels())
    if (!st) {
      notify('warn', s.modelsLoading)
      return
    }
    const pairing = pairPicked(list, st.media_exts ?? [], st.transcript_exts ?? [])
    if (pairing.skipped.length) {
      notify('info', unsupportedCount(lang, pairing.skipped.length))
    }
    if (pairing.unpairedTranscripts.length) {
      notify('warn', transcriptsUnpaired(lang, pairing.unpairedTranscripts.map(fileName)))
    }
    if (!pairing.media.length) return
    setTasks(await window.oneasr.addTasks(pairingItems(pairing)))
    if (pairing.pairs.length) {
      notify('success', transcriptsPaired(lang, pairing.pairs.length))
    }
  }

  const onDragEnter = (e: React.DragEvent) => {
    e.preventDefault()
    dragDepth.current += 1
    setDragging(true)
  }

  const onDragOver = (e: React.DragEvent) => {
    e.preventDefault()
    // 不让 Windows 画「禁止」光标：这是接受文件，不是移动/链接
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'
  }

  const onDragLeave = () => {
    dragDepth.current = Math.max(0, dragDepth.current - 1)
    if (dragDepth.current === 0) setDragging(false)
  }

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    dragDepth.current = 0
    setDragging(false)
    // 过滤与提示都在 addFiles 里统一做（对话框与拖放走同一条路）
    const files = Array.from(e.dataTransfer.files || [])
    // Electron 32 起 File.path 没了：必须走 preload 的 webUtils.getPathForFile
    // （旧写法拿到的一直是 undefined → 拖放只会弹「未选择文件」）
    const paths = files
      .map(
        (f) =>
          window.oneasr.getPathForFile?.(f) ||
          (f as File & { path?: string }).path ||
          ''
      )
      .filter((p): p is string => !!p)
    if (paths.length) void addFiles(paths)
    else notify('warn', s.noFiles)
  }

  const emptyStats: StatsSnapshot = {
    tasksOk: 0,
    tasksErr: 0,
    tasksTotal: 0,
    mediaSec: 0,
    processMs: 0,
    savedSec: null,
    avgRealtime: null,
    cues: 0,
    sepTasks: 0,
    langs: [],
    perDay: [],
    longestMediaSec: null,
    fastestSpeed: null,
    firstDay: null,
    recentSession: []
  }

  const activeTranscriptTask = transcriptId
    ? tasks.find((t) => t.id === transcriptId)
    : undefined
  const canStartAll = tasks.some((task) =>
    isBulkStartCandidate(task.status, task.queueRank != null)
  )

  const confirmClear = async (): Promise<void> => {
    setClearOpen(false)
    const next = await window.oneasr.clearTasks()
    setTasks(next)
    if (next.some((task) => task.status === 'running')) notify('info', s.clearedKeptRunning)
  }

  const closeSettingsFn = useCallback(() => {
    if (settings) setDraft(structuredClone(settings))
    if (hotwords) setHotwordDraft(structuredClone(hotwords))
    setOverlay((cur) => closeOverlay(cur, 'settings'))
    setSettingsTab('general')
  }, [settings, hotwords])

  const tourHost = useMemo(
    () => ({
      modelsReady,
      hasTasks: tasks.length > 0,
      settingsOpen,
      openSettings: async (tab: SettingsTab = 'models') => {
        await openSettingsFn(tab)
      },
      closeSettings: () => closeSettingsFn(),
      closeGuide: () => setOverlay((cur) => closeOverlay(cur, 'guide')),
      openStats: async () => {
        await refreshStats()
        setOverlay((cur) => openOverlay(cur, 'stats'))
      },
      closeStats: () => setOverlay((cur) => closeOverlay(cur, 'stats'))
    }),
    [modelsReady, tasks.length, settingsOpen, openSettingsFn, closeSettingsFn, refreshStats]
  )

  return (
    <TourProvider lang={lang} host={tourHost} statusReady={status != null || statusAt > 0}>
    <div
      className="app"
      onDragEnter={onDragEnter}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      <TitleBar lang={lang} />
      <div className="toolbar">
        <button
          className="btn btn-primary"
          data-tour="toolbar-add"
          disabled={picking}
          onClick={() => void addFiles()}
        >
          <span className="btn-glyph i-plus" aria-hidden />
          {s.add}
        </button>
        <button
          className="btn btn-ghost"
          disabled={!canStartAll}
          onClick={() => void window.oneasr.startAll().then(reportStart)}
        >
          <span className="btn-glyph i-play" aria-hidden />
          {s.startAll}
        </button>
        <button
          type="button"
          className={`btn btn-ghost btn-danger-ghost ${clearOpen ? 'open' : ''}`}
          disabled={!tasks.length}
          aria-expanded={clearOpen}
          onClick={() => setClearOpen((open) => !open)}
        >
          <span className="btn-glyph i-trash" aria-hidden />
          {s.clear}
        </button>
        <div className="grow" />
        <GuideTourButton
          lang={lang}
          onOpenManual={() => setOverlay((cur) => openOverlay(cur, 'guide'))}
        />
        <button
          className="btn btn-ghost"
          data-shot="stats"
          data-tour="toolbar-stats"
          onClick={() => {
            void refreshStats()
            setOverlay((cur) => openOverlay(cur, 'stats'))
          }}
        >
          <span className="btn-glyph i-chart" aria-hidden />
          {s.stats}
        </button>
        <ToolbarSettingsButton
          label={s.settings}
          openSettings={openSettingsFn}
        />
      </div>

      <div className={`main ${dragging ? 'dragging' : ''}`}>
        {tasks.length === 0 ? (
          <motion.div
            className={`dropzone ${dragging ? 'active' : ''}`}
            data-tour="dropzone"
            {...dropzoneMotion}
          >
            <div className="icon">
              <span className="audio-glyph" />
            </div>
            <h2>{dragging ? s.dropActive : modelsReady ? s.emptyTitle : s.emptyTitleNotReady}</h2>
            <p>{modelsReady ? s.emptyHint : s.emptyHintNotReady}</p>
            <div className="dropzone-actions">
              <button className="btn btn-primary" disabled={picking} onClick={() => void addFiles()}>
                {s.add}
              </button>
              <button
                className="btn btn-ghost"
                data-tour="empty-open-settings"
                onClick={() => void openSettingsFn('models')}
              >
                {s.openSettings}
              </button>
            </div>
          </motion.div>
        ) : (
          <TaskList
            lang={lang}
            tasks={tasks}
            hotwordGroups={hotwords?.groups ?? []}
            defaultLanguage={settings?.language ?? 'zh'}
            transcriptExts={status?.transcript_exts ?? []}
            timedOutputEnabled={!!settings && (settings.output_srt || settings.output_ass)}
            onStart={(id) => void window.oneasr.startTask(id).then(reportStart)}
            onCancel={(id) => void window.oneasr.cancelTask(id)}
            onRemove={(id) => void window.oneasr.removeTask(id).then(setTasks)}
            onOpen={(p) => void window.oneasr.openOutputFolder(p)}
            onSetLanguage={(id, language) =>
              void window.oneasr.setTaskLanguage(id, language).then(setTasks)
            }
            onSetVocalSeparation={(id, on) =>
              void window.oneasr.setTaskVocalSeparation(id, on).then(setTasks)
            }
            onApplyToAll={(id) => void window.oneasr.applyTaskToAll(id).then(setTasks)}
            onSetHotwordGroup={(id, groupId) =>
              void window.oneasr.setTaskHotwordGroup(id, groupId).then(setTasks)
            }
            onPickTranscript={(id) => void pickTranscriptFor(id)}
            onOpenTranscript={(id) => void openTranscript(id)}
            onAttachTranscript={(id, path) => void attachTranscript(id, path)}
            onCopyError={(text) =>
              void window.oneasr.copyText(text).then(() => notify('success', s.copied))
            }
          />
        )}
      </div>

      <StatusBar
        lang={lang}
        status={status}
        tasks={tasks}
        message={statusMsg}
        onOpenModels={() => void openSettingsFn('models')}
        onCopyMessage={(text) => void window.oneasr.copyText(text).then(() => notify('success', s.copied))}
      />

      <AnimatePresence>
        {settingsOpen && draft && hotwordDraft && (
          <SettingsDrawer
            lang={lang}
            draft={draft}
            status={status}
            statusAt={statusAt}
            downloads={downloads}
            onDownloadStart={(id, dir) => void window.oneasr.startDownload(id, dir)}
            onDownloadCancel={(id) => void window.oneasr.cancelDownload(id)}
            dirty={dirty}
            hotwordDraft={hotwordDraft}
            initialTab={settingsTab}
            onChange={setDraft}
            onHotwordsChange={setHotwordDraft}
            savedAck={settingsAck}
            onSave={() => void saveAllSettings()}
            onReset={() => {
              if (settingsBaseline) setDraft(structuredClone(settingsBaseline))
              if (hotwordsBaseline) setHotwordDraft(structuredClone(hotwordsBaseline))
              notify('info', s.discardChanges)
            }}
            onRestoreDefaults={() => {
              // 真·恢复默认：写盘（settings:reset），再把草稿与基线对齐
              void window.oneasr.resetSettings().then((saved) => {
                setSettings(saved)
                setSettingsBaseline(structuredClone(saved))
                setDraft(structuredClone(saved))
                void refreshModels()
                notify('success', s.restored)
              })
            }}
            onClose={closeSettingsFn}
            onPickDir={() =>
              void window.oneasr.pickDirectory(draft.output_dir).then((r) => {
                if (!r.ok) {
                  notify('error', `${s.dialogFailed}: ${r.error || ''}`)
                  return
                }
                if (r.path) setDraft({ ...draft, output_dir: r.path })
              })
            }
            onPickModel={(field, modelId) =>
              void window.oneasr.pickModelDir(draft[field] as string).then((r) => {
                if (!r.ok) {
                  notify('error', `${s.dialogFailed}: ${r.error || ''}`)
                  return
                }
                if (!r.path) return
                const path = r.path
                setDraft((d) => {
                  if (!d) return d
                  const next = { ...d, [field]: path } as Settings
                  // 浏览选的也是「这个模型的选择」，必须进 per-model 表；
                  // 只改派生字段的话，切尺寸/引擎时会被推导覆盖。
                  if (field === 'asr_model_dir' && modelId) {
                    next.asr_dirs = { ...d.asr_dirs, [modelId]: path }
                  }
                  if (field === 'aligner_model_dir' && modelId) {
                    next.aligner_dirs = { ...d.aligner_dirs, [modelId]: path }
                  }
                  return next
                })
              })
            }
            onOpenModels={() => void window.oneasr.openModelsFolder()}
            onRefreshModels={() => void refreshModels()}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {statsOpen && (
          <StatsPanel
            lang={lang}
            stats={stats || emptyStats}
            onClose={() => setOverlay((cur) => closeOverlay(cur, 'stats'))}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {guideOpen && (
          <GuidePanel lang={lang} onClose={() => setOverlay((cur) => closeOverlay(cur, 'guide'))} />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {activeTranscriptTask && transcriptInfo && (
          <TranscriptPanel
            key={activeTranscriptTask.id}
            lang={lang}
            info={activeTranscriptTask.transcript ?? transcriptInfo}
            text={transcriptText}
            locked={
              activeTranscriptTask.status === 'running' ||
              (activeTranscriptTask.status === 'queued' &&
                activeTranscriptTask.queueRank != null)
            }
            onClose={() => {
              setTranscriptId(null)
              setTranscriptInfo(null)
            }}
            onSave={saveTranscript}
            onSplit={splitTranscript}
            onReread={rereadTranscript}
            onRemove={removeTranscript}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {clearOpen && (
          <ConfirmDialog
            key="clear-tasks"
            fixed
            danger
            message={confirmClearTasks(lang, tasks.length)}
            cancelLabel={s.cancel}
            confirmLabel={s.clear}
            onCancel={() => setClearOpen(false)}
            onConfirm={() => void confirmClear()}
          />
        )}
      </AnimatePresence>
    </div>
    </TourProvider>
  )
}

/** 指南菜单必须在 TourProvider 内，才能调 start() */
function GuideTourButton({
  lang,
  onOpenManual
}: {
  lang: import('./i18n').UiLang
  onOpenManual: () => void
}) {
  const tour = useTour()
  return (
    <GuideMenu
      lang={lang}
      tourActive={tour.active}
      onStartTour={() => tour.start()}
      onOpenManual={onOpenManual}
    />
  )
}

/**
 * 引导进行中点工具栏「设置」必须进模型页，避免与 tour 的 openSettings('models')
 * 竞态成 general，导致 models 步 waitFor 失败被跳过。
 */
function ToolbarSettingsButton({
  label,
  openSettings
}: {
  label: string
  openSettings: (tab?: SettingsTab) => Promise<void>
}) {
  const tour = useTour()
  return (
    <button
      className="btn btn-ghost"
      data-shot="settings"
      data-tour="toolbar-settings"
      onClick={() => void openSettings(tour.active ? 'models' : 'general')}
    >
      <span className="btn-glyph i-gear" aria-hidden />
      {label}
    </button>
  )
}
