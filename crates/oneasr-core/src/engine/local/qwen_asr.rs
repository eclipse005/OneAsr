//! Qwen3-ASR adapter.

use std::path::Path;
use super::cuda::ComputeBackend;

use qwen3_asr::{AsrInference, Backend as AsrBackend, TranscribeOptions};

use crate::engine::{
    AsrEngine, EngineError, TranscribeRequest, Transcript,
};
pub(super) struct QwenAsrAdapter {
    inner: AsrInference,
}

impl QwenAsrAdapter {
    pub(super) fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        let backend = match backend {
            ComputeBackend::Cpu => AsrBackend::Cpu,
            ComputeBackend::Cuda => AsrBackend::Cuda,
        };
        AsrInference::load(model_dir, backend)
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}

impl AsrEngine for QwenAsrAdapter {
    fn transcribe(&self, req: TranscribeRequest<'_>) -> Result<Transcript, EngineError> {
        let path = req
            .wav
            .to_str()
            .ok_or_else(|| EngineError::new("音频路径非 UTF-8"))?;
        let opts = TranscribeOptions::default()
            .with_max_new_tokens(req.max_new_tokens)
            .with_language(req.language.to_string());
        self.inner
            .transcribe(path, opts)
            .map(|report| Transcript { text: report.text })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}
