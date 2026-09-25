//! GUI 界面文案双语表（zh / en）。
//!
//! 机制（[`UiLang`](oneasr_core::i18n::UiLang) / [`Str`] / `t` / 系统语言探测）
//! 在 `oneasr_core::i18n`；本文件只集中**界面标签**。管线自产的消息
//! （错误、阶段名、下载进度、统计时长）的双语表在 core 的 `i18n`。
//!
//! 规则：
//! - 新增文案 zh / en 必须成对书写（编译期保证同构）；
//! - 带参数的句子写成接受参数的小函数，返回拼好的 `String`，两种语言的
//!   语序差异在函数内消化；
//! - 纯格式化函数（被单测引用的）显式接收 `UiLang` 参数，不读全局。

use oneasr_core::i18n::{Str, UiLang, ui_lang};

/// 取词函数（转发 core 机制，方便 `crate::i18n::t(...)` 全路径调用）。
pub use oneasr_core::i18n::t;

/// 界面标签表。调用方式：`t(L::SETTINGS)`。
#[allow(non_snake_case)]
pub mod L {
    use super::Str;

    // ---- 设置抽屉骨架 ----
    pub const SETTINGS: Str = Str::new("设置", "Settings");
    pub const UNSAVED: Str = Str::new("未保存", "Unsaved");
    pub const RESET: Str = Str::new("重置", "Reset");
    pub const SAVE_SETTINGS: Str = Str::new("保存设置", "Save settings");
    pub const SAVED: Str = Str::new("已保存", "Saved");

    // ---- 界面语言（标题行图标按钮：tooltip + 暂存提示） ----
    pub const UI_LANGUAGE_APPLY_ON_SAVE: Str = Str::new(
        "界面语言将在保存设置后生效",
        "Interface language applies when settings are saved",
    );
    pub const UI_LANG_SYSTEM: Str = Str::new("跟随系统", "System");
    /// 语言名一律用本名（endonym），两种界面语言下都显示「中文 / English」。
    pub const UI_LANG_ZH: Str = Str::new("中文", "中文");
    pub const UI_LANG_EN: Str = Str::new("English", "English");
    /// 语言按钮上直接渲染的字形（endonym，两种界面语言相同）。
    pub const UI_LANG_ZH_GLYPH: Str = Str::new("中", "中");
    pub const UI_LANG_EN_GLYPH: Str = Str::new("En", "En");

    // ---- 设置已保存提示 ----
    pub const SETTINGS_SAVED_READY: Str =
        Str::new("设置已保存 · 模型就绪", "Settings saved · models ready");
    pub const SETTINGS_SAVED_NOT_READY: Str =
        Str::new("设置已保存 · 模型未就绪", "Settings saved · models incomplete");

    // ---- 文件对话框标题 ----
    pub const DLG_ADD_MEDIA: Str = Str::new("添加音视频", "Add audio / video");
    pub const DLG_ASR_DIR: Str = Str::new("选择语音识别模型目录", "Choose ASR model folder");
    pub const DLG_ALIGNER_DIR: Str = Str::new("选择对齐模型目录", "Choose aligner model folder");
    pub const DLG_DEMUCS_DIR: Str =
        Str::new("选择人声分离模型目录", "Choose vocal-separation model folder");
    pub const DLG_OUTPUT_DIR: Str = Str::new("选择字幕输出目录", "Choose subtitle output folder");

    // ---- 顶部工具栏 ----
    pub const SELECTING: Str = Str::new("选择中…", "Selecting…");
    pub const ADD: Str = Str::new("添加", "Add");
    pub const START_ALL: Str = Str::new("全部开始", "Start all");
    pub const START_ALL_BLOCKED: Str = Str::new(
        "模型未就绪，请先在设置中选择完整模型目录",
        "Models not ready — pick complete model folders in Settings first",
    );
    pub const CLEAR: Str = Str::new("清空", "Clear");

    // ---- 状态栏 ----
    pub const STATS: Str = Str::new("统计", "Stats");
    pub const MODEL_READY: Str = Str::new("模型就绪", "Models ready");
    pub const MODEL_NOT_READY: Str = Str::new("模型未就绪", "Models not ready");

    // ---- 任务列表空态 ----
    pub const ADD_FILES: Str = Str::new("添加文件", "Add files");
    pub const OPEN_SETTINGS: Str = Str::new("打开设置", "Open settings");

    // ---- 下载 ----
    pub const DOWNLOAD_KIND_BUSY: Str =
        Str::new("已有同类下载任务进行中", "A download of this kind is already running");
    pub const MODEL_FILES_COMPLETE: Str = Str::new(
        "模型文件完整，无需重新下载",
        "Model files are complete — no need to re-download",
    );
    pub const CANCELLING_DOWNLOAD: Str = Str::new("正在取消下载…", "Cancelling download…");
    pub const PREPARING: Str = Str::new("准备中…", "Preparing…");
    pub const TIP_CANCEL_DOWNLOAD: Str = Str::new("点击取消下载", "Click to cancel the download");
    pub const TIP_REDOWNLOAD: Str = Str::new("重新下载", "Re-download");
    pub const TIP_DOWNLOAD_MODEL: Str = Str::new("下载模型", "Download model");

    // ---- 任务行 ----
    pub const TIP_OPEN_OUTPUT: Str = Str::new("打开输出位置", "Open output location");
    pub const TIP_START: Str = Str::new("开始", "Start");
    pub const TIP_DELETE: Str = Str::new("删除", "Delete");
    pub const TIMING_TITLE: Str = Str::new("处理耗时", "Processing time");
    pub const SEP_CHIP: Str = Str::new("分离", "Sep");
    pub const TIP_SEP_ON: Str = Str::new(
        "人声分离：开启（转录前分离人声，点击关闭）",
        "Vocal separation: on (isolates vocals before transcribing — click to turn off)",
    );
    pub const TIP_SEP_OFF: Str =
        Str::new("人声分离：关闭（点击开启）", "Vocal separation: off (click to turn on)");

    // ---- 任务状态锁提示 ----
    pub const LOCKED_LANG: Str = Str::new(
        "处理中的任务不能改语言",
        "Can't change language while the task is processing",
    );
    pub const LOCKED_SEPARATION: Str = Str::new(
        "处理中的任务不能修改人声分离",
        "Can't change vocal separation while the task is processing",
    );
    pub const LOCKED_DELETE: Str =
        Str::new("处理中的任务不能删除", "Can't delete a task while it's processing");
    pub const SEPARATION_NEEDS_MODEL: Str = Str::new(
        "请先在设置中下载人声分离模型",
        "Download the vocal-separation model in Settings first",
    );
    pub const SEPARATION_ON: Str = Str::new("本任务已开启人声分离", "Vocal separation is on for this task");
    pub const SEPARATION_OFF: Str =
        Str::new("本任务已关闭人声分离", "Vocal separation is off for this task");

    // ---- 开始 / 清空 ----
    pub const LIST_EMPTY: Str = Str::new("列表已空", "List is already empty");
    pub const CLEARED_KEEP_RUNNING: Str = Str::new(
        "已清空队列，当前任务继续处理",
        "Queue cleared — the current task keeps running",
    );
    pub const WORKER_EXITED: Str = Str::new("识别工作线程已退出", "The transcription worker exited");

    // ---- 设置：默认语言 / 字幕长度 ----
    pub const DEFAULT_LANGUAGE: Str = Str::new("默认语言", "Default language");
    pub const SUBTITLE_LENGTH: Str = Str::new("字幕长度", "Subtitle length");
    pub const LEN_SHORT: Str = Str::new("短", "Short");
    pub const LEN_STANDARD: Str = Str::new("标准", "Standard");
    pub const LEN_LOOSE: Str = Str::new("宽松", "Loose");

    // ---- 设置：分段时长 ----
    pub const CHUNK_DURATION: Str = Str::new("分段时长", "Segment length");
    pub const CHUNK_4GB_HINT: Str = Str::new(
        "建议 4GB 显存使用 60 秒分段时长",
        "60-second segments recommended for 4 GB of VRAM",
    );

    // ---- 设置：输出 ----
    pub const OUTPUT_FORMAT: Str = Str::new("输出格式", "Output format");
    pub const KEEP_ONE_FORMAT: Str = Str::new("至少保留一种输出格式", "Keep at least one output format");
    pub const CHINESE_OUTPUT: Str = Str::new("中文输出", "Chinese script");
    pub const ZH_YUE_ONLY: Str = Str::new("仅中文/粤语", "Chinese & Cantonese only");
    pub const OUTPUT_LOCATION: Str = Str::new("字幕输出位置", "Subtitle output location");
    pub const NEXT_TO_VIDEO: Str = Str::new("视频同目录", "Next to video");
    pub const CUSTOM_DIR: Str = Str::new("指定目录", "Custom folder");

    // ---- 设置：模型与后端 ----
    pub const ASR_MODEL: Str = Str::new("语音识别模型", "ASR model");
    pub const ASR_DL_BUSY_RESIZE: Str = Str::new(
        "ASR 下载进行中，请稍后再切换尺寸",
        "ASR download in progress — try switching size later",
    );
    pub const ASR_DL_BUSY: Str =
        Str::new("ASR 下载进行中，请稍后再切换", "ASR download in progress — try again later");
    pub const QUANT: Str = Str::new("量化", "int8");
    pub const ALIGNER_MODEL: Str = Str::new("对齐模型", "Aligner model");
    pub const VOCAL_SEPARATION: Str = Str::new("人声分离", "Vocal separation");
    pub const SEP_DEFAULT_ON: Str = Str::new("默认启用", "On by default");
    pub const SEP_NEEDS_MODEL: Str =
        Str::new("请先下载人声分离模型", "Download the vocal-separation model first");
    pub const BACKEND: Str = Str::new("推理后端", "Inference backend");
    pub const BACKEND_AUTO: Str = Str::new("自动", "Auto");
    pub const SOUND: Str = Str::new("提示音", "Sounds");

    // ---- 统计面板 ----
    pub const STATS_EMPTY: Str =
        Str::new("完成第一个任务后，这里开始记账。", "Stats start once the first task is done.");
    pub const STATS_SAVED_TOTAL: Str = Str::new("累计省下", "Time saved");
    pub const STATS_MEDIA_TOTAL: Str = Str::new("素材总时长", "Total media");
    pub const STATS_PROCESS_TOTAL: Str = Str::new("机器耗时", "Processing time");
    pub const STATS_AVG_SPEED: Str = Str::new("平均速度", "Average speed");
    pub const STATS_DAILY: Str = Str::new("每天处理的素材时长", "Media duration per day");
    pub const STATS_TASKS: Str = Str::new("完成任务", "Tasks");
    pub const STATS_OUTPUT: Str = Str::new("输出文本", "Output text");
    pub const STATS_LANGS: Str = Str::new("语种", "Languages");
    pub const STATS_SEPARATION: Str = Str::new("人声分离", "Vocal separation");
    pub const STATS_LONGEST: Str = Str::new("最长一次", "Longest run");
    pub const STATS_FASTEST: Str = Str::new("最快一次", "Fastest run");
    pub const LEGEND_LESS: Str = Str::new("少", "Less");
    pub const LEGEND_MORE: Str = Str::new("多", "More");
}

/// `{sec} 秒 · {min}–{max}` / `{sec} s · {min}–{max}`（分段时长档位说明）。
pub fn chunk_target_label(sec: u32, min: u32, max: u32) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{sec} 秒 · {min}–{max}"),
        UiLang::En => format!("{sec} s · {min}–{max}"),
    }
}

/// 界面语言按钮的 tooltip：`界面语言：{mode}（点击切换，保存后生效）`
/// / `Interface language: {mode} (click to cycle; applies on save)`。
pub fn ui_language_tooltip(mode: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("界面语言：{mode}（点击切换，保存后生效）"),
        UiLang::En => format!("Interface language: {mode} (click to cycle; applies on save)"),
    }
}

/// `保存失败: {e}`
pub fn save_failed(e: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("保存失败: {e}"),
        UiLang::En => format!("Save failed: {e}"),
    }
}

/// `已省 {span}`（状态栏统计 chip）。
pub fn saved_prefix(span: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("已省 {span}"),
        UiLang::En => format!("Saved {span}"),
    }
}

/// `{factor} 倍速` / `{factor}×`（平均速度、最快一次）。
pub fn rtfx(factor: f64) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{factor:.1} 倍速"),
        UiLang::En => format!("{factor:.1}×"),
    }
}

/// 月份标签：zh `3 月`，en `Mar`。
pub fn month_label(m: u32) -> String {
    let name = |i: usize| {
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ][i]
    };
    match ui_lang() {
        UiLang::Zh => format!("{m} 月"),
        UiLang::En => name(m.clamp(1, 12) as usize - 1).to_string(),
    }
}

/// `{n} 个任务` / `{n} task(s)`（英文按个数取复数）。
pub fn n_tasks(lang: UiLang, n: usize) -> String {
    match lang {
        UiLang::Zh => format!("{n} 个任务"),
        UiLang::En if n == 1 => "1 task".into(),
        UiLang::En => format!("{n} tasks"),
    }
}

/// `{n} 行` / `{n} lines`。
pub fn n_lines(n: u64) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{} 行", crate::app::ui::util::format_thousands(n)),
        UiLang::En => format!("{} lines", crate::app::ui::util::format_thousands(n)),
    }
}

/// `{n} 个 · 全部成功` / `{n} · all successful`。
pub fn tasks_all_ok(lang: UiLang, n: usize) -> String {
    match lang {
        UiLang::Zh => format!("{n} 个 · 全部成功"),
        UiLang::En => format!("{} · all successful", n_tasks(lang, n)),
    }
}

/// `{n} 个 · 成功 {ok} · 失败 {err}` / `{n} · {ok} ok · {err} failed`。
pub fn tasks_split(total: usize, ok: usize, err: usize) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{total} 个 · 成功 {ok} · 失败 {err}"),
        UiLang::En => format!("{total} · {ok} ok · {err} failed"),
    }
}

/// ` · 等 {n} 种` / ` · +{n} more`（语种行超过两个时）。
pub fn langs_more(n: usize) -> String {
    match ui_lang() {
        UiLang::Zh => format!(" · 等 {n} 种"),
        UiLang::En => format!(" · +{n} more"),
    }
}

/// `用时 {total}` / `Took {total}`（任务行用时 chip）。
pub fn took_time(total: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("用时 {total}"),
        UiLang::En => format!("Took {total}"),
    }
}

/// `下载启动失败: {e}`
pub fn download_start_failed(e: &std::io::Error) -> String {
    match ui_lang() {
        UiLang::Zh => format!("下载启动失败: {e}"),
        UiLang::En => format!("Failed to start download: {e}"),
    }
}

/// `{label} 已就绪` / `{label} is ready`。
pub fn model_ready(label: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{label} 已就绪"),
        UiLang::En => format!("{label} is ready"),
    }
}

/// `{label} 下载完成` / `{label} download complete`。
pub fn model_download_done(label: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{label} 下载完成"),
        UiLang::En => format!("{label} download complete"),
    }
}

/// `{label} 已就绪，可在设置中切换使用` / `{label} is ready — switch to it in Settings`。
pub fn model_ready_switchable(label: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{label} 已就绪，可在设置中切换使用"),
        UiLang::En => format!("{label} is ready — switch to it in Settings"),
    }
}

/// `{label} 下载失败: {msg}` / `{label} download failed: {msg}`。
pub fn model_download_failed(label: &str, msg: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{label} 下载失败: {msg}"),
        UiLang::En => format!("{label} download failed: {msg}"),
    }
}

/// `{label} 已取消` / `{label} cancelled`。
pub fn model_cancelled(label: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{label} 已取消"),
        UiLang::En => format!("{label} cancelled"),
    }
}

/// `处理线程异常: {msg}` / `Task thread panicked: {msg}`。
pub fn task_thread_panic(msg: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("处理线程异常: {msg}"),
        UiLang::En => format!("Task thread panicked: {msg}"),
    }
}

/// 未知 panic 载荷的兜底文案。
pub fn unknown_panic() -> &'static str {
    match ui_lang() {
        UiLang::Zh => "未知 panic",
        UiLang::En => "Unknown panic",
    }
}

/// `{msg} · 已省 {saved} · 左下角看统计`
/// / `{msg} · {saved} saved · see Stats at the bottom left`。
pub fn run_complete_with_saved(msg: &str, saved: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("{msg} · 已省 {saved} · 左下角看统计"),
        UiLang::En => format!("{msg} · {saved} saved · see Stats at the bottom left"),
    }
}
