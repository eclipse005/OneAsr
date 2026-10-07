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
        let pinned = super::device::device_pinned();
        // 钉卡走 `load_on`：只开那一张，失败不改去另一张，也不在 crate 里退 CPU。
        // 未钉卡的 `gpu` 是 `Backend::Gpu`（先独显再集显，打不开就报错）。
        // 未钉卡的 `auto` 是 `Backend::Auto`（同一顺序，都失败才用 CPU）。
        let inner = match (backend, pinned) {
            (ComputeBackend::Cpu, _) => AsrInference::load(model_dir, AsrBackend::Cpu),
            (ComputeBackend::Gpu, false) => AsrInference::load(model_dir, AsrBackend::Gpu),
            (ComputeBackend::Auto, false) => AsrInference::load(model_dir, AsrBackend::Auto),
            (ComputeBackend::Gpu | ComputeBackend::Auto, true) => {
                AsrInference::load_on(model_dir, super::device::asr_selector_from_env()?)
            }
        };
        inner
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }

    pub(super) fn device_description(&self) -> String {
        self.inner.device_description()
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
