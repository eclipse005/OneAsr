//! Qwen3-ForcedAligner adapter (wgpu).

use std::path::Path;
use std::sync::Mutex;

use super::backend::ComputeBackend;

use qwen3_aligner_wgpu::align_inference::Aligner as WgpuAligner;
use qwen3_aligner_wgpu::gpu::DeviceSelector;

use crate::engine::{
    AlignRequest, AlignedToken, Aligner, EngineError,
};

pub(super) struct QwenAlignerAdapter {
    /// wgpu `align` takes `&mut self`; the pipeline trait is `&self`.
    inner: Mutex<WgpuAligner>,
}

impl QwenAlignerAdapter {
    pub(super) fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        let selector = match backend {
            ComputeBackend::Cpu => DeviceSelector::Cpu,
            ComputeBackend::Gpu => DeviceSelector::Auto,
        };
        WgpuAligner::load(selector, model_dir)
            .map(|inner| Self {
                inner: Mutex::new(inner),
            })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}

impl Aligner for QwenAlignerAdapter {
    fn align(&self, req: AlignRequest<'_>) -> Result<Vec<AlignedToken>, EngineError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| EngineError::new("对齐器被占用"))?;
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
