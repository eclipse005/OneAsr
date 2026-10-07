//! Qwen3-ForcedAligner adapter (wgpu).

use std::path::Path;
use std::sync::Mutex;

use super::backend::ComputeBackend;

use qwen3_aligner_wgpu::align_inference::Aligner as WgpuAligner;
use qwen3_aligner_wgpu::gpu::Backend as AlignerBackend;

use crate::engine::{AlignProgress, AlignRequest, AlignedToken, Aligner, EngineError};

pub(super) struct QwenAlignerAdapter {
    /// wgpu `align` takes `&mut self`; the pipeline trait is `&self`.
    inner: Mutex<WgpuAligner>,
}

impl QwenAlignerAdapter {
    pub(super) fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        // `load(Auto)` 会退 CPU。要求显卡时必须走 `Backend::Gpu`，未钉卡的
        // `Auto` 才把独显→集显→CPU 交给 crate。
        let policy = match backend {
            ComputeBackend::Cpu => AlignerBackend::Cpu,
            ComputeBackend::Gpu => AlignerBackend::Gpu(super::device::aligner_selector_from_env()?),
            ComputeBackend::Auto if super::device::device_pinned() => {
                AlignerBackend::Gpu(super::device::aligner_selector_from_env()?)
            }
            ComputeBackend::Auto => AlignerBackend::Auto,
        };
        WgpuAligner::load_backend(policy, model_dir)
            .map(|inner| Self {
                inner: Mutex::new(inner),
            })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }

    pub(super) fn backend_name(&self) -> &'static str {
        self.inner
            .lock()
            .map(|inner| inner.backend_name())
            .unwrap_or("unknown")
    }

    pub(super) fn device_desc(&self) -> String {
        self.inner
            .lock()
            .map(|inner| inner.describe())
            .unwrap_or_else(|_| "unavailable".into())
    }
}

impl Aligner for QwenAlignerAdapter {
    /// 忽略 `on_progress`：这个对齐器一次算完一整段，没有分母可报。上层拿到
    /// `None`，界面显示「打轴中」而不画条——给一条不动的条比没有条更糟。
    fn align(
        &self,
        req: AlignRequest<'_>,
        _on_progress: Option<AlignProgress<'_>>,
    ) -> Result<Vec<AlignedToken>, EngineError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| EngineError::new(crate::i18n::aligner_busy()))?;
        let items = inner
            .align(req.wav, req.text, Some(req.language))
            .map_err(|e| EngineError::new(format!("{e:#}")))?;
        Ok(items
            .into_iter()
            .map(|item| AlignedToken {
                text: item.text,
                start_sec: item.start_time,
                end_sec: item.end_time,
            })
            .collect())
    }
}
