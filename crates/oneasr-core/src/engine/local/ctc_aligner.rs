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

use super::backend::ComputeBackend;
use ctc_forced_aligner_wgpu::{Aligner as CtcAligner, Backend as CtcBackend};

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
    /// 把 crate 的窗口进度原样交给上层：一小时音频按 30 s 窗口是 120 个刻度，
    /// 足够画一条不跳的条。分母由 crate 数出来（`div_ceil`），这里不重算——
    /// 上面猜的秒数没有下面数出来的窗口准。
    fn align(
        &self,
        req: AlignRequest<'_>,
        on_progress: Option<AlignProgress<'_>>,
    ) -> Result<Vec<AlignedToken>, EngineError> {
        // window 30s + context 2s：crate 实测的内存/吞吐旋钮，跨合法区间
        // 不移动任何时间戳指标（见其 README）；None 才是整文件一次前向。
        const WINDOW_SEC: f64 = 30.0;
        const CONTEXT_SEC: f64 = 2.0;
        let out = self
            .inner
            .align(
                req.wav,
                req.text,
                Some(WINDOW_SEC),
                CONTEXT_SEC,
                on_progress,
            )
            .map_err(|e| EngineError::new(format!("{e:#}")))?;
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
