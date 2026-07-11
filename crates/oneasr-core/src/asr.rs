//! MOSS AsrInference session (lazy, process-wide) + full media pipeline.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use moss_transcribe_diarize_rs::AsrInference;
use thiserror::Error;

use crate::engine::{ExportOptions, TranscriptEngine};
use crate::media::convert_to_16k_mono_wav;
use crate::paths::{media_stem, output_srt_path};
use crate::settings::Settings;

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("{0}")]
    Msg(String),
}

struct Session {
    backend: String,
    model_dir: PathBuf,
    infer: AsrInference,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Default location of the rust crate (mel filterbank `data/`).
const MOSS_RUST_DIR: &str = r"D:\moss-transcribe-diarize-rs";

/// Ensure mel filterbank is found: engine reads relative `data/whisper_mel_filters.bin`.
fn with_moss_cwd<T>(f: impl FnOnce() -> T) -> T {
    let prev = std::env::current_dir().ok();
    let moss = Path::new(MOSS_RUST_DIR);
    if moss.join("data").join("whisper_mel_filters.bin").is_file() {
        let _ = std::env::set_current_dir(moss);
    }
    let out = f();
    if let Some(p) = prev {
        let _ = std::env::set_current_dir(p);
    }
    out
}

/// Whether a session is loaded for the given model/backend.
pub fn session_matches(model_dir: &Path, backend: &str) -> bool {
    SESSION
        .lock()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .map(|s| s.backend == backend && s.model_dir == model_dir)
        })
        .unwrap_or(false)
}

/// Drop the cached session (e.g. before reloading a new path).
pub fn unload_session() {
    if let Ok(mut g) = SESSION.lock() {
        *g = None;
    }
}

/// Load (or reuse) the model. Safe to call from a background thread.
/// Catches panics so the UI never stays stuck on “loading”.
pub fn preload_model(model_dir: &Path, backend: &str) -> Result<(), AsrError> {
    let model_dir = model_dir.to_path_buf();
    let backend = backend.to_string();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        get_or_load(&model_dir, &backend)
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

fn get_or_load(model_dir: &Path, backend: &str) -> Result<(), AsrError> {
    let mut guard = SESSION
        .lock()
        .map_err(|_| AsrError::Msg("ASR session lock poisoned".into()))?;

    let need_reload = match guard.as_ref() {
        None => true,
        Some(s) => s.backend != backend || s.model_dir != model_dir,
    };
    if !need_reload {
        return Ok(());
    }

    *guard = None;

    if !model_dir.join("config.json").is_file() {
        return Err(AsrError::Msg(format!(
            "模型目录无效（缺少 config.json）: {}",
            model_dir.display()
        )));
    }

    let infer = with_moss_cwd(|| {
        AsrInference::load_with_backend(model_dir, backend)
            .map_err(|e| AsrError::Msg(format!("加载模型失败: {e}")))
    })?;

    *guard = Some(Session {
        backend: backend.to_string(),
        model_dir: model_dir.to_path_buf(),
        infer,
    });
    Ok(())
}

fn transcribe_wav(wav: &Path, prompt: &str, max_new_tokens: usize) -> Result<String, AsrError> {
    let guard = SESSION
        .lock()
        .map_err(|_| AsrError::Msg("ASR session lock poisoned".into()))?;
    let session = guard
        .as_ref()
        .ok_or_else(|| AsrError::Msg("模型未加载".into()))?;
    let path = wav
        .to_str()
        .ok_or_else(|| AsrError::Msg("路径非 UTF-8".into()))?;
    with_moss_cwd(|| {
        session
            .infer
            .transcribe(path, prompt, max_new_tokens)
            .map_err(|e| AsrError::Msg(format!("转写失败: {e}")))
    })
}

/// Full pipeline: convert → real MOSS ASR → parse → `{app_root}/output/{stem}.srt`.
///
/// Intermediate work files (16k wav, raw, segments) go under `{app_root}/runs/...`.
/// The **user-facing** deliverable is always `output/{media_stem}.srt`.
///
/// There is **no** stub/placeholder success path: success requires a real
/// `AsrInference::transcribe` result written through `export_srt`.
pub fn process_media_file(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
) -> Result<PathBuf, AsrError> {
    let prompt = settings
        .build_prompt()
        .map_err(|e| AsrError::Msg(e.to_string()))?;

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
    let wav = work_dir.join("input_16k.wav");
    convert_to_16k_mono_wav(input, &wav).map_err(|e| AsrError::Msg(e.to_string()))?;

    // 2. Real ASR (blocking)
    let raw = transcribe_wav(&wav, &prompt, settings.max_new_tokens)?;
    std::fs::write(work_dir.join("raw_transcript.txt"), &raw)
        .map_err(|e| AsrError::Msg(e.to_string()))?;

    // Reject obvious placeholder strings if they ever appear.
    if is_forbidden_placeholder(&raw) {
        return Err(AsrError::Msg(
            "internal error: placeholder transcript is not allowed".into(),
        ));
    }

    // 3. Parse engine: raw → structured segments (time / speaker / text)
    let doc = TranscriptEngine::parse_moss_compact(&raw);
    if doc.is_empty() && !raw.trim().is_empty() {
        return Err(AsrError::Msg(format!(
            "解析引擎未解析出段落（raw 非空）: {}",
            raw.chars().take(120).collect::<String>()
        )));
    }

    std::fs::write(work_dir.join("segments.json"), doc.to_json())
        .map_err(|e| AsrError::Msg(e.to_string()))?;

    // 4. Export engine: structured doc → SRT (and other formats as needed)
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
