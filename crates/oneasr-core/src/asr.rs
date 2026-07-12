//! MOSS AsrInference session (lazy, process-wide) + full media pipeline.
//!
//! **Concurrency contract**
//! - Heavy work (load weights / transcribe) never holds [`SESSION`] for long.
//! - Workers clone an [`Arc`] handle, then drop the mutex before GPU/CPU work.
//! - UI may call [`unload_session`] / [`check_model_dir`] without freezing.
//! - Never mutates process CWD (mel filterbank is embedded in moss).
//! - Call [`crate::runtime::init_runtime`] at process start so rayon leaves a core for UI.
//! - Product deliverable is SRT only (runs/ keeps debug json/txt).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use moss_transcribe_diarize_rs::AsrInference;
use thiserror::Error;

use crate::engine::{split_long_segments, ExportOptions, Segment, TranscriptDocument, TranscriptEngine};
use crate::media::{convert_to_16k_mono_wav, probe_duration_sec, slice_wav};
use crate::paths::{media_stem, output_srt_path};
use crate::settings::Settings;
use crate::vad;

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("{0}")]
    Msg(String),
}

/// Target seconds per ASR chunk. Audio longer than this is VAD-split at
/// silence midpoints. Matches the validated Python asr.py default.
const CHUNK_SEC: f32 = 180.0;
const MIN_SILENCE_FALLBACK: f32 = 0.3;

/// Fine-grained pipeline stage for live UI status (never blocks UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrStage {
    LoadingModel,
    Converting,
    Transcribing,
    Exporting,
}

impl AsrStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::LoadingModel => "加载模型",
            Self::Converting => "转码音频",
            Self::Transcribing => "转写中",
            Self::Exporting => "导出字幕",
        }
    }
}

/// A stage update sent to the UI, optionally carrying chunk progress.
///
/// `chunk` is `Some((current_1based, total))` during multi-chunk transcription,
/// `None` for stages that don't have sub-progress (or single-chunk audio).
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

    /// Format as a display label, e.g. "转写中 2/4".
    pub fn label(&self) -> String {
        match self.chunk {
            Some((cur, total)) if total > 1 => format!("{} {cur}/{total}", self.stage.label()),
            _ => self.stage.label().to_string(),
        }
    }
}

struct Session {
    backend: String,
    model_dir: PathBuf,
    /// Shared so workers keep the engine alive after UI unloads the slot.
    infer: Arc<AsrInference>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Drop the cached session (e.g. when the user changes model dir / backend).
///
/// Never blocks the UI for long: if a worker briefly holds the mutex, we
/// finish the drop on a helper thread instead of freezing the event loop.
pub fn unload_session() {
    match SESSION.try_lock() {
        Ok(mut g) => {
            *g = None;
        }
        Err(std::sync::TryLockError::WouldBlock) => {
            std::thread::spawn(|| {
                if let Ok(mut g) = SESSION.lock() {
                    *g = None;
                }
            });
        }
        Err(std::sync::TryLockError::Poisoned(p)) => {
            *p.into_inner() = None;
        }
    }
}

/// Fast filesystem probe — does **not** load weights into memory.
///
/// Mirrors what [`moss_transcribe_diarize_rs::AsrInference`] actually opens:
/// - `config.json`
/// - `tokenizer.json`
/// - weights: either `model.safetensors`, **or** `model.safetensors.index.json`
///   plus **every** shard listed in its `weight_map`
///
/// Use for app start / folder pick / status bar; load only when starting jobs.
pub fn check_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    if !model_dir.is_dir() {
        return Err(AsrError::Msg(format!(
            "模型目录不存在: {}",
            model_dir.display()
        )));
    }

    let mut missing: Vec<String> = Vec::new();

    for name in ["config.json", "tokenizer.json"] {
        if !model_dir.join(name).is_file() {
            missing.push(name.into());
        }
    }

    // Same layout as moss `weights::load_weights`.
    let index_path = model_dir.join("model.safetensors.index.json");
    let single_path = model_dir.join("model.safetensors");
    if index_path.is_file() {
        match parse_weight_map_shards(&index_path) {
            Ok(shards) => {
                if shards.is_empty() {
                    missing.push("model.safetensors.index.json(weight_map 为空)".into());
                }
                for shard in shards {
                    if !model_dir.join(&shard).is_file() {
                        missing.push(shard);
                    }
                }
            }
            Err(e) => {
                missing.push(format!("model.safetensors.index.json({e})"));
            }
        }
    } else if single_path.is_file() {
        // single-file checkpoint
    } else {
        missing.push("model.safetensors 或 model.safetensors.index.json".into());
    }

    if !missing.is_empty() {
        // Cap list so UI remains readable.
        let shown: Vec<_> = missing.iter().take(8).cloned().collect();
        let extra = missing.len().saturating_sub(shown.len());
        let list = if extra > 0 {
            format!("{} …(+{extra})", shown.join(", "))
        } else {
            shown.join(", ")
        };
        return Err(AsrError::Msg(format!(
            "模型文件不全（缺少 {list}）: {}",
            model_dir.display()
        )));
    }
    Ok(())
}

/// Unique shard filenames from a HF-style safetensors index.
fn parse_weight_map_shards(index_path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(index_path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let wm = v
        .get("weight_map")
        .and_then(|x| x.as_object())
        .ok_or_else(|| "无 weight_map".to_string())?;
    let mut set = std::collections::BTreeSet::new();
    for val in wm.values() {
        if let Some(s) = val.as_str() {
            if !s.is_empty() {
                set.insert(s.to_string());
            }
        }
    }
    Ok(set.into_iter().collect())
}

/// Load weights **outside** the session mutex, then install under a short lock.
/// Safe only on a **worker** thread. Catches panics from bad model dirs.
fn get_or_load(model_dir: &Path, backend: &str) -> Result<(), AsrError> {
    let model_dir = model_dir.to_path_buf();
    let backend = backend.to_string();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_or_load_inner(&model_dir, &backend)
    })) {
        Ok(r) => r,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "模型加载过程 panic".into()
            };
            Err(AsrError::Msg(format!("加载崩溃: {msg}")))
        }
    }
}

fn get_or_load_inner(model_dir: &Path, backend: &str) -> Result<(), AsrError> {
    // 1) Fast path: already loaded for this dir/backend.
    {
        let guard = SESSION
            .lock()
            .map_err(|_| AsrError::Msg("ASR session lock poisoned".into()))?;
        if let Some(s) = guard.as_ref() {
            if s.backend == backend && s.model_dir == model_dir {
                return Ok(());
            }
        }
    } // lock released — load never runs under SESSION.

    // 2) Validate + heavy load without holding SESSION (UI can still interact).
    // Mel filterbank is embedded in moss — do not touch process CWD.
    check_model_dir(model_dir)?;
    let infer = AsrInference::load_with_backend(model_dir, backend)
        .map_err(|e| AsrError::Msg(format!("加载模型失败: {e}")))?;
    let infer = Arc::new(infer);

    // 3) Install (another worker may have won a race — keep whichever matches).
    let mut guard = SESSION
        .lock()
        .map_err(|_| AsrError::Msg("ASR session lock poisoned".into()))?;
    if let Some(s) = guard.as_ref() {
        if s.backend == backend && s.model_dir == model_dir {
            return Ok(());
        }
    }
    *guard = Some(Session {
        backend: backend.to_string(),
        model_dir: model_dir.to_path_buf(),
        infer,
    });
    Ok(())
}

/// Clone the live engine handle; **must not** hold [`SESSION`] during work.
fn take_infer() -> Result<Arc<AsrInference>, AsrError> {
    let guard = SESSION
        .lock()
        .map_err(|_| AsrError::Msg("ASR session lock poisoned".into()))?;
    guard
        .as_ref()
        .map(|s| s.infer.clone())
        .ok_or_else(|| AsrError::Msg("模型未加载".into()))
}

fn transcribe_wav(wav: &Path, prompt: &str, max_new_tokens: usize) -> Result<String, AsrError> {
    let infer = take_infer()?;
    let path = wav
        .to_str()
        .ok_or_else(|| AsrError::Msg("路径非 UTF-8".into()))?;
    // SESSION is free here — UI unload/settings remain responsive.
    infer
        .transcribe(path, prompt, max_new_tokens, None)
        .map_err(|e| AsrError::Msg(format!("转写失败: {e}")))
}

/// Full pipeline: convert → real MOSS ASR → parse → `{app_root}/output/{stem}.srt`.
///
/// Intermediate work files (16k wav, raw, segments) go under `{app_root}/runs/...`.
/// The **user-facing** deliverable is always `output/{media_stem}.srt`.
///
/// There is **no** stub/placeholder success path: success requires a real
/// `AsrInference::transcribe` result written through `export_srt`.
///
/// **Call only from a worker thread** — never on the GPUI UI thread.
pub fn process_media_file(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
) -> Result<PathBuf, AsrError> {
    process_media_file_with_progress(input, media_name, settings, app_root, |_| {})
}

/// Same as [`process_media_file`], with stage callbacks for non-blocking UI labels.
pub fn process_media_file_with_progress(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
    mut on_stage: impl FnMut(StageUpdate),
) -> Result<PathBuf, AsrError> {
    let prompt = settings
        .build_prompt()
        .map_err(|e| AsrError::Msg(e.to_string()))?;

    on_stage(StageUpdate::new(AsrStage::LoadingModel));
    get_or_load(&settings.model_dir, &settings.backend)?;

    let stem = media_stem(input);
    let srt_path = output_srt_path(app_root, &stem);

    // Work dir for intermediates only.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let work_dir = app_root.join("runs").join(format!("{stem}_{stamp}"));
    std::fs::create_dir_all(&work_dir).map_err(|e| AsrError::Msg(e.to_string()))?;
    if let Some(parent) = srt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AsrError::Msg(e.to_string()))?;
    }

    // 1. Convert with bundled ffmpeg
    on_stage(StageUpdate::new(AsrStage::Converting));
    let wav = work_dir.join("input_16k.wav");
    convert_to_16k_mono_wav(input, &wav).map_err(|e| AsrError::Msg(e.to_string()))?;

    // 2. VAD chunk planning: split long audio at silence midpoints (~180s each).
    //    Short audio (<= CHUNK_SEC) skips VAD and transcribes in one shot.
    let duration = probe_duration_sec(&wav).unwrap_or(0.0) as f32;
    let chunks = if duration > CHUNK_SEC {
        let speech = vad::run_vad(&wav)?;
        let silences = vad::speech_to_silences(&speech, duration, MIN_SILENCE_FALLBACK);
        let planned = vad::plan_chunks(duration, &silences, CHUNK_SEC);
        eprintln!(
            "[vad] duration={duration:.1}s speech={} silence_gaps={} chunks={}",
            speech.len(),
            silences.len(),
            planned.len(),
        );
        planned
    } else {
        vec![vad::Chunk {
            start: 0.0,
            end: duration,
        }]
    };

    // 3. Real ASR per chunk (blocking on this worker thread only).
    //    Each chunk is transcribed independently; timestamps are offset to the
    //    global timeline by adding chunk.start before merging.
    let total_chunks = chunks.len();
    let mut all_segments: Vec<Segment> = Vec::new();
    let mut all_raw: Vec<String> = Vec::new();

    for (i, chunk) in chunks.iter().enumerate() {
        on_stage(StageUpdate::with_chunk(
            AsrStage::Transcribing,
            i + 1,
            total_chunks,
        ));
        let chunk_wav = work_dir.join(format!("chunk_{i:03}.wav"));
        // If single chunk covering the whole file, reuse the converted wav directly.
        if chunks.len() == 1 {
            // whole-file path: transcribe the already-converted input_16k.wav
            let raw = transcribe_wav(&wav, &prompt, settings.max_new_tokens)?;
            all_raw.push(raw);
        } else {
            slice_wav(&wav, chunk.start, chunk.end, &chunk_wav)
                .map_err(|e| AsrError::Msg(e.to_string()))?;
            let raw = transcribe_wav(&chunk_wav, &prompt, settings.max_new_tokens)?;
            all_raw.push(raw);
        }

        let chunk_raw = all_raw.last().unwrap();
        let chunk_doc = TranscriptEngine::parse_moss_compact(chunk_raw);
        let offset = chunk.start as f64;
        for mut seg in chunk_doc.segments {
            seg.start += offset;
            seg.end += offset;
            all_segments.push(seg);
        }
    }

    // Renumber merged segments globally.
    for (i, seg) in all_segments.iter_mut().enumerate() {
        seg.id = format!("seg_{:04}", i + 1);
    }

    // Split overlong segments for readability (only when enabled; English/Latin
    // text is split at clause/connector points, CJK is left untouched).
    if settings.split_long_sentences {
        all_segments = split_long_segments(&all_segments);
    }

    let raw = all_raw.join("");
    std::fs::write(work_dir.join("raw_transcript.txt"), &raw)
        .map_err(|e| AsrError::Msg(e.to_string()))?;

    // Reject obvious placeholder strings if they ever appear.
    if is_forbidden_placeholder(&raw) {
        return Err(AsrError::Msg(
            "internal error: placeholder transcript is not allowed".into(),
        ));
    }

    // 4. Build merged document + export
    on_stage(StageUpdate::new(AsrStage::Exporting));
    // Export is near-instant (parse + write SRT). Hold the "导出字幕" label for
    // a beat so the user can perceive the stage transition before Done.
    std::thread::sleep(std::time::Duration::from_secs(1));
    let doc = TranscriptDocument::new(all_segments).with_raw(&raw);
    if doc.is_empty() && !raw.trim().is_empty() {
        return Err(AsrError::Msg(format!(
            "解析引擎未解析出段落（raw 非空）: {}",
            raw.chars().take(120).collect::<String>()
        )));
    }

    std::fs::write(work_dir.join("segments.json"), doc.to_json())
        .map_err(|e| AsrError::Msg(e.to_string()))?;

    let opts = ExportOptions {
        show_speaker: settings.export_show_speaker,
    };
    let srt_body = doc.to_srt(&opts);
    if is_forbidden_placeholder(&srt_body) {
        return Err(AsrError::Msg(
            "internal error: refusing to write stub SRT".into(),
        ));
    }
    std::fs::write(&srt_path, &srt_body).map_err(|e| AsrError::Msg(e.to_string()))?;
    // Also keep a copy next to work artifacts for debugging.
    let _ = std::fs::write(work_dir.join(format!("{stem}.srt")), &srt_body);
    let _ = std::fs::write(work_dir.join(format!("{stem}.txt")), doc.to_plain(&opts));

    let _ = std::fs::write(
        work_dir.join("meta.txt"),
        format!(
            "source={media_name}\nbackend={}\noutput={}\n",
            settings.backend,
            srt_path.display()
        ),
    );

    Ok(srt_path)
}

/// Static proof used by tests: pipeline always targets install `output/` naming.
pub fn planned_output_path(app_root: &Path, input: &Path) -> PathBuf {
    output_srt_path(app_root, &media_stem(input))
}

fn is_forbidden_placeholder(s: &str) -> bool {
    // Split tokens so source-level contract tests can ban success-path templates
    // without matching these rejection guards themselves.
    let a = ["[", "stub", "]"].concat();
    let b = format!("{}{}", "ASR ", "尚未接入");
    s.contains(&a) || s.contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ExportOptions, TranscriptEngine};
    use crate::prompt::build_prompt;
    use std::path::PathBuf;

    #[test]
    fn planned_output_path_uses_stem() {
        let root = PathBuf::from(r"C:\Install\OneAsr");
        let input = PathBuf::from(r"D:\clips\lecture_01.mp4");
        assert_eq!(
            planned_output_path(&root, &input),
            PathBuf::from(r"C:\Install\OneAsr\output\lecture_01.srt")
        );
    }

    #[test]
    fn check_model_dir_rejects_missing() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr_empty_model_probe_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = check_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("不全") || err.contains("缺少"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_model_dir_rejects_nonexistent() {
        let p = PathBuf::from(r"D:\__oneasr_no_such_model_dir__");
        assert!(check_model_dir(&p).is_err());
    }

    #[test]
    fn check_model_dir_rejects_index_without_shards() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr_index_only_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), "{}").unwrap();
        std::fs::write(dir.join("tokenizer.json"), "{}").unwrap();
        std::fs::write(
            dir.join("model.safetensors.index.json"),
            r#"{"weight_map":{"a":"model-00000-of-00001.safetensors"}}"#,
        )
        .unwrap();
        let err = check_model_dir(&dir).unwrap_err().to_string();
        assert!(
            err.contains("model-00000-of-00001.safetensors"),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_model_dir_accepts_single_weight_file() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr_single_weight_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), "{}").unwrap();
        std::fs::write(dir.join("tokenizer.json"), "{}").unwrap();
        std::fs::write(dir.join("model.safetensors"), b"not-real").unwrap();
        assert!(check_model_dir(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shipped_prompt_parse_export_roundtrip_no_stub() {
        let prompt = build_prompt(false, "").expect("prompt A");
        assert!(prompt.contains("speaker ID"));
        assert!(!prompt.contains("stub"));

        let raw = "[1.00][S01]Hello world[2.50][2.60][S02]Next line[3.00]";
        let doc = TranscriptEngine::parse_moss_compact(raw);
        assert_eq!(doc.len(), 2);
        let srt = doc.to_srt(&ExportOptions {
            show_speaker: true,
        });
        assert!(srt.contains("-->"));
        assert!(srt.contains("S01: Hello world"));
        assert!(!srt.contains("[stub]"));
    }
}
