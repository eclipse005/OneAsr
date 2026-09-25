//! 界面双语（zh / en）机制 + 本 crate 内产生文案的双语表。
//!
//! 设计约束：
//! - **零依赖、无 key 查找**：每条文案是成对的 [`Str`]，编译期保证 zh/en 同时存在；
//!   取词用 [`t`]，当前语言是进程级状态（[`set_ui_lang`] / [`ui_lang`]）。
//! - **管线自产的文案（错误、阶段名、下载进度、统计时长）集中在本文件**；
//!   界面标签（按钮、区块标题）属于 GUI crate，在其自己的 `i18n` 模块里建表，
//!   复用本模块的 [`Str`] 与 [`t`]。
//! - **纯格式化函数显式接收 [`UiLang`] 参数**（如统计时长），不在函数内读全局，
//!   这样单测无共享状态；全局只在渲染/动作边界与 `Display` 实现里读取。
//! - 语言设置持久化在 [`crate::settings::Settings::ui_language`]
//!   （`system` | `zh` | `en`），`system` 表示跟随系统（[`detect_system_lang`]）。

use std::sync::atomic::{AtomicU8, Ordering};

/// `Settings::ui_language`：跟随系统。
pub const UI_LANGUAGE_SYSTEM: &str = "system";
/// `Settings::ui_language`：中文。
pub const UI_LANGUAGE_ZH: &str = "zh";
/// `Settings::ui_language`：English。
pub const UI_LANGUAGE_EN: &str = "en";

/// 界面语言。文案只有中英两份，其他系统语言一律落到英文。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLang {
    #[default]
    Zh,
    En,
}

impl UiLang {
    /// 从 BCP-47 风格语言代码解析（`zh` / `zh-CN` / `en-US` / `C`…）。
    /// 非 `zh*` 一律视为英文档。
    pub fn from_code(code: &str) -> UiLang {
        let lower = code.to_ascii_lowercase();
        if lower.starts_with("zh") {
            UiLang::Zh
        } else {
            UiLang::En
        }
    }

    /// 存进 `Settings::ui_language` 的稳定值。
    pub fn setting_value(self) -> &'static str {
        match self {
            UiLang::Zh => UI_LANGUAGE_ZH,
            UiLang::En => UI_LANGUAGE_EN,
        }
    }
}

/// 当前界面语言：`0` = zh，`1` = en（默认中文，启动时由设置覆盖）。
static UI_LANG: AtomicU8 = AtomicU8::new(0);

/// 设置进程级界面语言（启动时与设置保存成功后调用）。
pub fn set_ui_lang(lang: UiLang) {
    UI_LANG.store(lang as u8, Ordering::Relaxed);
}

/// 读取进程级界面语言。渲染与文案构造边界使用；纯函数请改为显式传参。
pub fn ui_lang() -> UiLang {
    if UI_LANG.load(Ordering::Relaxed) == UiLang::En as u8 {
        UiLang::En
    } else {
        UiLang::Zh
    }
}

/// 一条双语文案。zh 与 en 必须成对书写，新增文案缺一不可。
#[derive(Debug, Clone, Copy)]
pub struct Str {
    pub zh: &'static str,
    pub en: &'static str,
}

impl Str {
    pub const fn new(zh: &'static str, en: &'static str) -> Self {
        Self { zh, en }
    }
}

/// 按当前界面语言取词。
pub fn t(s: Str) -> &'static str {
    match ui_lang() {
        UiLang::Zh => s.zh,
        UiLang::En => s.en,
    }
}

/// 归一化 `Settings::ui_language`：只允许 `system` / `zh` / `en`，
/// 其余（含损坏值）回落 `system`。大小写与常见书写形式都能识别。
pub fn normalize_ui_language(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "zh" | "zh-cn" | "zh-hans" => UI_LANGUAGE_ZH.into(),
        "en" => UI_LANGUAGE_EN.into(),
        _ => UI_LANGUAGE_SYSTEM.into(),
    }
}

/// 探测系统语言（首次启动 `ui_language = "system"` 时用）。
/// 探测失败时保守回退中文。
pub fn detect_system_lang() -> UiLang {
    match system_lang_code() {
        Some(code) => UiLang::from_code(&code),
        None => UiLang::Zh,
    }
}

// ─── 管线自产文案的双语表 ────────────────────────────────────────────
//
// 这些消息在 core 内构造（错误 Display、阶段更新、下载进度、模型校验），
// 构造时按进程当前语言定格。带参数的句子写成小函数；纯枚举标签在各自
// 类型的 `label(UiLang)` 方法里。

/// 模型角色名（模型校验 / 完整性错误共用）。
pub const ROLE_ASR: Str = Str::new("语音识别", "ASR");
/// 见 [`ROLE_ASR`]。
pub const ROLE_ALIGNER: Str = Str::new("对齐", "Aligner");
/// 见 [`ROLE_ASR`]。
pub const ROLE_DEMUCS: Str = Str::new("人声分离", "Vocal separation");

// ---- AsrError ----
pub const ERR_IO: Str = Str::new("I/O 错误", "I/O error");
pub const ERR_MEDIA: Str = Str::new("音频处理错误", "Audio processing error");
pub const ERR_EMPTY_ALIGNMENT: Str =
    Str::new("对齐后词列表为空", "Word list is empty after alignment");
pub const ERR_EMPTY_SENTENCE_BOUNDARY: Str =
    Str::new("断句后字幕为空", "Subtitle is empty after sentence segmentation");
pub const ERR_READ_DURATION: Str =
    Str::new("无法读取转码后音频时长", "Cannot read the converted audio duration");
pub const ERR_NO_OUTPUT_FORMAT: Str =
    Str::new("未启用任何输出格式", "No output format is enabled");

/// `语音识别未产生任何有效文本（{n} 段全部为空）`
/// / `Transcription produced no usable text ({n} segments, all empty)`。
pub fn empty_transcribe(n: usize) -> String {
    match ui_lang() {
        UiLang::Zh => format!("语音识别未产生任何有效文本（{n} 段全部为空）"),
        UiLang::En => format!("Transcription produced no usable text ({n} segments, all empty)"),
    }
}

/// `加载语音识别模型失败: {e}` / `Failed to load the ASR model: {e}`。
pub fn load_asr_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("加载语音识别模型失败: {e}"),
        UiLang::En => format!("Failed to load the ASR model: {e}"),
    }
}

/// `加载对齐模型失败: {e}` / `Failed to load the aligner model: {e}`。
pub fn load_aligner_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("加载对齐模型失败: {e}"),
        UiLang::En => format!("Failed to load the aligner model: {e}"),
    }
}

/// `转写失败 chunk {i}: {e}` / `Transcription failed for chunk {i}: {e}`。
pub fn transcribe_chunk(i: usize, e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("转写失败 chunk {i}: {e}"),
        UiLang::En => format!("Transcription failed for chunk {i}: {e}"),
    }
}

/// `对齐失败 chunk {i}（{inner}）: {e}`
/// / `Alignment failed for chunk {i} ({inner}): {e}`。
pub fn align_chunk(i: usize, inner: &str, e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("对齐失败 chunk {i}（{inner}）: {e}"),
        UiLang::En => format!("Alignment failed for chunk {i} ({inner}): {e}"),
    }
}

/// `{role}模型文件不完整（{missing}）: {dir}`
/// / `{role} model files are incomplete ({missing}): {dir}`。
pub fn model_incomplete(role: Str, missing: &str, dir: &str) -> String {
    let name = t(role);
    match ui_lang() {
        UiLang::Zh => format!("{name}模型文件不完整（{missing}）: {dir}"),
        UiLang::En => format!("{name} model files are incomplete ({missing}): {dir}"),
    }
}

/// `[{stage}] I/O 错误: {e}` / `[{stage}] I/O error: {e}`。
pub fn stage_io_error(stage: &str, e: &std::io::Error) -> String {
    match ui_lang() {
        UiLang::Zh => format!("[{stage}] I/O 错误: {e}"),
        UiLang::En => format!("[{stage}] I/O error: {e}"),
    }
}

/// `人声分离 GPU 不可用，已改用 CPU（速度明显变慢）：{reason}`
/// / `Vocal separation: GPU unavailable, falling back to CPU (much slower): {reason}`。
pub fn sep_gpu_fallback(reason: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("人声分离 GPU 不可用，已改用 CPU（速度明显变慢）：{reason}"),
        UiLang::En => {
            format!("Vocal separation: GPU unavailable, falling back to CPU (much slower): {reason}")
        }
    }
}

// ---- 模型目录校验 ----

/// `{role} 模型目录不存在: {dir}` / `{role} model folder not found: {dir}`。
pub fn model_dir_missing(role: Str, dir: &std::path::Path) -> String {
    let name = t(role);
    match ui_lang() {
        UiLang::Zh => format!("{name} 模型目录不存在: {}", dir.display()),
        UiLang::En => format!("{name} model folder not found: {}", dir.display()),
    }
}

/// `{name}（不存在）` / `{name} (missing)`。
pub fn file_missing(name: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{name}（不存在）"),
        UiLang::En => format!("{name} (missing)"),
    }
}

/// `{name}（{actual}/{expected} 字节）` / `{name} ({actual}/{expected} bytes)`。
pub fn file_size_mismatch(name: &str, actual: u64, expected: u64) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{name}（{actual}/{expected} 字节）"),
        UiLang::En => format!("{name} ({actual}/{expected} bytes)"),
    }
}

/// `{name} (过小: {len} 字节)` / `{name} (too small: {len} bytes)`。
pub fn file_too_small(name: &str, len: u64) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{name} (过小: {len} 字节)"),
        UiLang::En => format!("{name} (too small: {len} bytes)"),
    }
}

/// `{name} (不存在)` / `{name} (not found)`。
pub fn file_absent(name: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{name} (不存在)"),
        UiLang::En => format!("{name} (not found)"),
    }
}

/// `{names}… (共 {total} 项)` / `{names}… ({total} in total)`。
pub fn missing_summary(names: &str, total: usize) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{names}… (共 {total} 项)"),
        UiLang::En => format!("{names}… ({total} in total)"),
    }
}

/// weight_map 为空的提示（文件名前缀通用，无需翻译文件名本身）。
pub fn weight_map_empty() -> String {
    match ui_lang() {
        UiLang::Zh => "model.safetensors.index.json (weight_map 为空)".into(),
        UiLang::En => "model.safetensors.index.json (empty weight_map)".into(),
    }
}

// ---- 人声分离 / 引擎 ----

/// `人声分离模型不存在: {dir}（请在设置中下载）`
/// / `Vocal-separation model not found: {dir} (download it in Settings)`。
pub fn demucs_model_missing(dir: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("人声分离模型不存在: {}（请在设置中下载）", dir.display()),
        UiLang::En => format!(
            "Vocal-separation model not found: {} (download it in Settings)",
            dir.display()
        ),
    }
}

/// `人声分离 GPU 加载失败，改用 CPU: {e}`
/// / `Vocal separation: GPU load failed, using CPU: {e}`。
pub fn sep_gpu_load_failed_cpu(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("人声分离 GPU 加载失败，改用 CPU: {e}"),
        UiLang::En => format!("Vocal separation: GPU load failed, using CPU: {e}"),
    }
}

/// `加载人声分离模型失败（{backend}）: {e}`
/// / `Failed to load the vocal-separation model ({backend}): {e}`。
pub fn sep_load_failed(backend: &str, e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("加载人声分离模型失败（{backend}）: {e}"),
        UiLang::En => format!("Failed to load the vocal-separation model ({backend}): {e}"),
    }
}

/// `GPU 加载失败` / `GPU load failed`（FellBackToCpu 的 reason）。
pub fn sep_gpu_load_failed_reason() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "GPU 加载失败",
        UiLang::En => "GPU load failed",
    }
}

/// `人声分离失败: {e}` / `Vocal separation failed: {e}`。
pub fn sep_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("人声分离失败: {e}"),
        UiLang::En => format!("Vocal separation failed: {e}"),
    }
}

/// `人声分离没有返回 vocals 轨道`
/// / `Vocal separation returned no vocals track`。
pub fn sep_no_vocals() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "人声分离没有返回 vocals 轨道",
        UiLang::En => "Vocal separation returned no vocals track",
    }
}

/// `读取分离输入失败 {path}: {e}` / `Failed to read the separation input {path}: {e}`。
pub fn sep_read_input_failed(path: &std::path::Path, e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("读取分离输入失败 {}: {e}", path.display()),
        UiLang::En => format!("Failed to read the separation input {}: {e}", path.display()),
    }
}

/// `读取分离输入失败: {e}` / `Failed to read the separation input: {e}`。
pub fn sep_read_input_failed_bare(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("读取分离输入失败: {e}"),
        UiLang::En => format!("Failed to read the separation input: {e}"),
    }
}

/// `分离输入没有声道` / `Separation input has no channels`。
pub fn sep_no_channels() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "分离输入没有声道",
        UiLang::En => "Separation input has no channels",
    }
}

/// `写入人声轨道失败 {path}: {e}`
/// / `Failed to write the vocals track {path}: {e}`。
pub fn sep_write_failed(path: &std::path::Path, e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("写入人声轨道失败 {}: {e}", path.display()),
        UiLang::En => format!("Failed to write the vocals track {}: {e}", path.display()),
    }
}

/// `写入人声轨道失败: {e}` / `Failed to write the vocals track: {e}`。
pub fn sep_write_failed_bare(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("写入人声轨道失败: {e}"),
        UiLang::En => format!("Failed to write the vocals track: {e}"),
    }
}

/// `音频路径非 UTF-8` / `Audio path is not valid UTF-8`。
pub fn audio_path_not_utf8() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "音频路径非 UTF-8",
        UiLang::En => "Audio path is not valid UTF-8",
    }
}

/// `对齐器被占用` / `The aligner is busy`。
pub fn aligner_busy() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "对齐器被占用",
        UiLang::En => "The aligner is busy",
    }
}

/// `未检测到可用 GPU（需要已安装的显卡驱动）`
/// / `No usable GPU detected (an installed graphics driver is required)`。
pub fn no_gpu_detected() -> String {
    match ui_lang() {
        UiLang::Zh => "未检测到可用 GPU（需要已安装的显卡驱动）".into(),
        UiLang::En => "No usable GPU detected (an installed graphics driver is required)".into(),
    }
}

/// `{e}。GPU 加速需要已安装的显卡驱动；或在设置中改用 CPU`
/// / `{e}. GPU acceleration requires an installed graphics driver; or switch to CPU in Settings`。
pub fn gpu_forced_but_unavailable(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{e}。GPU 加速需要已安装的显卡驱动；或在设置中改用 CPU"),
        UiLang::En => format!(
            "{e}. GPU acceleration requires an installed graphics driver; or switch to CPU in Settings"
        ),
    }
}

/// `加载语音识别模型失败: {e}{probe_hint}…`（GPU 装载失败的完整指引）。
pub fn asr_gpu_load_failed(e: &str, probe_hint: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!(
            "加载语音识别模型失败: {e}{probe_hint}\n\
             请检查：1) 显卡驱动已安装并更新；2) 关闭占用 GPU 的程序后重试。\
             或在设置中将后端改为 CPU"
        ),
        UiLang::En => format!(
            "Failed to load the ASR model: {e}{probe_hint}\n\
             Check: 1) the graphics driver is installed and up to date; 2) close other apps \
             using the GPU and retry. Or switch the backend to CPU in Settings"
        ),
    }
}

/// `（{gpu}）` / ` ({gpu})`（GPU 装载失败信息里的探测提示括号）。
pub fn gpu_probe_hint(gpu: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("（{gpu}）"),
        UiLang::En => format!(" ({gpu})"),
    }
}

// ---- 下载进度 ----

/// `{file}: {msg}（已尝试 {n} 次，可再次点击下载续传）`
/// / `{file}: {msg} (tried {n} times — click download again to resume)`。
pub fn dl_file_retries_exhausted(file: &str, msg: &str, n: u32) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{file}: {msg}（已尝试 {n} 次，可再次点击下载续传）"),
        UiLang::En => format!("{file}: {msg} (tried {n} times — click download again to resume)"),
    }
}

/// `{name}（{actual}/{expected} 字节）` / `{name} ({actual}/{expected} bytes)`（下载校验）。
pub fn dl_size_mismatch(name: &str, actual: u64, expected: u64) -> String {
    file_size_mismatch(name, actual, expected)
}

/// `文件缺失或损坏: {list}` / `Missing or corrupted files: {list}`。
pub fn dl_files_missing(list: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("文件缺失或损坏: {list}"),
        UiLang::En => format!("Missing or corrupted files: {list}"),
    }
}

/// `服务器拒绝断点续传（HTTP 416），已重置`
/// / `Server rejected resume (HTTP 416) — restarted`。
pub fn dl_resume_rejected() -> String {
    match ui_lang() {
        UiLang::Zh => "服务器拒绝断点续传（HTTP 416），已重置".into(),
        UiLang::En => "Server rejected resume (HTTP 416) — restarted".into(),
    }
}

/// `服务器返回了错误的续传偏移，已重置`
/// / `Server returned a wrong resume offset — restarted`。
pub fn dl_resume_offset_invalid() -> String {
    match ui_lang() {
        UiLang::Zh => "服务器返回了错误的续传偏移，已重置".into(),
        UiLang::En => "Server returned a wrong resume offset — restarted".into(),
    }
}

/// `文件不完整（{have}/{expected} 字节）` / `File incomplete ({have}/{expected} bytes)`。
pub fn dl_incomplete(have: u64, expected: u64) -> String {
    match ui_lang() {
        UiLang::Zh => format!("文件不完整（{have}/{expected} 字节）"),
        UiLang::En => format!("File incomplete ({have}/{expected} bytes)"),
    }
}

/// `SHA-256 校验失败，已删除并重新下载`
/// / `SHA-256 mismatch — deleted and re-downloading`。
pub fn dl_sha256_reset() -> String {
    match ui_lang() {
        UiLang::Zh => "SHA-256 校验失败，已删除并重新下载".into(),
        UiLang::En => "SHA-256 mismatch — deleted and re-downloading".into(),
    }
}

/// `model.safetensors 或 model.safetensors.index.json`
/// / `model.safetensors or model.safetensors.index.json`。
pub fn weight_file_missing() -> String {
    match ui_lang() {
        UiLang::Zh => "model.safetensors 或 model.safetensors.index.json".into(),
        UiLang::En => "model.safetensors or model.safetensors.index.json".into(),
    }
}

/// `找不到应用目录，无法保存设置`
/// / `Cannot find the app folder — settings were not saved`。
pub fn settings_no_dir() -> String {
    match ui_lang() {
        UiLang::Zh => "找不到应用目录，无法保存设置".into(),
        UiLang::En => "Cannot find the app folder — settings were not saved".into(),
    }
}

/// `warning: --no-srt 需要配合 --txt；已保留 SRT 输出`
/// / `warning: --no-srt requires --txt; SRT output stays enabled`。
pub fn cli_no_srt_warning() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "warning: --no-srt 需要配合 --txt；已保留 SRT 输出",
        UiLang::En => "warning: --no-srt requires --txt; SRT output stays enabled",
    }
}

#[cfg(windows)]
fn system_lang_code() -> Option<String> {
    // kernel32 的 GetUserDefaultUILanguage：不引新依赖，主语言 ID 直接映射。
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    let langid = unsafe { GetUserDefaultUILanguage() };
    Some(match langid & 0x3ff {
        0x0004 => "zh".to_string(),
        _ => "en".to_string(),
    })
}

#[cfg(not(windows))]
fn system_lang_code() -> Option<String> {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .ok()
}

#[cfg(test)]
pub fn with_ui_lang<R>(lang: UiLang, f: impl FnOnce() -> R) -> R {
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = ui_lang();
    set_ui_lang(lang);
    let out = f();
    set_ui_lang(prev);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_code_maps_zh_and_rest_to_en() {
        assert_eq!(UiLang::from_code("zh"), UiLang::Zh);
        assert_eq!(UiLang::from_code("zh-CN"), UiLang::Zh);
        assert_eq!(UiLang::from_code("zh_TW"), UiLang::Zh);
        assert_eq!(UiLang::from_code("en-US"), UiLang::En);
        assert_eq!(UiLang::from_code("ja"), UiLang::En);
        assert_eq!(UiLang::from_code("C"), UiLang::En);
        assert_eq!(UiLang::from_code(""), UiLang::En);
    }

    #[test]
    fn setting_value_round_trips() {
        assert_eq!(UiLang::Zh.setting_value(), "zh");
        assert_eq!(UiLang::En.setting_value(), "en");
    }

    #[test]
    fn normalize_ui_language_whitelists() {
        assert_eq!(normalize_ui_language("system"), "system");
        assert_eq!(normalize_ui_language("zh"), "zh");
        assert_eq!(normalize_ui_language(" ZH "), "zh");
        assert_eq!(normalize_ui_language("zh-CN"), "zh");
        assert_eq!(normalize_ui_language("zh-Hans"), "zh");
        assert_eq!(normalize_ui_language("en"), "en");
        assert_eq!(normalize_ui_language("fr"), "system");
        assert_eq!(normalize_ui_language(""), "system");
        assert_eq!(normalize_ui_language("garbage"), "system");
    }

    #[test]
    fn t_returns_pair_by_lang() {
        let s = Str::new("设置", "Settings");
        with_ui_lang(UiLang::Zh, || assert_eq!(t(s), "设置"));
        with_ui_lang(UiLang::En, || assert_eq!(t(s), "Settings"));
    }
}
