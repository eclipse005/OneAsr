//! Optional HTDemucs v4 vocal separation before VAD/ASR.
//!
//! Native Rust inference (`demucs-core-native`) — no `demucs.exe`, no Python.
//! The separated vocal stem replaces the pipeline's master WAV, so everything
//! downstream (VAD → ASR → aligner) sees music-suppressed audio. The model is
//! loaded, used, and dropped inside this stage, preserving the pipeline's
//! "one model resident at a time" rule.
//!
//! Backend policy (one rule for every engine in this crate):
//! - `auto` picks GPU when it works, otherwise CPU — and says so via
//!   [`SeparationEvent::FellBackToCpu`] so the UI can warn the user.
//! - `cuda` fails loudly; it never silently burns hours on CPU.
//! - `cpu` always runs on CPU.
//!
//! The fallback happens **only while loading** the model. A failure in the
//! middle of a long run is reported as an error instead of restarting the whole
//! file on CPU (which would look like a hang without any user-visible reason).
//!
//! GPU note: the CUDA backend loads precompiled multi-arch PTX from the crate,
//! so end users need only the driver + the CUDA runtime DLLs OneAsr ships.

use std::path::{Path, PathBuf};

use demucs_core_native::{
    Backend, Demucs, LoadOptions, ModelVariant, SeparationProgress, StemId, StemSelection,
};
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::asr::AsrError;
use crate::media;

/// Events raised while separating (progress + non-fatal backend decisions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeparationEvent {
    /// One finished inference chunk (1 unit = `TRAINING_LENGTH` of 44.1 kHz
    /// audio); always ends at `done == total`.
    Progress { done: usize, total: usize },
    /// `auto` could not use the GPU and continued on CPU.
    FellBackToCpu { reason: String },
}

/// Weights file expected inside the configured Demucs model directory.
pub const WEIGHTS_FILE: &str = "htdemucs_ft.safetensors";

/// Path of the HTDemucs weights for `model_dir`.
pub fn weights_path(model_dir: &Path) -> PathBuf {
    model_dir.join(WEIGHTS_FILE)
}

/// Separate `input` into a vocals-only WAV inside `work_dir`.
///
/// `backend_pref` is the settings backend id (`auto` | `cuda` | `cpu`).
/// `on_event` receives progress updates (so the UI can show「人声分离 n/N」)
/// and backend fallbacks.
/// Returns the vocals WAV path (original sample rate, stereo, f32).
pub fn separate_vocals(
    input: &Path,
    model_dir: &Path,
    work_dir: &Path,
    backend_pref: &str,
    on_event: &mut dyn FnMut(SeparationEvent),
) -> Result<PathBuf, AsrError> {
    let weights = weights_path(model_dir);
    if !weights.is_file() {
        return Err(AsrError::Other(format!(
            "人声分离模型不存在: {}（请在设置中下载）",
            weights.display()
        )));
    }

    let source_wav = media::extract_audio_wav(input, &work_dir.join("separate_input.wav"))?;
    let (left, right, sample_rate) = read_stereo_f32(&source_wav)?;

    let opts = LoadOptions {
        variant: ModelVariant::FineTuned,
        stems: StemSelection::Some(vec![StemId::Vocals]),
    };
    let demucs = load_with_policy(&weights, &opts, backend_pref, on_event)?;
    let stems = demucs
        .separate_with_progress(&left, &right, sample_rate, &mut |p: SeparationProgress| {
            on_event(SeparationEvent::Progress {
                done: p.done,
                total: p.total,
            });
        })
        .map_err(|e| AsrError::Other(format!("人声分离失败: {e}")))?;

    let vocals = stems
        .iter()
        .find(|s| s.id == StemId::Vocals)
        .ok_or_else(|| AsrError::Other("人声分离没有返回 vocals 轨道".into()))?;

    let out = work_dir.join("vocals.wav");
    write_stereo_f32(&out, &vocals.left, &vocals.right, sample_rate)?;
    Ok(out)
}

/// Load the model under the backend policy; the only place a CPU fallback may
/// happen (see the module docs).
fn load_with_policy(
    weights: &Path,
    opts: &LoadOptions,
    backend_pref: &str,
    on_event: &mut dyn FnMut(SeparationEvent),
) -> Result<Demucs, AsrError> {
    let attempt = preferred_backend(backend_pref);
    match load_once(weights, opts, attempt) {
        Ok(d) => Ok(d),
        // `auto` may fall back to CPU; an explicit GPU choice never does.
        Err(e) if attempt != Backend::Cpu && !is_forced_gpu(backend_pref) => {
            on_event(SeparationEvent::FellBackToCpu { reason: e.clone() });
            eprintln!("warning: 人声分离 GPU 不可用（{e}），按「自动」改用 CPU（会慢很多）");
            load_once(weights, opts, Backend::Cpu).map_err(AsrError::Other)
        }
        Err(e) if is_forced_gpu(backend_pref) => Err(AsrError::Other(format!(
            "{e}\n已按设置强制使用 GPU，不会自动改用 CPU；可在设置中把「推理后端」改为「自动」或「CPU」"
        ))),
        Err(e) => Err(AsrError::Other(e)),
    }
}

fn load_once(weights: &Path, opts: &LoadOptions, backend: Backend) -> Result<Demucs, String> {
    Demucs::load(weights, opts.clone(), backend)
        .map_err(|e| format!("加载人声分离模型失败（{}）: {e}", backend.tag()))
}

/// Backend for the first attempt.
///
/// `auto` resolves to an explicit GPU attempt (when this build has CUDA) so the
/// fallback is observable and reported, instead of being hidden inside the
/// engine's own `Auto` probing.
fn preferred_backend(pref: &str) -> Backend {
    match pref.trim().to_ascii_lowercase().as_str() {
        "cpu" => Backend::Cpu,
        #[cfg(feature = "cuda")]
        "cuda" => Backend::Cuda,
        #[cfg(feature = "cuda")]
        _ => Backend::Cuda,
        #[cfg(not(feature = "cuda"))]
        _ => Backend::Cpu,
    }
}

/// `backend = "cuda"` means the user explicitly picked GPU.
fn is_forced_gpu(backend_pref: &str) -> bool {
    backend_pref.trim().eq_ignore_ascii_case("cuda")
}

/// Read a WAV as two f32 channels (mono is duplicated to both sides).
fn read_stereo_f32(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32), AsrError> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| AsrError::Other(format!("读取分离输入失败 {}: {e}", path.display())))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err(AsrError::Other("分离输入没有声道".into()));
    }
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AsrError::Other(format!("读取分离输入失败: {e}")))?,
        SampleFormat::Int => {
            // ffmpeg wrote PCM s16le, but stay tolerant of other integer widths.
            let bits = spec.bits_per_sample.max(1);
            let scale = (1i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AsrError::Other(format!("读取分离输入失败: {e}")))?
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
) -> Result<(), AsrError> {
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)
        .map_err(|e| AsrError::Other(format!("写入人声轨道失败 {}: {e}", path.display())))?;
    for (l, r) in left.iter().zip(right.iter()) {
        writer
            .write_sample(*l)
            .and_then(|_| writer.write_sample(*r))
            .map_err(|e| AsrError::Other(format!("写入人声轨道失败: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| AsrError::Other(format!("写入人声轨道失败: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_path_is_install_layout_file() {
        let p = weights_path(Path::new(r"D:\OneAsr\models\htdemucs_ft"));
        assert_eq!(
            p,
            PathBuf::from(r"D:\OneAsr\models\htdemucs_ft\htdemucs_ft.safetensors")
        );
    }

    #[test]
    fn backend_pref_picks_the_first_attempt() {
        assert_eq!(preferred_backend("cpu"), Backend::Cpu);
        assert_eq!(preferred_backend(" CPU "), Backend::Cpu);
        // `auto` (and unknown ids) probe the GPU explicitly when this build has
        // CUDA, so a fallback is observable instead of hidden in the engine.
        #[cfg(feature = "cuda")]
        {
            assert_eq!(preferred_backend("auto"), Backend::Cuda);
            assert_eq!(preferred_backend("nonsense"), Backend::Cuda);
        }
        #[cfg(not(feature = "cuda"))]
        {
            assert_eq!(preferred_backend("auto"), Backend::Cpu);
            assert_eq!(preferred_backend("nonsense"), Backend::Cpu);
        }
        // Only an explicit 「GPU」 is allowed to fail loudly.
        assert!(is_forced_gpu("cuda"));
        assert!(is_forced_gpu(" CUDA "));
        assert!(!is_forced_gpu("auto"));
        assert!(!is_forced_gpu("cpu"));
    }

    #[test]
    fn missing_weights_is_a_clear_error() {
        let dir = std::env::temp_dir().join(format!("oneasr_sep_missing_{}", std::process::id()));
        let err = separate_vocals(Path::new("whatever.mp4"), &dir, &dir, "cpu", &mut |_| {})
            .unwrap_err()
            .to_string();
        assert!(err.contains("人声分离模型不存在"), "{err}");
    }

    #[test]
    fn progress_event_shape_is_stable() {
        let ev = SeparationEvent::Progress { done: 3, total: 30 };
        assert_eq!(ev, SeparationEvent::Progress { done: 3, total: 30 });
        let fell_back = SeparationEvent::FellBackToCpu {
            reason: "no device".into(),
        };
        assert!(matches!(fell_back, SeparationEvent::FellBackToCpu { .. }));
    }
}
