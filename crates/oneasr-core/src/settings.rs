//! The persisted settings model: what the app remembers between runs, how the
//! values are clamped, and how a damaged file is repaired rather than dropped.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::i18n;
use crate::lang::{default_source_language, normalize_source_language};
use crate::model::{
    ModelId, ModelKind, OMNI_ASR_CTC_300M, QWEN_ALIGN_06B, QWEN3_ASR_06B,
    default_aligner_model_dir, default_asr_model_dir, resolve_model_dir,
};
use crate::paths;
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
/// Persisted as `{data}/settings.json` — the data directory (`crate::paths`),
/// which is the install directory in the portable layout and the platform user
/// directory when the install directory is read-only.
///
/// # ASR selection model
/// - [`Self::asr_model`] is the **active** catalog id (`Qwen3-ASR-0.6B-hf` | `1.7B-hf`).
/// - [`Self::asr_model_dir`] is the path loaded at runtime — always
///   [`Self::asr_dir_for`] of the active size, never stored twice.
/// - A size's directory is either a hand-picked folder (stored **verbatim** in
///   [`Self::asr_dirs`]) or, when nothing was picked for that size, the
///   install layout `{app}/models/{name}`. Nothing else ever resolves a path.
/// - Completing a download for a **non-selected** size only installs files on
///   disk; it must not change the active selection (see
///   [`Self::bind_download_if_active`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Active ASR catalog id (`Qwen3-ASR-0.6B-hf` | `Qwen3-ASR-1.7B-hf`).
    #[serde(default = "default_asr_model")]
    pub asr_model: String,
    /// Directory loaded for inference (install layout or user-picked).
    #[serde(default = "default_asr_model_dir")]
    pub asr_model_dir: PathBuf,

    /// Per-size ASR directories, keyed by catalog id (`Qwen3-ASR-0.6B-hf` …).
    ///
    /// ASR has one directory per **size**, stored exactly as the user picked
    /// it — what you select is what that size loads. A size with no entry
    /// falls back to the install layout. Only deliberate choices live here:
    /// the install-layout default is recomputed from the app location on
    /// every read, and storing it would pin this machine's absolute path
    /// into the config (and break a moved app).
    #[serde(default)]
    pub asr_dirs: BTreeMap<String, PathBuf>,
    /// Active aligner catalog id (`omniASR-CTC-300M-v2-hf` |
    /// `Qwen3-ForcedAligner-0.6B-hf`). Empty = unset (first run): normalize
    /// decides — CTC when its weights are present or nothing is, Qwen when
    /// only Qwen's are — and persists the choice.
    #[serde(default)]
    pub aligner_model: String,
    /// Directory loaded for the aligner (install layout or user-picked).
    #[serde(default = "default_aligner_model_dir")]
    pub aligner_model_dir: PathBuf,
    /// Per-aligner-model directories, keyed by catalog id — same per-model
    /// memory as [`Self::asr_dirs`]; only deliberate picks live here.
    #[serde(default)]
    pub aligner_dirs: BTreeMap<String, PathBuf>,
    /// `auto` | `gpu` | `cpu`
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_max_new_tokens")]
    pub max_new_tokens: usize,
    /// Default source language for **new** tasks (`zh`, `en`, …).
    /// Each task can override; pipeline uses the task language at run time.
    #[serde(default = "default_source_language")]
    pub language: String,
    /// Interface language: `system` (follow the OS) | `zh` | `en`.
    /// Normalized by [`i18n::normalize_ui_language`]; resolved to a concrete
    /// [`i18n::UiLang`] by [`Self::resolved_ui_language`].
    #[serde(default = "default_ui_language")]
    pub ui_language: String,
    /// `short` | `standard` | `loose`
    #[serde(default = "default_subtitle_length_preset")]
    pub subtitle_length_preset: String,
    /// VAD ASR chunk target seconds
    /// ([`CHUNK_TARGET_MIN_SEC`]..=[`CHUNK_TARGET_MAX_SEC`]; default
    /// [`CHUNK_TARGET_DEFAULT_SEC`]).
    #[serde(default = "default_chunk_target_seconds")]
    pub chunk_target_seconds: u32,
    /// Directory for finished `.srt` files. Default: `{data}/output`.
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
    /// Write a karaoke `.ass`: the same cues, a colour sweep per aligned unit.
    #[serde(default)]
    pub output_ass: bool,
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
    #[serde(
        default,
        rename = "task_notify",
        skip_serializing_if = "Option::is_none"
    )]
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

fn default_ui_language() -> String {
    // 跟随系统：zh 系统中文、其余英文；首次运行后用户可在设置里固定。
    i18n::UI_LANGUAGE_SYSTEM.into()
}

fn default_subtitle_length_preset() -> String {
    "standard".into()
}

fn default_chunk_target_seconds() -> u32 {
    CHUNK_TARGET_DEFAULT_SEC
}

fn default_output_dir() -> PathBuf {
    paths::default_output_dir()
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
            asr_dirs: BTreeMap::new(),
            // 自洽：引擎 id 与目录指向同一个模型。留空会让 `selected_aligner_id()`
            // 兜底成默认（CTC）而目录还停在别处，于是「选了 CTC 却往 Qwen 目录加载/
            // 下载」。磁盘上「只装了 Qwen」的情况由 `load_with_report` 的兜底分支
            // 过一遍 `normalize` 来决定，不靠这个默认值猜。
            aligner_model: ModelId::default_aligner().as_str().to_string(),
            aligner_model_dir: default_aligner_model_dir(),
            aligner_dirs: BTreeMap::new(),
            backend: default_backend(),
            max_new_tokens: default_max_new_tokens(),
            language: default_source_language(),
            ui_language: default_ui_language(),
            subtitle_length_preset: default_subtitle_length_preset(),
            chunk_target_seconds: default_chunk_target_seconds(),
            output_dir: default_output_dir(),
            save_next_to_source: default_save_next_to_source(),
            output_srt: default_output_srt(),
            output_txt: false,
            output_ass: false,
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
    /// `settings.json` 实际读到的那份（数据目录优先、安装目录兜底）；都没读到
    /// 时是数据目录里的写路径。写入永远只走数据目录。
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
    /// `settings.json` 的**写**路径：数据目录里（`crate::paths`）。
    ///
    /// 数据目录解析不出来（既没有可写安装目录、也没有平台用户目录）时返回
    /// `None`：那说明这台机器没有别的可写位置，[`Self::save`] 会给出清晰错误，
    /// 而不是悄悄落进操作系统临时目录。
    pub fn config_path() -> Option<PathBuf> {
        crate::paths::data_root()
            .is_ok()
            .then(crate::paths::settings_path)
    }

    pub fn load() -> Self {
        Self::load_with_report().0
    }

    /// Load + report why defaults were used, so the GUI can log it and a pasted
    /// `oneasr-error.log` explains "my settings are gone" without guessing.
    ///
    /// **老数据不丢**：数据目录里的 `settings.json` 优先；读不到时（老用户升级
    /// 前把配置写在了安装目录，或安装目录后来变只读）再兜底读安装目录里的那一份，
    /// 不下沉到默认值。写入永远只走数据目录。
    pub fn load_with_report() -> (Self, SettingsLoadReport) {
        let mut report = SettingsLoadReport {
            config_path: Self::config_path(),
            ..SettingsLoadReport::default()
        };
        let candidates =
            Self::read_candidates(&report.config_path, &paths::install_settings_path());
        match Self::load_first_of(&candidates, &mut report) {
            Some(settings) => (settings, report),
            // 一个候选都没读到：落默认值（原因已记进 report）。
            None => (Self::default_normalized(&mut report), report),
        }
    }

    /// 落默认值的三个回退分支共用的收尾。
    ///
    /// [`Self::default`] 自身是自洽的（引擎 id 与目录指向同一个模型），但
    /// 「磁盘上只装了另一个引擎的权重」这种首启情况要靠 `normalize` 的在盘
    /// 决策来配平。不在这里过一遍 normalize 的话，GUI 会拿着默认引擎的 id
    /// 去默认目录里找权重，1.3 GB 权重也会下错目录。
    fn default_normalized(report: &mut SettingsLoadReport) -> Self {
        let mut s = Self::default();
        s.normalize_with_notes(&mut report.repairs);
        s
    }

    /// 按候选顺序读第一份存在的 `settings.json`；一个都没有就返回 `None`，
    /// 由调用方落默认值。
    ///
    /// 候选列表由调用方注入（[`Self::read_candidates`]），所以"老数据不丢"
    /// 这条兜底能在单测里跑真文件断言，不必去碰进程全局的数据目录。
    fn load_first_of(candidates: &[PathBuf], report: &mut SettingsLoadReport) -> Option<Self> {
        for path in candidates {
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    report.config_path = Some(path.clone());
                    return Some(match serde_json::from_str::<Settings>(&text) {
                        Ok(mut s) => {
                            s.normalize_with_notes(&mut report.repairs);
                            s
                        }
                        Err(e) => {
                            report.parse_error = Some(e.to_string());
                            Self::default_normalized(report)
                        }
                    });
                }
                // 缺失不是错误：继续看下一个候选（老用户升级后第一次读）。
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    report.read_error = Some(e.to_string());
                    report.config_path = Some(path.clone());
                    return Some(Self::default_normalized(report));
                }
            }
        }
        None
    }

    /// 读取顺序：数据目录优先、安装目录兜底；两处同路径时只读一次。
    ///
    /// 纯函数：数据目录那份（`primary`）与老布局那份（`legacy`）都由调用方注入
    /// —— `primary` 在数据目录解析不出来时是 `None` —— 顺序因此可直接单测。
    fn read_candidates(primary: &Option<PathBuf>, legacy: &Path) -> Vec<PathBuf> {
        match primary {
            Some(primary) if primary != legacy => vec![primary.clone(), legacy.to_path_buf()],
            Some(primary) => vec![primary.clone()],
            None => vec![legacy.to_path_buf()],
        }
    }

    /// Reconcile language, ASR catalog id, and model path after load / edit.
    ///
    /// Authority — the active size plus its per-size picks:
    /// 1. `asr_model` picks the size; `asr_dirs[id]` is that size's folder,
    ///    stored exactly as picked.
    /// 2. A size with no pick uses the install layout `{app}/models/{name}`.
    /// 3. `asr_model_dir` is then derived from the two above (never stored
    ///    twice), and a folder that vanished on disk falls back to the install
    ///    layout. A config from before `asr_dirs` existed is seeded from its
    ///    single folder, so a hand-picked path survives the upgrade.
    pub fn normalize(&mut self) {
        let mut notes = Vec::new();
        self.normalize_with_notes(&mut notes);
    }

    /// [`Self::normalize`] while recording every self-heal into `notes`.
    ///
    /// `notes` 是诊断文本：和 `repair_stale_model_dir` 一样写英文，GUI 会用
    /// 英文前缀 `settings repaired: {note}` 落进同一份日志，中英混排不可读。
    pub fn normalize_with_notes(&mut self, notes: &mut Vec<String>) {
        self.language = normalize_source_language(&self.language);
        let ui_lang = i18n::normalize_ui_language(&self.ui_language);
        if ui_lang != self.ui_language {
            self.ui_language = ui_lang;
            notes.push(format!("invalid ui_language -> {}", self.ui_language));
        }
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
            notes.push("legacy ui_sound / task_notify was off -> sound stays off".into());
        }
        if self.legacy_ui_sound.is_some() || self.legacy_task_notify.is_some() {
            self.legacy_ui_sound = None;
            self.legacy_task_notify = None;
        }
        // At least one output format must stay enabled.
        if !self.output_srt && !self.output_txt && !self.output_ass {
            self.output_srt = true;
            notes.push("all output formats were off -> SRT restored".into());
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

        // One-time migration from the root-pick shape: `asr_dirs` used to
        // store the picked *root* while `asr_model_dir` held the sibling
        // folder resolved from it. Rewrite the entry to the folder itself so
        // the pick keeps meaning what the user actually selected.
        let active = ModelId::parse_asr(&self.asr_model);
        if let Some(pick) = self.asr_dirs.get(active.as_str()).cloned()
            && self.asr_model_dir.parent() == Some(pick.as_path())
            && ModelId::try_from_asr_dir(&self.asr_model_dir) == Some(active)
        {
            self.asr_dirs
                .insert(active.as_str().to_string(), self.asr_model_dir.clone());
        }
        // A config written before `asr_dirs` existed carries a single folder:
        // seed it as the active size's pick so a hand-picked path survives the
        // upgrade, and let a catalog-named folder say which size it holds.
        if self.asr_dirs.is_empty() {
            let mut id = ModelId::parse_asr(&self.asr_model);
            if let Some(named) = ModelId::try_from_asr_dir(&self.asr_model_dir)
                && self.asr_model_dir != resolve_model_dir(named.as_str())
            {
                // A catalog-named folder outside the install layout is a
                // deliberate pick — it wins over the stored id. A folder that
                // IS the install layout (e.g. the serde default backing an
                // absent field) must not override the parsed id: the int8
                // variant would otherwise be reset to its fp16 sibling.
                self.asr_model = named.as_str().into();
                id = named;
            }
            let dir = self.asr_model_dir.clone();
            // Install-layout defaults are recomputed per id on every read —
            // the only thing worth seeding is a genuine hand-picked folder.
            if dir != resolve_model_dir(id.as_str())
                && ModelId::try_from_asr_dir(&dir)
                    .is_none_or(|named| dir != resolve_model_dir(named.as_str()))
            {
                self.remember_asr_dir(id, &dir);
            }
        }
        let id = ModelId::parse_asr(&self.asr_model);
        self.asr_model = id.as_str().into();
        // Derived, never stored twice: the active size's own pick, else the
        // install layout.
        self.asr_model_dir = self.asr_dir_for(id);
        repair_stale_model_dir(
            &mut self.asr_model_dir,
            resolve_model_dir(id.as_str()),
            notes,
        );

        // Aligner reconcile — mirrors the ASR block: the catalog id decides
        // the layout default, per-model picks survive, and a legacy
        // single-folder config (before aligner_model/aligner_dirs existed)
        // seeds that model's pick so a hand-picked path survives the upgrade.
        let parsed = ModelId::try_parse_aligner(&self.aligner_model);
        let align_id = match parsed {
            Some(id) => id,
            None => {
                // Unset (first run / upgrade from a config without the
                // field): keep CTC when its weights are on disk or nothing
                // is — the default selection matches the default button;
                // fall back to Qwen only when it is the one that exists.
                let ctc = resolve_model_dir(OMNI_ASR_CTC_300M).is_dir();
                let qwen = resolve_model_dir(QWEN_ALIGN_06B).is_dir();
                let id = decide_default_aligner(ctc, qwen);
                notes.push(format!("aligner model unset -> {}", id.as_str()));
                id
            }
        };
        // 老配置里那**一个**目录属于谁：目录名认得出 catalog 名就对号入座，
        // 认不出（用户自己改过名，GUI 的目录选择器允许）就归给这次决策选中的
        // 模型。两种都得先保住路径 —— 丢掉它等于让升级用户重新找一遍权重，
        // 而下面那行派生赋值会无条件覆盖它。
        //
        // 判据是「不是**任何一个**对齐器的布局默认」而不是「不是选中那个的」：
        // 旧版本写进配置的就是 Qwen 布局目录，那不是用户挑的路径，不该在决策
        // 翻到别的引擎时被记成它的「专属选择」。
        let is_layout_default = ModelId::ALIGNER_CHOICES
            .iter()
            .any(|m| self.aligner_model_dir == resolve_model_dir(m.as_str()));
        if parsed.is_none()
            && self.aligner_dirs.is_empty()
            && !self.aligner_model_dir.as_os_str().is_empty()
            && !is_layout_default
        {
            let picked = self.aligner_model_dir.clone();
            self.remember_aligner_dir(
                ModelId::try_from_aligner_dir(&picked).unwrap_or(align_id),
                &picked,
            );
        }
        self.aligner_model = align_id.as_str().into();
        // Derived, never stored twice: the active model's own pick, else the
        // install layout.
        self.aligner_model_dir = self.aligner_dir_for(align_id);
        repair_stale_model_dir(
            &mut self.aligner_model_dir,
            resolve_model_dir(align_id.as_str()),
            notes,
        );
    }

    /// Default empty → `{data}/output`; relative → join the data directory
    /// (stable vs CWD, and the same place the models live).
    fn normalize_output_dir(&mut self) {
        if self.output_dir.as_os_str().is_empty() {
            self.output_dir = default_output_dir();
            return;
        }
        if self.output_dir.is_relative() {
            self.output_dir = paths::data_dir().join(&self.output_dir);
        }
    }

    /// Folder used for finished `.srt` (always non-empty after [`Self::normalize`]).
    pub fn resolved_output_dir(&self) -> PathBuf {
        if self.output_dir.as_os_str().is_empty() {
            return default_output_dir();
        }
        if self.output_dir.is_relative() {
            return paths::data_dir().join(&self.output_dir);
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

    /// 把 `ui_language` 设置解析成具体语言：`system` 时探测操作系统，
    /// 探测失败保守回退中文（见 [`i18n::detect_system_lang`]）。
    pub fn resolved_ui_language(&self) -> i18n::UiLang {
        match self.ui_language.as_str() {
            i18n::UI_LANGUAGE_ZH => i18n::UiLang::Zh,
            i18n::UI_LANGUAGE_EN => i18n::UiLang::En,
            _ => i18n::detect_system_lang(),
        }
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

    /// True when the active ASR selection is the int8-quantized variant of
    /// its size (`Qwen3-ASR-*-int8`).
    pub fn asr_quantized(&self) -> bool {
        self.selected_asr_id().is_quantized()
    }

    /// Switch the active ASR to the fp16 / int8 variant of the current size.
    ///
    /// Quantized checkpoints are catalog ids of their own, so each of the
    /// four ASR variants (two sizes × fp16/int8) keeps its own directory in
    /// [`Self::asr_dirs`] — same per-size memory as the size switch.
    pub fn set_asr_quantized(&mut self, quantized: bool) {
        let id = self.selected_asr_id().with_quant(quantized);
        if id != self.selected_asr_id() {
            self.select_asr_model(id);
        }
    }

    /// Directory bound to one ASR size: the folder hand-picked for it, else
    /// the layout default ([`resolve_model_dir`] — 数据目录优先)。
    ///
    /// WYSIWYG — no folder-name guessing, no sibling lookups: what the user
    /// picked is what that size loads, and a size nobody picked simply uses
    /// the layout default. Lookups never persist — nothing here writes
    /// `asr_dirs`.
    pub fn asr_dir_for(&self, id: ModelId) -> PathBuf {
        self.asr_dirs
            .get(id.as_str())
            .cloned()
            .unwrap_or_else(|| resolve_model_dir(id.as_str()))
    }

    /// Bind a hand-picked directory to the **active** size, verbatim.
    ///
    /// The pick is stored and loaded exactly as given. Picking never switches
    /// the active size and never re-resolves the folder name — moving between
    /// sizes is [`Self::select_asr_model`]'s job, and a size only ever has two
    /// answers: its own pick or the install layout.
    pub fn set_asr_dir(&mut self, dir: PathBuf) -> ModelId {
        let id = self.selected_asr_id();
        self.remember_asr_dir(id, &dir);
        self.asr_model_dir = dir;
        id
    }

    /// Remember `dir` for `id` — the layout default ([`resolve_model_dir`]) is
    /// *removed* rather than stored: it is recomputed from the resolved data
    /// directory on every read, and keeping it would pin this machine's absolute
    /// path into the config (and break a moved app).
    fn remember_asr_dir(&mut self, id: ModelId, dir: &Path) {
        if dir == resolve_model_dir(id.as_str()).as_path() {
            self.asr_dirs.remove(id.as_str());
        } else {
            self.asr_dirs
                .insert(id.as_str().to_string(), dir.to_path_buf());
        }
    }

    /// Switch active ASR size; every size keeps its own directory — the
    /// folder picked for it, or the layout default when it has none.
    pub fn select_asr_model(&mut self, id: ModelId) {
        if id.kind() != ModelKind::Asr {
            return;
        }
        self.asr_model = id.as_str().into();
        // Per-size memory: switching *back* to a size restores the folder that
        // size was given, instead of resetting it to the layout default.
        self.asr_model_dir = self.asr_dir_for(id);
    }

    // ── Aligner selection（镜像 ASR 尺寸选择的模式）────────────────────

    /// Active aligner catalog id.
    pub fn selected_aligner_id(&self) -> ModelId {
        ModelId::parse_aligner(&self.aligner_model)
    }

    /// Directory bound to one aligner model: the folder hand-picked for it,
    /// else the install layout. Lookups never persist.
    pub fn aligner_dir_for(&self, id: ModelId) -> PathBuf {
        self.aligner_dirs
            .get(id.as_str())
            .cloned()
            .unwrap_or_else(|| resolve_model_dir(id.as_str()))
    }

    /// Switch active aligner model; each model keeps its own directory —
    /// same per-model memory as the ASR size switch.
    pub fn select_aligner_model(&mut self, id: ModelId) {
        if id.kind() != ModelKind::Align {
            return;
        }
        self.aligner_model = id.as_str().into();
        self.aligner_model_dir = self.aligner_dir_for(id);
    }

    /// Bind a hand-picked directory to the **active** aligner model, verbatim.
    pub fn set_aligner_dir(&mut self, dir: PathBuf) -> ModelId {
        let id = self.selected_aligner_id();
        self.aligner_model_dir = dir.clone();
        self.remember_aligner_dir(id, &dir);
        id
    }

    fn remember_aligner_dir(&mut self, id: ModelId, dir: &Path) {
        if dir == resolve_model_dir(id.as_str()).as_path() {
            self.aligner_dirs.remove(id.as_str());
        } else {
            self.aligner_dirs
                .insert(id.as_str().to_string(), dir.to_path_buf());
        }
    }

    /// Qwen 的对齐器输出不带标点，管线从转写文本把标点贴回词上；CTC 对齐器的
    /// token 原生保留标点（零时长搭在前一字符上）。决定对齐阶段是否要做
    /// 标点恢复。
    pub fn aligner_strips_punctuation(&self) -> bool {
        self.selected_aligner_id() == ModelId::QwenAlign06B
    }

    /// Normalize in place, then persist to `settings.json`.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.normalize();
        let path = match Self::config_path() {
            Some(path) => path,
            None => {
                // 清晰错误：说清"没有可写的数据目录"，而不是笼统的"找不到应用目录"，
                // 也不要在这里退回临时目录。
                let reason = crate::paths::data_root()
                    .as_ref()
                    .err()
                    .map(|e| e.message())
                    .unwrap_or("data directory unavailable");
                return Err(crate::i18n::settings_no_writable_dir(reason));
            }
        };
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
    /// selection. A non-selected install leaves the selection alone — its files
    /// already live under install-layout dirs.
    ///
    /// Returns `true` when active settings changed (caller should re-probe).
    /// Nothing new to save: downloads land in the directory the settings already
    /// point at, and that pick is already recorded per model.
    pub fn bind_download_if_active(&mut self, id: ModelId, model_dir: PathBuf) -> bool {
        match id.kind() {
            ModelKind::Asr => {
                if self.selected_asr_id() != id {
                    return false;
                }
                self.asr_model = id.as_str().into();
                self.remember_asr_dir(id, &model_dir);
                self.asr_model_dir = model_dir;
                true
            }
            ModelKind::Align => {
                // 两个对齐引擎并存后，「下载的一定是当前选中那个」不再由类型
                // 保证：非选中引擎的下载不该动当前选择的目录，也不该把它的路径
                // 写进 `aligner_dirs`（那是「用户挑过」的记录，不是下载产物）。
                if self.selected_aligner_id() != id {
                    return false;
                }
                self.remember_aligner_dir(id, &model_dir);
                self.aligner_model_dir = model_dir;
                true
            }
            // Weights always land in the install-layout dir the settings
            // already point at; nothing to re-bind.
            ModelKind::Demucs => false,
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
/// Aligner 模型的首次运行决策：CTC 缺席而 Qwen 在场才退 Qwen，否则一律 CTC
/// （默认按钮 = 默认选择；两者都没装时选 CTC 让下载提示指向默认引擎）。
fn decide_default_aligner(ctc_present: bool, qwen_present: bool) -> ModelId {
    if !ctc_present && qwen_present {
        ModelId::QwenAlign06B
    } else {
        ModelId::default_aligner()
    }
}

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
    fn ui_language_defaults_to_system() {
        let mut s = Settings::default();
        assert_eq!(s.ui_language, i18n::UI_LANGUAGE_SYSTEM);
        s.ui_language = "zh".into();
        assert_eq!(s.resolved_ui_language(), i18n::UiLang::Zh);
        s.ui_language = "en".into();
        assert_eq!(s.resolved_ui_language(), i18n::UiLang::En);
        s.ui_language = "system".into();
        assert!(matches!(
            s.resolved_ui_language(),
            i18n::UiLang::Zh | i18n::UiLang::En
        ));
    }

    #[test]
    fn ui_language_missing_key_defaults_to_system() {
        // 旧版 settings.json 没有这个字段，反序列化必须落到默认值。
        let s: Settings = serde_json::from_str("{}").expect("empty settings object");
        assert_eq!(s.ui_language, i18n::UI_LANGUAGE_SYSTEM);
    }

    #[test]
    fn normalize_repairs_bad_ui_language() {
        let mut s = Settings {
            ui_language: "klingon".into(),
            ..Settings::default()
        };
        let mut notes = Vec::new();
        s.normalize_with_notes(&mut notes);
        assert_eq!(s.ui_language, i18n::UI_LANGUAGE_SYSTEM);
        assert!(notes.iter().any(|n| n.contains("ui_language")));
    }

    #[test]
    fn normalize_keeps_valid_ui_language() {
        let mut s = Settings {
            ui_language: "en".into(),
            ..Settings::default()
        };
        let mut notes = Vec::new();
        s.normalize_with_notes(&mut notes);
        assert_eq!(s.ui_language, "en");
        assert!(!notes.iter().any(|n| n.contains("ui_language")));
    }

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
    fn can_start_requires_demucs_only_when_separation_on() {
        let root =
            std::env::temp_dir().join(format!("oneasr_can_start_demucs_{}", std::process::id()));
        let asr = root.join("asr-test");
        let align = root.join("align-test");
        let demucs = root.join("demucs-test");
        let _ = std::fs::remove_dir_all(&root);
        for dir in [&asr, &align, &demucs] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let blob = vec![0u8; 4096];
        std::fs::write(asr.join("config.json"), &blob).unwrap();
        std::fs::write(asr.join("tokenizer.json"), &blob).unwrap();
        std::fs::write(asr.join("model.safetensors"), &blob).unwrap();
        std::fs::write(align.join("config.json"), &blob).unwrap();
        std::fs::write(align.join("tokenizer.json"), &blob).unwrap();
        std::fs::write(align.join("tokenizer_config.json"), &blob).unwrap();
        std::fs::write(align.join("model.safetensors"), &blob).unwrap();

        let mut s = Settings {
            asr_model_dir: asr,
            aligner_model_dir: align,
            demucs_model_dir: demucs.clone(),
            vocal_separation: false,
            ..Settings::default()
        };
        assert!(s.can_start().is_ok(), "{:?}", s.can_start());

        s.vocal_separation = true;
        let err = crate::i18n::with_ui_lang(crate::i18n::UiLang::Zh, || s.can_start().unwrap_err());
        assert!(err.contains("人声分离"), "{err}");
        assert!(err.contains("htdemucs_ft_vocals.safetensors"), "{err}");

        std::fs::write(
            demucs.join(crate::engine::local::DEMUCS_WEIGHTS_FILE),
            vec![0u8; 4096],
        )
        .unwrap();
        assert!(s.can_start().is_ok(), "{:?}", s.can_start());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normalize_syncs_asr_model_from_catalog_path() {
        let mut s = Settings {
            asr_model: QWEN3_ASR_06B.into(),
            // 目录名匹配靠 file_name()，而 Linux 上 r"C:\App\models\X" 没有
            // 分隔符、整体被当成文件名，解析必然失败。用 join 构造。
            asr_model_dir: Path::new("App").join("models").join("Qwen3-ASR-1.7B-hf"),
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
        let dir_06 = PathBuf::from(r"C:\App\models\Qwen3-ASR-0.6B-hf");
        assert!(!s.bind_download_if_active(ModelId::Qwen3Asr06B, dir_06));
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
        assert!(
            s.asr_model_dir
                .to_string_lossy()
                .contains("Qwen3-ASR-1.7B-hf")
        );

        let dir_17 = PathBuf::from(r"C:\App\models\Qwen3-ASR-1.7B-hf");
        assert!(s.bind_download_if_active(ModelId::Qwen3Asr17B, dir_17.clone()));
        assert_eq!(s.asr_model_dir, dir_17);
    }

    /// 对齐侧与 ASR 侧同一口径：非选中引擎的下载完成不得改动当前选择的目录，
    /// 也不得把它写进 `aligner_dirs`（那是「用户挑过」的记录，不是下载产物）。
    #[test]
    fn bind_aligner_download_only_when_selected() {
        let mut s = Settings::default();
        let ctc = ModelId::OmniAsrCtc300M;
        assert_eq!(s.selected_aligner_id(), ctc);
        let ctc_pick = std::env::temp_dir().join("oneasr_bind_ctc");
        s.set_aligner_dir(ctc_pick.clone());

        // 非选中引擎：目录不动，也不进 per-model 记忆。
        let qwen_dir = std::env::temp_dir().join("oneasr_bind_qwen");
        assert!(!s.bind_download_if_active(ModelId::QwenAlign06B, qwen_dir.clone()));
        assert_eq!(s.aligner_model_dir, ctc_pick);
        assert!(!s.aligner_dirs.contains_key(ModelId::QwenAlign06B.as_str()));

        // 选中的那个：绑定并记住（是用户挑过的目录，不是布局默认）。
        assert!(s.bind_download_if_active(ctc, ctc_pick.clone()));
        assert_eq!(s.aligner_model_dir, ctc_pick);
        assert_eq!(s.aligner_dirs.get(ctc.as_str()), Some(&ctc_pick));
    }

    #[test]
    fn select_17b_updates_path() {
        let mut s = Settings::default();
        s.select_asr_model(ModelId::Qwen3Asr17B);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
        assert!(
            s.asr_model_dir
                .to_string_lossy()
                .contains("Qwen3-ASR-1.7B-hf")
        );
    }

    #[test]
    fn quantized_selection_switches_variant_and_keeps_per_variant_dirs() {
        let mut s = Settings::default();
        assert!(!s.asr_quantized());

        // 0.6B → 0.6B int8: the size stays, the catalog id and default dir change.
        s.set_asr_quantized(true);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr06BInt8);
        assert!(
            s.asr_model_dir
                .to_string_lossy()
                .contains("Qwen3-ASR-0.6B-int8"),
            "{}",
            s.asr_model_dir.display()
        );

        // Each of the four variants remembers its own folder.
        let custom_int8 = PathBuf::from(r"C:\custom\models\Qwen3-ASR-0.6B-int8");
        s.set_asr_dir(custom_int8.clone());
        s.set_asr_quantized(false);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr06B);
        assert!(
            s.asr_model_dir
                .to_string_lossy()
                .contains("Qwen3-ASR-0.6B-hf"),
            "{}",
            s.asr_model_dir.display()
        );
        s.set_asr_quantized(true);
        assert_eq!(s.asr_model_dir, custom_int8);

        // Core selection is a plain catalog id: picking a size always lands on
        // its fp16 id, and the quantized state is a separate, explicit choice
        // (the GUI combines the two when it renders the chips).
        s.select_asr_model(ModelId::Qwen3Asr17B);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
        s.set_asr_quantized(true);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17BInt8);
        assert!(
            s.asr_model_dir
                .to_string_lossy()
                .contains("Qwen3-ASR-1.7B-int8"),
            "{}",
            s.asr_model_dir.display()
        );

        // Setting the same state twice is a no-op (no dir reset).
        let before = s.asr_model_dir.clone();
        s.set_asr_quantized(true);
        assert_eq!(s.asr_model_dir, before);
    }

    #[test]
    fn parse_round_trips_an_int8_active_model() {
        // A settings.json saved with the int8 variant active must load back as
        // exactly that variant, not fall back to fp16 0.6B.
        let mut s: Settings =
            serde_json::from_str(r#"{"asr_model":"Qwen3-ASR-0.6B-int8"}"#).unwrap();
        s.normalize();
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr06BInt8);
        assert!(s.asr_quantized());
    }

    #[test]
    fn each_asr_size_keeps_its_own_dir() {
        let mut s = Settings::default();
        // The macOS report this fixes: models moved out of the app bundle into a
        // custom root, 0.6B pointed at it, then a size switch threw the path back
        // to the install layout and the choice was gone for good.
        let custom = PathBuf::from(r"C:\custom-models\Qwen3-ASR-0.6B-hf");
        s.set_asr_dir(custom.clone());
        assert_eq!(s.asr_model_dir, custom);

        s.select_asr_model(ModelId::Qwen3Asr17B);
        // Nothing was ever chosen for 1.7B, so the install layout answers…
        assert!(s.asr_model_dir.to_string_lossy().contains(QWEN3_ASR_17B));
        // …but switching back must restore 0.6B's own folder.
        s.select_asr_model(ModelId::Qwen3Asr06B);
        assert_eq!(s.asr_model_dir, custom);
    }

    #[test]
    fn no_sibling_lookup_the_install_layout_is_the_only_default() {
        // A custom root for 0.6B says nothing about 1.7B: with no pick of its
        // own, 1.7B gets the install layout — never the folder next door.
        let root = std::env::temp_dir().join(format!("oneasr_no_sibling_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(QWEN3_ASR_06B)).unwrap();
        std::fs::create_dir_all(root.join(QWEN3_ASR_17B)).unwrap();

        let mut s = Settings::default();
        s.set_asr_dir(root.join(QWEN3_ASR_06B));
        s.select_asr_model(ModelId::Qwen3Asr17B);
        assert!(
            s.asr_model_dir.to_string_lossy().contains(QWEN3_ASR_17B),
            "{}",
            s.asr_model_dir.display()
        );
        assert!(
            !s.asr_model_dir.starts_with(&root),
            "must not reach into the 0.6B root: {}",
            s.asr_model_dir.display()
        );
        // A lookup is not a decision: nothing is written for 1.7B.
        assert!(!s.asr_dirs.contains_key(QWEN3_ASR_17B), "{:?}", s.asr_dirs);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn set_asr_dir_is_verbatim_and_keeps_the_active_size() {
        // WYSIWYG: the pick is stored and loaded exactly as given, and it
        // binds to the size that was active — a folder named after the other
        // size must not hijack the selection.
        let mut s = Settings::default();
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr06B);
        let picked = PathBuf::from(r"D:\my-models\Qwen3-ASR-1.7B-hf");
        let id = s.set_asr_dir(picked.clone());
        assert_eq!(id, ModelId::Qwen3Asr06B);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr06B);
        assert_eq!(s.asr_model_dir, picked);
        assert_eq!(s.asr_dirs.get(QWEN3_ASR_06B), Some(&picked));
    }

    #[test]
    fn picking_the_install_layout_dir_clears_the_pick() {
        // Pointing a size back at the install layout is "no pick" — the entry
        // must go, or the config pins this machine's absolute path.
        let mut s = Settings::default();
        let custom = PathBuf::from(r"D:\my-models\Qwen3-ASR-0.6B-hf");
        s.set_asr_dir(custom);
        assert!(s.asr_dirs.contains_key(QWEN3_ASR_06B));

        s.set_asr_dir(default_asr_model_dir());
        assert!(!s.asr_dirs.contains_key(QWEN3_ASR_06B), "{:?}", s.asr_dirs);
        assert_eq!(s.asr_model_dir, default_asr_model_dir());
    }

    #[test]
    fn normalize_migrates_the_old_root_pick_to_the_resolved_folder() {
        // The pre-1.0.3 shape: `asr_dirs` holds the picked root while
        // `asr_model_dir` holds the sibling resolved from it. The entry is
        // rewritten to the folder so the pick keeps meaning what was chosen.
        let root = std::env::temp_dir().join(format!("oneasr_migrate_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join(QWEN3_ASR_06B);
        std::fs::create_dir_all(&folder).unwrap();

        let mut s = Settings::default();
        s.asr_dirs.insert(QWEN3_ASR_06B.to_string(), root.clone());
        s.asr_model_dir = folder.clone();
        s.normalize();
        assert_eq!(s.asr_dirs.get(QWEN3_ASR_06B), Some(&folder));
        assert_eq!(s.asr_model_dir, folder);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normalize_seeds_a_legacy_custom_dir_instead_of_clobbering_it() {
        // A config from before `asr_dirs` existed: one hand-picked folder
        // with no catalog name. It survives the upgrade instead of being
        // reset to the install layout.
        let custom = std::env::temp_dir().join(format!("oneasr_legacy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&custom);
        std::fs::create_dir_all(&custom).unwrap();

        let mut s = Settings::default();
        s.asr_dirs.clear();
        s.asr_model_dir = custom.clone();
        s.normalize();
        assert_eq!(s.asr_dirs.get(QWEN3_ASR_06B), Some(&custom));
        assert_eq!(s.asr_model_dir, custom);

        let _ = std::fs::remove_dir_all(&custom);
    }

    #[test]
    fn install_layout_defaults_stay_out_of_the_config() {
        // Storing the default would pin this machine's absolute path (the macOS
        // install lives inside the .app), so only hand-picked folders are kept.
        let mut s = Settings::default();
        s.normalize();
        assert!(s.asr_dirs.is_empty(), "{:?}", s.asr_dirs);

        s.set_asr_dir(default_asr_model_dir());
        assert!(s.asr_dirs.is_empty(), "{:?}", s.asr_dirs);
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
        // 路径用 join 构造：写死 r"D:\Movies\lecture.mp4" 的话，Linux 上它没有
        // 分隔符、`parent()` 返回空，这条测试根本走不到「用父目录」那个分支，
        // 变成在验「Windows 路径在 Linux 上没有父目录」。
        let movies = std::env::temp_dir().join("Movies");
        let s = Settings {
            output_dir: std::env::temp_dir().join("App").join("output"),
            ..Settings::default()
        };
        assert_eq!(s.srt_target_dir(&movies.join("lecture.mp4")), movies);
    }

    #[test]
    fn srt_target_dir_falls_back_without_parent() {
        // 输出目录用绝对路径（temp_dir 下），否则 resolved_output_dir() 会把它
        // 相对当前目录补全，期望值就随 cwd 漂移。
        let out = std::env::temp_dir().join("App").join("output");
        let s = Settings {
            output_dir: out.clone(),
            ..Settings::default()
        };
        // Bare file name has no usable parent dir → configured output dir.
        assert_eq!(s.srt_target_dir(Path::new("lecture.mp4")), out);
    }

    #[test]
    fn srt_target_dir_respects_output_dir_mode() {
        let out = std::env::temp_dir().join("App").join("output");
        let s = Settings {
            save_next_to_source: false,
            output_dir: out.clone(),
            ..Settings::default()
        };
        let media = std::env::temp_dir().join("Movies").join("lecture.mp4");
        assert_eq!(s.srt_target_dir(&media), out);
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
    fn default_aligner_decision_prefers_ctc_unless_only_qwen_exists() {
        use ModelId::OmniAsrCtc300M as Ctc;
        use ModelId::QwenAlign06B as Qwen;
        assert_eq!(decide_default_aligner(false, false), Ctc);
        assert_eq!(decide_default_aligner(true, false), Ctc);
        assert_eq!(decide_default_aligner(true, true), Ctc);
        // Qwen is the fallback only when it is the one that exists.
        assert_eq!(decide_default_aligner(false, true), Qwen);
    }

    #[test]
    fn aligner_selection_keeps_per_model_dirs() {
        let mut s = Settings {
            aligner_model: ModelId::OmniAsrCtc300M.as_str().into(),
            ..Settings::default()
        };
        assert!(!s.aligner_strips_punctuation());

        let ctc_custom =
            std::env::temp_dir().join(format!("oneasr_ctc_dir_{}", std::process::id()));
        let qwen_custom =
            std::env::temp_dir().join(format!("oneasr_qwen_dir_{}", std::process::id()));
        s.set_aligner_dir(ctc_custom.clone());
        assert_eq!(s.selected_aligner_id(), ModelId::OmniAsrCtc300M);
        s.select_aligner_model(ModelId::QwenAlign06B);
        assert!(s.aligner_strips_punctuation());
        s.set_aligner_dir(qwen_custom.clone());
        assert_eq!(s.selected_aligner_id(), ModelId::QwenAlign06B);

        // Switching back restores each model's own pick.
        s.select_aligner_model(ModelId::OmniAsrCtc300M);
        assert_eq!(s.aligner_model_dir, ctc_custom);
        s.select_aligner_model(ModelId::QwenAlign06B);
        assert_eq!(s.aligner_model_dir, qwen_custom);
        assert_eq!(s.aligner_dirs.len(), 2);

        let _ = std::fs::remove_dir_all(ctc_custom);
        let _ = std::fs::remove_dir_all(qwen_custom);
    }

    /// 默认值必须自洽：引擎 id 与目录指向**同一个**模型。不自洽时 GUI 会点着
    /// CTC 的按钮、把权重下进 Qwen 目录，再按 Qwen 的文件表校验并报「不完整」。
    #[test]
    fn default_settings_pair_the_aligner_id_with_its_dir() {
        let s = Settings::default();
        let id = s.selected_aligner_id();
        assert_eq!(id, ModelId::default_aligner());
        assert_eq!(s.aligner_model_dir, resolve_model_dir(id.as_str()));
        // 标点恢复的分流也跟着同一个 id 走，不看目录。
        assert_eq!(s.aligner_strips_punctuation(), id == ModelId::QwenAlign06B);
    }

    /// 老配置（无 `aligner_model` 字段）里手挑的目录必须在升级后活下来，
    /// 两种目录名都要保：catalog 原名按号入座，自改名归给决策选中的模型。
    /// 目录名认不出来时若直接丢路径，升级用户就得重新找一遍权重。
    #[test]
    fn legacy_aligner_dir_survives_normalize_under_any_folder_name() {
        // 目录必须是**真实存在**的：不存在时 `repair_stale_model_dir` 会按设计
        // 回退到布局默认，测的就不是「保住手挑路径」这条规则了。
        let root =
            std::env::temp_dir().join(format!("oneasr_aligner_legacy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in [QWEN_ALIGN_06B, "my-own-aligner"] {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();

            let mut s: Settings =
                serde_json::from_str(r#"{"language":"zh"}"#).expect("legacy config parses");
            s.aligner_model_dir = dir.clone();
            s.normalize();

            // 归属：认得出 catalog 名就对号，否则归给这次决策选中的模型。
            let owner =
                ModelId::try_from_aligner_dir(&dir).unwrap_or_else(|| s.selected_aligner_id());
            assert_eq!(
                s.aligner_dirs.get(owner.as_str()),
                Some(&dir),
                "{name} 的目录没被记进 aligner_dirs：{:?}",
                s.aligner_dirs
            );
            // 选中的正是那个模型时，派生目录就是它自己挑的那个。
            if owner == s.selected_aligner_id() {
                assert_eq!(s.aligner_model_dir, dir);
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_settings_without_aligner_fields_parse_and_decide() {
        let json = r#"{"language": "zh"}"#;
        let mut s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.aligner_model, "");
        s.normalize();
        // 决策落回配置里成为显式 catalog id，且 id 与目录仍然配平。
        let id = ModelId::try_parse_aligner(&s.aligner_model).expect("decision persisted");
        assert!(id.kind() == ModelKind::Align);
        // 布局目录不是用户挑的（这台机器上没挑过），所以不占 `aligner_dirs`。
        assert!(s.aligner_dirs.is_empty(), "{:?}", s.aligner_dirs);
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

    /// 候选顺序：数据目录优先、安装目录兜底；两处同路径只读一次。
    #[test]
    fn read_candidates_prefers_data_dir_then_legacy() {
        let data = PathBuf::from(r"D:\data\settings.json");
        let legacy = PathBuf::from(r"D:\app\settings.json");
        assert_eq!(
            Settings::read_candidates(&Some(data.clone()), &legacy),
            vec![data.clone(), legacy.clone()]
        );
        // 便携布局：两处其实是同一个文件，只读一次。
        assert_eq!(
            Settings::read_candidates(&Some(data.clone()), &data),
            vec![data.clone()]
        );
        // 数据目录解析不出来（既无写权限又无平台用户目录）时，只剩安装目录兜底。
        assert_eq!(Settings::read_candidates(&None, &legacy), vec![legacy]);
    }

    /// 临时目录里造一对 `(数据目录 settings.json, 安装目录 settings.json)`。
    fn scratch_settings_pair(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("oneasr_{label}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = root.join("data").join(paths::SETTINGS_FILE);
        let legacy = root.join("install").join(paths::SETTINGS_FILE);
        (root, data, legacy)
    }

    fn write_settings(path: &Path, secs: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let s = Settings {
            chunk_target_seconds: secs,
            ..Settings::default()
        };
        std::fs::write(path, serde_json::to_string(&s).unwrap()).unwrap();
    }

    /// 数据目录里没有 `settings.json`（老用户升级，或安装目录后来变只读）→
    /// 仍读到安装目录里那一份，"老数据不丢"。
    #[test]
    fn load_falls_back_to_install_settings() {
        let (root, data, legacy) = scratch_settings_pair("settings_legacy");
        write_settings(&legacy, 90);
        // `data` 故意不创建：读它必然 NotFound，候选要顺延到 legacy。

        let mut report = SettingsLoadReport::default();
        let candidates = Settings::read_candidates(&Some(data), &legacy);
        let loaded = Settings::load_first_of(&candidates, &mut report).expect("legacy settings");
        assert_eq!(loaded.chunk_target_seconds, 90);
        assert_eq!(report.config_path.as_deref(), Some(legacy.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 两份都在 → 数据目录那份优先（写入也只走那里，安装目录那份不动）。
    #[test]
    fn load_prefers_data_dir_settings_over_install_copy() {
        let (root, data, legacy) = scratch_settings_pair("settings_both");
        write_settings(&legacy, 30);
        write_settings(&data, 120);

        let mut report = SettingsLoadReport::default();
        let candidates = Settings::read_candidates(&Some(data.clone()), &legacy);
        let loaded = Settings::load_first_of(&candidates, &mut report).expect("data settings");
        assert_eq!(loaded.chunk_target_seconds, 120);
        assert_eq!(report.config_path.as_deref(), Some(data.as_path()));

        let _ = std::fs::remove_dir_all(&root);
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
            notes.iter().any(|n| n.contains("output format")),
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

        // ASS alone is a legal choice too — it carries the same cues as the SRT.
        let mut ass_only = Settings {
            output_srt: false,
            output_txt: false,
            output_ass: true,
            ..Settings::default()
        };
        ass_only.normalize();
        assert!(ass_only.output_ass, "ASS-only must survive normalization");
        assert!(!ass_only.output_srt);
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
        let default = crate::model::default_demucs_model_dir();
        let mut s = Settings::default();
        assert!(
            s.resolved_demucs_model_dir().ends_with("htdemucs_ft"),
            "default dir should be the install-layout folder: {}",
            s.resolved_demucs_model_dir().display()
        );

        s.demucs_model_dir = PathBuf::new();
        s.normalize();
        // 修复只在「默认目录确实存在」时发生 —— 这是刻意的：默认目录
        // 不存在时保留空值，好过替用户编一个不存在的路径。因此按默认
        // 目录是否存在分别断言 —— 不能假设它一定在：CI 的 checkout 里
        // models/ 被 .gitignore 了，没有 htdemucs_ft，写死会在 Linux 上失败。
        if default.is_dir() {
            assert_eq!(
                s.demucs_model_dir, default,
                "默认目录存在时，空值应被修复成它"
            );
        } else {
            assert!(
                s.demucs_model_dir.as_os_str().is_empty(),
                "默认目录不存在时不应凭空造路径: {:?}",
                s.demucs_model_dir
            );
        }
        // 无论走哪条分支，读出来的都必须落在 htdemucs_ft 上（空值回落到默认）。
        assert!(
            s.resolved_demucs_model_dir().ends_with("htdemucs_ft"),
            "resolved dir should fall back to the install-layout folder: {}",
            s.resolved_demucs_model_dir().display()
        );
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
        let mut loud: Settings =
            serde_json::from_str(r#"{"language":"zh","ui_sound":true,"task_notify":true}"#)
                .expect("0.1.9 config parses");
        loud.normalize();
        assert!(loud.sound);

        // The new switch itself is never overridden by anything.
        let mut off: Settings = serde_json::from_str(r#"{"language":"zh","sound":false}"#)
            .expect("0.2.0 config parses");
        off.normalize();
        assert!(!off.sound);
    }
}
