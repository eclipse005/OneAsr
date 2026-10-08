//! CTC forced-alignment adapter (omniASR-CTC-300M-v2, wgpu)。
//!
//! 与 Qwen 对齐器并存的第二个引擎（设置里选择）。
//!
//! **这里不做任何分组**。crate 的 `AlignOutput::words` 已经是成品对齐单元：
//! 脚本不带空格的（中日韩、假名、谚文）逐字，脚本带空格的整词，标点零时长
//! 搭在前面的单元上，`space_before` 带的是文稿自己的空格。判断在 crate 的
//! `spans::unit_runs` 里，一处回答，`words` 与它自己的断句器读的是同一份。
//!
//! 别在这里补分组。`AlignOutput::tokens` 是另一个视图：逐 CTC target，而这个
//! checkpoint 的词表是字符级的，所以**每个脚本**都是逐字符的。上层曾经直接吃
//! `tokens`，而空格是在 `sentence_boundary::util::join_tokens` 里逐 token 插的，
//! 于是 `Whisper` 被渲染成 `W h i s p e r`、`AI` 变成 `A I`。要更细的粒度
//! （例如卡拉OK 逐字）时读 `tokens`，那是刻意的选择，不是漏了分组。
//!
//! `language` 参数 CTC 不需要（脚本判定逐段做，不查语言表），忽略。

use std::path::Path;
use std::sync::Mutex;

use super::backend::ComputeBackend;
use ctc_forced_aligner_wgpu::{
    AlignOutput, Aligner as CtcAligner, Backend as CtcBackend, Progress,
};

use crate::engine::{AlignProgress, AlignRequest, AlignedToken, Aligner, EngineError};

pub(super) struct CtcAlignerAdapter {
    inner: CtcAligner,
}

impl CtcAlignerAdapter {
    pub(super) fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        // `load_on(Auto)` 仍会在没有卡时退 CPU。要求显卡用 `Backend::Gpu`。
        let policy = match backend {
            ComputeBackend::Cpu => CtcBackend::Cpu,
            ComputeBackend::Gpu => CtcBackend::Gpu(super::device::ctc_selector_from_env()?),
            ComputeBackend::Auto if super::device::device_pinned() => {
                CtcBackend::Gpu(super::device::ctc_selector_from_env()?)
            }
            ComputeBackend::Auto => CtcBackend::Auto,
        };
        CtcAligner::load_with(model_dir, policy)
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }

    pub(super) fn backend_name(&self) -> &'static str {
        self.inner.backend_name()
    }

    pub(super) fn device_desc(&self) -> String {
        self.inner.device_desc()
    }
}

impl Aligner for CtcAlignerAdapter {
    /// 把上游的「整条 run 进度」搬进我们自己的单线程 `FnMut` 契约。
    ///
    /// 上游现在把 `Progress`（阶段 + 整条 run 的 done/total）交给一个
    /// `Fn(Progress) + Send + Sync` 的 sink，而且**可能在自己的编码线程或 DP
    /// worker 上触发**——不再保证在调用方线程上。契约要的是「单线程的
    /// `&mut FnMut`」，所以把回调放进一个锁后面：上游拿到的那个闭包只做
    /// 「加锁 → 调一次 → 解锁」，于是并发调用自动串行，调用方的独占语义不变。
    ///
    /// 上游明确说它在回调期间持有自己的进度锁，并要求回调「不要回头等调用方」，
    /// 所以这里绝不能反调 align、也不能阻塞——一次 `Mutex::lock` 之后直接调用。
    /// 顺带的好处：回调被串行化之后，`done` 到达我们的顺序就是上游发出它们的
    /// 顺序，界面上不会看到进度条往回跳。
    fn align(
        &self,
        req: AlignRequest<'_>,
        on_progress: Option<AlignProgress<'_>>,
    ) -> Result<Vec<AlignedToken>, EngineError> {
        let out = match on_progress {
            None => self.align_inner(req, None)?,
            Some(sink) => {
                // 引擎可能从别的线程回调它，所以它必须能过去。
                let guarded: Mutex<&mut (dyn FnMut(usize, usize) + Send)> = Mutex::new(sink);
                self.align_inner(
                    req,
                    Some(&|tick: Progress| {
                        // 毒化的锁说明上一次回调 panic 过；把守卫取回来继续，
                        // 而不是让整次打轴死在别人的 panic 上。
                        let mut guard = guarded.lock().unwrap_or_else(|e| e.into_inner());
                        guard(tick.done, tick.total);
                    }),
                )?
            }
        };
        Ok(out
            .words
            .into_iter()
            .map(|w| AlignedToken {
                text: w.text,
                start_sec: w.start,
                end_sec: w.end,
            })
            .collect())
    }
}

impl CtcAlignerAdapter {
    fn align_inner(
        &self,
        req: AlignRequest<'_>,
        on_progress: Option<&(dyn Fn(Progress) + Send + Sync)>,
    ) -> Result<AlignOutput, EngineError> {
        // window 30s + context 2s：crate 实测的内存/吞吐旋钮，跨合法区间
        // 不移动任何时间戳指标（见其 README）；None 才是整文件一次前向。
        const WINDOW_SEC: f64 = 30.0;
        const CONTEXT_SEC: f64 = 2.0;
        self.inner
            .align(
                req.wav,
                req.text,
                Some(WINDOW_SEC),
                CONTEXT_SEC,
                on_progress,
            )
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}
