//! The concrete [`EngineProvider`] the pipeline is handed.

use std::path::PathBuf;
use std::cell::Cell;
use super::aligner::QwenAlignerAdapter;
use super::backend::{ComputeBackend, gpu_load_failure_msg, is_forced_gpu, resolve_compute_backend};
use super::demucs::DemucsSeparatorAdapter;
use super::qwen_asr::QwenAsrAdapter;

#[cfg(debug_assertions)]
use crate::diagnostics::pipeline_trace;
use crate::diagnostics::trace_log;
use crate::engine::{
    Aligner, AsrEngine, EngineError, EngineProvider, Separator,
};
use crate::settings::Settings;

/// Real engines backed by local weights, with the product's backend policy.
///
/// The resolved backend is remembered after the ASR load: when a GPU load
/// falls back to CPU, the aligner must follow (one policy per run).
pub struct LocalEngineProvider {
    asr_model_dir: PathBuf,
    aligner_model_dir: PathBuf,
    demucs_model_dir: PathBuf,
    backend_pref: String,
    resolved: Cell<ComputeBackend>,
}

impl LocalEngineProvider {
    /// Resolve the compute backend for this run (cheap; no weights are read).
    pub fn from_settings(settings: &Settings) -> Result<Self, EngineError> {
        let resolved = resolve_compute_backend(&settings.backend)?;
        let gpu = match super::backend::probe_gpu_device() {
            Ok(p) => p.description.as_str(),
            Err(_) => "none",
        };
        let summary = format!(
            "setting={} resolved={} gpu={gpu}",
            settings.backend,
            resolved.label(),
        );
        trace_log(format!("backend {summary}"));
        #[cfg(debug_assertions)]
        if !pipeline_trace() {
            eprintln!("[backend] {summary}");
        }
        Ok(Self {
            asr_model_dir: settings.asr_model_dir.clone(),
            aligner_model_dir: settings.aligner_model_dir.clone(),
            demucs_model_dir: settings.resolved_demucs_model_dir(),
            backend_pref: settings.backend.clone(),
            resolved: Cell::new(resolved),
        })
    }

    /// Backend currently in use (after any fallback).
    pub fn resolved_backend(&self) -> &'static str {
        self.resolved.get().label()
    }

    fn forced_gpu(&self) -> bool {
        is_forced_gpu(&self.backend_pref)
    }
}

impl EngineProvider for LocalEngineProvider {
    fn load_asr(&self) -> Result<Box<dyn AsrEngine>, EngineError> {
        let dir = &self.asr_model_dir;
        let first = self.resolved.get();
        match QwenAsrAdapter::load(dir, first) {
            Ok(engine) => Ok(Box::new(engine)),
            Err(e) if first == ComputeBackend::Gpu && !self.forced_gpu() => {
                trace_log(format!("gpu load failed, falling back to cpu: {e}"));
                self.resolved.set(ComputeBackend::Cpu);
                QwenAsrAdapter::load(dir, ComputeBackend::Cpu)
                    .map(|engine| Box::new(engine) as Box<dyn AsrEngine>)
            }
            Err(e) if first == ComputeBackend::Gpu => {
                Err(EngineError::new(gpu_load_failure_msg(&e)))
            }
            Err(e) => Err(e),
        }
    }

    fn load_aligner(&self) -> Result<Box<dyn Aligner>, EngineError> {
        QwenAlignerAdapter::load(&self.aligner_model_dir, self.resolved.get())
            .map(|engine| Box::new(engine) as Box<dyn Aligner>)
    }

    fn load_separator(&self) -> Result<Box<dyn Separator>, EngineError> {
        DemucsSeparatorAdapter::load(
            &self.demucs_model_dir,
            self.resolved.get(),
            self.forced_gpu(),
            |msg| trace_log(msg),
        )
        .map(|engine| Box::new(engine) as Box<dyn Separator>)
    }
}
