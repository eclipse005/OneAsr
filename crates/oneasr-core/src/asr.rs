//! Qwen3-ASR + ForcedAligner pipeline.
//!
//! ```text
//! load ASR once → all VAD chunks transcribed → unload ASR
//! load Aligner once → all chunks aligned → unload Aligner
//! word normalize → sentence_boundary → SRT
//! ```
//!
//! Never keeps ASR and Aligner in VRAM at the same time.
//!
//! **Scratch lifecycle**: `runs/{stem}_{ts}/` holds only the full 16 kHz WAV
//! plus at most one temporary chunk file. Product output is solely
//! `output/{stem}.srt` (atomic write). On success the scratch dir is removed.

use std::path::{Path, PathBuf};

use qwen3_asr::{AsrInference, Backend as AsrBackend, TranscribeOptions};
use qwen_forced_aligner_rs::{
    AlignRequest, AudioInput, DeviceRequest, ModelOptions, Qwen3ForcedAligner, TextInput,
};
use thiserror::Error;

use crate::lang::{to_lang_key, to_qwen_language_label};
use crate::media::{
    convert_to_16k_mono_wav, slice_wav, wav_duration_sec, write_atomic,
};
use crate::paths::{media_stem, output_srt_path};
use crate::sentence_boundary::{
    build_source_sentences_from_words, source_sentences_to_srt, SentenceBoundaryRequest,
    WordTokenDto,
};
use crate::settings::Settings;
use crate::subtitle::alignment::align_text_to_timestamps;
use crate::subtitle::segmenter::{normalize_word_tokens, WordToken};
use crate::vad;

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("{0}")]
    Msg(String),
}

const MIN_SILENCE_FALLBACK: f32 = 0.3;

/// Fine-grained pipeline stage for live UI status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrStage {
    LoadingModel,
    Converting,
    Transcribing,
    Aligning,
    Exporting,
}

impl AsrStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::LoadingModel => "加载模型",
            Self::Converting => "转码音频",
            Self::Transcribing => "转写中",
            Self::Aligning => "打轴中",
            Self::Exporting => "导出字幕",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StageUpdate {
    pub stage: AsrStage,
    pub chunk: Option<(usize, usize)>,
}

impl StageUpdate {
    pub fn new(stage: AsrStage) -> Self {
        Self { stage, chunk: None }
    }

    pub fn with_chunk(stage: AsrStage, current: usize, total: usize) -> Self {
        Self {
            stage,
            chunk: Some((current, total)),
        }
    }

    pub fn label(&self) -> String {
        match self.chunk {
            Some((cur, total)) if total > 1 => format!("{} {cur}/{total}", self.stage.label()),
            _ => self.stage.label().to_string(),
        }
    }
}

pub fn check_asr_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    if !model_dir.is_dir() {
        return Err(AsrError::Msg(format!(
            "ASR 模型目录不存在: {}",
            model_dir.display()
        )));
    }
    let mut missing = Vec::new();
    for name in ["config.json", "tokenizer.json"] {
        if !model_dir.join(name).is_file() {
            missing.push(name.to_string());
        }
    }
    let index = model_dir.join("model.safetensors.index.json");
    let single = model_dir.join("model.safetensors");
    if index.is_file() {
        // Shards checked loosely — load will fail with a clear error if incomplete.
    } else if !single.is_file() {
        missing.push("model.safetensors 或 model.safetensors.index.json".into());
    }
    if !missing.is_empty() {
        return Err(AsrError::Msg(format!(
            "ASR 模型文件不全（缺少 {}）: {}",
            missing.join(", "),
            model_dir.display()
        )));
    }
    Ok(())
}

pub fn check_aligner_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    if !model_dir.is_dir() {
        return Err(AsrError::Msg(format!(
            "Aligner 模型目录不存在: {}",
            model_dir.display()
        )));
    }
    let mut missing = Vec::new();
    if !model_dir.join("config.json").is_file() {
        missing.push("config.json");
    }
    if !model_dir.join("model.safetensors").is_file()
        && !model_dir.join("model.safetensors.index.json").is_file()
    {
        missing.push("model.safetensors");
    }
    if !missing.is_empty() {
        return Err(AsrError::Msg(format!(
            "Aligner 模型文件不全（缺少 {}）: {}",
            missing.join(", "),
            model_dir.display()
        )));
    }
    Ok(())
}

/// Resolved compute target for both ASR and Aligner (one policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComputeBackend {
    Cpu,
    Cuda,
}

impl ComputeBackend {
    fn to_asr(self) -> AsrBackend {
        match self {
            Self::Cpu => AsrBackend::Cpu,
            Self::Cuda => AsrBackend::Cuda,
        }
    }

    fn to_align_device(self) -> DeviceRequest {
        match self {
            Self::Cpu => DeviceRequest::Cpu,
            Self::Cuda => DeviceRequest::Cuda(0),
        }
    }
}

/// Resolve inference backend from settings.
///
/// Product rule (single installer + optional Settings「安装组件」):
/// - **cpu** → always CPU
/// - **cuda** → require app `dll/` CUDA runtime; error if missing
/// - **auto** → CUDA only when app `dll/` is ready; otherwise CPU
///   (do **not** fall through to system CUDA — that bypasses the install gate)
fn resolve_compute_backend(backend: &str) -> Result<ComputeBackend, AsrError> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "cpu" => Ok(ComputeBackend::Cpu),
        "cuda" => {
            #[cfg(feature = "cuda")]
            {
                if !crate::model::is_cuda_runtime_ready() {
                    return Err(AsrError::Msg(
                        "未检测到 CUDA 运行库，请在设置中下载后再使用 GPU".into(),
                    ));
                }
                Ok(ComputeBackend::Cuda)
            }
            #[cfg(not(feature = "cuda"))]
            {
                Err(AsrError::Msg(
                    "此构建未启用 CUDA，请使用 backend=cpu".into(),
                ))
            }
        }
        // auto
        _ => {
            #[cfg(feature = "cuda")]
            {
                if crate::model::is_cuda_runtime_ready() {
                    Ok(ComputeBackend::Cuda)
                } else {
                    Ok(ComputeBackend::Cpu)
                }
            }
            #[cfg(not(feature = "cuda"))]
            {
                Ok(ComputeBackend::Cpu)
            }
        }
    }
}

fn clean_asr_text(raw: &str) -> String {
    let mut text = raw.trim();
    for marker in ["<asr_text>", "asr_text>"] {
        if let Some((_, rest)) = text.split_once(marker) {
            text = rest.trim();
        }
    }
    text.trim_matches(['<', '>']).trim().to_string()
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// One non-empty ASR segment; times are on the global timeline (seconds).
/// Audio is re-sliced from the master 16 kHz WAV on demand — no persistent chunk files.
struct ChunkTranscript {
    start_sec: f64,
    end_sec: f64,
    text: String,
    language: String,
}

/// Full pipeline: convert → VAD plan → ASR all → unload → align all → SRT.
pub fn process_media_file_with_progress(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
    mut on_stage: impl FnMut(StageUpdate),
) -> Result<PathBuf, AsrError> {
    let stem = media_stem(input);
    let srt_path = output_srt_path(app_root, &stem);

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let work_dir = app_root.join("runs").join(format!("{stem}_{stamp}"));
    std::fs::create_dir_all(&work_dir).map_err(|e| AsrError::Msg(e.to_string()))?;
    if let Some(parent) = srt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AsrError::Msg(e.to_string()))?;
    }

    // 1. Convert → single master PCM under work_dir
    on_stage(StageUpdate::new(AsrStage::Converting));
    let wav = work_dir.join("input_16k.wav");
    convert_to_16k_mono_wav(input, &wav).map_err(|e| AsrError::Msg(e.to_string()))?;

    let duration = wav_duration_sec(&wav).unwrap_or(0.0) as f32;
    let chunk_sec = settings.chunk_target_seconds_clamped() as f32;

    // 2. VAD plan (ranges only — no chunk files yet)
    let (chunks, vad_speech) = if duration > chunk_sec {
        let speech = vad::run_vad(&wav)?;
        let silences = vad::speech_to_silences(&speech, duration, MIN_SILENCE_FALLBACK);
        let planned = vad::plan_chunks(duration, &silences, chunk_sec);
        #[cfg(debug_assertions)]
        eprintln!(
            "[vad] duration={duration:.1}s speech={} silence_gaps={} chunks={}",
            speech.len(),
            silences.len(),
            planned.len(),
        );
        let vad_pairs: Vec<(f64, f64)> = speech
            .iter()
            .map(|&(s, e)| (s as f64, e as f64))
            .collect();
        (planned, vad_pairs)
    } else {
        (
            vec![vad::Chunk {
                start: 0.0,
                end: duration,
            }],
            vec![(0.0, duration as f64)],
        )
    };

    let multi_chunk = chunks.len() > 1;
    let force_lang = to_qwen_language_label(&settings.language);
    let source_lang_key = to_lang_key(&settings.language);
    let total_chunks = chunks.len().max(1);

    // Scratch path for the one active slice (deleted after each use when multi).
    let chunk_tmp = work_dir.join("chunk_tmp.wav");

    // 3. Load ASR once → all chunks → drop
    on_stage(StageUpdate::new(AsrStage::LoadingModel));
    check_asr_model_dir(&settings.asr_model_dir)?;
    let compute = resolve_compute_backend(&settings.backend)?;
    #[cfg(debug_assertions)]
    {
        let backend_label = match compute {
            ComputeBackend::Cpu => "cpu",
            ComputeBackend::Cuda => "cuda",
        };
        eprintln!(
            "[backend] setting={} resolved={} cuda_dlls={}",
            settings.backend,
            backend_label,
            crate::model::is_cuda_runtime_ready(),
        );
    }
    let asr = AsrInference::load(&settings.asr_model_dir, compute.to_asr())
        .map_err(|e| AsrError::Msg(format!("加载 ASR 失败: {e:#}")))?;

    let mut transcripts: Vec<ChunkTranscript> = Vec::new();
    #[cfg(debug_assertions)]
    let mut empty_chunks: Vec<usize> = Vec::new();

    for (i, chunk) in chunks.iter().enumerate() {
        on_stage(StageUpdate::with_chunk(
            AsrStage::Transcribing,
            i + 1,
            total_chunks,
        ));

        let chunk_path = if multi_chunk {
            slice_wav(&wav, chunk.start, chunk.end, &chunk_tmp)
                .map_err(|e| AsrError::Msg(e.to_string()))?;
            chunk_tmp.as_path()
        } else {
            wav.as_path()
        };

        let opts = TranscribeOptions::default()
            .with_max_new_tokens(settings.max_new_tokens)
            .with_language(force_lang.clone());

        let path_str = chunk_path
            .to_str()
            .ok_or_else(|| AsrError::Msg("路径非 UTF-8".into()))?;
        let report = asr
            .transcribe(path_str, opts)
            .map_err(|e| AsrError::Msg(format!("转写失败 chunk {}: {e:#}", i + 1)))?;
        let text = clean_asr_text(&report.text);

        if multi_chunk {
            let _ = std::fs::remove_file(&chunk_tmp);
        }

        if text.is_empty() {
            #[cfg(debug_assertions)]
            empty_chunks.push(i);
            continue;
        }

        transcripts.push(ChunkTranscript {
            start_sec: chunk.start as f64,
            end_sec: chunk.end as f64,
            text,
            language: force_lang.clone(),
        });
    }
    drop(asr);

    if transcripts.is_empty() {
        return Err(AsrError::Msg(format!(
            "ASR 未产生任何有效文本（{total_chunks} 段全部为空）"
        )));
    }
    #[cfg(debug_assertions)]
    if !empty_chunks.is_empty() {
        eprintln!(
            "[asr] skipped empty chunks: {}/{} indices={empty_chunks:?}",
            empty_chunks.len(),
            total_chunks
        );
    }

    // 4. Load Aligner once → all chunks → drop (re-slice from master as needed)
    on_stage(StageUpdate::new(AsrStage::LoadingModel));
    check_aligner_model_dir(&settings.aligner_model_dir)?;
    let aligner = Qwen3ForcedAligner::load(
        &settings.aligner_model_dir,
        ModelOptions {
            device: compute.to_align_device(),
        },
    )
    .map_err(|e| AsrError::Msg(format!("加载 Aligner 失败: {e:#}")))?;

    let mut all_words: Vec<WordToken> = Vec::new();
    let align_total = transcripts.len();
    for (i, seg) in transcripts.iter().enumerate() {
        on_stage(StageUpdate::with_chunk(
            AsrStage::Aligning,
            i + 1,
            align_total,
        ));

        let chunk_path = if multi_chunk {
            slice_wav(
                &wav,
                seg.start_sec as f32,
                seg.end_sec as f32,
                &chunk_tmp,
            )
            .map_err(|e| AsrError::Msg(e.to_string()))?;
            chunk_tmp.as_path()
        } else {
            wav.as_path()
        };

        let result = aligner
            .align(AlignRequest::new(
                AudioInput::Path(chunk_path.to_path_buf()),
                TextInput::Text(seg.text.clone()),
                seg.language.clone(),
            ))
            .map_err(|e| AsrError::Msg(format!("对齐失败 chunk {}: {e:#}", i + 1)))?;

        if multi_chunk {
            let _ = std::fs::remove_file(&chunk_tmp);
        }

        let mut segment_words = Vec::new();
        for item in result.items {
            let word = item.text.trim();
            if word.is_empty() {
                continue;
            }
            segment_words.push(WordToken {
                start: round_millis(seg.start_sec + item.start_time.max(0.0)),
                end: round_millis(seg.start_sec + item.end_time.max(item.start_time)),
                word: word.to_string(),
            });
        }

        // Qwen aligner strips punctuation — restore from ASR transcript.
        let restored = attach_transcript_punctuation(&seg.text, &segment_words);
        all_words.extend(restored);
    }
    drop(aligner);

    // 5. Normalize + sentence boundary → atomic SRT → drop scratch
    on_stage(StageUpdate::new(AsrStage::Exporting));
    let words = normalize_word_tokens(all_words);
    if words.is_empty() {
        return Err(AsrError::Msg("对齐后词列表为空".into()));
    }

    let word_dtos: Vec<WordTokenDto> = words
        .into_iter()
        .map(|w| WordTokenDto {
            start: w.start,
            end: w.end,
            word: w.word,
        })
        .collect();

    let step2 = build_source_sentences_from_words(SentenceBoundaryRequest {
        task_id: stem.clone(),
        media_path: media_name.to_string(),
        source_lang: source_lang_key,
        subtitle_length_preset: settings.subtitle_length_preset.clone(),
        words: word_dtos,
        vad_speech_segments: vad_speech,
    })
    .map_err(AsrError::Msg)?;

    let srt_body = source_sentences_to_srt(&step2);
    if srt_body.trim().is_empty() {
        return Err(AsrError::Msg("断句后字幕为空".into()));
    }

    write_atomic(&srt_path, &srt_body).map_err(|e| AsrError::Msg(e.to_string()))?;

    // Product is only output/*.srt — scratch is ephemeral.
    let _ = std::fs::remove_dir_all(&work_dir);

    Ok(srt_path)
}

fn attach_transcript_punctuation(transcript_text: &str, aligned_words: &[WordToken]) -> Vec<WordToken> {
    if transcript_text.trim().is_empty() || aligned_words.is_empty() {
        return aligned_words.to_vec();
    }
    let mapped = align_text_to_timestamps(transcript_text, aligned_words);
    if mapped.len() == aligned_words.len() {
        mapped
    } else {
        aligned_words.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::output_srt_path;

    #[test]
    fn output_path_uses_stem() {
        let root = PathBuf::from(r"C:\Install\OneAsr");
        let input = PathBuf::from(r"D:\clips\lecture_01.mp4");
        assert_eq!(
            output_srt_path(&root, &media_stem(&input)),
            PathBuf::from(r"C:\Install\OneAsr\output\lecture_01.srt")
        );
    }

    #[test]
    fn check_asr_model_dir_rejects_missing() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr_empty_model_probe_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = check_asr_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("不全") || err.contains("缺少") || err.contains("不存在"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
