import { AnimatePresence, motion } from 'framer-motion'
import { t, UiLang } from '../i18n'
import { useLayerMotion } from '../motion'

import type { StatusSnapshot, TaskSnapshot } from '../../../shared/types'

export function StatusBar({
  lang,
  status,
  tasks,
  message,
  onCopyMessage,
  onOpenModels,
  cliOutdated = false
}: {
  lang: UiLang
  /** CLI 的 status --json 快照：就绪、门禁、后端全在里面，壳只显示 */
  status: StatusSnapshot | null
  tasks: TaskSnapshot[]
  /** 点「模型就绪 / 未就绪」直接进设置 → 模型（用户要修模型时不必自己找） */
  onOpenModels?: () => void
  /** 最近一条提示：toast 会消失，这里留 3~6 秒并且可点击复制（长路径/报错不至于看了就丢） */
  message?: { kind: string; text: string } | null
  onCopyMessage?: (text: string) => void
  /** CLI 缺 status/transcript 时优先提示更新引擎，不要把 help 碎片当错误 */
  cliOutdated?: boolean
}) {
  const s = t(lang)
  const chipMotion = useLayerMotion()
  // 就绪与否 = Rust 的 can_start（含「只导 TXT 就不要求对齐」这类细则），失败时 title 给原因
  const ready = !cliOutdated && !!status?.can_start.ok
  const notReadyReason = cliOutdated ? s.cliOutdated : (status?.can_start.reason ?? undefined)
  // 模型短名不铺在状态栏上；悬停「模型就绪」能看到，设置 → 模型里也有
  const detail = [
    notReadyReason,
    `${s.asr}: ${status?.models.asr.ready ? '✓' : '✗'} ${status?.models.asr.id ?? ''}`.trim(),
    `${s.aligner}: ${status?.models.aligner.ready ? '✓' : '✗'} ${status?.models.aligner.id ?? ''}`.trim(),
    `${s.demucs}: ${status?.models.demucs.ready ? '✓' : '—'}`,
    `${s.sensevoice}: ${status?.models.sensevoice.ready ? '✓' : '—'}`,
    `${s.backend}: ${status?.backend || '—'}`
  ]
    .filter((line): line is string => !!line)
    .join('\n')
  const running = tasks.filter((x) => x.status === 'running').length
  // 「排队」只算真正进了运行队列的（有 queueRank）；刚添加还没点开始的是「待处理」，行内有徽章
  const queued = tasks.filter((x) => x.status === 'queued' && x.queueRank != null).length
  const done = tasks.filter((x) => ['done', 'error', 'cancelled'].includes(x.status)).length
  return (
    <div className="statusbar">
      <span className={`dot ${ready ? 'ok' : ''}`} />
      <button
        type="button"
        className="status-link"
        style={{ color: ready ? 'var(--logo)' : 'var(--warn)' }}
        title={detail}
        data-tour="status-models"
        onClick={onOpenModels}
      >
        {cliOutdated ? s.cliOutdatedShort : ready ? s.modelsReady : s.modelsNotReady}
      </button>
      {(running > 0 || queued > 0) && (
        <>
          <span className="sep" />
          <span>
            {s.progress} {Math.min(done + 1, tasks.length)}/{tasks.length} · {s.running} {running} ·{' '}
            {s.queued} {queued}
          </span>
        </>
      )}
      {/* 提示贴着右下角：左侧是"环境信息"，右侧是"最近发生的事" */}
      <span className="grow" />
      <AnimatePresence>
        {message && (
          <motion.span key={message.text} className="status-chip-slot" {...chipMotion}>
            <span className="sep" />
            <button
              type="button"
              className={`status-chip ${message.kind}`}
              title={message.text}
              onClick={() => onCopyMessage?.(message.text)}
            >
              {message.text}
            </button>
          </motion.span>
        )}
      </AnimatePresence>
    </div>
  )
}
