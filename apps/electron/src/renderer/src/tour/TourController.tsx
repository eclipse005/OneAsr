import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState
} from 'react'
import { AnimatePresence } from 'framer-motion'
import { t, type UiLang } from '../i18n'
import { useEscClose } from '../esc'
import { TourOverlay } from './TourOverlay'
import { WelcomeModal } from './WelcomeModal'
import { visibleSteps } from './steps'
import {
  markTourCompleted,
  markTourSkipped,
  readTourPrefs,
  shouldShowWelcome,
  TOUR_STORAGE_KEY
} from './storage'
import { waitForSelector } from './wait'
import type { TourStep, TourStepApi, TourStepId } from './types'
import { tourEscClosesSettings } from './tourEsc'

export type TourHostApi = TourStepApi & {
  modelsReady: boolean
  hasTasks: boolean
  settingsOpen: boolean
}

type TourApi = {
  start: () => void
  stop: (reason: 'skip' | 'complete') => void
  active: boolean
  welcomeOpen: boolean
}

const TourCtx = createContext<TourApi | null>(null)

export function useTour(): TourApi {
  const ctx = useContext(TourCtx)
  if (!ctx) throw new Error('useTour outside TourProvider')
  return ctx
}

/** 可选：App 工具栏菜单用，Provider 外返回 stub */
export function useTourOptional(): TourApi | null {
  return useContext(TourCtx)
}

function liveCtxFrom(host: TourHostApi) {
  return {
    modelsReady: host.modelsReady,
    hasTasks: host.hasTasks,
    settingsOpen: host.settingsOpen
  }
}

export function TourProvider({
  lang,
  host,
  children,
  /** 等 status 首包回来再决定要不要欢迎卡，避免闪一下又关 */
  statusReady
}: {
  lang: UiLang
  host: TourHostApi
  children: React.ReactNode
  statusReady: boolean
}) {
  const s = t(lang)
  const [welcomeOpen, setWelcomeOpen] = useState(false)
  const [running, setRunning] = useState(false)
  const [stepId, setStepId] = useState<TourStepId | null>(null)
  const stepIdRef = useRef(stepId)
  stepIdRef.current = stepId
  const advancing = useRef(false)
  const hostRef = useRef(host)
  hostRef.current = host

  const ctx = useMemo(() => liveCtxFrom(host), [
    host.modelsReady,
    host.hasTasks,
    host.settingsOpen
  ])

  const steps = useMemo(() => visibleSteps(ctx), [ctx])
  const stepIndex = stepId ? steps.findIndex((x) => x.id === stepId) : -1
  const step: TourStep | null = stepIndex >= 0 ? steps[stepIndex]! : null

  // 首启欢迎卡
  useEffect(() => {
    if (!statusReady) return
    if (running) return
    if (shouldShowWelcome(readTourPrefs())) {
      setWelcomeOpen(true)
    }
  }, [statusReady, running])

  const stop = useCallback((reason: 'skip' | 'complete') => {
    setRunning(false)
    setStepId(null)
    setWelcomeOpen(false)
    if (reason === 'complete') markTourCompleted()
    else markTourSkipped()
  }, [])

  const hostApi = useCallback((): TourStepApi => {
    return {
      openSettings: (tab) => hostRef.current.openSettings(tab),
      closeSettings: () => hostRef.current.closeSettings(),
      closeGuide: () => hostRef.current.closeGuide(),
      openStats: () => hostRef.current.openStats(),
      closeStats: () => hostRef.current.closeStats()
    }
  }, [])

  const goToIndex = useCallback(
    async (index: number) => {
      const list = visibleSteps(liveCtxFrom(hostRef.current))
      if (index >= list.length) {
        stop('complete')
        return
      }
      if (index < 0) return
      const next = list[index]!
      const api = hostApi()
      const live = liveCtxFrom(hostRef.current)
      if (next.before) {
        const ok = await next.before(live, api)
        if (ok === false) {
          await goToIndex(index + 1)
          return
        }
      }
      // 设置面板步：再强制一次对应 tab，压过工具栏 general 竞态 / 标签点击时序
      const paneTab =
        next.id === 'settings-models'
          ? 'models'
          : next.id === 'settings-hotwords'
            ? 'hotwords'
            : next.id === 'settings-output'
              ? 'output'
              : null
      if (paneTab) {
        await api.openSettings(paneTab)
      }
      const waitSel = next.waitFor ?? next.selector
      if (waitSel) {
        let found = await waitForSelector(waitSel, next.waitTimeoutMs ?? 8000)
        // 面板步：超时则再开一次对应 tab 再等，避免竞态后误跳过（热词→输出）
        if (!found && paneTab) {
          await api.openSettings(paneTab)
          found = await waitForSelector(waitSel, 4000)
        }
        if (!found) {
          // 选择器没出现就跳过这一步，别卡死（面板步仅在真等不到时才跳）
          await goToIndex(index + 1)
          return
        }
      }
      setStepId(next.id)
    },
    [stop, hostApi]
  )

  const advance = useCallback(async () => {
    if (advancing.current) return
    advancing.current = true
    try {
      const list = visibleSteps(liveCtxFrom(hostRef.current))
      const id = stepIdRef.current
      const cur = id ? list.findIndex((x) => x.id === id) : -1
      if (cur < 0) {
        stop('complete')
        return
      }
      if (list[cur]?.id === 'done') {
        stop('complete')
        return
      }
      await goToIndex(cur + 1)
    } finally {
      advancing.current = false
    }
  }, [goToIndex, stop])

  const onTargetClick = useCallback(() => {
    void advance()
  }, [advance])

  const start = useCallback(() => {
    setWelcomeOpen(false)
    setRunning(true)
    void goToIndex(0)
  }, [goToIndex])

  // Esc：设置步内先关设置（effect 会跳到 toolbar-add）；其它步才退出引导。
  // 用稳定 callback + ref，避免每次 render 重注册把 Settings 的 Esc 挤出栈顶。
  const onTourEsc = useCallback(() => {
    if (tourEscClosesSettings(stepIdRef.current, hostRef.current.settingsOpen)) {
      hostRef.current.closeSettings()
      return
    }
    stop('skip')
  }, [stop])
  useEscClose(onTourEsc, running || welcomeOpen)

  // 设置被 Esc/意外关掉时：直接跳到设置块之后，避免 click-only 孤儿气泡
  // （不要逐步 advance：后面几步 waitFor 会各空等数秒）
  useEffect(() => {
    if (!running || !stepId) return
    if (host.settingsOpen) return
    if (!stepId.startsWith('settings-')) return
    if (advancing.current) return
    advancing.current = true
    void (async () => {
      try {
        const list = visibleSteps(liveCtxFrom(hostRef.current))
        const after = list.findIndex((x) => x.id === 'toolbar-add')
        if (after >= 0) await goToIndex(after)
        else {
          const cur = list.findIndex((x) => x.id === stepId)
          await goToIndex(cur >= 0 ? cur + 1 : 0)
        }
      } finally {
        advancing.current = false
      }
    })()
  }, [running, stepId, host.settingsOpen, goToIndex])

  // 任务列表从空变有（用户在引导中加了文件）时，重算可见步骤仍停在当前 id
  useEffect(() => {
    if (!running || !stepId) return
    const list = visibleSteps(ctx)
    if (!list.some((x) => x.id === stepId)) {
      // 当前步因条件消失（例如加了任务后 dropzone 没了）→ 进下一步
      const fallback = list.findIndex((x) =>
        ['toolbar-add', 'task-lang', 'toolbar-stats', 'toolbar-guide', 'done'].includes(x.id)
      )
      void goToIndex(fallback >= 0 ? fallback : 0)
    }
  }, [ctx, running, stepId, goToIndex])

  const copy = (key: string): string => {
    const dict = s as Record<string, string>
    return dict[key] ?? key
  }

  const api = useMemo<TourApi>(
    () => ({
      start,
      stop,
      active: running,
      welcomeOpen
    }),
    [start, stop, running, welcomeOpen]
  )

  // 测试/调试钩子
  useEffect(() => {
    ;(window as unknown as { __oneasrTour?: { start: () => void; key: string } }).__oneasrTour = {
      start,
      key: TOUR_STORAGE_KEY
    }
    return () => {
      delete (window as unknown as { __oneasrTour?: unknown }).__oneasrTour
    }
  }, [start])

  return (
    <TourCtx.Provider value={api}>
      {children}
      <AnimatePresence>
        {welcomeOpen && !running && (
          <WelcomeModal
            title={s.tourWelcomeTitle}
            body={s.tourWelcomeBody}
            startLabel={s.tourStart}
            skipLabel={s.tourSkip}
            onStart={start}
            onSkip={() => stop('skip')}
          />
        )}
      </AnimatePresence>
      <TourOverlay
        active={running && !!step}
        selector={step?.selector ?? null}
        title={
          step
            ? copy(typeof step.titleKey === 'function' ? step.titleKey(ctx) : step.titleKey)
            : ''
        }
        body={
          step ? copy(typeof step.bodyKey === 'function' ? step.bodyKey(ctx) : step.bodyKey) : ''
        }
        stepIndex={Math.max(0, stepIndex)}
        stepTotal={Math.max(1, steps.length)}
        advance={step?.advance ?? 'next'}
        nextLabel={s.tourNext}
        skipStepLabel={s.tourSkipStep}
        skipLabel={s.tourSkip}
        doneLabel={s.tourFinish}
        clickHint={s.tourClickHint}
        clickOrNextHint={s.tourClickOrNextHint}
        skipWaitLabel={s.tourSkipWait}
        showSkipWait={!!step?.allowSkipWait}
        isLast={step?.id === 'done'}
        onNext={() => void advance()}
        onSkip={() => stop('skip')}
        onSkipWait={() => void advance()}
        onTargetClick={onTargetClick}
      />
    </TourCtx.Provider>
  )
}
