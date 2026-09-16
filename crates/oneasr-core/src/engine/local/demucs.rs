//! HTDemucs vocal separation adapter + its PCM I/O.

use std::path::{Path, PathBuf};
use std::cell::Cell;
use super::cuda::is_forced_cuda;

use crate::engine::{
    EngineError, SeparateRequest,
    SeparationEvent, Separator,
};
/// Weights file expected inside the configured Demucs model directory.
pub const DEMUCS_WEIGHTS_FILE: &str = "htdemucs_ft.safetensors";

/// Backend for the first separation attempt.
///
/// `auto` becomes an explicit GPU attempt (when this build has CUDA) so the
/// fallback is observable and reported, instead of being hidden inside the
/// engine's own `Auto` probing.
pub(super) fn preferred_separator_backend(pref: &str) -> demucs_core_native::Backend {
    use demucs_core_native::Backend;
    match pref.trim().to_ascii_lowercase().as_str() {
        "cpu" => Backend::Cpu,
        #[cfg(feature = "cuda")]
        _ => Backend::Cuda,
        #[cfg(not(feature = "cuda"))]
        _ => Backend::Cpu,
    }
}

pub(super) struct DemucsSeparatorAdapter {
    inner: demucs_core_native::Demucs,
    /// Set when `auto` had to fall back to CPU — reported once, on the first run.
    fell_back: Cell<bool>,
}

impl DemucsSeparatorAdapter {
    pub(super) fn load(
        model_dir: &Path,
        backend_pref: &str,
        log: impl Fn(String),
    ) -> Result<Self, EngineError> {
        use demucs_core_native::{Demucs, LoadOptions, ModelVariant, StemId, StemSelection};

        let weights = model_dir.join(DEMUCS_WEIGHTS_FILE);
        if !weights.is_file() {
            return Err(EngineError::new(format!(
                "人声分离模型不存在: {}（请在设置中下载）",
                weights.display()
            )));
        }

        let opts = LoadOptions {
            variant: ModelVariant::FineTuned,
            stems: StemSelection::Some(vec![StemId::Vocals]),
        };
        let attempt = preferred_separator_backend(backend_pref);
        let mut fell_back = false;

        let inner = match Demucs::load(&weights, opts.clone(), attempt) {
            Ok(d) => d,
            Err(e)
                if attempt != demucs_core_native::Backend::Cpu && !is_forced_cuda(backend_pref) =>
            {
                log(format!(
                    "人声分离 GPU 不可用（{e}），按「自动」改用 CPU（会慢很多）"
                ));
                fell_back = true;
                Demucs::load(&weights, opts, demucs_core_native::Backend::Cpu)
                    .map_err(|e2| EngineError::new(format!("加载人声分离模型失败（cpu）: {e2}")))?
            }
            Err(e) if is_forced_cuda(backend_pref) => {
                return Err(EngineError::new(format!(
                    "加载人声分离模型失败（{}）: {e}\n\
                     已按设置强制使用 GPU，不会自动改用 CPU；\
                     可在设置中把「推理后端」改为「自动」或「CPU」",
                    attempt.tag()
                )));
            }
            Err(e) => {
                return Err(EngineError::new(format!(
                    "加载人声分离模型失败（{}）: {e}",
                    attempt.tag()
                )));
            }
        };

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
        use demucs_core_native::{SeparationProgress, StemId};

        if self.fell_back.replace(false) {
            on_event(SeparationEvent::FellBackToCpu {
                reason: "GPU 加载失败".into(),
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
            .map_err(|e| EngineError::new(format!("人声分离失败: {e}")))?;

        let vocals = stems
            .iter()
            .find(|s| s.id == StemId::Vocals)
            .ok_or_else(|| EngineError::new("人声分离没有返回 vocals 轨道"))?;
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
        .map_err(|e| format!("读取分离输入失败 {}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err("分离输入没有声道".into());
    }
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("读取分离输入失败: {e}"))?,
        SampleFormat::Int => {
            // ffmpeg wrote PCM s16le; stay tolerant of other integer widths.
            let bits = spec.bits_per_sample.max(1);
            let scale = (1i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("读取分离输入失败: {e}"))?
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
        .map_err(|e| format!("写入人声轨道失败 {}: {e}", path.display()))?;
    for (l, r) in left.iter().zip(right.iter()) {
        writer
            .write_sample(*l)
            .and_then(|_| writer.write_sample(*r))
            .map_err(|e| format!("写入人声轨道失败: {e}"))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("写入人声轨道失败: {e}"))?;
    Ok(())
}
