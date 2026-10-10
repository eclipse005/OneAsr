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
/// 见 [`ERR_EMPTY_ALIGNMENT`]。
pub const ERR_EMPTY_TRANSCRIPT: Str = Str::new("文稿是空的", "Transcript is empty");
pub const ERR_TRANSCRIPT_REQUIRES_TIMED_OUTPUT: Str = Str::new(
    "文稿匹配需要至少启用 SRT 或 ASS 输出",
    "Transcript matching requires SRT or ASS output",
);
pub const ERR_EMPTY_SENTENCE_BOUNDARY: Str = Str::new(
    "断句后字幕为空",
    "Subtitle is empty after sentence segmentation",
);
pub const ERR_READ_DURATION: Str = Str::new(
    "无法读取转码后音频时长",
    "Cannot read the converted audio duration",
);
pub const ERR_NO_OUTPUT_FORMAT: Str = Str::new("未启用任何输出格式", "No output format is enabled");

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
            format!(
                "Vocal separation: GPU unavailable, falling back to CPU (much slower): {reason}"
            )
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

/// `ONEASR_DEVICE 无效（{spec}）: {e}` / `Invalid ONEASR_DEVICE ({spec}): {e}`
/// （设备选择 spec 解析失败——显式指定的卡名/runtime 拼错时直接报错，不静默回退）。
pub fn device_spec_invalid(spec: &str, e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("ONEASR_DEVICE 无效（{spec}）: {e}"),
        UiLang::En => format!("Invalid ONEASR_DEVICE ({spec}): {e}"),
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
        UiLang::En => format!(
            "Failed to read the separation input {}: {e}",
            path.display()
        ),
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

/// `找不到可写的数据目录（…），设置未保存`
/// / `Cannot find a writable data directory (…) — settings were not saved`。
///
/// `reason` 是 [`crate::paths::DataRootError::message`] 的英文技术原因（哪个
/// 环境变量缺失），两种语言都原样带上，用户照着补环境变量即可。
pub fn settings_no_writable_dir(reason: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("找不到可写的数据目录（{reason}），设置未保存"),
        UiLang::En => {
            format!("Cannot find a writable data directory ({reason}) — settings were not saved")
        }
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

// ─── CLI（oneasr-cli）自产文案的双语表 ──────────────────────────────
//
// `oneasr-cli` 随安装包发货，是用户可见入口，所以 **CLI 的每一条输出（用法、
// 参数校验错误、运行摘要、诊断行）都从这里取词**，bin 里不再出现硬编码文案。
// 不一致的地方才是真正的例外：`lang=` / `wall=` / `chars=` 这类键值摘要的键名
// 就是 flag 名，`--- ASR TEXT ---` 是定位标记，两种语言文本一致，但仍集中
// 定义在这里，保证「所有 CLI 输出只有一个来源」。

/// `--help` 全文；两种语言各一份，成对维护。
pub fn cli_help() -> &'static str {
    match ui_lang() {
        UiLang::Zh => CLI_HELP_ZH,
        UiLang::En => CLI_HELP_EN,
    }
}

const CLI_HELP_EN: &str = "\
oneasr-cli — headless Qwen ASR + ForcedAligner pipeline

Usage:
  oneasr-cli transcribe --input <media> [options]
  oneasr-cli asr-chunk  --wav <16k.wav> --start <sec> --end <sec> [options]
  oneasr-cli render     --timeline <{stem}.timeline.json> [options]
  oneasr-cli align      --audio <media> --text <transcript> [options]

Commands:
  transcribe   Full pipeline → {data-root}/output/{stem}.srt by default  (alias: run, pipeline)
  asr-chunk    ASR only for one time range (hallucination / length debug)
  render       Re-present a measured timeline. No models, no ffmpeg, no audio
  align        Audio + YOUR transcript → subtitles. Skips recognition entirely;
               the transcript's own line breaks become the subtitle lines
               (one line = one cue). Forced to the CTC aligner, which takes the
               whole file in one pass
  render       Re-present a measured timeline. No models, no ffmpeg, no audio:
               reads {stem}.timeline.json and writes a subtitle again

transcribe options:
  --input <path>           Media file (required)
  --app-root <dir>         App root with bin/ffmpeg  (default: this exe's install dir)
  --data-root <dir>        Directory holding models/, output/, runs/ and settings.json
                           (default: --app-root; when the install dir is not writable
                           the platform user data dir is used instead)
  --language <code>        zh|en|yue|ja|ko|...  (default: zh)
  --chunk-seconds <30-180> VAD chunk target (default: 60)
  --backend <gpu|cpu|auto> Default: auto (GPU if a driver is present, else CPU)
  --aligner <ctc|qwen>     Forced-aligner engine (default: scan CTC first, then Qwen)
  --max-new-tokens <n>     ASR decode ceiling (default: settings / 2048)
  --output <path>          Copy the primary result (SRT, else TXT) to this path
  --txt                    Also write {stem}.txt (one transcript line per cue)
  --ass                    Also write {stem}.ass (karaoke: a sweep per word / CJK char)
  --no-srt                 Suppress the .srt file (requires --txt or --ass)
  --script <simplified|traditional>
                           Chinese output script for zh / yue (default: simplified)
  --vocal-separation       Run HTDemucs vocal separation before ASR
  --demucs-model-dir <dir> Directory holding htdemucs_ft_vocals.safetensors
  --words-json <path>      Write ForcedAligner word/char tokens + timestamps (JSON)

asr-chunk options:
  --wav <path>             16 kHz mono wav (required)
  --start <sec>  --end <sec>
  --language <code>
  --app-root <dir>
  --data-root <dir>
  --max-new-tokens <n>
  --out <path>             Write ASR text to file
  --backend <gpu|cpu|auto>

align options:
  --audio <path>           Media file (required)
  --text <path>            Transcript .txt / .md / .srt (required). One line = one
                           subtitle line; an .srt's own timings are ignored
  --app-root <dir>         App root with bin/ffmpeg
  --data-root <dir>        Where models/, output/ and runs/ live
  --language <code>        zh|en|yue|ja|ko|...  (default: zh)
  --backend <gpu|cpu|auto> Inference backend (default: auto)
  --chunk-seconds <30-180> Kept for parity with transcribe; the aligner windows
                           the audio itself
  --txt                    Also write {stem}.txt (your transcript)
  --ass                    Also write {stem}.ass (karaoke)
  --no-srt                 Suppress the .srt file (requires --txt or --ass)
  --beautify               Polish finished cues (punctuation; spaces beside Han
                           and English or digits, not beside punctuation)

render options:
  --timeline <path>        Measured timeline written by transcribe (required)
  --output <dir>           Where to write  (default: the timeline's own directory)
  --transcript <path>      Re-present with THIS transcript's line breaks instead of
                           the layout DP's. Reads a .txt / .srt, ignores its timings
  --preset <id>            Segment length: short|standard|loose
                           (default: the one the timeline was measured with)
  --script <simplified|traditional>
                           Chinese output script, as in transcribe
  --txt                    Also write {stem}.txt
  --ass                    Also write {stem}.ass (karaoke)
  --no-srt                 Suppress the .srt file (requires --txt or --ass)

Env:
  ONEASR_DATA_DIR=<dir>    Override the data directory (same as --data-root)
  ONEASR_PIPELINE_TRACE=1  Per-chunk ASR/align logs

Examples:
  oneasr-cli transcribe --input video.mp4 --app-root D:\\OneAsr --chunk-seconds 120 --backend auto
  oneasr-cli asr-chunk --wav runs\\x\\input_16k.wav --start 722 --end 843 --language zh";

const CLI_HELP_ZH: &str = "\
oneasr-cli — 免界面的 Qwen ASR + ForcedAligner 流水线

用法：
  oneasr-cli transcribe --input <音视频> [选项]
  oneasr-cli asr-chunk  --wav <16k.wav> --start <秒> --end <秒> [选项]
  oneasr-cli render     --timeline <{stem}.timeline.json> [选项]
  oneasr-cli align      --audio <音视频> --text <文稿> [选项]

子命令：
  transcribe   完整流水线，默认输出到 {data-root}/output/{stem}.srt（别名：run、pipeline）
  asr-chunk    只对一段时间做转写（排查幻觉 / 长度问题）
  render       重新呈现已测好的时间轴。不加载模型、不用 ffmpeg、不碰音频
  align        音视频 + 你自己的文稿 → 字幕，整个识别阶段跳过。文稿自己的分行
               就是字幕的分行（一行 = 一条）。固定用 CTC 对齐器，它能整段一次
               对齐，长音频不必切块：
               读 {stem}.timeline.json，再写一遍字幕

transcribe 选项：
  --input <路径>           音视频文件（必填）
  --app-root <目录>        应用目录（含 bin/ffmpeg；默认取本程序安装目录）
  --data-root <目录>       数据目录（放 models/、output/、runs/ 与 settings.json；
                           默认取 --app-root；安装目录不可写时自动改用平台用户目录）
  --language <代码>        zh|en|yue|ja|ko|...（默认 zh）
  --chunk-seconds <30-180> VAD 分段目标时长（默认 60）
  --backend <gpu|cpu|auto> 推理后端，默认 auto（有显卡驱动走 GPU，否则 CPU）
  --aligner <ctc|qwen>     对齐引擎（默认按 CTC→Qwen 顺序扫描已装目录）
  --max-new-tokens <n>     转写解码上限（默认取设置 / 2048）
  --output <路径>          把主产物（SRT，没有则 TXT）复制到该路径
  --txt                    额外写出 {stem}.txt（每条字幕一行转写）
  --ass                    额外写出 {stem}.ass（卡拉OK：逐词 / 中日韩逐字扫光）
  --no-srt                 不写 .srt 文件（需配合 --txt 或 --ass）
  --script <simplified|traditional>
                           zh / yue 的中文字形（默认 simplified）
  --vocal-separation       转写前先做人声分离
  --demucs-model-dir <目录> 存放 htdemucs_ft_vocals.safetensors 的目录
  --words-json <路径>      写出 ForcedAligner 的词/字级时间轴（JSON）

asr-chunk 选项：
  --wav <路径>             16 kHz 单声道 wav（必填）
  --start <秒>  --end <秒>
  --language <代码>
  --app-root <目录>
  --data-root <目录>
  --max-new-tokens <n>
  --out <路径>             把转写文本写入文件
  --backend <gpu|cpu|auto>

align 选项：
  --audio <路径>           音视频文件（必填）
  --text <路径>            文稿 .txt / .md / .srt（必填）。一行 = 一条字幕行；
                           .srt 自带的时间轴会被忽略
  --app-root <目录>        应用目录（含 bin/ffmpeg）
  --data-root <目录>       数据目录（放 models/、output/、runs/）
  --language <代码>        zh|en|yue|ja|ko|...（默认 zh）
  --backend <gpu|cpu|auto> 推理后端，默认 auto
  --chunk-seconds <30-180> 与 transcribe 保持一致；对齐器自己会开窗
  --txt                    额外写出 {stem}.txt（你的文稿）
  --ass                    额外写出 {stem}.ass（卡拉OK）
  --no-srt                 不写 .srt 文件（需配合 --txt 或 --ass）
  --beautify               成稿字幕美化（标点；汉字与英文、数字之间加空格，标点旁边不加）

render 选项：
  --timeline <路径>        transcribe 写出的已测时间轴（必填）
  --output <目录>          输出目录（默认与时间轴同目录）
  --transcript <路径>      改用这份文稿自己的分行，而不是排版 DP 的结果。
                           读 .txt / .srt，忽略它原有的时间轴
  --preset <id>            分段时长：short|standard|loose
                           （默认沿用测量时的那个）
  --script <simplified|traditional>
                           中文字形，同 transcribe
  --txt                    额外写出 {stem}.txt
  --no-srt                 不写 .srt 文件（需配合 --txt）

环境变量：
  ONEASR_DATA_DIR=<目录>   覆盖数据目录（等价于 --data-root）
  ONEASR_PIPELINE_TRACE=1  打印每个分段的转写/对齐细节

示例：
  oneasr-cli transcribe --input video.mp4 --app-root D:\\OneAsr --chunk-seconds 120 --backend auto
  oneasr-cli asr-chunk --wav runs\\x\\input_16k.wav --start 722 --end 843 --language zh
  oneasr-cli render --timeline output\\clip.timeline.json --preset short --output output";

/// `=== OneAsr CLI · 重渲染 ===` / `=== OneAsr CLI · render ===`。
pub fn cli_render_banner() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "=== OneAsr CLI · 重渲染 ===",
        UiLang::En => "=== OneAsr CLI · render ===",
    }
}

/// 见 [`CLI_KV_INPUT`]。
pub const CLI_KV_TIMELINE: Str = Str::new("时间轴:  ", "timeline: ");
/// 见 [`CLI_KV_INPUT`]。
pub const CLI_KV_TRANSCRIPT: Str = Str::new("文稿:    ", "transcript:");

/// `=== OneAsr CLI · 文稿匹配 ===` / `=== OneAsr CLI · align ===`。
pub fn cli_align_banner() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "=== OneAsr CLI · 文稿匹配 ===",
        UiLang::En => "=== OneAsr CLI · align ===",
    }
}

/// `— {lines} 行 · {chars} 字{ignored}`，括号里那句只在 SRT 出现：原时间轴被丢了。
pub fn cli_transcript_loaded(lines: usize, chars: usize, ignored: bool) -> String {
    let dropped = match (ui_lang(), ignored) {
        (UiLang::Zh, true) => "（已忽略原时间轴，将重新打轴）",
        (UiLang::En, true) => " (original timings ignored; re-aligning)",
        _ => "",
    };
    match ui_lang() {
        UiLang::Zh => format!("— {lines} 行 · {chars} 字{dropped}"),
        UiLang::En => format!("— {lines} lines · {chars} chars{dropped}"),
    }
}

/// `重渲染失败: {e}` / `render failed: {e}`。
pub fn cli_render_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("重渲染失败: {e}"),
        UiLang::En => format!("render failed: {e}"),
    }
}

/// `.{ext} 是空的，没有写出` / `.{ext} came out empty; nothing written`。
pub fn cli_render_empty(ext: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!(".{ext} 是空的，没有写出"),
        UiLang::En => format!(".{ext} came out empty; nothing written"),
    }
}

/// `读不了时间轴: {e}` / `cannot read timeline: {e}`。
pub fn cli_timeline_read_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("读不了时间轴: {e}"),
        UiLang::En => format!("cannot read timeline: {e}"),
    }
}

/// `未知的分段时长：{raw}（可选 short | standard | loose）`
/// / `Unknown segment length: {raw} (choose short | standard | loose)`。
pub fn cli_preset_unknown(raw: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("未知的分段时长：{raw}（可选 short | standard | loose）"),
        UiLang::En => format!("Unknown segment length: {raw} (choose short | standard | loose)"),
    }
}

/// `读不了文稿: {e}` / `cannot read transcript: {e}`。
pub fn cli_transcript_read_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("读不了文稿: {e}"),
        UiLang::En => format!("cannot read transcript: {e}"),
    }
}

/// `未知的对齐引擎：{raw}（可选 ctc | qwen）`
/// / `Unknown aligner engine: {raw} (choose ctc | qwen)`。
pub fn cli_aligner_unknown(raw: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("未知的对齐引擎：{raw}（可选 ctc | qwen）"),
        UiLang::En => format!("Unknown aligner engine: {raw} (choose ctc | qwen)"),
    }
}

/// `未知命令: {other}` / `unknown command: {other}`。
pub fn cli_unknown_command(other: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("未知命令: {other}"),
        UiLang::En => format!("unknown command: {other}"),
    }
}

/// `缺少必需参数 {name}` / `missing required {name}`。
pub fn cli_missing_required(name: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("缺少必需参数 {name}"),
        UiLang::En => format!("missing required {name}"),
    }
}

/// `{flag} 必须是数字（秒）` / `{flag} must be a number (seconds)`。
pub fn cli_flag_not_number(flag: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{flag} 必须是数字（秒）"),
        UiLang::En => format!("{flag} must be a number (seconds)"),
    }
}

/// `--end 必须大于 --start` / `--end must be > --start`。
pub fn cli_end_before_start() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "--end 必须大于 --start",
        UiLang::En => "--end must be > --start",
    }
}

/// `找不到输入文件: {path}` / `input not found: {path}`。
pub fn cli_input_not_found(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("找不到输入文件: {}", path.display()),
        UiLang::En => format!("input not found: {}", path.display()),
    }
}

/// `找不到 wav 文件: {path}` / `wav not found: {path}`。
pub fn cli_wav_not_found(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("找不到 wav 文件: {}", path.display()),
        UiLang::En => format!("wav not found: {}", path.display()),
    }
}

/// `--app-root 不是目录: {path}` / `--app-root not a directory: {path}`。
pub fn cli_app_root_not_dir(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("--app-root 不是目录: {}", path.display()),
        UiLang::En => format!("--app-root not a directory: {}", path.display()),
    }
}

/// `--data-root 已存在但不是目录: {path}`
/// / `--data-root exists but is not a directory: {path}`。
///
/// 不存在的目录是合法的（第一次运行会在那里建 `models/`），只有"存在但不是
/// 目录"才是参数错误。
pub fn cli_data_root_not_dir(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("--data-root 已存在但不是目录: {}", path.display()),
        UiLang::En => format!(
            "--data-root exists but is not a directory: {}",
            path.display()
        ),
    }
}

/// `=== OneAsr CLI · 转写 ===` / `=== OneAsr CLI · transcribe ===`。
pub fn cli_transcribe_banner() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "=== OneAsr CLI · 转写 ===",
        UiLang::En => "=== OneAsr CLI · transcribe ===",
    }
}

/// 运行摘要的字段名（与 CLI flag 对应，两种语言各自排版）。
pub const CLI_KV_INPUT: Str = Str::new("输入:    ", "input:   ");
/// 见 [`CLI_KV_INPUT`]。
pub const CLI_KV_APP: Str = Str::new("应用:    ", "app:     ");
/// 见 [`CLI_KV_INPUT`]。
pub const CLI_KV_DATA: Str = Str::new("数据:    ", "data:    ");
/// 见 [`CLI_KV_INPUT`]。
pub const CLI_KV_ASR: Str = Str::new("ASR 模型:", "asr:     ");
/// 见 [`CLI_KV_INPUT`]。
pub const CLI_KV_ALIGN: Str = Str::new("对齐模型:", "align:   ");
/// 见 [`CLI_KV_INPUT`]。
pub const CLI_KV_OUTPUT: Str = Str::new("输出:    ", "output:  ");

/// 运行摘要的一行：`{label}{path}`（label 取 [`CLI_KV_INPUT`] 等）。
pub fn cli_kv(label: Str, path: &std::path::Path) -> String {
    format!("{}{}", t(label), path.display())
}

/// `lang={lang} chunk={chunk}s backend={backend} max_new_tokens={n}`（键名即 flag 名）。
pub fn cli_run_flags_decode(
    lang: &str,
    chunk: u32,
    backend: &str,
    max_new_tokens: usize,
) -> String {
    format!("lang={lang} chunk={chunk}s backend={backend} max_new_tokens={max_new_tokens}")
}

/// `srt={srt} txt={txt} script={script} vocal_sep={vocal_sep}`（键名即 flag 名）。
pub fn cli_run_flags_output(srt: bool, txt: bool, script: &str, vocal_sep: bool) -> String {
    format!("srt={srt} txt={txt} script={script} vocal_sep={vocal_sep}")
}

/// `[阶段] {label}` / `[stage] {label}`。
pub fn cli_stage(label: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("[阶段] {label}"),
        UiLang::En => format!("[stage] {label}"),
    }
}

/// `[警告] {msg}` / `[warn] {msg}`。
pub fn cli_warn(msg: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("[警告] {msg}"),
        UiLang::En => format!("[warn] {msg}"),
    }
}

/// `完成 output={path}` / `OK output={path}`。
pub fn cli_ok_output(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("完成 output={}", path.display()),
        UiLang::En => format!("OK output={}", path.display()),
    }
}

/// `复制输出失败: {e}` / `copy output failed: {e}`。
pub fn cli_copy_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("复制输出失败: {e}"),
        UiLang::En => format!("copy output failed: {e}"),
    }
}

/// `已复制 → {path}` / `copied → {path}`。
pub fn cli_copied(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("已复制 → {}", path.display()),
        UiLang::En => format!("copied → {}", path.display()),
    }
}

/// `词级时间轴 → {path}` / `words → {path}`。
pub fn cli_words_written(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("词级时间轴 → {}", path.display()),
        UiLang::En => format!("words → {}", path.display()),
    }
}

/// `wall={wall:.1}s total_ms={total_ms} stages={stages}`（机器可读摘要）。
pub fn cli_timing(wall_secs: f64, total_ms: u64, stages: usize) -> String {
    format!("wall={wall_secs:.1}s total_ms={total_ms} stages={stages}")
}

/// 阶段耗时行：`  {label:<12} {dur:>8}`（列宽对齐，两种语言一致）。
pub fn cli_stage_row(label: &str, dur: &str) -> String {
    format!("  {label:<12} {dur:>8}")
}

/// `失败（耗时 {wall:.1}s）: {e}` / `FAIL after {wall:.1}s: {e}`。
pub fn cli_failed(wall_secs: f64, e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("失败（耗时 {wall_secs:.1}s）: {e}"),
        UiLang::En => format!("FAIL after {wall_secs:.1}s: {e}"),
    }
}

/// `切片 {start:.3}-{end:.3}（{len:.1}s）→ {path}` / `slice … ({len:.1}s) → {path}`。
pub fn cli_slice_done(start: f32, end: f32, len_secs: f32, path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!(
            "切片 {start:.3}-{end:.3}（{len_secs:.1}s）→ {}",
            path.display()
        ),
        UiLang::En => format!(
            "slice {start:.3}-{end:.3} ({len_secs:.1}s) → {}",
            path.display()
        ),
    }
}

/// `切片失败: {e}` / `slice_wav: {e}`。
pub fn cli_slice_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("切片失败: {e}"),
        UiLang::En => format!("slice_wav: {e}"),
    }
}

/// `转写失败: {e}` / `transcription failed: {e}`。
pub fn cli_transcribe_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("转写失败: {e}"),
        UiLang::En => format!("transcription failed: {e}"),
    }
}

/// `chars={chars} raw_chars={raw} elapsed={secs:.1}s`（机器可读摘要）。
pub fn cli_chars(chars: usize, raw_chars: usize, elapsed_secs: f64) -> String {
    format!("chars={chars} raw_chars={raw_chars} elapsed={elapsed_secs:.1}s")
}

/// `--- 识别文本开始 ---` / `--- ASR TEXT BEGIN ---`（定位标记）。
pub const CLI_ASR_TEXT_BEGIN: Str = Str::new("--- 识别文本开始 ---", "--- ASR TEXT BEGIN ---");
/// 见 [`CLI_ASR_TEXT_BEGIN`]。
pub const CLI_ASR_TEXT_END: Str = Str::new("--- 识别文本结束 ---", "--- ASR TEXT END ---");

/// `写入失败: {e}` / `write: {e}`。
pub fn cli_write_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("写入失败: {e}"),
        UiLang::En => format!("write: {e}"),
    }
}

/// `已写入 {path}` / `wrote {path}`。
pub fn cli_wrote(path: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!("已写入 {}", path.display()),
        UiLang::En => format!("wrote {}", path.display()),
    }
}

/// `ffmpeg: {bin} 下没有 ffmpeg，改用 {source}`
/// / `ffmpeg: no binary under {bin} — using {source}`。
pub fn cli_ffmpeg_fallback(bin: &std::path::Path, source: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("ffmpeg: {} 下没有 ffmpeg，改用 {source}", bin.display()),
        UiLang::En => format!("ffmpeg: no binary under {} — using {source}", bin.display()),
    }
}

/// `找不到 ffmpeg：把二进制放进 {bin}，或把 ffmpeg 加进 PATH`
/// / `ffmpeg not found: put a binary in {bin} or install ffmpeg on PATH`。
pub fn cli_ffmpeg_not_found(bin: &std::path::Path) -> String {
    match ui_lang() {
        UiLang::Zh => format!(
            "找不到 ffmpeg：把二进制放进 {}，或把 ffmpeg 加进 PATH",
            bin.display()
        ),
        UiLang::En => format!(
            "ffmpeg not found: put a binary in {} or install ffmpeg on PATH",
            bin.display()
        ),
    }
}

#[cfg(windows)]
fn system_lang_code() -> Option<String> {
    // kernel32 的 GetUserDefaultUILanguage：不引新依赖，主语言 ID 直接映射。
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // SAFETY: 无参数、无前置条件，只读系统 UI 语言设置；不返回指针、不转移所有权、
    // 不会失败（未知时返回 0，下面的掩码映射已把它归到 en）。调用可重复、无副作用。
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

    #[test]
    fn cli_help_is_bilingual() {
        let zh = with_ui_lang(UiLang::Zh, cli_help);
        let en = with_ui_lang(UiLang::En, cli_help);
        assert!(zh.contains("用法："), "zh help must be Chinese: {zh}");
        assert!(
            zh.contains("转写"),
            "zh help must use the glossary term 转写"
        );
        assert!(
            en.contains("Usage:"),
            "en help keeps the upstream wording: {en}"
        );
        assert!(en.contains("transcribe"));
        assert_ne!(zh, en);
    }

    #[test]
    fn cli_messages_follow_the_ui_language() {
        let path = std::path::Path::new("C:/tmp/video.mp4");
        with_ui_lang(UiLang::Zh, || {
            assert_eq!(cli_unknown_command("foo"), "未知命令: foo");
            assert_eq!(cli_flag_not_number("--start"), "--start 必须是数字（秒）");
            assert_eq!(cli_missing_required("--input"), "缺少必需参数 --input");
            assert_eq!(
                cli_input_not_found(path),
                "找不到输入文件: C:/tmp/video.mp4"
            );
            assert_eq!(cli_stage("转写中"), "[阶段] 转写中");
            assert_eq!(cli_kv(CLI_KV_INPUT, path), "输入:    C:/tmp/video.mp4");
            assert_eq!(t(CLI_ASR_TEXT_BEGIN), "--- 识别文本开始 ---");
        });
        with_ui_lang(UiLang::En, || {
            assert_eq!(cli_unknown_command("foo"), "unknown command: foo");
            assert_eq!(
                cli_flag_not_number("--start"),
                "--start must be a number (seconds)"
            );
            assert_eq!(cli_missing_required("--input"), "missing required --input");
            assert_eq!(
                cli_input_not_found(path),
                "input not found: C:/tmp/video.mp4"
            );
            assert_eq!(cli_stage("Transcribing"), "[stage] Transcribing");
            assert_eq!(cli_kv(CLI_KV_INPUT, path), "input:   C:/tmp/video.mp4");
            assert_eq!(t(CLI_ASR_TEXT_END), "--- ASR TEXT END ---");
        });
    }

    #[test]
    fn cli_machine_readable_lines_stay_language_neutral() {
        // 键名就是 flag 名，两种语言必须逐字相同：贴日志的人和脚本都按它检索。
        for lang in [UiLang::Zh, UiLang::En] {
            with_ui_lang(lang, || {
                assert_eq!(
                    cli_run_flags_decode("zh", 60, "auto", 2048),
                    "lang=zh chunk=60s backend=auto max_new_tokens=2048"
                );
                assert_eq!(
                    cli_run_flags_output(true, false, "原文", false),
                    "srt=true txt=false script=原文 vocal_sep=false"
                );
                assert_eq!(
                    cli_timing(12.3, 12_300, 3),
                    "wall=12.3s total_ms=12300 stages=3"
                );
                assert_eq!(cli_chars(5, 7, 2.0), "chars=5 raw_chars=7 elapsed=2.0s");
                let row = cli_stage_row("asr", "12.3s");
                assert!(row.starts_with("  asr"), "{row}");
                assert!(row.ends_with("12.3s"), "{row}");
                assert_eq!(row.chars().count(), 2 + 12 + 1 + 8, "{row}");
            });
        }
    }

    #[test]
    fn cli_str_table_has_no_single_sided_entries() {
        let table = [
            ("CLI_KV_INPUT", CLI_KV_INPUT),
            ("CLI_KV_APP", CLI_KV_APP),
            ("CLI_KV_DATA", CLI_KV_DATA),
            ("CLI_KV_ASR", CLI_KV_ASR),
            ("CLI_KV_ALIGN", CLI_KV_ALIGN),
            ("CLI_KV_OUTPUT", CLI_KV_OUTPUT),
            ("CLI_KV_TIMELINE", CLI_KV_TIMELINE),
            ("CLI_ASR_TEXT_BEGIN", CLI_ASR_TEXT_BEGIN),
            ("CLI_ASR_TEXT_END", CLI_ASR_TEXT_END),
        ];
        for (name, s) in table {
            assert!(!s.zh.trim().is_empty(), "{name}: zh 侧为空");
            assert!(!s.en.trim().is_empty(), "{name}: en 侧为空");
        }
    }
}
