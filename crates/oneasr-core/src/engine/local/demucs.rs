//! HTDemucs vocal separation adapter + its PCM I/O.

use std::path::{Path, PathBuf};
use std::cell::Cell;

use super::backend::ComputeBackend;
use crate::engine::{
    EngineError, SeparateRequest,
    SeparationEvent, Separator,
};

/// Weights file expected inside the configured Demucs model directory.
/// Vocals-only shard of `htdemucs_ft`, loaded as a single FourStem network.
pub const DEMUCS_WEIGHTS_FILE: &str = "htdemucs_ft_vocals.safetensors";

pub(super) fn separator_backend(backend: ComputeBackend) -> demucs_core::Backend {
    match backend {
        ComputeBackend::Cpu => demucs_core::Backend::Cpu,
        ComputeBackend::Gpu => {
            demucs_core::Backend::Gpu(demucs_core::gpu::DeviceSelector::Auto)
        }
    }
}

pub(super) struct DemucsSeparatorAdapter {
    inner: demucs_core::Demucs,
    /// Set when GPU load had to fall back to CPU — reported once, on the first run.
    fell_back: Cell<bool>,
}

impl DemucsSeparatorAdapter {
    pub(super) fn load(
        model_dir: &Path,
        backend: ComputeBackend,
        forced_gpu: bool,
        log: impl Fn(String),
    ) -> Result<Self, EngineError> {
        use demucs_core::{Demucs, LoadOptions, ModelVariant, StemId, StemSelection};

        let weights = model_dir.join(DEMUCS_WEIGHTS_FILE);
        if !weights.is_file() {
            return Err(EngineError::new(crate::i18n::demucs_model_missing(&weights)));
        }

        let opts = LoadOptions {
            // Single-stem shard, not the four-network `htdemucs_ft` bag.
            variant: ModelVariant::FourStem,
            stems: StemSelection::Some(vec![StemId::Vocals]),
        };
        let attempt = separator_backend(backend);
        log(format!("vocal-separation backend {}", attempt.tag()));

        let (inner, fell_back) = match Demucs::load(&weights, opts.clone(), attempt.clone()) {
            Ok(inner) => (inner, false),
            Err(e) if backend == ComputeBackend::Gpu && !forced_gpu => {
                log(crate::i18n::sep_gpu_load_failed_cpu(&e.to_string()));
                let inner = Demucs::load(&weights, opts, demucs_core::Backend::Cpu).map_err(
                    |cpu_e| {
                        EngineError::new(crate::i18n::sep_load_failed("cpu", &cpu_e.to_string()))
                    },
                )?;
                (inner, true)
            }
            Err(e) => {
                return Err(EngineError::new(crate::i18n::sep_load_failed(
                    attempt.tag(),
                    &e.to_string(),
                )));
            }
        };
        log(format!("vocal separation loaded {}", inner.backend_tag()));

        Ok(Self {
            inner,
            fell_back: Cell::new(fell_back),
        })
    }
}

impl Separator for DemucsSeparatorAdapter {
    fn separate(
        &self,
        req: SeparateRequest<'_>,
        on_event: &mut dyn FnMut(SeparationEvent),
    ) -> Result<PathBuf, EngineError> {
        use demucs_core::{SeparationProgress, StemId};

        if self.fell_back.replace(false) {
            on_event(SeparationEvent::FellBackToCpu {
                reason: crate::i18n::sep_gpu_load_failed_reason().into(),
            });
        }

        let source_wav =
            crate::media::extract_audio_wav(req.input, &req.out_dir.join("separate_input.wav"))
                .map_err(|e| EngineError::new(format!("{e}")))?;
        let (left, right, sample_rate) = read_stereo_f32(&source_wav).map_err(EngineError::new)?;

        let stems = self
            .inner
            .separate_with_progress(&left, &right, sample_rate, &mut |p: SeparationProgress| {
                on_event(SeparationEvent::Progress {
                    done: p.done,
                    total: p.total,
                });
            })
            .map_err(|e| EngineError::new(crate::i18n::sep_failed(&e.to_string())))?;

        let vocals = stems
            .iter()
            .find(|s| s.id == StemId::Vocals)
            .ok_or_else(|| EngineError::new(crate::i18n::sep_no_vocals()))?;
        let out = req.out_dir.join("vocals.wav");
        write_stereo_f32(&out, &vocals.left, &vocals.right, sample_rate)
            .map_err(EngineError::new)?;
        Ok(out)
    }
}

/// Read a WAV as two f32 channels (mono was duplicated by ffmpeg).
fn read_stereo_f32(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32), String> {
    use hound::SampleFormat;

    let mut reader = hound::WavReader::open(path)
        .map_err(|e| crate::i18n::sep_read_input_failed(path, &e.to_string()))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err(crate::i18n::sep_no_channels().into());
    }
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::i18n::sep_read_input_failed_bare(&e.to_string()))?,
        SampleFormat::Int => {
            // ffmpeg wrote PCM s16le; stay tolerant of other integer widths.
            let bits = spec.bits_per_sample.max(1);
            let scale = (1i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| crate::i18n::sep_read_input_failed_bare(&e.to_string()))?
        }
    };

    let channels = spec.channels as usize;
    let mut left = Vec::with_capacity(samples.len() / channels);
    let mut right = Vec::with_capacity(samples.len() / channels);
    for frame in samples.chunks_exact(channels) {
        left.push(frame[0]);
        right.push(frame[channels - 1]);
    }
    Ok((left, right, spec.sample_rate))
}

fn write_stereo_f32(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
) -> Result<(), String> {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)
        .map_err(|e| crate::i18n::sep_write_failed(path, &e.to_string()))?;
    for (l, r) in left.iter().zip(right.iter()) {
        writer
            .write_sample(*l)
            .and_then(|_| writer.write_sample(*r))
            .map_err(|e| crate::i18n::sep_write_failed_bare(&e.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|e| crate::i18n::sep_write_failed_bare(&e.to_string()))?;
    Ok(())
}
