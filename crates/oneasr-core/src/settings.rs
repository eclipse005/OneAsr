//! The persisted settings model: what the app remembers between runs, how the
//! values are clamped, and how a damaged file is repaired rather than dropped.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::lang::{default_source_language, normalize_source_language};
use crate::media::resolve_app_root;
use crate::model::{
    ModelId, ModelKind, QWEN3_ASR_06B, default_aligner_model_dir, default_asr_model_dir,
    resolve_app_root_dir, resolve_model_dir,
};
use crate::paths::default_output_dir_for;
use crate::text_script::{self, TextScript};

/// Inclusive lower bound for VAD ASR chunk target (seconds).
pub const CHUNK_TARGET_MIN_SEC: u32 = 30;
/// Inclusive upper bound for VAD ASR chunk target (seconds).
pub const CHUNK_TARGET_MAX_SEC: u32 = 180;
/// Product default when settings omit `chunk_target_seconds`.
pub const CHUNK_TARGET_DEFAULT_SEC: u32 = 60;
/// Settings UI preset values (seconds), all within
/// [`CHUNK_TARGET_MIN_SEC`]..=[`CHUNK_TARGET_MAX_SEC`].
pub const CHUNK_TARGET_PRESETS: &[(u32, &str)] = &[
    (30, "30"),
    (60, "60"),
    (90, "90"),
    (120, "120"),
    (180, "180"),
];

/// Clamp a chunk target into the legal product range.
#[inline]
pub fn clamp_chunk_target_seconds(value: u32) -> u32 {
    value.clamp(CHUNK_TARGET_MIN_SEC, CHUNK_TARGET_MAX_SEC)
}

/// User-facing settings for the Qwen ASR + ForcedAligner pipeline.
///
/// Persisted as `{app_root}/settings.json` when possible.
///
/// # ASR selection model
/// - [`Self::asr_model`] is the **active** catalog id (`Qwen3-ASR-0.6B` | `1.7B`).
/// - [`Self::asr_model_dir`] is the path loaded at runtime.
/// - Switching size via [`Self::select_asr_model`] always binds install-layout
///   `{app}/models/{name}`.
/// - Completing a download for a **non-selected** size only installs files on
///   disk; it must not change the active selection (see
///   [`Self::bind_download_if_active`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Active ASR catalog id (`Qwen3-ASR-0.6B` | `Qwen3-ASR-1.7B`).
    #[serde(default = "default_asr_model")]
    pub asr_model: String,
    /// Directory loaded for inference (install layout or user-picked).
    #[serde(default = "default_asr_model_dir")]
    pub asr_model_dir: PathBuf,
    /// Qwen3-ForcedAligner directory.
    #[serde(default = "default_aligner_model_dir")]
    pub aligner_model_dir: PathBuf,
    /// `auto` | `cuda` | `cpu`
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_max_new_tokens")]
    pub max_new_tokens: usize,
    /// Default source language for **new** tasks (`zh`, `en`, …).
    /// Each task can override; pipeline uses the task language at run time.
    #[serde(default = "default_source_language")]
    pub language: String,
    /// `short` | `standard` | `loose`
    #[serde(default = "default_subtitle_length_preset")]
    pub subtitle_length_preset: String,
    /// VAD ASR chunk target seconds
    /// ([`CHUNK_TARGET_MIN_SEC`]..=[`CHUNK_TARGET_MAX_SEC`]; default
    /// [`CHUNK_TARGET_DEFAULT_SEC`]).
    #[serde(default = "default_chunk_target_seconds")]
    pub chunk_target_seconds: u32,
    /// Directory for finished `.srt` files. Default: `{app_root}/output`.
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    /// `true` → SRT saved next to the source media file; `output_dir` is the
    /// fallback when that directory is not writable (or has no parent dir).
    #[serde(default = "default_save_next_to_source")]
    pub save_next_to_source: bool,
    /// Write finished `.srt` files. At least one of SRT / TXT stays on.
    #[serde(default = "default_output_srt")]
    pub output_srt: bool,
    /// Write a plain-text `.txt` transcript (one cue per line, no timestamps).
    #[serde(default)]
    pub output_txt: bool,
    /// Chinese output script: `simplified` (default) | `traditional`.
    /// Applies to `zh` / `yue` sources; other languages ignore it.
    #[serde(default = "default_text_script")]
    pub text_script: String,
    /// Run HTDemucs vocal separation before VAD/ASR (needs Demucs weights).
    #[serde(default)]
    pub vocal_separation: bool,
    /// HTDemucs weights directory (default `{app}/models/htdemucs_ft`).
    #[serde(default = "default_demucs_model_dir")]
    pub demucs_model_dir: PathBuf,
    /// The one sound switch (「提示音」): interaction taps AND the run
    /// reminder together.
    #[serde(default = "default_sound")]
    pub sound: bool,
    /// Pre-0.2.0 switch, read-only and never written back: `ui_sound` merged
    /// into [`Self::sound`], and an install that had muted either family must
    /// not get sound back just because the field it turned off disappeared from
    /// the schema. Absent (the common case) means "nothing to carry over".
    #[serde(default, rename = "ui_sound", skip_serializing_if = "Option::is_none")]
    pub legacy_ui_sound: Option<bool>,
    /// Pre-0.2.0 switch, read-only — see [`Self::legacy_ui_sound`].
    #[serde(default, rename = "task_notify", skip_serializing_if = "Option::is_none")]
    pub legacy_task_notify: Option<bool>,
}

fn default_asr_model() -> String {
    QWEN3_ASR_06B.into()
}

fn default_backend() -> String {
    "auto".into()
}

fn default_max_new_tokens() -> usize {
    2048
}

fn default_subtitle_length_preset() -> String {
    "standard".into()
}

fn default_chunk_target_seconds() -> u32 {
    CHUNK_TARGET_DEFAULT_SEC
}

fn default_output_dir() -> PathBuf {
    default_output_dir_for(&resolve_app_root_dir())
}

fn default_save_next_to_source() -> bool {
    true
}

fn default_output_srt() -> bool {
    true
}

fn default_text_script() -> String {
    // Product default: never re-write the recognizer's own script.
    TextScript::ORIGINAL_ID.into()
}

fn default_demucs_model_dir() -> PathBuf {
    crate::model::default_demucs_model_dir()
}

// Sounds default on: that has been the shipped behaviour, and the one switch
// to turn them all off lives in an obvious place.
fn default_sound() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            asr_model: default_asr_model(),
            asr_model_dir: default_asr_model_dir(),
            aligner_model_dir: default_aligner_model_dir(),
            backend: default_backend(),
            max_new_tokens: default_max_new_tokens(),
            language: default_source_language(),
            subtitle_length_preset: default_subtitle_length_preset(),
            chunk_target_seconds: default_chunk_target_seconds(),
            output_dir: default_output_dir(),
            save_next_to_source: default_save_next_to_source(),
            output_srt: default_output_srt(),
            output_txt: false,
            text_script: default_text_script(),
            vocal_separation: false,
            demucs_model_dir: default_demucs_model_dir(),
            sound: default_sound(),
            legacy_ui_sound: None,
            legacy_task_notify: None,
        }
    }
}

/// What [`Settings::load_with_report`] found on disk. The GUI logs it at startup,
/// so a pasted `oneasr-error.log` explains fallback-to-defaults and the
/// portable-move model-dir self-heal without guessing.
#[derive(Debug, Default, Clone)]
pub struct SettingsLoadReport {
    pub config_path: Option<PathBuf>,
    /// Non-NotFound read failure (permission, file lock, …).
    pub read_error: Option<String>,
    /// File exists but is not valid settings JSON.
    pub parse_error: Option<String>,
    /// Model-dir repairs applied by `normalize`, as `"old → new"` strings.
    pub repairs: Vec<String>,
}

impl SettingsLoadReport {
    /// True when something notable happened and deserves a log entry.
    pub fn is_notable(&self) -> bool {
        self.read_error.is_some() || self.parse_error.is_some() || !self.repairs.is_empty()
    }
}

impl Settings {
    pub fn config_path() -> Option<PathBuf> {
        if let Some(root) = resolve_app_root() {
            return Some(root.join("settings.json"));
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("settings.json")))
    }

    pub fn load() -> Self {
        Self::load_with_report().0
    }

    /// Load + report why defaults were used, so the GUI can log it and a pasted
    /// `oneasr-error.log` explains "my settings are gone" without guessing.
    pub fn load_with_report() -> (Self, SettingsLoadReport) {
        let mut report = SettingsLoadReport {
            config_path: Self::config_path(),
            ..SettingsLoadReport::default()
        };
        let Some(path) = report.config_path.clone() else {
            return (Self::default(), report);
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(mut s) => {
                    s.normalize_with_notes(&mut report.repairs);
                    (s, report)
                }
                Err(e) => {
                    report.parse_error = Some(e.to_string());
                    (Self::default(), report)
                }
            },
            Err(e) => {
                // Missing file = first run, not an error; keep only real failures.
                if e.kind() != std::io::ErrorKind::NotFound {
                    report.read_error = Some(e.to_string());
                }
                (Self::default(), report)
            }
        }
    }

    /// Reconcile language, ASR catalog id, and model path after load / edit.
    ///
    /// Authority:
    /// 1. If `asr_model_dir` is a known catalog folder → that name **is** `asr_model`.
    /// 2. Else if path empty → install-layout for `asr_model`.
    /// 3. Else custom path → keep path; clamp `asr_model` to a valid catalog id
    ///    for the size picker only (inference still uses the custom path).
    pub fn normalize(&mut self) {
        let mut notes = Vec::new();
        self.normalize_with_notes(&mut notes);
    }

    /// [`Self::normalize`] while recording every self-heal into `notes`.
    pub fn normalize_with_notes(&mut self, notes: &mut Vec<String>) {
        self.language = normalize_source_language(&self.language);
        self.chunk_target_seconds = clamp_chunk_target_seconds(self.chunk_target_seconds);
        self.normalize_output_dir();
        // A muted install stays muted. The two switches of 0.1.9 were merged
        // into one, so the merged switch inherits "on" only when neither family
        // was turned off; this can only ever turn sound *off*, and once the
        // config is saved the legacy keys are gone and `sound` is authoritative.
        if self.sound
            && (self.legacy_ui_sound == Some(false) || self.legacy_task_notify == Some(false))
        {
            self.sound = false;
            notes.push("升级前关过界面音效 / 完成提醒 → 提示音保持关闭".into());
        }
        if self.legacy_ui_sound.is_some() || self.legacy_task_notify.is_some() {
            self.legacy_ui_sound = None;
            self.legacy_task_notify = None;
        }
        // At least one output format must stay enabled.
        if !self.output_srt && !self.output_txt {
            self.output_srt = true;
            notes.push("输出格式全关 → 恢复 SRT".into());
        }
        let script = TextScript::from_id(&self.text_script).id();
        if script != self.text_script {
            self.text_script = script.into();
        }
        repair_stale_model_dir(
            &mut self.demucs_model_dir,
            default_demucs_model_dir(),
            notes,
        );

        if let Some(id) = ModelId::try_from_asr_dir(&self.asr_model_dir) {
            self.asr_model = id.as_str().into();
            repair_stale_model_dir(
                &mut self.asr_model_dir,
                resolve_model_dir(id.as_str()),
                notes,
            );
            repair_stale_model_dir(
                &mut self.aligner_model_dir,
                default_aligner_model_dir(),
                notes,
            );
            return;
        }

        let id = ModelId::parse_asr(&self.asr_model);
        self.asr_model = id.as_str().into();
        if self.asr_model_dir.as_os_str().is_empty() {
            self.asr_model_dir = resolve_model_dir(id.as_str());
        }
        repair_stale_model_dir(
            &mut self.aligner_model_dir,
            default_aligner_model_dir(),
            notes,
        );
    }

    /// Default empty → `{app}/output`; relative → join app root (stable vs CWD).
    fn normalize_output_dir(&mut self) {
        if self.output_dir.as_os_str().is_empty() {
            self.output_dir = default_output_dir();
            return;
        }
        if self.output_dir.is_relative() {
            self.output_dir = resolve_app_root_dir().join(&self.output_dir);
        }
    }

    /// Folder used for finished `.srt` (always non-empty after [`Self::normalize`]).
    pub fn resolved_output_dir(&self) -> PathBuf {
        if self.output_dir.as_os_str().is_empty() {
            return default_output_dir();
        }
        if self.output_dir.is_relative() {
            return resolve_app_root_dir().join(&self.output_dir);
        }
        self.output_dir.clone()
    }

    /// Primary SRT folder for a media file: its own directory when
    /// [`Self::save_next_to_source`] and the file has a parent dir, else
    /// [`Self::resolved_output_dir`]. Callers fall back to
    /// [`Self::resolved_output_dir`] when a write there fails.
    pub fn srt_target_dir(&self, media: &Path) -> PathBuf {
        if self.save_next_to_source
            && let Some(parent) = media.parent()
            && !parent.as_os_str().is_empty()
        {
            return parent.to_path_buf();
        }
        self.resolved_output_dir()
    }

    /// Chunk target used by the pipeline (always within product range).
    #[inline]
    pub fn chunk_target_seconds_clamped(&self) -> u32 {
        clamp_chunk_target_seconds(self.chunk_target_seconds)
    }

    /// Parsed Chinese output script (`simplified` when unset / unknown).
    #[inline]
    pub fn text_script_choice(&self) -> TextScript {
        TextScript::from_id(&self.text_script)
    }

    /// Conversion to apply for a source language, or `None` when the setting
    /// does not apply (non-Chinese sources keep the raw ASR script).
    pub fn text_script_for(&self, lang_key: &str) -> Option<TextScript> {
        text_script::applies_to_language(lang_key).then(|| self.text_script_choice())
    }

    /// Directory that owns the Demucs weights (never empty).
    pub fn resolved_demucs_model_dir(&self) -> PathBuf {
        if self.demucs_model_dir.as_os_str().is_empty() {
            return default_demucs_model_dir();
        }
        self.demucs_model_dir.clone()
    }

    pub fn selected_asr_id(&self) -> ModelId {
        ModelId::parse_asr(&self.asr_model)
    }

    /// Switch active ASR size and bind install-layout path for that size.
    pub fn select_asr_model(&mut self, id: ModelId) {
        if id.kind() != ModelKind::Asr {
            return;
        }
        self.asr_model = id.as_str().into();
        self.asr_model_dir = resolve_model_dir(id.as_str());
    }

    /// Normalize in place, then persist to `settings.json`.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.normalize();
        let path = Self::config_path().ok_or_else(|| "找不到应用目录，无法保存设置".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::media::write_atomic(&path, text).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// True when ASR + Aligner model dirs look complete enough to load.
    pub fn can_start(&self) -> Result<(), String> {
        crate::asr::check_asr_model_dir(&self.asr_model_dir).map_err(|e| e.to_string())?;
        crate::asr::check_aligner_model_dir(&self.aligner_model_dir).map_err(|e| e.to_string())?;
        if self.vocal_separation {
            crate::asr::check_demucs_model_dir(&self.resolved_demucs_model_dir())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// After a successful download: bind paths **only if** this id is the active
    /// selection (or is the single Align slot). Non-selected ASR installs leave
    /// selection alone — files already live under install-layout dirs.
    ///
    /// Returns `true` when active settings changed (caller should save / re-probe).
    pub fn bind_download_if_active(&mut self, id: ModelId, model_dir: PathBuf) -> bool {
        match id.kind() {
            ModelKind::Asr => {
                if self.selected_asr_id() != id {
                    return false;
                }
                self.asr_model = id.as_str().into();
                self.asr_model_dir = model_dir;
                true
            }
            ModelKind::Align => {
                self.aligner_model_dir = model_dir;
                true
            }
            // Weights always land in the install-layout dir the settings
            // already point at; nothing to re-bind.
            ModelKind::Demucs => false,
            ModelKind::CudaRuntime => false,
        }
    }
}

/// If `current` doesn't exist on disk but `default` does, switch to `default`.
///
/// This handles the portable-app case: when the user moves the app folder,
/// stored absolute model paths become stale, but if the models were moved
/// together with the app (same `models/` layout), we auto-repair to the
/// current install-layout location.  Custom user-chosen paths outside the
/// install layout are left alone.
/// Replace `current` with `default` when `current` no longer exists but the
/// install-layout default does. Every actual switch is pushed into `notes` so
/// the GUI can log the repair instead of silently rewriting settings; no-op
/// calls record nothing.
fn repair_stale_model_dir(current: &mut PathBuf, default: PathBuf, notes: &mut Vec<String>) {
    if !current.is_dir() && default.is_dir() {
        notes.push(format!(
            "model dir reset (missing): {} → {}",
            current.display(),
            default.display()
        ));
        *current = default;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QWEN3_ASR_17B;

    #[test]
    fn default_language_and_asr_id() {
        assert_eq!(Settings::default().language, "zh");
        assert_eq!(Settings::default().selected_asr_id(), ModelId::Qwen3Asr06B);
    }

    #[test]
    fn can_start_rejects_missing_model_dirs() {
        let s = Settings {
            asr_model_dir: PathBuf::from(r"D:\__oneasr_no_such_asr__"),
            aligner_model_dir: PathBuf::from(r"D:\__oneasr_no_such_align__"),
            ..Settings::default()
        };
        assert!(s.can_start().is_err());
    }

    #[test]
    fn normalize_syncs_asr_model_from_catalog_path() {
        let mut s = Settings {
            asr_model: QWEN3_ASR_06B.into(),
            asr_model_dir: PathBuf::from(r"C:\App\models\Qwen3-ASR-1.7B"),
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.asr_model, QWEN3_ASR_17B);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
    }

    #[test]
    fn bind_download_only_when_selected() {
        let mut s = Settings::default();
        s.select_asr_model(ModelId::Qwen3Asr17B);
        let dir_06 = PathBuf::from(r"C:\App\models\Qwen3-ASR-0.6B");
        assert!(!s.bind_download_if_active(ModelId::Qwen3Asr06B, dir_06));
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
        assert!(s.asr_model_dir.to_string_lossy().contains("Qwen3-ASR-1.7B"));

        let dir_17 = PathBuf::from(r"C:\App\models\Qwen3-ASR-1.7B");
        assert!(s.bind_download_if_active(ModelId::Qwen3Asr17B, dir_17.clone()));
        assert_eq!(s.asr_model_dir, dir_17);
    }

    #[test]
    fn select_17b_updates_path() {
        let mut s = Settings::default();
        s.select_asr_model(ModelId::Qwen3Asr17B);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
        assert!(s.asr_model_dir.to_string_lossy().contains("Qwen3-ASR-1.7B"));
    }

    #[test]
    fn default_chunk_target_is_product_default() {
        assert_eq!(
            Settings::default().chunk_target_seconds,
            CHUNK_TARGET_DEFAULT_SEC
        );
        assert_eq!(CHUNK_TARGET_DEFAULT_SEC, 60);
    }

    #[test]
    fn normalize_clamps_chunk_target_seconds() {
        let mut s = Settings {
            chunk_target_seconds: 10,
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.chunk_target_seconds, CHUNK_TARGET_MIN_SEC);
        s.chunk_target_seconds = 200;
        s.normalize();
        assert_eq!(s.chunk_target_seconds, CHUNK_TARGET_MAX_SEC);
        s.chunk_target_seconds = 90;
        s.normalize();
        assert_eq!(s.chunk_target_seconds, 90);
    }

    #[test]
    fn presets_are_within_legal_range() {
        for &(sec, label) in CHUNK_TARGET_PRESETS {
            assert_eq!(label, sec.to_string());
            assert_eq!(clamp_chunk_target_seconds(sec), sec);
        }
    }

    #[test]
    fn default_output_dir_ends_with_output() {
        let s = Settings::default();
        assert!(
            s.output_dir.file_name().is_some_and(|n| n == "output"),
            "default output_dir should be …/output, got {}",
            s.output_dir.display()
        );
    }

    #[test]
    fn normalize_fills_empty_output_dir() {
        let mut s = Settings {
            output_dir: PathBuf::new(),
            ..Settings::default()
        };
        s.normalize();
        assert!(!s.output_dir.as_os_str().is_empty());
        assert_eq!(s.resolved_output_dir(), s.output_dir);
    }

    #[test]
    fn default_saves_next_to_source() {
        assert!(Settings::default().save_next_to_source);
    }

    #[test]
    fn srt_target_dir_uses_media_parent() {
        let s = Settings {
            output_dir: PathBuf::from(r"D:\App\output"),
            ..Settings::default()
        };
        assert_eq!(
            s.srt_target_dir(Path::new(r"D:\Movies\lecture.mp4")),
            PathBuf::from(r"D:\Movies")
        );
    }

    #[test]
    fn srt_target_dir_falls_back_without_parent() {
        let s = Settings {
            output_dir: PathBuf::from(r"D:\App\output"),
            ..Settings::default()
        };
        // Bare file name has no usable parent dir → configured output dir.
        assert_eq!(
            s.srt_target_dir(Path::new("lecture.mp4")),
            PathBuf::from(r"D:\App\output")
        );
    }

    #[test]
    fn srt_target_dir_respects_output_dir_mode() {
        let s = Settings {
            save_next_to_source: false,
            output_dir: PathBuf::from(r"D:\App\output"),
            ..Settings::default()
        };
        assert_eq!(
            s.srt_target_dir(Path::new(r"D:\Movies\lecture.mp4")),
            PathBuf::from(r"D:\App\output")
        );
    }

    #[test]
    fn normalize_absolutizes_relative_output_dir() {
        let mut s = Settings {
            output_dir: PathBuf::from("output"),
            ..Settings::default()
        };
        s.normalize();
        assert!(
            s.output_dir.is_absolute(),
            "expected absolute, got {}",
            s.output_dir.display()
        );
        assert!(
            s.output_dir.file_name().is_some_and(|n| n == "output"),
            "expected …/output, got {}",
            s.output_dir.display()
        );
        assert_eq!(s.resolved_output_dir(), s.output_dir);
    }

    #[test]
    fn repair_stale_model_dir_switches_when_default_exists() {
        let tmp = std::env::temp_dir();
        // default exists (temp_dir is a real directory)
        let mut current = PathBuf::from(r"D:\__no_such_dir_for_test__");
        let mut notes = Vec::new();
        repair_stale_model_dir(&mut current, tmp.clone(), &mut notes);
        assert_eq!(current, tmp, "stale path should switch to existing default");
        assert_eq!(notes.len(), 1, "switch must be recorded: {notes:?}");
    }

    #[test]
    fn repair_stale_model_dir_keeps_custom_path_when_neither_exists() {
        let mut current = PathBuf::from(r"D:\__custom_model_path__");
        let default = PathBuf::from(r"D:\__also_missing__");
        let original = current.clone();
        let mut notes = Vec::new();
        repair_stale_model_dir(&mut current, default, &mut notes);
        assert_eq!(
            current, original,
            "custom path should be preserved when neither exists"
        );
        assert!(notes.is_empty(), "no-op repair must not record: {notes:?}");
    }

    #[test]
    fn repair_stale_model_dir_keeps_existing_path() {
        let tmp = std::env::temp_dir();
        let mut current = tmp.clone();
        let default = PathBuf::from(r"D:\__no_such_dir__");
        let mut notes = Vec::new();
        repair_stale_model_dir(&mut current, default, &mut notes);
        assert_eq!(current, tmp, "existing path should not be changed");
        assert!(notes.is_empty(), "no-op repair must not record: {notes:?}");
    }

    #[test]
    fn load_report_is_notable_reflects_errors() {
        // The report contract, without touching the real settings file on disk:
        // any recorded error/repair is notable, a clean load is not.
        let report = SettingsLoadReport {
            config_path: Some(PathBuf::from("unused.json")),
            parse_error: Some("test".into()),
            ..SettingsLoadReport::default()
        };
        assert!(report.is_notable());
        let clean = SettingsLoadReport::default();
        assert!(!clean.is_notable());
    }

    #[test]
    fn output_formats_never_end_up_all_off() {
        let mut s = Settings {
            output_srt: false,
            output_txt: false,
            ..Settings::default()
        };
        let mut notes = Vec::new();
        s.normalize_with_notes(&mut notes);
        assert!(s.output_srt, "SRT must come back when everything is off");
        assert!(!s.output_txt);
        assert!(
            notes.iter().any(|n| n.contains("输出格式")),
            "repair should be reported: {notes:?}"
        );

        // TXT-only is a legal choice and must survive normalization.
        let mut txt_only = Settings {
            output_srt: false,
            output_txt: true,
            ..Settings::default()
        };
        txt_only.normalize();
        assert!(!txt_only.output_srt);
        assert!(txt_only.output_txt);
    }

    #[test]
    fn text_script_defaults_to_original_and_repairs_unknown() {
        assert_eq!(
            Settings::default().text_script_choice(),
            TextScript::Original,
            "fresh installs must not re-write the model's script"
        );

        let mut s = Settings {
            text_script: "zh-Hant".into(),
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.text_script, TextScript::TRADITIONAL_ID);
        assert_eq!(s.text_script_choice(), TextScript::Traditional);

        s.text_script = "wat".into();
        s.normalize();
        assert_eq!(s.text_script, TextScript::ORIGINAL_ID);
    }

    #[test]
    fn script_scope_is_chinese_only() {
        let s = Settings {
            text_script: TextScript::TRADITIONAL_ID.into(),
            ..Settings::default()
        };
        assert_eq!(s.text_script_for("zh"), Some(TextScript::Traditional));
        assert_eq!(s.text_script_for("yue"), Some(TextScript::Traditional));
        assert_eq!(s.text_script_for("ja"), None);
        assert_eq!(s.text_script_for("en"), None);
    }

    #[test]
    fn demucs_dir_defaults_under_models_and_repairs_empty() {
        let mut s = Settings::default();
        assert!(
            s.resolved_demucs_model_dir().ends_with("htdemucs_ft"),
            "default dir should be the install-layout folder: {}",
            s.resolved_demucs_model_dir().display()
        );

        s.demucs_model_dir = PathBuf::new();
        s.normalize();
        assert!(!s.demucs_model_dir.as_os_str().is_empty());
        assert!(s.resolved_demucs_model_dir().is_absolute());
    }

    #[test]
    fn vocal_separation_defaults_off() {
        let mut s = Settings::default();
        assert!(!s.vocal_separation, "sep must be opt-in");
        // Point at a missing dir so the gate is deterministic on any machine.
        s.asr_model_dir = PathBuf::from(r"D:\__oneasr_no_such_asr_dir__");
        assert!(s.can_start().is_err(), "missing ASR dir must fail the gate");
    }

    #[test]
    fn sound_switch_defaults_on_and_normalize_keeps_it() {
        let s = Settings::default();
        assert!(s.sound, "sounds ship on");

        let mut off = Settings {
            sound: false,
            ..Settings::default()
        };
        off.normalize();
        assert!(!off.sound, "normalize must not re-enable sounds");
    }

    #[test]
    fn legacy_settings_without_sound_field_stay_audible() {
        // Upgrading must not silently silence an existing install: the field is
        // `#[serde(default)]`, so an old settings.json keeps sounds on.
        let parsed: Settings =
            serde_json::from_str(r#"{"language":"en"}"#).expect("legacy config parses");
        assert!(parsed.sound, "missing sound must default to on");
    }

    #[test]
    fn a_muted_install_is_not_unmuted_by_the_merged_switch() {
        // 0.1.9 had two switches. Either one being off means the user wanted
        // quiet, and merging them must not hand the noise back.
        let mut muted: Settings =
            serde_json::from_str(r#"{"language":"zh","ui_sound":false,"task_notify":true}"#)
                .expect("0.1.9 config parses");
        muted.normalize();
        assert!(!muted.sound, "关过的音效不能被合并开关重新打开");
        assert!(
            muted.legacy_ui_sound.is_none() && muted.legacy_task_notify.is_none(),
            "legacy keys stop round-tripping after the first load"
        );

        // Both on (or neither present) keeps the shipped default.
        let mut loud: Settings = serde_json::from_str(
            r#"{"language":"zh","ui_sound":true,"task_notify":true}"#,
        )
        .expect("0.1.9 config parses");
        loud.normalize();
        assert!(loud.sound);

        // The new switch itself is never overridden by anything.
        let mut off: Settings =
            serde_json::from_str(r#"{"language":"zh","sound":false}"#).expect("0.2.0 config parses");
        off.normalize();
        assert!(!off.sound);
    }
}
