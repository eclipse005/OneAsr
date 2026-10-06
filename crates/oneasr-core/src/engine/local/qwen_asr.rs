//! Qwen3-ASR adapter (wgpu).

use super::backend::ComputeBackend;
use std::path::Path;

use qwen3_asr_wgpu::{AsrInference, Backend as AsrBackend, TranscribeOptions};

use crate::engine::{AsrEngine, EngineError, TranscribeRequest, Transcript};

pub(super) struct QwenAsrAdapter {
    inner: AsrInference,
}

impl QwenAsrAdapter {
    pub(super) fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        let backend = match backend {
            ComputeBackend::Cpu => AsrBackend::Cpu,
            ComputeBackend::Gpu => AsrBackend::Gpu,
        };
        // GPU + `ONEASR_DEVICE` → 用户指定的适配器（如独显）；spec 非法直接报错。
        let inner = if backend == AsrBackend::Gpu {
            AsrInference::load_on(model_dir, super::device::asr_selector_from_env()?)
        } else {
            AsrInference::load(model_dir, backend)
        };
        inner
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}

impl AsrEngine for QwenAsrAdapter {
    fn transcribe(&self, req: TranscribeRequest<'_>) -> Result<Transcript, EngineError> {
        let path = req
            .wav
            .to_str()
            .ok_or_else(|| EngineError::new(crate::i18n::audio_path_not_utf8()))?;
        let opts = TranscribeOptions::default()
            .with_max_new_tokens(req.max_new_tokens)
            .with_language(req.language.to_string());
        self.inner
            .transcribe(path, opts)
            .map(|report| Transcript { text: report.text })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}
