//! Qwen3-ForcedAligner adapter.

use std::path::Path;
use super::cuda::ComputeBackend;

use qwen_forced_aligner_rs::{
    AlignRequest as QwenAlignRequest, AudioInput, DeviceRequest, ModelOptions, Qwen3ForcedAligner,
    TextInput,
};

use crate::engine::{
    AlignRequest, AlignedToken, Aligner, EngineError,
};
pub(super) struct QwenAlignerAdapter {
    inner: Qwen3ForcedAligner,
}

impl QwenAlignerAdapter {
    pub(super) fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        let device = match backend {
            ComputeBackend::Cpu => DeviceRequest::Cpu,
            ComputeBackend::Cuda => DeviceRequest::Cuda(0),
        };
        Qwen3ForcedAligner::load(model_dir, ModelOptions { device })
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}

impl Aligner for QwenAlignerAdapter {
    fn align(&self, req: AlignRequest<'_>) -> Result<Vec<AlignedToken>, EngineError> {
        let result = self
            .inner
            .align(QwenAlignRequest::new(
                AudioInput::Path(req.wav.to_path_buf()),
                TextInput::Text(req.text.to_string()),
                req.language.to_string(),
            ))
            .map_err(|e| EngineError::new(format!("{e:#}")))?;
        Ok(result
            .items
            .into_iter()
            .map(|item| AlignedToken {
                text: item.text,
                start_sec: item.start_time,
                end_sec: item.end_time,
            })
            .collect())
    }
}
