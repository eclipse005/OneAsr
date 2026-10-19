//! Headless OneAsr CLI — same pipeline as the GUI worker.
//!
//! ```powershell
//! cargo run -p oneasr-core --release --bin oneasr-cli -- `
//!   transcribe --input "C:\path\to\video.mp4" --app-root "D:\OneAsr" `
//!   --language zh --chunk-seconds 60 --backend auto
//!
//! cargo run -p oneasr-core --release --bin oneasr-cli -- `
//!   asr-chunk --wav "D:\OneAsr\runs\...\input_16k.wav" `
//!   --start 722.75 --end 842.75 --language zh
//!
//! cargo run -p oneasr-core --release --bin oneasr-cli -- `
//!   download --model 0.6b --data-root "D:\\OneAsr"
//! ```
//!
//! 机器可读出口有三个：`status --json`（安装 / 模型的完整快照）、`probe --json`
//! （一个文件的尺寸 / 时长 / 类型）与 `transcribe|align --events=json`（每行一个
//! JSON 事件）。三者都只在 stdout 上说话，人读行一律 stderr。
//!
//! `download` 把人类可读的进度写 stderr（双语，与其它子命令一致），把机器可读的
//! 那一行写 **stdout**（`[download] state=… model=… done=… total=… pct=…`）——上层只解析
//! stdout，报警文本不会混进解析流。完整格式与「先验后下」的语义见 `--help` 的 download 段落。
//!
//! Layout: `--app-root` is the **install** directory (`bin/ffmpeg`) and keeps its
//! old meaning; `--data-root` (or `ONEASR_DATA_DIR`) pins the **data** directory
//! that owns `models/`, `output/`, `runs/` and `settings.json`. Without
//! `--data-root` the two are the same, exactly as before.
//!
//! Env: `ONEASR_PIPELINE_TRACE=1` prints per-chunk ASR/align detail.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use oneasr_core::i18n;
use oneasr_core::media::slice_wav;
use oneasr_core::model::HTDEMUCS_FT;
use oneasr_core::{
    AsrStage, DownloadHandle, DownloadOutcome, GroupLookup, HotwordRun, HotwordStore, ModelId,
    ModelKind, ProcessExportOptions, Settings, StageClock, StageUpdate, TaskTiming, download_model,
    ffmpeg_source, process_media_file_with_export, resolve_app_root,
};
use qwen3_asr_wgpu::{AsrInference, Backend as AsrBackend, TranscribeOptions};
use serde_json::json;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || is_help(&args[1]) {
        print_help();
        return if args.len() < 2 {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    // 线程池 / 线程优先级：这里**刻意不调用** `oneasr_core::init_runtime()`。
    // 它做的唯一一件事是"给 UI 事件循环留一个核"（为全局 rayon 池保留一个
    // 逻辑核），理由是让 UI 不被模型 CPU 算子饿死；CLI 没有事件循环要保护，
    // 一次性批处理把整台机器用满才是对的，ffmpeg 作为子进程也不需要别人让路。
    // `demote_current_thread()` 同理，那是 GUI worker 线程的优先级策略。

    // CLI 不读 settings.json（全部走参数），文案语言直接跟随系统语言——
    // 对所有子命令、所有输出生效：用法、参数校验错误、运行摘要都取自
    // `oneasr_core::i18n`，本文件里不出现硬编码文案（键值摘要的键名即 flag 名，
    // 两种语言一致，同样定义在 i18n）。
    oneasr_core::i18n::set_ui_lang(oneasr_core::i18n::detect_system_lang());

    // Default subcommand: bare flags act as `transcribe`.
    let (cmd, rest) = if args[1].starts_with('-') {
        ("transcribe", &args[1..])
    } else {
        (args[1].as_str(), &args[2..])
    };

    let result = match cmd {
        "transcribe" | "run" | "pipeline" => cmd_transcribe(rest),
        "asr-chunk" | "chunk" => cmd_asr_chunk(rest),
        "align" => cmd_align(rest),
        "transcript" => cmd_transcript(rest),
        "render" => cmd_render(rest),
        "download" => cmd_download(rest),
        "status" => cmd_status(rest),
        "probe" => cmd_probe(rest),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("{}", i18n::cli_unknown_command(other));
            print_help();
            Err(2)
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code as u8),
    }
}

/// `--help` 全文取自 `i18n::cli_help()`，两种语言各一份，改文案只改 i18n。
fn print_help() {
    eprintln!("{}", i18n::cli_help());
}

// ─── transcribe ───────────────────────────────────────────────────

fn cmd_transcribe(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let events_json = events_json_arg(args)?;
    let input = require_arg(args, "--input")?;
    let app_root = parse_app_root(args)?;
    // 数据目录必须在任何 `Settings::default()` / 路径解析之前钉住。
    let data_root = pin_data_root(args, &app_root)?;
    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let chunk_seconds: u32 = arg(args, "--chunk-seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let backend = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    let max_new_tokens = arg(args, "--max-new-tokens").and_then(|s| s.parse().ok());
    let output_copy = arg(args, "--output").map(PathBuf::from);
    let words_json = arg(args, "--words-json").map(PathBuf::from);
    let want_txt = flag(args, "--txt");
    let want_ass = flag(args, "--ass");
    let no_srt = flag(args, "--no-srt");
    let no_timeline = flag(args, "--no-timeline");
    let script = arg(args, "--script");
    let vocal_separation = flag(args, "--vocal-separation");
    let demucs_model_dir = arg(args, "--demucs-model-dir").map(PathBuf::from);
    let hotword_group = arg(args, "--hotword-group");
    let hotword_words = arg(args, "--hotwords");
    let sensevoice_model_dir = arg(args, "--sensevoice-model-dir").map(PathBuf::from);
    let output_dir = arg(args, "--output-dir").map(PathBuf::from);
    let save_next_to_source = flag(args, "--save-next-to-source");

    let input_path = PathBuf::from(&input);
    if !input_path.is_file() {
        eprintln!("{}", i18n::cli_input_not_found(&input_path));
        return Err(1);
    }
    ensure_ffmpeg(&app_root)?;

    let mut settings = Settings {
        language,
        ..Settings::default()
    };
    // `Settings::normalize()` below clamps the chunk target to the product range.
    settings.chunk_target_seconds = chunk_seconds;
    settings.backend = backend;
    if let Some(n) = max_new_tokens {
        settings.max_new_tokens = n;
    }
    settings.output_txt = want_txt;
    settings.output_ass = want_ass;
    settings.output_srt = !no_srt;
    if no_srt && !want_txt && !want_ass {
        eprintln!("{}", i18n::cli_no_srt_warning());
    }
    if let Some(s) = script {
        settings.text_script = s;
    }
    settings.vocal_separation = vocal_separation;
    // `--asr 0.6b|1.7b|<目录名>`：显式指定只认那一个，否则按 0.6B→1.7B、
    // fp16 优先的顺序扫描已装目录。与 `--aligner` 同一套优先级。
    let asr_override = match arg(args, "--asr").as_deref() {
        None => None,
        Some(raw) => match parse_asr_arg(raw) {
            Some(id) => Some(id),
            None => {
                eprintln!("{}", i18n::cli_asr_unknown(raw));
                return Err(2);
            }
        },
    };
    // `--aligner ctc|qwen|<目录名>`：显式指定只认那一个，否则按 CTC→Qwen 扫描。
    let aligner_override = match arg(args, "--aligner").as_deref() {
        None => None,
        Some(raw) => match parse_aligner_arg(raw) {
            Some(id) => Some(id),
            None => {
                eprintln!("{}", i18n::cli_aligner_unknown(raw));
                return Err(2);
            }
        },
    };
    apply_app_root_paths(
        &mut settings,
        &app_root,
        &data_root,
        asr_override,
        aligner_override,
    );
    apply_output_flags(&mut settings, output_dir, save_next_to_source);
    if let Some(dir) = demucs_model_dir {
        settings.demucs_model_dir = dir;
    }
    if let Some(dir) = sensevoice_model_dir {
        settings.sensevoice_model_dir = dir;
    }
    // 热词只在这一趟识别里有消费者，所以**只有给了旗标才碰 hotword.json**——
    // 不碰它，普通转写就不会触发老配置迁移之类的写盘副作用。
    // `--hotword-group`（从库里查组）与 `--hotwords`（即席词表）二选一。
    if hotword_group.is_some() || hotword_words.is_some() {
        let store = load_hotwords_or_warn();
        settings.hotword_run = resolve_cli_hotwords(
            &store,
            hotword_group.as_deref(),
            hotword_words.as_deref(),
            i18n::ui_lang(),
        )?;
    }
    settings.normalize();
    if let Err(e) = settings.can_start() {
        eprintln!("{e}");
        return Err(1);
    }

    eprintln!("{}", i18n::cli_transcribe_banner());
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_INPUT, &input_path));
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_APP, &app_root));
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_DATA, &data_root));
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_ASR, &settings.asr_model_dir)
    );
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_ALIGN, &settings.aligner_model_dir)
    );
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_OUTPUT, &settings.srt_target_dir(&input_path))
    );
    if let Some(run) = &settings.hotword_run {
        // 只报条数、不回显组名（banner 会进日志、会被贴出去，组名是用户内容）。
        eprintln!("{}", i18n::cli_hotword_run(run.word_count()));
        // 语言不支持时管线会整趟跳过（粗转不跑、context 不注入）：选了组却
        // 什么都不说等于静默无视旗标，那条警告是「旗标真的被看见了」的凭据。
        if !oneasr_core::lang::supports_hotwords(&settings.language) {
            eprintln!(
                "{}",
                i18n::cli_warn(&i18n::cli_hotwords_language_skipped(&settings.language))
            );
        }
    }
    eprintln!(
        "{} {}",
        i18n::cli_run_flags_decode(
            &settings.language,
            settings.chunk_target_seconds,
            &settings.backend,
            settings.max_new_tokens,
        ),
        i18n::cli_run_flags_output(
            settings.output_srt,
            settings.output_txt,
            settings.text_script_choice().label(i18n::ui_lang()),
            settings.vocal_separation,
        ),
    );

    let media_name = input_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| input_path.display().to_string());

    let export = ProcessExportOptions {
        words_json: words_json.clone(),
        // The sidecar is what makes `render` possible, so it stays on by default.
        // `--no-timeline` drops it — and with it the only reason a TXT-only run
        // would need the aligner at all (see `needs_alignment`).
        timeline_json: !no_timeline,
    };
    // 阶段编号的分母在**开跑前**算好：条件阶段与管线同一套判据（见
    // `transcribe_stage_plan`），运行中不会再变。
    let mut events = StageEvents::new(events_json, transcribe_stage_plan(&settings, &export));
    let mut clock = StageClock::new();
    let t0 = Instant::now();
    let result = process_media_file_with_export(
        &input_path,
        &media_name,
        &settings,
        &data_root,
        |update: StageUpdate| {
            clock.note(&update);
            events.note(&update);
            if let Some(w) = &update.warning {
                eprintln!("{}", i18n::cli_warn(w));
            }
            eprintln!("{}", i18n::cli_stage(&update.label(i18n::ui_lang())));
        },
        export,
    );
    let timing = clock.finish();
    let wall = t0.elapsed();

    match result {
        Ok(primary) => {
            eprintln!("{}", i18n::cli_ok_output(&primary));
            if let Some(dst) = output_copy {
                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::copy(&primary, &dst) {
                    let message = e.to_string();
                    eprintln!("{}", i18n::cli_copy_failed(&message));
                    // 复制失败也是失败收尾：事件流最后一条必须是 done 或 error。
                    events.finish(&timing);
                    events.error(&i18n::cli_copy_failed(&message));
                    return Err(1);
                }
                eprintln!("{}", i18n::cli_copied(&dst));
            }
            if let Some(wj) = words_json {
                eprintln!("{}", i18n::cli_words_written(&wj));
            }
            eprintln!(
                "{}",
                i18n::cli_timing(wall.as_secs_f64(), timing.total_ms, timing.stages.len())
            );
            for s in &timing.stages {
                eprintln!(
                    "{}",
                    i18n::cli_stage_row(
                        s.stage.label(i18n::ui_lang()),
                        &oneasr_core::format_process_ms(s.elapsed_ms)
                    )
                );
            }
            events.finish(&timing);
            events.done(&primary);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", i18n::cli_failed(wall.as_secs_f64(), &e.to_string()));
            for s in &timing.stages {
                eprintln!(
                    "{}",
                    i18n::cli_stage_row(
                        s.stage.label(i18n::ui_lang()),
                        &oneasr_core::format_process_ms(s.elapsed_ms)
                    )
                );
            }
            events.finish(&timing);
            events.error(&e.to_string());
            Err(1)
        }
    }
}

// ─── 事件流（--events=json） ───────────────────────────────────────

/// `--events=json`：机器可读进度走 stdout，人读行仍走 stderr。
///
/// 同时接受 `--events=json` 与 `--events json` 两种写法（壳用的是前者）；值只认
/// `json`。别的值报用法错误（2）而不是猜一个默认——猜错等于让调用方以为拿到的是
/// JSON，下一句 `JSON.parse` 才发现。
fn events_json_arg(args: &[String]) -> Result<bool, i32> {
    let raw = args
        .iter()
        .find_map(|a| a.strip_prefix("--events="))
        .map(str::to_string)
        .or_else(|| arg(args, "--events"));
    if raw.is_none() && flag(args, "--events") {
        // 末尾孤零零一个 `--events`：当成「没给」等于静默关掉事件流。
        eprintln!("{}", i18n::cli_missing_required("--events"));
        return Err(2);
    }
    match raw.as_deref() {
        None => Ok(false),
        Some(v) if v.eq_ignore_ascii_case("json") => Ok(true),
        Some(v) => {
            eprintln!("{}", i18n::cli_events_unknown(v));
            Err(2)
        }
    }
}

/// 一个阶段在**这一次运行**里的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StageProgress {
    /// 从 1 数。
    index: usize,
    /// 计划里的阶段总数（条件阶段已经筛过）。
    total: usize,
    /// `index / total` 四舍五入到整数。
    pct: u32,
}

/// 阶段在计划里的编号；不在计划里返回 `None`（调用方自己兜底）。
///
/// 纯函数：分母是**计划**的长度，所以不参与运行的阶段不会被算进去。
fn stage_progress(plan: &[AsrStage], stage: AsrStage) -> Option<StageProgress> {
    let index = plan.iter().position(|s| *s == stage)? + 1;
    let total = plan.len();
    let pct = ((index as f64 / total as f64) * 100.0).round() as u32;
    Some(StageProgress { index, total, pct })
}

/// `transcribe` 这一趟会出现的阶段，顺序 = 管线实际发射顺序。
///
/// 条件阶段与管线**同一套判据**（这里不重抄条件）：
/// - `Separating` ← `settings.vocal_separation`；
/// - `RoughTranscribing` ← 语言接热词 + 词表可用 + 音素索引非空
///   （`asr.rs::stage_rough_transcribe` 的前三道闸）；
/// - `LoadingAligner` / `Aligning` ← [`oneasr_core::needs_alignment`]（TXT-only 不跑）。
fn transcribe_stage_plan(settings: &Settings, export: &ProcessExportOptions) -> Vec<AsrStage> {
    let mut plan = Vec::new();
    if settings.vocal_separation {
        plan.push(AsrStage::Separating);
    }
    plan.push(AsrStage::Converting);
    plan.push(AsrStage::Planning);
    if hotword_rough_will_run(settings) {
        plan.push(AsrStage::RoughTranscribing);
    }
    plan.push(AsrStage::LoadingAsr);
    plan.push(AsrStage::Transcribing);
    if oneasr_core::needs_alignment(settings, export) {
        plan.push(AsrStage::LoadingAligner);
        plan.push(AsrStage::Aligning);
    }
    plan.push(AsrStage::Exporting);
    plan
}

/// `align` 这一趟的阶段：没有识别、没有分段规划、没有热词
/// （见 `asr.rs::Pipeline::run_with_transcript`）。
fn align_stage_plan(settings: &Settings) -> Vec<AsrStage> {
    let mut plan = Vec::new();
    if settings.vocal_separation {
        plan.push(AsrStage::Separating);
    }
    plan.push(AsrStage::Converting);
    plan.push(AsrStage::LoadingAligner);
    plan.push(AsrStage::Aligning);
    plan.push(AsrStage::Exporting);
    plan
}

/// 热词粗转这一趟会不会真跑（与 `asr.rs::stage_rough_transcribe` 同款三闸）。
fn hotword_rough_will_run(settings: &Settings) -> bool {
    if !oneasr_core::lang::supports_hotwords(&settings.language) {
        return false;
    }
    let Some(run) = settings.hotword_run.as_ref().filter(|run| run.is_usable()) else {
        return false;
    };
    !oneasr_core::hotword::HotwordIndex::new(&run.text, run.threshold).is_empty()
}

/// 分段计数：`total <= 1` 不算分段（界面不出现 1/1），与 `StageUpdate::label` 同一条规则。
fn chunk_mark(chunk: Option<(usize, usize)>) -> Option<(usize, usize)> {
    match chunk {
        Some((cur, total)) if total > 1 => Some((cur, total)),
        _ => None,
    }
}

/// 一条 stage 事件的 JSON。
///
/// 字段名是契约（壳按名取值）；抽成纯函数是为了让**形状**能在单测里断言，
/// 不必去捕获 stdout。`index` / `total` / `pct` 是**阶段计划**里的位置，不是分段。
/// 分段（转写 2/5、打轴 3/10）只在 `total > 1` 时多带 `chunk` / `chunkTotal`。
fn stage_event(
    stage: AsrStage,
    progress: StageProgress,
    chunk: Option<(usize, usize)>,
) -> serde_json::Value {
    let mut value = json!({
        "event": "stage",
        "key": stage.key(),
        "label": stage.label(i18n::ui_lang()),
        "index": progress.index,
        "total": progress.total,
        "pct": progress.pct,
    });
    if let Some((cur, total)) = chunk_mark(chunk) {
        value["chunk"] = json!(cur);
        value["chunkTotal"] = json!(total);
    }
    value
}

/// 收尾的耗时事件：每个阶段一条 `timing`，最后一条 `total`。
///
/// `timing` 也带 `key`：耗时明细卡要按它出**界面语言**的文案，只有 label 的话
/// 会跟着 CLI 进程语言（系统语言）跑，与界面语言不一致。key 与 stage 事件同源
/// （`AsrStage::key()`）。
fn timing_events(timing: &TaskTiming) -> Vec<serde_json::Value> {
    let mut events: Vec<serde_json::Value> = timing
        .stages
        .iter()
        .map(|s| {
            json!({
                "event": "timing",
                "key": s.stage.key(),
                "label": s.stage.label(i18n::ui_lang()),
                "ms": s.elapsed_ms,
            })
        })
        .collect();
    events.push(json!({ "event": "total", "ms": timing.total_ms }));
    events
}

/// 成功收尾：`output` 是最终产物路径。
fn done_event(output: &Path) -> serde_json::Value {
    json!({ "event": "done", "output": output.display().to_string() })
}

/// 失败收尾：`message` 是双语原因。
fn error_event(message: &str) -> serde_json::Value {
    json!({ "event": "error", "message": message })
}

/// 上次发出的阶段，以及当时的分段（没有分段是 `None`）。
type StageCursor = Option<(AsrStage, Option<(usize, usize)>)>;

/// 这一次阶段更新要不要发 stage 事件。
///
/// 阶段变了就发。同一阶段里，只有分段计数变了才再发（`total <= 1` 不算分段）。
/// 同阶段、没有新分段的更新（例如人声分离改走 CPU 的警告）不发，免得把已经
/// 显示的 n/N 清掉。返回 `(事件, 下一次用来去重的游标)`。
fn stage_note(
    plan: &mut Vec<AsrStage>,
    last: StageCursor,
    update: &StageUpdate,
) -> (Option<serde_json::Value>, StageCursor) {
    let mark = chunk_mark(update.chunk);
    if let Some((stage, prev)) = last
        && stage == update.stage
        && (mark.is_none() || mark == prev)
    {
        return (None, last);
    }
    if !plan.contains(&update.stage) {
        plan.push(update.stage);
    }
    let event = stage_progress(plan, update.stage)
        .map(|progress| stage_event(update.stage, progress, mark));
    (event, Some((update.stage, mark)))
}

/// `--events=json` 的事件出口：阶段变化时发 stage，同一阶段里分段变了再发一条，
/// 收尾发 timing / total，最后一条是 done 或 error。
///
/// 计划外的阶段理论上是「算漏了」，但接口上每个 stage 事件都必须带 index/total/pct；
/// 宁可就地把它补进计划（total 随之 +1），也不发一个缺字段的事件。正常路径走不到。
struct StageEvents {
    enabled: bool,
    plan: Vec<AsrStage>,
    last: StageCursor,
}

impl StageEvents {
    fn new(enabled: bool, plan: Vec<AsrStage>) -> Self {
        Self {
            enabled,
            plan,
            last: None,
        }
    }

    /// 收到一次阶段更新。分段进度（转写 2/5、打轴 3/10）各发一条；完全相同的不重复。
    fn note(&mut self, update: &StageUpdate) {
        if !self.enabled {
            return;
        }
        let (event, next) = stage_note(&mut self.plan, self.last, update);
        self.last = next;
        if let Some(event) = event {
            println!("{event}");
        }
    }

    /// 每个阶段的耗时 + 总耗时（收尾的第一段）。
    fn finish(&self, timing: &TaskTiming) {
        if !self.enabled {
            return;
        }
        for event in timing_events(timing) {
            println!("{event}");
        }
    }

    /// 成功收尾：`output` 是最终产物路径（不是 `--output` 的副本）。
    fn done(&self, output: &Path) {
        if !self.enabled {
            return;
        }
        println!("{}", done_event(output));
    }

    /// 失败收尾：`message` 是双语原因。
    fn error(&self, message: &str) {
        if !self.enabled {
            return;
        }
        println!("{}", error_event(message));
    }
}

// ─── status ───────────────────────────────────────────────────────

/// `status` —— 这台机器 + 这份设置的机器可读快照（上层壳的就绪判据都在这里）。
///
/// 与其它子命令的差别只有一处：它**读 settings.json**（全局设置唯一的一份在那里），
/// 所以文案语言也跟着 `resolved_ui_language()`；其余子命令只认参数、跟随系统语言。
/// `--json` 时 stdout 上只有一个 JSON 对象，人读行一律 stderr。
///
/// 逐行开关（`--language` / `--vocal-separation` / `--hotword-group` /
/// `--hotwords` / `--txt` / `--ass` / `--no-srt`）回答的是「**这一行**这样跑能不能起」：
/// 给了哪些就叠到设置副本上再问 `Settings::can_start()`，一个都不给就是全局设置的
/// 结论。判据本身只有 core 那一处，这里不做第二次判断。
fn cmd_status(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let app_root = parse_app_root(args)?;
    // 数据目录必须先钉住：settings.json、models/ 都从它算。
    let data_root = pin_data_root(args, &app_root)?;
    let want_json = flag(args, "--json");

    let mut settings = Settings::load();
    // can_start 的原因由 core 按**进程语言**定格，所以先把语言设对再问门禁。
    i18n::set_ui_lang(settings.resolved_ui_language());
    apply_status_overrides(&mut settings, args)?;
    let settings = resolve_status_paths(settings, &app_root, &data_root);
    let mut report = status_report(
        &settings,
        &app_root,
        &data_root,
        &oneasr_core::paths::models_root(),
    );
    // 文稿任务换一条门禁：文字来自用户，不需要 ASR；--transcript 还把
    // 「至少 SRT 或 ASS」提前到点开始之前。原因仍是 core 的双语串。
    if flag(args, "--transcript") {
        report.can_start_reason = settings.can_start_transcript().err();
    }

    if want_json {
        println!("{}", report.to_json());
    } else {
        print_status_human(&report);
    }
    Ok(())
}

/// 把「这一行怎么跑」的旗标叠到设置副本上。
///
/// 只应用**给到的**开关：没给的一项保持 settings.json 原样 —— 这正是「不给任何开关
/// = 全局 can_start」的实现方式。
///
/// 三个输出格式旗标与 transcribe **逐字同义**（`--txt` / `--ass` 打开对应产物、
/// `--no-srt` 关掉 SRT，其余不动）。别把它做成「只导点名的那些」：壳是按设置原样
/// 转发这些旗标的，settings 里 SRT 还开着时只传 `--txt` 必须仍然要求对齐模型，
/// 否则门禁会放行一个跑不起来的任务（假绿）。
fn apply_status_overrides(settings: &mut Settings, args: &[String]) -> Result<(), i32> {
    if let Some(language) = arg(args, "--language") {
        settings.language = language;
    }
    // 文稿任务固定 CTC；壳在问门禁时显式点名它，否则设置的 Qwen 已装会
    // 放行、跑到打轴阶段才发现 CTC 缺失（假绿）。
    if let Some(raw) = arg(args, "--aligner") {
        match parse_aligner_arg(&raw) {
            Some(id) => settings.aligner_model = id.as_str().into(),
            None => {
                eprintln!("{}", i18n::cli_aligner_unknown(&raw));
                return Err(2);
            }
        }
    }
    if flag(args, "--vocal-separation") {
        settings.vocal_separation = true;
    }
    if flag(args, "--txt") {
        settings.output_txt = true;
    }
    if flag(args, "--ass") {
        settings.output_ass = true;
    }
    if flag(args, "--no-srt") {
        settings.output_srt = false;
    }
    let hotword_group = arg(args, "--hotword-group");
    let hotword_words = arg(args, "--hotwords");
    if hotword_group.is_some() || hotword_words.is_some() {
        let store = load_hotwords_or_warn();
        settings.hotword_run = resolve_cli_hotwords(
            &store,
            hotword_group.as_deref(),
            hotword_words.as_deref(),
            i18n::ui_lang(),
        )?;
    }
    Ok(())
}

/// 模型目录的解析：与 transcribe 同一个函数、同一套 root 顺序。
///
/// 与 transcribe 唯一的差别是**显式点名**当前的选中项（[`AsrPick::Exact`]）：不带
/// 旗标时 `apply_app_root_paths` 按 ASR_CHOICES 扫描（0.6B 优先），那会把设置里选的
/// 1.7B 静默压回 0.6B —— 门禁于是去看另一个模型的目录，是典型的假绿。壳调 transcribe
/// 时传的正是设置里的 id（`--asr <id>` / `--aligner <id>`），这里跟上它的选择。
fn resolve_status_paths(mut settings: Settings, app_root: &Path, data_root: &Path) -> Settings {
    // transcribe 从 `Settings::default()` 起手：settings 里那种「每个尺寸手选的目录」
    // （`asr_dirs` / `aligner_dirs`）它一次都读不到，只认 catalog id + root 布局。
    // status 报的是**命令行这一趟真会用的目录**，所以这里跟着丢掉手选记录 ——
    // 否则门禁会去查一个 run 根本不加载的目录（假绿）。
    settings.asr_dirs.clear();
    settings.aligner_dirs.clear();
    let asr = settings.selected_asr_id();
    let aligner = settings.selected_aligner_id();
    apply_app_root_paths(
        &mut settings,
        app_root,
        data_root,
        Some(AsrPick::Exact(asr)),
        Some(aligner),
    );
    settings.normalize();
    settings
}

/// `status` 的一条模型槽位。
struct ModelSlot {
    id: String,
    dir: PathBuf,
    ready: bool,
    /// 不就绪的原因（就绪时 `None`）；文案来自 core，双语。
    error: Option<String>,
}

/// `status` 的 catalog 条目。
struct CatalogEntry {
    id: &'static str,
    kind: &'static str,
    dir: PathBuf,
    ready: bool,
}

/// `status` 的全部事实：JSON 与人读摘要共用一份，两个出口不各算一遍。
struct StatusReport {
    version: &'static str,
    app_root: PathBuf,
    data_root: PathBuf,
    models_root: PathBuf,
    output_dir: PathBuf,
    backend: String,
    language: String,
    /// 已解析成 `zh` / `en`（不会是 `system`）——上层不必再探一次系统语言。
    ui_language: &'static str,
    /// 全仓库唯一的音视频扩展名清单（core `media::supported_exts()`）：全小写、
    /// 不带点，音频在前、视频在后。壳拿它拼文件过滤器，不再自己维护正则。
    media_exts: Vec<&'static str>,
    /// 文稿扩展名（core `transcript::TRANSCRIPT_EXTS`）：壳拿它做配对与过滤器。
    transcript_exts: Vec<&'static str>,
    asr: ModelSlot,
    aligner: ModelSlot,
    demucs: ModelSlot,
    sensevoice: ModelSlot,
    catalog: Vec<CatalogEntry>,
    /// `None` = 可以开跑；`Some(reason)` 就是 `can_start()` 的双语错误串。
    can_start_reason: Option<String>,
}

impl StatusReport {
    fn can_start_ok(&self) -> bool {
        self.can_start_reason.is_none()
    }

    /// `status --json` 的 stdout 形状（一个对象，一行）。
    ///
    /// **键名是契约**（壳按名取值），key 顺序不是：serde_json 默认按字典序排，
    /// 调用方按名读即可。
    fn to_json(&self) -> serde_json::Value {
        fn slot(slot: &ModelSlot) -> serde_json::Value {
            json!({
                "id": slot.id,
                "dir": slot.dir.display().to_string(),
                "ready": slot.ready,
                "error": slot.error,
            })
        }
        json!({
            "version": self.version,
            "app_root": self.app_root.display().to_string(),
            "data_root": self.data_root.display().to_string(),
            "models_root": self.models_root.display().to_string(),
            "output_dir": self.output_dir.display().to_string(),
            "backend": self.backend,
            "language": self.language,
            "ui_language": self.ui_language,
            "media_exts": self.media_exts,
            "transcript_exts": self.transcript_exts,
            "models": {
                "asr": slot(&self.asr),
                "aligner": slot(&self.aligner),
                "demucs": slot(&self.demucs),
                "sensevoice": slot(&self.sensevoice),
            },
            "catalog": self
                .catalog
                .iter()
                .map(|entry| json!({
                    "id": entry.id,
                    "kind": entry.kind,
                    "dir": entry.dir.display().to_string(),
                    "ready": entry.ready,
                }))
                .collect::<Vec<_>>(),
            "can_start": {
                "ok": self.can_start_ok(),
                "reason": self.can_start_reason,
            },
        })
    }
}

/// 组装 `status` 的全部事实（**纯函数**：四个入参给全，单测不读进程级 data root）。
///
/// 四条槽位与 catalog 的 `ready` 走**同一个** `check_*_model_dir`（全仓库唯一判据），
/// `can_start` 直接用 `Settings::can_start()` 的结果。
fn status_report(
    settings: &Settings,
    app_root: &Path,
    data_root: &Path,
    models_root: &Path,
) -> StatusReport {
    let slot = |id: ModelId, dir: &Path| {
        let (ready, error) = ready_and_error(id, dir);
        ModelSlot {
            id: id.as_str().to_string(),
            dir: dir.to_path_buf(),
            ready,
            error,
        }
    };
    StatusReport {
        version: env!("CARGO_PKG_VERSION"),
        app_root: app_root.to_path_buf(),
        data_root: data_root.to_path_buf(),
        models_root: models_root.to_path_buf(),
        output_dir: settings.resolved_output_dir(),
        backend: settings.backend.clone(),
        language: settings.language.clone(),
        ui_language: settings.resolved_ui_language().setting_value(),
        // 表在 core：status 只转发，壳与 probe 读的是同一份（全仓库唯一判据）。
        media_exts: oneasr_core::media::supported_exts(),
        transcript_exts: oneasr_core::transcript::TRANSCRIPT_EXTS.to_vec(),
        asr: slot(settings.selected_asr_id(), &settings.asr_model_dir),
        aligner: slot(settings.selected_aligner_id(), &settings.aligner_model_dir),
        demucs: slot(ModelId::HtdemucsFt, &settings.resolved_demucs_model_dir()),
        sensevoice: slot(
            ModelId::SenseVoiceSmall,
            &settings.resolved_sensevoice_model_dir(),
        ),
        catalog: ModelId::ALL
            .iter()
            .map(|id| {
                let dir = models_root.join(id.as_str());
                CatalogEntry {
                    id: id.as_str(),
                    kind: id.kind().as_str(),
                    ready: check_model_dir(*id, &dir).is_ok(),
                    dir,
                }
            })
            .collect(),
        can_start_reason: settings.can_start().err(),
    }
}

/// （就绪, 原因）：判据是 core 的 `check_*_model_dir`，这里只做包装。
fn ready_and_error(id: ModelId, dir: &Path) -> (bool, Option<String>) {
    match check_model_dir(id, dir) {
        Ok(()) => (true, None),
        Err(error) => (false, Some(error)),
    }
}

/// 没有 `--json` 时的人读摘要（stdout；双语跟随 `resolved_ui_language()`）。
fn print_status_human(report: &StatusReport) {
    println!("{}", i18n::cli_status_banner());
    println!("{}", i18n::cli_kv_str(i18n::CLI_KV_VERSION, report.version));
    println!("{}", i18n::cli_kv(i18n::CLI_KV_APP, &report.app_root));
    println!("{}", i18n::cli_kv(i18n::CLI_KV_DATA, &report.data_root));
    println!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_MODELS_ROOT, &report.models_root)
    );
    println!("{}", i18n::cli_kv(i18n::CLI_KV_OUTPUT, &report.output_dir));
    println!(
        "{}",
        i18n::cli_kv_str(i18n::CLI_KV_BACKEND, &report.backend)
    );
    println!(
        "{}",
        i18n::cli_kv_str(i18n::CLI_KV_LANGUAGE, &report.language)
    );
    println!(
        "{}",
        i18n::cli_kv_str(i18n::CLI_KV_UI_LANGUAGE, report.ui_language)
    );
    println!(
        "{}",
        i18n::cli_kv_str(i18n::CLI_KV_MEDIA_EXTS, &report.media_exts.join(", "))
    );
    println!("{}", i18n::cli_status_models_header());
    for (role, slot) in [
        ("asr", &report.asr),
        ("aligner", &report.aligner),
        ("demucs", &report.demucs),
        ("sensevoice", &report.sensevoice),
    ] {
        println!(
            "{}",
            i18n::cli_model_row(role, &slot.id, &slot.dir, slot.ready, slot.error.as_deref())
        );
    }
    println!("{}", i18n::cli_status_catalog_header());
    for entry in &report.catalog {
        println!(
            "{}",
            i18n::cli_catalog_row(entry.kind, entry.id, entry.ready)
        );
    }
    println!(
        "{}",
        i18n::cli_kv_str(
            i18n::CLI_KV_CAN_START,
            &i18n::cli_can_start_line(report.can_start_ok(), report.can_start_reason.as_deref()),
        )
    );
}

// ─── probe ────────────────────────────────────────────────────────

/// `probe` —— 一个媒体文件的尺寸 / 时长 / 类型。
///
/// 上层壳以前自己 spawn ffmpeg 正则 `Duration`，现在只解析这里的 JSON：
/// - 时长走 [`oneasr_core::media::probe_duration_sec`]（WAV 读头，其余走 ffmpeg）；
///   探不到就是 `null`，**不是 0**——「探不到」与「真的是 0 秒」必须能分开；
/// - 类型由扩展名在 core 判定（audio / video / other 三个值）：两张表都不在就是
///   `other`（`.txt` 不再是「音频」），壳不再自己看后缀；
/// - 读不到文件元数据（不存在 / 无权限）是非 0 退出 + 双语原因。
fn cmd_probe(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let input = PathBuf::from(require_arg(args, "--input")?);
    let want_json = flag(args, "--json");

    let info = probe_info(&input).map_err(|error| {
        eprintln!("{}", i18n::cli_probe_failed(&input, &error));
        1
    })?;

    if want_json {
        println!("{}", info.to_json());
        return Ok(());
    }
    // 不带 --json 就是三行：大小 / 时长 / 类型。
    println!(
        "{}",
        i18n::cli_kv_str(i18n::CLI_KV_SIZE, &format!("{} B", info.size_bytes))
    );
    println!(
        "{}",
        i18n::cli_kv_str(
            i18n::CLI_KV_DURATION,
            &i18n::cli_probe_duration(info.duration_sec)
        )
    );
    println!(
        "{}",
        i18n::cli_kv_str(i18n::CLI_KV_KIND, info.kind.label(i18n::ui_lang()))
    );
    Ok(())
}

/// `probe` 的结果（`--json` 的四个字段 / 人读的三行）。
struct ProbeInfo {
    input: PathBuf,
    size_bytes: u64,
    duration_sec: Option<f64>,
    kind: oneasr_core::MediaKind,
}

impl ProbeInfo {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "input": self.input.display().to_string(),
            "size_bytes": self.size_bytes,
            "duration_sec": self.duration_sec,
            "kind": self.kind.as_str(),
        })
    }
}

/// 文件元数据 + 时长 + 类型；读不到元数据直接是错误（与探不探得到时长无关）。
fn probe_info(path: &Path) -> Result<ProbeInfo, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    Ok(ProbeInfo {
        input: path.to_path_buf(),
        size_bytes: meta.len(),
        duration_sec: oneasr_core::media::probe_duration_sec(path),
        kind: oneasr_core::media::media_kind(path),
    })
}

// ─── asr-chunk ────────────────────────────────────────────────────

fn cmd_asr_chunk(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let wav = PathBuf::from(require_arg(args, "--wav")?);
    let start: f32 = require_arg(args, "--start")?.parse().map_err(|_| {
        eprintln!("{}", i18n::cli_flag_not_number("--start"));
        2
    })?;
    let end: f32 = require_arg(args, "--end")?.parse().map_err(|_| {
        eprintln!("{}", i18n::cli_flag_not_number("--end"));
        2
    })?;
    if end <= start {
        eprintln!("{}", i18n::cli_end_before_start());
        return Err(2);
    }
    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let out = arg(args, "--out").map(PathBuf::from);
    let backend_s = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    let max_new_tokens = arg(args, "--max-new-tokens").and_then(|s| s.parse().ok());
    let app_root = parse_app_root(args).unwrap_or_else(|_| default_app_root());
    let data_root = pin_data_root(args, &app_root)?;

    if !wav.is_file() {
        eprintln!("{}", i18n::cli_wav_not_found(&wav));
        return Err(1);
    }

    let mut settings = Settings {
        language,
        backend: backend_s.clone(),
        ..Settings::default()
    };
    if let Some(n) = max_new_tokens {
        settings.max_new_tokens = n;
    }
    // 与 transcribe 同一个旗标：排查幻觉时能指定型号，才对得上 GUI 那边选的。
    let asr_override = match arg(args, "--asr").as_deref() {
        None => None,
        Some(raw) => match parse_asr_arg(raw) {
            Some(id) => Some(id),
            None => {
                eprintln!("{}", i18n::cli_asr_unknown(raw));
                return Err(2);
            }
        },
    };
    apply_app_root_paths(&mut settings, &app_root, &data_root, asr_override, None);
    settings.normalize();

    let tmp = env::temp_dir().join(format!(
        "oneasr_dump_{}_{}.wav",
        (start * 1000.0) as u32,
        (end * 1000.0) as u32
    ));
    // 拿到路径就交给守卫：下面任何一步提前返回（切片失败、路径非 UTF-8、
    // 转写失败、写 `--out` 失败）都不会把临时切片留在 %TEMP% 里。
    let tmp = TempFileGuard::new(tmp);
    slice_wav(&wav, start, end, tmp.path()).map_err(|e| {
        eprintln!("{}", i18n::cli_slice_failed(&e.to_string()));
        1
    })?;
    eprintln!(
        "{}",
        i18n::cli_slice_done(start, end, end - start, tmp.path())
    );

    let backend = match settings.backend.to_ascii_lowercase().as_str() {
        "cpu" => AsrBackend::Cpu,
        "gpu" => AsrBackend::Gpu,
        _ => AsrBackend::Auto,
    };
    let asr = AsrInference::load(&settings.asr_model_dir, backend).map_err(|e| {
        eprintln!("{}", i18n::load_asr_failed(&e.to_string()));
        1
    })?;
    let lang = oneasr_core::lang::to_qwen_language_label(&settings.language);
    let opts = TranscribeOptions::default()
        .with_max_new_tokens(settings.max_new_tokens)
        .with_language(lang);
    let path_str = tmp.path().to_str().ok_or_else(|| {
        eprintln!("{}", i18n::audio_path_not_utf8());
        1
    })?;
    let t0 = Instant::now();
    let report = asr.transcribe(path_str, opts).map_err(|e| {
        eprintln!("{}", i18n::cli_transcribe_failed(&e.to_string()));
        1
    })?;
    let text = report.text.trim();
    let chars = text.chars().count();
    eprintln!(
        "{}",
        i18n::cli_chars(
            chars,
            report.raw_output.chars().count(),
            t0.elapsed().as_secs_f64()
        )
    );
    eprintln!("{}", i18n::t(i18n::CLI_ASR_TEXT_BEGIN));
    println!("{text}");
    eprintln!("{}", i18n::t(i18n::CLI_ASR_TEXT_END));

    if let Some(p) = out {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&p, text.as_bytes()).map_err(|e| {
            eprintln!("{}", i18n::cli_write_failed(&e.to_string()));
            1
        })?;
        eprintln!("{}", i18n::cli_wrote(&p));
    }

    // 临时切片的清理交给 `TempFileGuard`（函数返回时删除，含上面每条早退路径）。
    Ok(())
}

/// `render` 的 TXT 出口：已渲染的句子列表 → 纯文本正文。
///
/// 单独成函数，是为了让「句子 → TXT」这一步能被单测钉住（见文件末尾的
/// `rendered_txt_body_keeps_english_sentences_separate`）：接线退回先
/// `join("")` 再断句时，英文句子会粘成一行。
fn rendered_txt_body(sentences: &[oneasr_core::sentence_boundary::SourceSentence]) -> String {
    oneasr_core::txt::body_from_parts(sentences.iter().map(|s| s.text.as_str()))
}

/// `render` — Phase B on its own: present a measured timeline again, with no
/// model, no ffmpeg and no audio in sight.
///
/// This is the reason the timeline is a file. Changing the segment length, the
/// output script or the output directory used to mean re-running two models;
/// here it is a re-render in milliseconds, and the words it works from cannot
/// drift from the words the original run measured because they *are* those
/// words, on disk.
fn cmd_render(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let timeline_arg = require_arg(args, "--timeline")?;
    let timeline_path = PathBuf::from(&timeline_arg);
    if !timeline_path.is_file() {
        eprintln!("{}", i18n::cli_input_not_found(&timeline_path));
        return Err(1);
    }

    // `--preset` is checked here rather than parsed leniently downstream: a
    // typo that silently rendered the default would be indistinguishable from
    // the option having done nothing.
    let preset = match arg(args, "--preset") {
        None => None,
        Some(raw) => match raw.as_str() {
            "short" | "standard" | "loose" => Some(raw),
            other => {
                eprintln!("{}", i18n::cli_preset_unknown(other));
                return Err(2);
            }
        },
    };

    let want_txt = flag(args, "--txt");
    let want_ass = flag(args, "--ass");
    let want_srt = !flag(args, "--no-srt");
    if !want_srt && !want_txt && !want_ass {
        eprintln!("{}", i18n::cli_no_srt_warning());
    }

    eprintln!("{}", i18n::cli_render_banner());
    let measured = oneasr_core::timeline::read_timeline(&timeline_path).map_err(|e| {
        eprintln!("{}", i18n::cli_timeline_read_failed(&e));
        1
    })?;
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_TIMELINE, &timeline_path));

    let script = match arg(args, "--script") {
        Some(raw) => oneasr_core::TextScript::from_id(&raw),
        None => oneasr_core::TextScript::Original,
    };
    // `--transcript` switches the boundary source: the transcript's own lines
    // become the cues. Re-presenting an aligned transcript therefore needs no
    // audio, no model and no re-alignment.
    let transcript = match arg(args, "--transcript") {
        None => None,
        Some(path) => {
            let path = PathBuf::from(path);
            match oneasr_core::transcript::read_transcript(&path) {
                Ok(t) => Some(t.text),
                Err(e) => {
                    eprintln!("{}", i18n::cli_transcript_read_failed(&e));
                    return Err(1);
                }
            }
        }
    };
    let rendered = oneasr_core::timeline::render(
        &measured,
        &oneasr_core::RenderOptions {
            preset,
            transcript,
            script,
            srt: want_srt,
            ass: want_ass,
            beautify: false,
        },
    )
    .map_err(|e| {
        eprintln!("{}", i18n::cli_render_failed(&e));
        1
    })?;

    // `clip.timeline.json` → `clip.srt`: the stem is the timeline's own, so a
    // re-render lands beside the timeline it came from unless told otherwise.
    let out_dir = arg(args, "--output")
        .map(PathBuf::from)
        .or_else(|| timeline_path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = oneasr_core::paths::media_stem(&timeline_path);
    let stem = stem.strip_suffix(".timeline").unwrap_or(&stem);

    let mut wrote_any = false;
    // TXT 不进字幕渲染：它只由文字按句末标点断行产出（`oneasr_core::txt`），
    // 与 SRT/ASS 是否同时产出无关。句子文本先按同一套空格规则拼接再整体断句
    // ——`join("")` 会把 "Hello world." 与下一句粘成一行。
    let txt_body = want_txt.then(|| rendered_txt_body(&rendered.sentences.translation_sentences));
    for (body, ext) in [
        (&rendered.srt, "srt"),
        (&txt_body, "txt"),
        (&rendered.ass, "ass"),
    ] {
        let Some(body) = body.as_deref() else {
            continue;
        };
        if body.trim().is_empty() {
            eprintln!("{}", i18n::cli_render_empty(ext));
            continue;
        }
        let path = out_dir.join(format!("{stem}.{ext}"));
        oneasr_core::media::write_atomic(&path, body).map_err(|e| {
            eprintln!("{}", i18n::cli_write_failed(&e.to_string()));
            1
        })?;
        eprintln!("{}", i18n::cli_wrote(&path));
        wrote_any = true;
    }
    if !wrote_any {
        return Err(1);
    }
    Ok(())
}

/// 删除临时文件的 RAII 守卫。
///
/// `asr-chunk` 会在切片之后经历转写、写 `--out` 等多个可能提前 `?` 返回的步骤；
/// 只有成功路径才 `remove_file` 会把几 MB 的切片永久留在 `%TEMP%` 里。守卫让
/// 成功、失败、切片本身失败三种情况都清理，且不必在每个 `?` 旁边写一次。
struct TempFileGuard(PathBuf);

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ─── shared ───────────────────────────────────────────────────────

/// Bind model dirs + SRT output folder to the CLI's `--app-root` / `--data-root`.
///
/// SRT path is driven by `settings.output_dir` (not the pipeline `data_root`
/// argument alone), so headless runs must pin it here.
///
/// 目录扫描顺序 = 数据目录优先、安装目录兜底。
///
/// 这是**显式根**之间的选择，不是 `resolve_model_dir` 那条"显式覆盖不回头找安装
/// 目录"的规则：CLI 的 `--app-root` / `--data-root` 都是用户自己指的路，两边都查
/// 一遍最不容易"看不见已下好的模型"（老脚本把权重放在 `--app-root` 下，换到
/// `--data-root` 后权重在新家）；那里防的是程序自己猜出来的 exe 目录。
/// CLI 默认把输出放在 `{data-root}/output`（批处理脚本的稳定约定）。GUI 通过
/// 这两个旗标把「保存到源文件旁」与自定义输出目录带回给核心；最终目录仍由
/// [`Settings::srt_target_dir`] 决定，源目录不可写时回退到 `--output-dir`。
fn apply_output_flags(
    settings: &mut Settings,
    output_dir: Option<PathBuf>,
    save_next_to_source: bool,
) {
    if let Some(dir) = output_dir {
        settings.output_dir = dir;
    }
    if save_next_to_source {
        settings.save_next_to_source = true;
    }
}

fn apply_app_root_paths(
    settings: &mut Settings,
    app_root: &Path,
    data_root: &Path,
    asr_override: Option<AsrPick>,
    aligner_override: Option<ModelId>,
) {
    let roots: Vec<&Path> = if data_root == app_root {
        vec![data_root]
    } else {
        vec![data_root, app_root]
    };
    // 显式指定优先，扫描只是兜底 —— 两者对 ASR 与对齐器是同一套规则。
    //
    // 关键在「扫不到时怎么办」：不能什么都不做就留着 `Settings::default()`
    // 里的默认模型路径，那等于**悄悄退回另一个已装的模型**跑（点名 1.7B 却
    // 跑成 0.6B，最难发现的一种）。所以显式指定时先把「要哪个」钉死，扫不到
    // 就指向那个确实不存在的目录，让模型检查报「你选的型号没装」。
    //
    // 候选怎么排由 `AsrPick` 给：短别名（尺寸级）fp16 与 int8 都试，fp16 优先；
    // 完整目录名（具体级）只认它自己。两个变体都没装才退回钉死并报缺。
    if let Some(pick) = asr_override {
        // 候选在外层：fp16 优先是**全局**规则，不能被 root 顺序截胡。反过来写
        // （root 在外）时，`--data-root` 里只装了 int8 就会盖过 `--app-root` 里的
        // fp16。同一个候选仍按 root 顺序取（data_root 优先）。
        let candidates = pick.candidates();
        let found = candidates.iter().find_map(|id| {
            roots
                .iter()
                .map(|root| root.join("models").join(id.as_str()))
                .find(|dir| dir.is_dir())
                .map(|dir| (dir, *id))
        });
        match found {
            Some((dir, id)) => {
                settings.asr_model = id.as_str().into();
                settings.asr_model_dir = dir;
            }
            None => {
                let id = pick.primary();
                settings.asr_model = id.as_str().into();
                settings.asr_model_dir = roots[0].join("models").join(id.as_str());
            }
        }
    } else {
        // 没指定：按 ASR_CHOICES 顺序（0.6B 先）再带上两个 int8 变体扫描
        // 安装布局，fp16 优先于 int8。都没有就保留默认路径让模型检查报缺目录。
        // 候选同样在外层，理由见上面那支。
        let candidates: Vec<ModelId> = ModelId::ASR_CHOICES
            .into_iter()
            .chain([ModelId::Qwen3Asr06BInt8, ModelId::Qwen3Asr17BInt8])
            .collect();
        let found = candidates.iter().find_map(|id| {
            roots
                .iter()
                .map(|root| root.join("models").join(id.as_str()))
                .find(|dir| dir.is_dir())
                .map(|dir| (dir, *id))
        });
        if let Some((dir, id)) = found {
            settings.asr_model = id.as_str().into();
            settings.asr_model_dir = dir;
        }
    }
    // 对齐模型：与 ASR 同一套优先级 —— `--aligner` 显式指定只认那一个，
    // 扫不到就指向那个不存在的目录报缺，而不是退回默认的 CTC；没指定才按
    // ALIGNER_CHOICES 顺序（CTC 优先）扫描安装布局。
    if let Some(id) = aligner_override {
        settings.aligner_model = id.as_str().into();
        settings.aligner_model_dir = roots
            .iter()
            .map(|root| root.join("models").join(id.as_str()))
            .find(|dir| dir.is_dir())
            .unwrap_or_else(|| roots[0].join("models").join(id.as_str()));
    } else {
        // 与 ASR 那支同一个形状：候选在外层，引擎优先级（CTC 先）才是全局规则，
        // 不被 root 顺序截胡；同一个候选仍按 root 顺序取。
        let found = ModelId::ALIGNER_CHOICES.iter().find_map(|id| {
            roots
                .iter()
                .map(|root| root.join("models").join(id.as_str()))
                .find(|dir| dir.is_dir())
                .map(|dir| (dir, *id))
        });
        if let Some((dir, id)) = found {
            settings.aligner_model = id.as_str().into();
            settings.aligner_model_dir = dir;
        }
    }
    let demucs = roots
        .iter()
        .map(|root| root.join("models").join(HTDEMUCS_FT))
        .find(|dir| dir.is_dir());
    if let Some(dir) = demucs {
        settings.demucs_model_dir = dir;
    }
    // 热词粗转模型（SenseVoiceSmall）：与 demucs 同一个形状——数据目录优先、
    // 安装目录兜底。都没有就保留 `Settings::default()` 解析出的落点（那会再看
    // 一次 exe 安装目录），让 `can_start` 报缺。
    let sensevoice = roots
        .iter()
        .map(|root| root.join("models").join(ModelId::SenseVoiceSmall.as_str()))
        .find(|dir| dir.is_dir());
    if let Some(dir) = sensevoice {
        settings.sensevoice_model_dir = dir;
    }
    settings.output_dir = oneasr_core::paths::output_dir_under(data_root);
    // Headless runs keep writing to {data-root}/output (GUI's "next to source"
    // default would be surprising for batch scripts).
    settings.save_next_to_source = false;
}

/// 读热词库（`{data-root}/hotword.json`）；迁移 / 自愈 / 坏文件的原因进 stderr。
///
/// 只在真的要热词时才调用：普通转写不该顺带触发迁移之类的写盘副作用。
/// 「哪些算值得说的」的判据在 core 侧（`HotwordLoadReport::is_notable`），
/// 这里只负责问——不重新枚举字段。
fn load_hotwords_or_warn() -> HotwordStore {
    let (store, report) = HotwordStore::load();
    if report.is_notable() {
        eprintln!(
            "{}",
            i18n::cli_warn(&i18n::cli_hotword_store_issue(&report.summary()))
        );
    }
    store
}

/// `--hotword-group` / `--hotwords` → 本次运行的热词参数（`settings.hotword_run`）。
///
/// 纯函数：store 与语言都从参数进，单测不读进程级语言。错误直接打屏并返回
/// 退出码 2（用法 / 取值错误），与 `--asr` 那类旗标同一口径：
/// - 两个旗标同时给：二选一；
/// - 组找不到或重名（查找规则见 [`HotwordStore::group_by_id_or_name`]）——
///   报错里把可用组全列出来，照着改就行；
/// - `--hotwords` 解析后没有词条：按用例把它当「空组」放行等于静默不跑热词，
///   比说清楚更糟（与 `can_start` 对空组的立场一致）。
///
/// 组**存在但没词**不在这里拦：留给 `settings.can_start()` 用那条现成的
/// 「热词组「X」还没有词」报错，命令行的说法与界面逐字一致。
fn resolve_cli_hotwords(
    store: &HotwordStore,
    group: Option<&str>,
    words: Option<&str>,
    lang: i18n::UiLang,
) -> Result<Option<HotwordRun>, i32> {
    match (group, words) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => {
            eprintln!("{}", i18n::cli_hotwords_both_flags());
            Err(2)
        }
        (Some(query), None) => match store.group_by_id_or_name(query) {
            GroupLookup::Found(g) => Ok(store.resolve_run(Some(&g.id), lang)),
            GroupLookup::Ambiguous(ids) => {
                eprintln!("{}", i18n::cli_hotword_group_ambiguous(query, &ids));
                Err(2)
            }
            GroupLookup::NotFound => {
                eprintln!(
                    "{}",
                    i18n::cli_hotword_group_unknown(query, &store.menu_options(lang))
                );
                Err(2)
            }
        },
        (None, Some(raw)) => {
            if oneasr_core::hotword::parse_word_list(raw).is_empty() {
                eprintln!("{}", i18n::cli_hotwords_empty());
                return Err(2);
            }
            // 即席词表没有组：调参沿用库里的（`hotword.json` 缺失时就是默认值）。
            Ok(Some(HotwordRun {
                group_id: "cli".into(),
                group_name: i18n::t(i18n::CLI_HOTWORD_INLINE).to_string(),
                text: raw.to_string(),
                top_k: store.top_k,
                threshold: store.threshold,
            }))
        }
    }
}

/// `align` —— 音频 + 文稿 → 字幕，**整个识别阶段跳过**。
///
/// 与 `transcribe` 的差别只有一句：文字从哪来。所以这里没有识别模型的检查、
/// 没有语言模型的加载，阶段列表里也就**没有「转写」这一项**——那就是用户判断
/// 「这次是文稿匹配」的全部依据。
fn cmd_align(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let events_json = events_json_arg(args)?;
    let audio = PathBuf::from(require_arg(args, "--audio")?);
    if !audio.is_file() {
        eprintln!("{}", i18n::cli_input_not_found(&audio));
        return Err(1);
    }
    let text_path = PathBuf::from(require_arg(args, "--text")?);
    let transcript = oneasr_core::transcript::read_transcript(&text_path).map_err(|e| {
        eprintln!("{}", i18n::cli_transcript_read_failed(&e));
        1
    })?;

    // 输出格式先判：**用法错**要在碰 ffmpeg、模型与目录解析之前就报出来。
    //
    // 文稿匹配的产物必须带时间轴（SRT 或 ASS 至少一个）——纯 TXT 的匹配没有
    // 时间轴产物可言。判据在 core（`settings::timed_output_selected`），门禁
    // 与这里问的是同一条；这里先判，是为了把「用法错」和「模型没就绪」分开报。
    //
    // 顺序也是刻意的：这一段只读旗标，不碰任何进程级状态（`pin_data_root`
    // 会把数据目录钉死在进程里），所以它能单独被单测盖住。
    let want_txt = flag(args, "--txt");
    let want_ass = flag(args, "--ass");
    let no_srt = flag(args, "--no-srt");
    if let Err(msg) = check_align_outputs(no_srt, want_ass) {
        eprintln!("{msg}");
        return Err(2);
    }
    let output_dir = arg(args, "--output-dir").map(PathBuf::from);
    let save_next_to_source = flag(args, "--save-next-to-source");

    let app_root = parse_app_root(args)?;
    // 数据目录必须在任何 `Settings::default()` / 路径解析之前钉住。
    let data_root = pin_data_root(args, &app_root)?;

    ensure_ffmpeg(&app_root)?;

    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let chunk_seconds: u32 = arg(args, "--chunk-seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let backend = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    // 热词旗标只服务识别阶段；align 整个跳过识别（文字来自文稿），给了也只会
    // 静默失效——说出来，别让用户对着「一点变化都没有」猜。
    if arg(args, "--hotword-group").is_some() || arg(args, "--hotwords").is_some() {
        eprintln!(
            "{}",
            i18n::cli_warn(i18n::t(i18n::CLI_ALIGN_HOTWORDS_IGNORED))
        );
    }

    let mut settings = Settings {
        language,
        backend,
        vocal_separation: flag(args, "--vocal-separation"),
        save_next_to_source: false,
        ..Settings::default()
    };
    settings.chunk_target_seconds = chunk_seconds;
    settings.output_srt = !no_srt;
    settings.output_txt = want_txt;
    settings.output_ass = want_ass;
    settings.subtitle_beautify = flag(args, "--beautify");
    if let Some(dir) = arg(args, "--demucs-model-dir") {
        settings.demucs_model_dir = PathBuf::from(dir);
    }
    // 文稿匹配固定用 CTC：整段一次对齐，块边界不伤词。见设计文档 §1.1。
    settings.select_aligner_model(oneasr_core::ModelId::OmniAsrCtc300M);
    apply_output_flags(&mut settings, output_dir, save_next_to_source);
    settings.normalize();
    if let Err(e) = settings.can_start_transcript() {
        eprintln!("{e}");
        return Err(1);
    }

    eprintln!("{}", i18n::cli_align_banner());
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_INPUT, &audio));
    eprintln!(
        "{} {}",
        i18n::cli_kv(i18n::CLI_KV_TRANSCRIPT, &text_path),
        i18n::cli_transcript_loaded(
            transcript.line_count(),
            transcript.char_count(),
            transcript.dropped_timecodes()
        )
    );
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_OUTPUT, &settings.srt_target_dir(&audio))
    );

    let media_name = audio
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| audio.display().to_string());
    let mut events = StageEvents::new(events_json, align_stage_plan(&settings));
    let mut clock = StageClock::new();
    let t0 = Instant::now();
    let result = oneasr_core::process_media_file_with_transcript_export(
        &audio,
        &media_name,
        &oneasr_core::asr::TranscriptInput {
            text: transcript.text,
            path: Some(text_path),
        },
        &settings,
        &data_root,
        |update: StageUpdate| {
            clock.note(&update);
            events.note(&update);
            if let Some(w) = &update.warning {
                eprintln!("{}", i18n::cli_warn(w));
            }
            eprintln!("{}", i18n::cli_stage(&update.label(i18n::ui_lang())));
        },
        ProcessExportOptions {
            words_json: None,
            timeline_json: true,
        },
    );
    let timing = clock.finish();
    match result {
        Ok(primary) => {
            eprintln!("{}", i18n::cli_ok_output(&primary));
            eprintln!(
                "{}",
                i18n::cli_timing(
                    t0.elapsed().as_secs_f64(),
                    timing.total_ms,
                    timing.stages.len()
                )
            );
            events.finish(&timing);
            events.done(&primary);
            Ok(())
        }
        Err(e) => {
            eprintln!("{e}");
            events.finish(&timing);
            events.error(&e.to_string());
            Err(1)
        }
    }
}

// ─── transcript ───────────────────────────────────────────────────

/// 文稿是纯数据：解析、统计、断句都在 core 里；这里只负责把结果变成
/// 壳能直接消费的 JSON（`--json`）。
fn cmd_transcript(args: &[String]) -> Result<(), i32> {
    match args.first().map(String::as_str) {
        Some("inspect") => cmd_transcript_inspect(&args[1..]),
        Some("split") => cmd_transcript_split(&args[1..]),
        _ => {
            print_help();
            Err(2)
        }
    }
}

fn transcript_speech_rate(rate: Option<oneasr_core::transcript::SpeechRate>) -> serde_json::Value {
    match rate {
        Some(rate) => json!({
            "per_second": rate.per_second,
            "spaced": rate.spaced,
            "plausible": rate.plausible(),
        }),
        None => serde_json::Value::Null,
    }
}

/// 文稿解析后的 JSON。`text` 一起返回：面板打开时不用再读一遍文件。
fn transcript_json(
    transcript: &oneasr_core::transcript::Transcript,
    audio_seconds: Option<f64>,
) -> serde_json::Value {
    let stats = transcript.stats();
    json!({
        "kind": match transcript.kind {
            oneasr_core::transcript::TranscriptKind::Plain => "plain",
            oneasr_core::transcript::TranscriptKind::SubRip => "subrip",
        },
        "text": &transcript.text,
        "lines": stats.lines,
        "chars": stats.chars,
        "dropped_timecodes": transcript.dropped_timecodes(),
        "speech_rate": transcript_speech_rate(audio_seconds.and_then(|s| stats.rate(s))),
    })
}

/// 断句 / 编辑后的纯文本 JSON。kind 仍取源文稿，好让界面继续说
/// 「原时间轴已忽略」。
fn transcript_text_json(
    text: &str,
    kind: oneasr_core::transcript::TranscriptKind,
    audio_seconds: Option<f64>,
) -> serde_json::Value {
    let stats = oneasr_core::transcript::text_stats(text);
    json!({
        "kind": match kind {
            oneasr_core::transcript::TranscriptKind::Plain => "plain",
            oneasr_core::transcript::TranscriptKind::SubRip => "subrip",
        },
        "text": text,
        "lines": stats.lines,
        "chars": stats.chars,
        "dropped_timecodes": kind == oneasr_core::transcript::TranscriptKind::SubRip,
        "speech_rate": transcript_speech_rate(audio_seconds.and_then(|s| stats.rate(s))),
    })
}

fn cmd_transcript_inspect(args: &[String]) -> Result<(), i32> {
    let input = require_arg(args, "--input")?;
    let audio_seconds = arg(args, "--audio-seconds").and_then(|s| s.parse::<f64>().ok());
    let transcript = oneasr_core::transcript::read_transcript(Path::new(&input)).map_err(|e| {
        eprintln!("{}", i18n::cli_transcript_read_failed(&e));
        1
    })?;
    if flag(args, "--json") {
        println!("{}", transcript_json(&transcript, audio_seconds));
    } else {
        eprintln!(
            "{}",
            i18n::cli_transcript_loaded(
                transcript.line_count(),
                transcript.char_count(),
                transcript.dropped_timecodes()
            )
        );
        println!("{}", transcript.text);
    }
    Ok(())
}

fn cmd_transcript_split(args: &[String]) -> Result<(), i32> {
    let input = require_arg(args, "--input")?;
    let lang = arg(args, "--lang").unwrap_or_else(|| "zh".into());
    let preset = arg(args, "--preset").unwrap_or_else(|| "standard".into());
    if !matches!(preset.as_str(), "short" | "standard" | "loose") {
        eprintln!("{}", i18n::cli_preset_unknown(&preset));
        return Err(2);
    }
    let audio_seconds = arg(args, "--audio-seconds").and_then(|s| s.parse::<f64>().ok());
    let transcript = oneasr_core::transcript::read_transcript(Path::new(&input)).map_err(|e| {
        eprintln!("{}", i18n::cli_transcript_read_failed(&e));
        1
    })?;
    let text = oneasr_core::sentence_boundary::break_long_lines(&transcript.text, &lang, &preset);
    if flag(args, "--json") {
        println!(
            "{}",
            transcript_text_json(&text, transcript.kind, audio_seconds)
        );
    } else {
        println!("{text}");
    }
    Ok(())
}

// ─── download ─────────────────────────────────────────────────────

/// `align` 的输出组合检查：文稿匹配的产物必须带时间轴（SRT 或 ASS 至少一个）。
///
/// 判据只有 core 那一处（[`oneasr_core::settings::timed_output_selected`]），
/// 门禁（`Settings::can_start_transcript`）与这里问的是同一条；这里先判，
/// 是为了把「用法错」和「模型没就绪」分开报——前者退出码 2，不该等模型。
///
/// 抽成纯函数是为了能在单测里断言**是哪一种组合被拒**（见
/// `align_needs_a_timestamped_output`）。
fn check_align_outputs(no_srt: bool, want_ass: bool) -> Result<(), &'static str> {
    if oneasr_core::settings::timed_output_selected(!no_srt, want_ass) {
        Ok(())
    } else {
        Err(i18n::cli_align_needs_timed_output())
    }
}

/// `download --model` 的解析结果：**下载落点**与**已装检查的候选**。
///
/// 两者分开，是因为尺寸级别名（`0.6b`）指的不是一个目录，而是「0.6B 这个尺寸」：
/// 与 `--asr 1.7b` 同一条规则，同尺寸的 int8 变体也算「这个尺寸已经装了」；但真要
/// 下的时候只能下一个，取 fp16（[`AsrPick::primary`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DownloadPick {
    /// 要下的那个目录级模型，也是新权重的落点目录名。
    target: ModelId,
    /// 「已经装好了吗」的检查顺序，按优先级排。
    candidates: Vec<ModelId>,
}

impl DownloadPick {
    /// 目录级写法：候选就是它自己（点名量化版不许换成同尺寸的 fp16）。
    fn one(id: ModelId) -> Self {
        Self {
            target: id,
            candidates: vec![id],
        }
    }

    /// 尺寸级别名：落点取 fp16，候选带上同尺寸的 int8。
    fn from_asr(pick: AsrPick) -> Self {
        Self {
            target: pick.primary(),
            candidates: pick.candidates(),
        }
    }
}

/// `download --model` 旗标值 → [`DownloadPick`]。
///
/// **复用 transcribe 的解析**：[`parse_asr_arg`]（尺寸别名 + 四个 ASR 目录名）与
/// [`parse_aligner_arg`]（`ctc` / `qwen` / `qwen-align` + 两个对齐目录名），这里只补
/// 两个没有转写旗标的可选模型（Demucs / SenseVoice）的短别名。别名到目录名的对应
/// 关系仍然只有 catalog 一处真源——这里不出现任何目录名字面量。
fn parse_download_arg(raw: &str) -> Option<DownloadPick> {
    let s = raw.trim();
    if let Some(pick) = parse_asr_arg(s) {
        return Some(DownloadPick::from_asr(pick));
    }
    if let Some(id) = parse_aligner_arg(s) {
        return Some(DownloadPick::one(id));
    }
    for (id, aliases) in [
        (ModelId::HtdemucsFt, &["demucs", "htdemucs"][..]),
        (ModelId::SenseVoiceSmall, &["sensevoice"][..]),
    ] {
        if s.eq_ignore_ascii_case(id.as_str()) || aliases.iter().any(|a| s.eq_ignore_ascii_case(a))
        {
            return Some(DownloadPick::one(id));
        }
    }
    None
}

/// `download` —— 把某个模型的权重下到数据目录的 `models/` 下。
///
/// 存在的理由是 GUI 之外还要一条可脚本化的下载路径（Electron 壳调它）：
/// - **落点用安装布局名**（`{data-root}/models/{目录名}`）：与设置里的
///   `asr_model_dir` / `aligner_model_dir` 是同一批目录，调用方拿 id 就能算出路径；
/// - **先验后下**，与 Electron 壳的下载入口同一道门禁：已装且校验通过就
///   一步都不下，只报「已存在」；
/// - 进度一行给人（stderr，双语）、一行给上层（stdout，机器可读），格式写在
///   `--help` 的 download 段落里。
fn cmd_download(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let raw = require_arg(args, "--model")?;
    let pick = match parse_download_arg(&raw) {
        Some(pick) => pick,
        None => {
            eprintln!("{}", i18n::cli_download_unknown(&raw));
            return Err(2);
        }
    };
    let app_root = parse_app_root(args)?;
    // 与 transcribe 一样：数据目录先钉住，模型落点从它算。下载不碰 ffmpeg，
    // 所以不像 transcribe 那样查 bin/ffmpeg。
    let data_root = pin_data_root(args, &app_root)?;

    let target = pick.target;
    let target_dir = oneasr_core::paths::models_root().join(target.as_str());

    eprintln!("{}", i18n::cli_download_banner());
    eprintln!("{}", i18n::cli_kv_str(i18n::CLI_KV_MODEL, target.as_str()));
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_APP, &app_root));
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_DATA, &data_root));
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_TARGET, &target_dir));

    // 先验后下：数据目录优先、安装目录兜底，与 `apply_app_root_paths` 的扫描同一
    // 形状。本进程刚起来，就绪缓存是空的，不必像 GUI 那样先失效一次。
    let roots: Vec<&Path> = if data_root == app_root {
        vec![&data_root]
    } else {
        vec![&data_root, &app_root]
    };
    let total = catalog_bytes(target);
    emit_download_event("verifying", target, 0, total, 0, None);
    eprintln!("{}", i18n::cli_download_verifying(target.as_str()));
    if let Some((id, dir)) = installed_model(&pick, &roots) {
        let total = catalog_bytes(id);
        eprintln!("{}", i18n::cli_download_already(id.as_str(), &dir));
        emit_download_event("done", id, total, total, 0, Some(&dir));
        return Ok(());
    }

    // 下载器会把「目录建不出来」包成 Failed，但那条信息里只有系统原文
    // （拒绝访问 (os error 5) 之类），不说是哪个目录。先探一次，报错就能点名。
    if let Err(e) = oneasr_core::probe_writable(&target_dir) {
        eprintln!(
            "{}",
            i18n::cli_download_dir_not_writable(&target_dir, &e.to_string())
        );
        emit_download_event("error", target, 0, total, 0, Some(&target_dir));
        return Err(1);
    }

    let handle = DownloadHandle::new(target, target_dir.clone());
    let outcome = download_model(&handle, |p| {
        eprintln!(
            "{}",
            i18n::cli_download_progress(
                target.as_str(),
                p.downloaded_bytes,
                p.total_bytes,
                p.speed_bytes_per_sec,
            )
        );
        emit_download_event(
            "downloading",
            target,
            p.downloaded_bytes,
            p.total_bytes,
            p.speed_bytes_per_sec,
            None,
        );
    });
    match outcome {
        DownloadOutcome::Completed { model_dir } => {
            // 终态用 catalog 的精确总量：下载器逐文件比对过尺寸与 SHA-256，而进度
            // 回调的最后一次 tick 可能还差几百毫秒。
            let total = catalog_bytes(target);
            eprintln!("{}", i18n::cli_download_done(target.as_str(), &model_dir));
            emit_download_event("done", target, total, total, 0, Some(&model_dir));
            Ok(())
        }
        DownloadOutcome::Failed {
            message,
            downloaded_bytes,
            total_bytes,
        } => {
            eprintln!("{}", i18n::cli_download_failed(target.as_str(), &message));
            emit_download_event(
                "error",
                target,
                downloaded_bytes,
                total_bytes.max(total),
                0,
                Some(&target_dir),
            );
            Err(1)
        }
        // CLI 没有取消入口（命令行上没人能再按一次取消），这条分支只是把枚举走完：
        // 真出现了也按失败报，绝不静默退出 0。
        DownloadOutcome::Cancelled { .. } => {
            eprintln!("{}", i18n::cli_download_cancelled(target.as_str()));
            emit_download_event("error", target, 0, total, 0, Some(&target_dir));
            Err(1)
        }
    }
}

/// catalog 里那个模型全部下载文件的总字节数（进度行的 total，与本机无关）。
fn catalog_bytes(id: ModelId) -> u64 {
    oneasr_core::model::model_definition(id)
        .download_files
        .iter()
        .map(|f| f.expected_size)
        .sum()
}

/// 机器可读的进度行 → **stdout**（人类可读的那些走 stderr）。
///
/// 上层（Electron 壳）只解析 stdout 就够了：双语横幅、报警原因都在 stderr，
/// 不会混进解析流。
fn emit_download_event(
    state: &str,
    model: ModelId,
    done: u64,
    total: u64,
    speed: u64,
    dir: Option<&Path>,
) {
    println!(
        "{}",
        i18n::cli_download_event(state, model.as_str(), done, total, speed, dir)
    );
}

/// 「已装好且校验通过」的目录：命中就返回 `(id, 目录)`。
///
/// 扫描顺序是**候选在外、root 在内**，与 `apply_app_root_paths` 同一条规则：fp16
/// 优先是全局规则，不被 root 顺序截胡——否则 `--data-root` 里只装了 int8 就会盖过
/// `--app-root` 里的 fp16。
fn installed_model(pick: &DownloadPick, roots: &[&Path]) -> Option<(ModelId, PathBuf)> {
    first_installed(&pick.candidates, roots, |id, dir| {
        check_model_dir(id, dir).is_ok()
    })
}

/// 按「候选 → root」的顺序找第一个 `ready` 的目录。
///
/// 判据从参数进，是为了让这条**顺序**能被单测钉住：真判据要跑 `check_*_model_dir`，
/// 那需要真的几 GB 权重（见 `download_gate_scans_candidates_before_roots`）。
fn first_installed(
    candidates: &[ModelId],
    roots: &[&Path],
    ready: impl Fn(ModelId, &Path) -> bool,
) -> Option<(ModelId, PathBuf)> {
    candidates.iter().find_map(|id| {
        roots
            .iter()
            .map(|root| root.join(oneasr_core::paths::MODELS_DIR).join(id.as_str()))
            .find(|dir| ready(*id, dir))
            .map(|dir| (*id, dir))
    })
}

/// 按模型种类走那一条现成的就绪判据（全仓库唯一的 `check_*_model_dir`）。
fn check_model_dir(id: ModelId, dir: &Path) -> Result<(), String> {
    let checked = match id.kind() {
        ModelKind::Asr => oneasr_core::check_asr_model_dir(dir),
        ModelKind::Align => oneasr_core::check_aligner_model_dir(dir),
        ModelKind::Demucs => oneasr_core::check_demucs_model_dir(dir),
        ModelKind::Hotword => oneasr_core::check_sensevoice_model_dir(dir),
    };
    checked.map_err(|e| e.to_string())
}

/// `--aligner` / `download --model` 旗标值 → 目录 id。接受短别名 `ctc` /
/// `qwen` / `qwen-align` 与完整目录名。
fn parse_aligner_arg(raw: &str) -> Option<ModelId> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("ctc") {
        return Some(ModelId::OmniAsrCtc300M);
    }
    // `qwen-align` 是 download 那边的写法（`--aligner` 只写 `qwen` 也能用，
    // 两个别名指同一个引擎，别让用户猜哪个旗标认哪个写法）。
    if s.eq_ignore_ascii_case("qwen") || s.eq_ignore_ascii_case("qwen-align") {
        return Some(ModelId::QwenAlign06B);
    }
    ModelId::try_parse_aligner(s)
}

/// `--asr` 点名的粒度：**尺寸**（`1.7b`）还是**具体权重目录**（`Qwen3-ASR-1.7B-int8`）。
///
/// 两者扫不到时的兜底规则不一样，所以不能都压成一个 `ModelId`：
/// - 尺寸：fp16 与 int8 都算候选（fp16 优先）——同尺寸的量化版不算「换了模型」；
/// - 具体目录名：就认它自己，没装就报缺，不许换成同尺寸的另一个变体。
///
/// 之前两种写法都返回 `ModelId`，尺寸级因此被当成目录级钉死：`--asr 1.7b`
/// 只找 `Qwen3-ASR-1.7B-hf`，机器上只装了 `-int8` 就报「模型目录不存在」，
/// 明明有 1.7B 却用不了（2026-10-14 真机冒烟撞到）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsrPick {
    Size(ModelId),
    Exact(ModelId),
}

impl AsrPick {
    /// 扫不到时要指给用户看的那一个：尺寸级取 fp16 名，报错文案里才有型号。
    fn primary(self) -> ModelId {
        match self {
            Self::Size(id) | Self::Exact(id) => id,
        }
    }

    /// 目录扫描的候选 id，按优先级排。
    fn candidates(self) -> Vec<ModelId> {
        match self {
            Self::Size(id) => vec![id.asr_base(), id.with_quant(true)],
            Self::Exact(id) => vec![id],
        }
    }
}

/// `--asr` 旗标值 → [`AsrPick`]。接受短别名 `0.6b` / `1.7b` 与完整目录名。
///
/// 与 [`parse_aligner_arg`] 的差别：对齐器没有量化变体，`ctc` / `qwen` 指的
/// 就是引擎本身，两种写法一个意思；识别模型是「尺寸 × fp16/int8」两维，
/// 短别名只点了其中一维，所以得把粒度一起带下去（见 [`AsrPick`]）。
fn parse_asr_arg(raw: &str) -> Option<AsrPick> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("0.6b") {
        return Some(AsrPick::Size(ModelId::Qwen3Asr06B));
    }
    if s.eq_ignore_ascii_case("1.7b") {
        return Some(AsrPick::Size(ModelId::Qwen3Asr17B));
    }
    // 量化版的短写法：粒度与完整目录名一样（只认它自己，不许换成 fp16）。
    // `download --model 0.6b-int8` 与 `--asr 0.6b-int8` 共用这两行。
    if s.eq_ignore_ascii_case("0.6b-int8") {
        return Some(AsrPick::Exact(ModelId::Qwen3Asr06BInt8));
    }
    if s.eq_ignore_ascii_case("1.7b-int8") {
        return Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8));
    }
    ModelId::try_parse_asr(s).map(AsrPick::Exact)
}

/// `--data-root`：给了就必须是目录（可以还不存在——第一次运行会在那里建
/// `models/`）；没给返回 `None`。
fn data_root_arg(args: &[String]) -> Result<Option<PathBuf>, i32> {
    let Some(s) = arg(args, "--data-root") else {
        return Ok(None);
    };
    let p = PathBuf::from(s);
    if p.exists() && !p.is_dir() {
        eprintln!("{}", i18n::cli_data_root_not_dir(&p));
        return Err(1);
    }
    Ok(Some(p))
}

/// 钉住数据目录并返回它：`--data-root` 优先，否则沿用 `--app-root`
/// （老语义：给了 `--app-root`，`models/`、`output/`、`runs/` 就都在它下面）。
///
/// 必须在任何 `Settings::default()` / 路径解析**之前**调用——数据目录在进程内
/// 只解析一次（`oneasr_core::paths`）。
fn pin_data_root(args: &[String], app_root: &Path) -> Result<PathBuf, i32> {
    let dir = match data_root_arg(args)? {
        Some(dir) => dir,
        None => app_root.to_path_buf(),
    };
    oneasr_core::paths::set_data_root_override(dir.clone());
    Ok(dir)
}

fn ensure_ffmpeg(app_root: &Path) -> Result<(), i32> {
    let bin = app_root.join("bin");
    if bin.join("ffmpeg.exe").is_file() || bin.join("ffmpeg").is_file() {
        return Ok(());
    }
    // No bundled copy — the pipeline itself falls back to `PATH`, so only fail
    // when that is empty too.
    match ffmpeg_source() {
        Some(source) => {
            eprintln!("{}", i18n::cli_ffmpeg_fallback(&bin, &source.to_string()));
            Ok(())
        }
        None => {
            eprintln!("{}", i18n::cli_ffmpeg_not_found(&bin));
            Err(1)
        }
    }
}

fn default_app_root() -> PathBuf {
    resolve_app_root().unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn parse_app_root(args: &[String]) -> Result<PathBuf, i32> {
    if let Some(s) = arg(args, "--app-root") {
        let p = PathBuf::from(s);
        if !p.is_dir() {
            eprintln!("{}", i18n::cli_app_root_not_dir(&p));
            return Err(1);
        }
        return Ok(p);
    }
    Ok(default_app_root())
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn require_arg(args: &[String], name: &str) -> Result<String, i32> {
    arg(args, name).ok_or_else(|| {
        eprintln!("{}", i18n::cli_missing_required(name));
        2
    })
}

fn is_help(s: &str) -> bool {
    matches!(s, "--help" | "-h" | "help")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oneasr_core::sentence_boundary::SourceSentence;

    /// 造一棵带模型的临时数据根。**四个变体全装上** —— 少建一个，
    /// 「扫描顺序」就测不出来：只装 0.6B-hf 与 1.7B-int8 时把候选顺序反过来，
    /// 第一个命中的仍然是 0.6B-hf，顺序断言会假绿。
    fn root_with_all_asr_variants(tag: &str) -> std::path::PathBuf {
        let root = env::temp_dir().join(format!("oneasr-asr-pick-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::Qwen3Asr06BInt8,
            ModelId::Qwen3Asr17BInt8,
        ] {
            std::fs::create_dir_all(root.join("models").join(id.as_str())).unwrap();
        }
        root
    }

    #[test]
    fn an_explicit_asr_flag_beats_the_installed_scan() {
        let root = root_with_all_asr_variants("explicit");
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8)),
            None,
        );
        // 断骨锚点：把 `asr_override` 那段 match 改成无条件扫描，这条立刻红。
        assert_eq!(s.asr_model, ModelId::Qwen3Asr17BInt8.as_str());
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()),
            "显式指定必须赢过扫描到的 0.6B"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_the_flag_the_installed_scan_still_runs() {
        let root = root_with_all_asr_variants("scan");
        let mut s = Settings::default();
        apply_app_root_paths(&mut s, &root, &root, None, None);
        // 显式指定之外的兜底没被改掉：ASR_CHOICES 里 0.6B 排在 1.7B 前面，
        // 两者都装了，所以必须绑 0.6B。断骨锚点：把候选顺序反过来，这条变红。
        assert_eq!(s.asr_model, ModelId::Qwen3Asr06B.as_str());
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr06B.as_str())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_asr_flag_naming_a_size_that_is_not_installed_is_not_silently_swapped() {
        // 选了 1.7B，但两个变体都没装：宁可不绑定让模型检查报缺目录，
        // 也不能悄悄换个型号跑——那正是「跑出来的不是我要的」最难发现的一种。
        let root = root_with_all_asr_variants("missing");
        for id in [ModelId::Qwen3Asr17B, ModelId::Qwen3Asr17BInt8] {
            std::fs::remove_dir_all(root.join("models").join(id.as_str())).unwrap();
        }
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17B.as_str()),
            "没装就该保留它自己的路径报错，而不是回退去跑已装的 0.6B"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_asr_size_alias_falls_back_to_the_installed_int8_sibling() {
        // 只装了 1.7B 的 int8 变体（量化版换 fp16 不算「换了模型」，同一个尺寸），
        // `--asr 1.7b` 必须绑到它，而不是报「模型目录不存在」。
        // 断骨锚点：把 `candidates()` 的 Size 分支改回只给一个 id，这条立刻红。
        let root = root_with_all_asr_variants("int8-only");
        std::fs::remove_dir_all(root.join("models").join(ModelId::Qwen3Asr17B.as_str())).unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(s.asr_model, ModelId::Qwen3Asr17BInt8.as_str());
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()),
            "尺寸级指定：同尺寸的 int8 装了就该用它，而不是报缺目录"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_asr_size_alias_prefers_fp16_when_both_variants_are_installed() {
        // 两个变体都装时仍是 fp16 优先，与不带旗标的扫描顺序一致。
        // 断骨锚点：把 `candidates()` 的两个候选反过来，这条立刻红。
        let root = root_with_all_asr_variants("fp16-first");
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17B.as_str())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// fp16 优先是**全局**规则，不被 root 顺序截胡：`--data-root` 只装了
    /// 1.7B-int8、`--app-root` 只装了 1.7B-fp16 时，不管点不点名尺寸，都该绑
    /// app_root 的 fp16。
    /// 断骨锚点：把候选扫描的两层换回 root 在外，两条断言一起变红。
    #[test]
    fn fp16_preference_is_global_across_data_and_app_roots() {
        let data = root_with_all_asr_variants("xroot-data");
        let app = root_with_all_asr_variants("xroot-app");
        // data_root 只留 1.7B-int8；app_root 只留 1.7B-fp16。
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::Qwen3Asr06BInt8,
        ] {
            std::fs::remove_dir_all(data.join("models").join(id.as_str())).unwrap();
        }
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17BInt8,
            ModelId::Qwen3Asr06BInt8,
        ] {
            std::fs::remove_dir_all(app.join("models").join(id.as_str())).unwrap();
        }
        let want = app.join("models").join(ModelId::Qwen3Asr17B.as_str());

        let mut explicit = Settings::default();
        apply_app_root_paths(
            &mut explicit,
            &app,
            &data,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(
            explicit.asr_model_dir, want,
            "点名尺寸：app_root 的 fp16 必须赢过 data_root 的 int8"
        );

        let mut scanned = Settings::default();
        apply_app_root_paths(&mut scanned, &app, &data, None, None);
        assert_eq!(
            scanned.asr_model_dir, want,
            "不带旗标：扫描同样要全局 fp16 优先"
        );

        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn an_exact_asr_directory_name_never_falls_back_to_another_variant() {
        // 点名了具体权重（int8），机器上只有 fp16：同尺寸也不换——换了就等于
        // 「跑出来的不是我点名的那份权重」，与默默换尺寸一样是骗人。
        let root = root_with_all_asr_variants("exact-no-fallback");
        std::fs::remove_dir_all(root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()))
            .unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8)),
            None,
        );
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()),
            "具体目录名只认它自己：宁可报缺，也不能悄悄用同尺寸的 fp16"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_aligner_flag_naming_a_missing_engine_is_not_silently_swapped() {
        // 对齐器那边是同一个形状：点名 qwen 却没有它，不能悄悄退回 CTC 跑。
        // 两边一起修，就得两边都有用例守着。
        let root = env::temp_dir().join(format!("oneasr-align-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("models").join(ModelId::OmniAsrCtc300M.as_str()))
            .unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(&mut s, &root, &root, None, Some(ModelId::QwenAlign06B));
        assert_eq!(s.aligner_model, ModelId::QwenAlign06B.as_str());
        assert_eq!(
            s.aligner_model_dir,
            root.join("models").join(ModelId::QwenAlign06B.as_str()),
            "没装就该指向它自己并报缺，而不是退回已装的 CTC"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_asr_arg_keeps_the_granularity_the_user_typed() {
        // 尺寸级：别名落到 Size，候选含 fp16 + int8。
        assert_eq!(
            parse_asr_arg("1.7b"),
            Some(AsrPick::Size(ModelId::Qwen3Asr17B))
        );
        assert_eq!(
            parse_asr_arg("0.6B"),
            Some(AsrPick::Size(ModelId::Qwen3Asr06B))
        );
        assert_eq!(
            parse_asr_arg(" 1.7B "),
            Some(AsrPick::Size(ModelId::Qwen3Asr17B))
        );
        // 目录级：完整名字只认它自己，哪怕它是量化变体。
        assert_eq!(
            parse_asr_arg(ModelId::Qwen3Asr17BInt8.as_str()),
            Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8))
        );
        assert_eq!(
            ModelId::Qwen3Asr17BInt8.as_str(),
            "Qwen3-ASR-1.7B-int8",
            "完整目录名走的是目录级，夹具依赖这个字面名"
        );
        assert_eq!(
            AsrPick::Size(ModelId::Qwen3Asr17B).candidates(),
            vec![ModelId::Qwen3Asr17B, ModelId::Qwen3Asr17BInt8]
        );
        assert_eq!(
            AsrPick::Exact(ModelId::Qwen3Asr17BInt8).candidates(),
            vec![ModelId::Qwen3Asr17BInt8]
        );
        // 量化版的短写法：与目录级同一个粒度，只认它自己。
        assert_eq!(
            parse_asr_arg("0.6b-int8"),
            Some(AsrPick::Exact(ModelId::Qwen3Asr06BInt8))
        );
        assert_eq!(
            parse_asr_arg("1.7B-INT8"),
            Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8))
        );
        assert_eq!(parse_asr_arg("ctc"), None, "对齐器的别名不该被认成识别模型");
        assert_eq!(parse_asr_arg("nope"), None);
    }

    // ── download：`--model` 解析与「先验后下」的扫描顺序 ────────────────

    /// catalog 里 8 个条目的**完整目录名**全都认得，且落到它自己（不换成同尺寸的
    /// 另一份）。清单从 `ModelId::ALL` 枚举，测试里不复制第二份字面量。
    #[test]
    fn download_arg_resolves_every_catalog_directory_name() {
        assert_eq!(ModelId::ALL.len(), 8, "catalog 清单只有一处");
        for id in ModelId::ALL {
            let pick = parse_download_arg(id.as_str())
                .unwrap_or_else(|| panic!("目录名 {} 必须认得", id.as_str()));
            assert_eq!(pick.target, id, "目录名 {} 必须落到它自己", id.as_str());
            assert_eq!(pick.candidates, vec![id], "目录级写法只有一个候选");
        }
    }

    /// 友好写法：尺寸别名带两个候选（fp16 优先），int8 短写法只认 int8，对齐器与
    /// 两个可选模型各有一组别名；大小写与首尾空格都不敏感。
    #[test]
    fn download_arg_resolves_the_friendly_aliases() {
        let size = parse_download_arg(" 0.6B ").expect("0.6b");
        assert_eq!(size.target, ModelId::Qwen3Asr06B);
        assert_eq!(
            size.candidates,
            vec![ModelId::Qwen3Asr06B, ModelId::Qwen3Asr06BInt8],
            "尺寸级别名：同尺寸的 int8 也算这个尺寸已装"
        );

        // 与 `--asr` 共用同一套：1.7b 指尺寸，1.7b-int8 指量化权重。
        assert_eq!(
            parse_download_arg("1.7b").unwrap().candidates,
            vec![ModelId::Qwen3Asr17B, ModelId::Qwen3Asr17BInt8]
        );
        for (raw, want) in [
            ("0.6b-int8", ModelId::Qwen3Asr06BInt8),
            ("1.7B-INT8", ModelId::Qwen3Asr17BInt8),
        ] {
            let quantized = parse_download_arg(raw).unwrap_or_else(|| panic!("{raw} 必须认得"));
            assert_eq!(quantized.target, want, "{raw}");
            assert_eq!(
                quantized.candidates,
                vec![want],
                "点名了量化版就只认它，不许换成 fp16"
            );
        }

        // 对齐器：`--aligner` 的 ctc / qwen 之外，download 还认 qwen-align。
        assert_eq!(
            parse_download_arg("ctc").unwrap().target,
            ModelId::OmniAsrCtc300M
        );
        assert_eq!(
            parse_download_arg("qwen").unwrap().target,
            ModelId::QwenAlign06B
        );
        assert_eq!(
            parse_download_arg("qwen-align").unwrap().target,
            ModelId::QwenAlign06B
        );

        // 没有转写旗标的两个可选模型：短别名与目录名都认。
        for alias in ["demucs", "htdemucs"] {
            assert_eq!(
                parse_download_arg(alias).unwrap().target,
                ModelId::HtdemucsFt
            );
        }
        assert_eq!(
            parse_download_arg("sensevoice").unwrap().target,
            ModelId::SenseVoiceSmall
        );
    }

    /// 认不出来的写法返回 `None`，由调用方报错退出 2——**不许猜一个默认**：
    /// `--asr` 猜错只是跑错型号，download 猜错是下错几个 GB。
    ///
    /// 断骨验证：给 `parse_download_arg` 末尾加一个兜底默认值
    /// （`Some(DownloadPick::one(ModelId::Qwen3Asr06B))`），本用例全红。
    #[test]
    fn download_arg_refuses_unknown_ids() {
        for raw in ["", "  ", "nope", "0.6", "0.6b-int4", "align", "--model"] {
            assert!(parse_download_arg(raw).is_none(), "raw={raw:?}");
        }
        // `--asr` 的别名表里没有对齐器：`ctc` 只有 download / `--aligner` 认。
        assert!(parse_asr_arg("ctc").is_none());
    }

    /// 「先验后下」的扫描顺序：**候选在外、root 在内**（fp16 优先是全局规则，不被
    /// root 顺序截胡），与 `apply_app_root_paths` 同一条。
    ///
    /// 断骨验证：把 `first_installed` 的两层循环对调（root 在外），`seen` 的顺序
    /// 断言立刻变红。
    #[test]
    fn download_gate_scans_candidates_before_roots() {
        use std::cell::RefCell;

        let data = env::temp_dir().join("oneasr-dl-gate-data");
        let app = env::temp_dir().join("oneasr-dl-gate-app");
        let roots = [data.as_path(), app.as_path()];
        let candidates = [ModelId::Qwen3Asr17B, ModelId::Qwen3Asr17BInt8];

        // 就绪的是「int8 @ data」：候选在外时，fp16 的两个根都要先问过。
        let seen = RefCell::new(Vec::new());
        let found = first_installed(&candidates, &roots, |id, dir| {
            seen.borrow_mut().push((id, dir.to_path_buf()));
            id == ModelId::Qwen3Asr17BInt8 && dir.starts_with(&data)
        });
        assert_eq!(
            found,
            Some((
                ModelId::Qwen3Asr17BInt8,
                data.join("models").join(ModelId::Qwen3Asr17BInt8.as_str())
            ))
        );
        assert_eq!(
            seen.into_inner(),
            vec![
                (
                    ModelId::Qwen3Asr17B,
                    data.join("models").join(ModelId::Qwen3Asr17B.as_str())
                ),
                (
                    ModelId::Qwen3Asr17B,
                    app.join("models").join(ModelId::Qwen3Asr17B.as_str())
                ),
                (
                    ModelId::Qwen3Asr17BInt8,
                    data.join("models").join(ModelId::Qwen3Asr17BInt8.as_str())
                ),
            ],
            "fp16 的两个根没问完，就不该轮到 int8"
        );
    }

    // ── 热词：`--hotword-group` / `--hotwords` ─────────────────────────

    fn hotword_group(id: &str, name: &str, words: &str) -> oneasr_core::HotwordGroup {
        oneasr_core::HotwordGroup {
            id: id.into(),
            name: name.into(),
            words: words.into(),
        }
    }

    fn hotword_store(groups: Vec<oneasr_core::HotwordGroup>) -> HotwordStore {
        HotwordStore {
            groups,
            default_group: None,
            default_group_enabled: false,
            top_k: 7,
            threshold: 0.42,
        }
    }

    /// `--hotword-group` 按 id 命中，并把组里的词表与库级调参带进 `HotwordRun`。
    #[test]
    fn cli_hotword_group_resolves_by_id() {
        let store = hotword_store(vec![
            hotword_group("default", "", ""),
            hotword_group("terms", "术语表", "甲词, 乙词"),
        ]);
        let run = resolve_cli_hotwords(&store, Some("terms"), None, i18n::UiLang::Zh)
            .expect("按 id 必须命中")
            .expect("给了组旗标就该有 run");
        assert_eq!(run.group_id, "terms");
        assert_eq!(run.text, "甲词, 乙词");
        assert_eq!(run.top_k, 7, "调参要沿用库里存的");
        assert_eq!(run.threshold, 0.42);
        assert!(run.is_usable());
    }

    /// 组名（含大小写不敏感）也能命中：用户记住的是 chip 上的名字，不是 id。
    ///
    /// 断骨验证：把 `HotwordStore::group_by_id_or_name` 的忽略大小写兜底删掉，
    /// `"glossary"` 一条立刻变红。
    #[test]
    fn cli_hotword_group_resolves_by_name() {
        let store = hotword_store(vec![
            hotword_group("default", "", ""),
            hotword_group("g1", "Glossary", "alpha"),
        ]);
        for query in ["Glossary", "glossary", "GLOSSARY"] {
            let run = resolve_cli_hotwords(&store, Some(query), None, i18n::UiLang::Zh)
                .expect("组名必须命中")
                .expect("run");
            assert_eq!(run.group_id, "g1", "query={query}");
        }
    }

    /// 重名：报错退出（2），不默默取一个——「编辑 A 却跑的是 B」正是要防的。
    ///
    /// 断骨验证：把 `group_by_id_or_name` 的重名分支改成取第一个，本测试变红。
    #[test]
    fn cli_hotword_group_name_collision_is_refused() {
        let store = hotword_store(vec![
            hotword_group("default", "", ""),
            hotword_group("g1", "甲", "a"),
            hotword_group("g2", "甲", "b"),
        ]);
        assert_eq!(
            resolve_cli_hotwords(&store, Some("甲"), None, i18n::UiLang::Zh).unwrap_err(),
            2
        );
    }

    /// 找不到：报错退出（2），不静默跑。
    #[test]
    fn cli_hotword_group_unknown_is_refused() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        assert_eq!(
            resolve_cli_hotwords(&store, Some("nope"), None, i18n::UiLang::Zh).unwrap_err(),
            2
        );
    }

    /// 两个旗标二选一。
    ///
    /// 断骨验证：去掉各自独立分支（让后给的那个静默胜出），本测试变红。
    #[test]
    fn cli_hotword_flags_are_mutually_exclusive() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        assert_eq!(
            resolve_cli_hotwords(&store, Some("default"), Some("甲"), i18n::UiLang::Zh)
                .unwrap_err(),
            2
        );
    }

    /// 没给旗标：不解析、不碰热词库（`Ok(None)`，管线自然不跑粗转）。
    #[test]
    fn cli_without_hotword_flags_resolves_to_none() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        assert_eq!(
            resolve_cli_hotwords(&store, None, None, i18n::UiLang::Zh).unwrap(),
            None
        );
    }

    /// `--hotwords`：即席词表直接进 `HotwordRun`，调参沿用库里的。
    #[test]
    fn cli_inline_hotwords_carry_the_store_knobs() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        let run = resolve_cli_hotwords(&store, None, Some("窦神归来, 柳宗元"), i18n::UiLang::Zh)
            .expect("即席词表必须解析出来")
            .expect("run");
        assert_eq!(run.text, "窦神归来, 柳宗元");
        assert_eq!(run.group_id, "cli");
        assert!(
            !run.group_name.trim().is_empty(),
            "报错与 banner 要有个名字"
        );
        assert_eq!(run.word_count(), 2);
        assert_eq!(run.top_k, 7);
        assert_eq!(run.threshold, 0.42);
    }

    /// `--hotwords` 解析后没有词条：用法错误（2），不当「空组」放行。
    #[test]
    fn cli_inline_hotwords_must_contain_words() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        for raw in ["", "   ", ",,,", "，", " , \n , "] {
            assert_eq!(
                resolve_cli_hotwords(&store, None, Some(raw), i18n::UiLang::Zh).unwrap_err(),
                2,
                "raw={raw:?}"
            );
        }
    }

    /// `apply_app_root_paths` 也认安装布局里的热词模型目录（与 demucs 同一套
    /// 数据目录优先、安装目录兜底）。
    ///
    /// 断骨验证：删掉 `apply_app_root_paths` 里 sensevoice 那一段扫描，本测试变红
    /// （`sensevoice_model_dir` 会停在 `Settings::default()` 解析的落点）。
    #[test]
    fn app_root_paths_also_find_the_sensevoice_model_dir() {
        let root = env::temp_dir().join(format!("oneasr-sensevoice-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("models").join(ModelId::SenseVoiceSmall.as_str());
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(&mut s, &root, &root, None, None);
        assert_eq!(s.sensevoice_model_dir, dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sentence(id: usize, text: &str) -> SourceSentence {
        SourceSentence {
            sentence_id: id,
            start_ms: id as u64 * 1000,
            end_ms: id as u64 * 1000 + 900,
            text: text.to_string(),
            word_start: 0,
            word_end: 1,
            chunk_start: 0,
            chunk_end: 1,
        }
    }

    /// `render` 的三条英文句子各占一行。接线若退回先 `join("")` 再断句，
    /// 三条会粘成一行，这条测试会红——它守的是 cmd_render 的接线，不只是
    /// `body_from_parts` 本身。
    #[test]
    fn rendered_txt_body_keeps_english_sentences_separate() {
        let sentences = [
            sentence(1, "Hello world."),
            sentence(2, "Second sentence."),
            sentence(3, "Third one."),
        ];
        assert_eq!(
            rendered_txt_body(&sentences),
            "Hello world.\nSecond sentence.\nThird one.\n"
        );
    }

    // ── status / probe / 事件流（--events=json） ────────────────────────

    /// 假模型目录：**目录名刻意不叫 catalog 名**。
    ///
    /// 安装布局名走清单精确校验（逐文件尺寸），4 KB 占位文件过不了；自定义目录名才
    /// 走「文件在不在 / 够不够大」这条宽松判据 —— 与 settings.rs 的 can_start 用例
    /// 同一套造法，**不依赖本机真权重**（CI 上没有几 GB 的权重可装）。
    fn fake_model_dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
        let root = env::temp_dir().join(format!("oneasr-cli-status-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let asr = root.join("asr-test");
        let aligner = root.join("aligner-test");
        let demucs = root.join("demucs-test");
        let sensevoice = root.join("sensevoice-test");
        for dir in [&asr, &aligner, &demucs, &sensevoice] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let blob = vec![0u8; 4096];
        for file in ["config.json", "tokenizer.json"] {
            std::fs::write(asr.join(file), &blob).unwrap();
        }
        std::fs::write(asr.join("model.safetensors"), &blob).unwrap();
        for file in ["config.json", "tokenizer.json", "tokenizer_config.json"] {
            std::fs::write(aligner.join(file), &blob).unwrap();
        }
        std::fs::write(aligner.join("model.safetensors"), &blob).unwrap();
        std::fs::write(
            demucs.join(oneasr_core::engine::local::DEMUCS_WEIGHTS_FILE),
            &blob,
        )
        .unwrap();
        for file in oneasr_core::engine::local::SENSEVOICE_REQUIRED_FILES {
            std::fs::write(sensevoice.join(file), &blob).unwrap();
        }
        (root, asr, aligner, demucs, sensevoice)
    }

    /// 四个槽位都指向假目录、界面语言钉成 zh 的 settings（探系统语言不适合进断言）。
    fn settings_with_all_models(
        asr: &Path,
        aligner: &Path,
        demucs: &Path,
        sensevoice: &Path,
    ) -> Settings {
        Settings {
            asr_model_dir: asr.to_path_buf(),
            aligner_model_dir: aligner.to_path_buf(),
            demucs_model_dir: demucs.to_path_buf(),
            sensevoice_model_dir: sensevoice.to_path_buf(),
            ui_language: "zh".into(),
            ..Settings::default()
        }
    }

    fn hotword_run(words: &str) -> HotwordRun {
        HotwordRun {
            group_id: "cli".into(),
            group_name: "测试组".into(),
            text: words.into(),
            top_k: 7,
            threshold: 0.42,
        }
    }

    /// `status --json` 的形状：四条槽位 + 8 条 catalog + can_start。
    ///
    /// 就绪用的是 core 那唯一一条 `check_*_model_dir`：假的自定义目录能过宽松判据
    /// （ready=true），假的安装布局目录过不了精确校验（catalog ready=false）——
    /// 两边都不是「目录非空」那种自造判据。
    ///
    /// 断骨锚点：把 catalog 那行的 `check_model_dir` 换成常量 false、或把
    /// `ModelId::ALL` 换成 ASR_CHOICES，本用例立刻变红。
    #[test]
    fn status_json_lists_the_four_slots_and_all_eight_catalog_entries() {
        let (root, asr, aligner, demucs, sensevoice) = fake_model_dirs("json");
        let settings = settings_with_all_models(&asr, &aligner, &demucs, &sensevoice);
        let models_root = root.join("models");
        // 8 条 catalog 目录都建出来，但**都是空目录**：松判据（目录存在 / 目录非空）
        // 会说 ready=true，只有清单精确校验会说 false —— 下面那条断言靠这个把
        // 「目录非空即就绪」挡在门外。
        for id in ModelId::ALL {
            std::fs::create_dir_all(models_root.join(id.as_str())).unwrap();
        }
        let report = status_report(&settings, &root, &root, &models_root);
        let value = report.to_json();

        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["app_root"], root.display().to_string());
        assert_eq!(value["data_root"], root.display().to_string());
        assert_eq!(value["models_root"], models_root.display().to_string());
        assert_eq!(value["backend"], "auto");
        assert_eq!(value["language"], "zh");
        // 界面语言是**解析后**的具体值（system → zh / en），上层不必再探系统语言。
        assert_eq!(value["ui_language"], "zh");

        for (key, dir, id) in [
            ("asr", &asr, ModelId::Qwen3Asr06B),
            ("aligner", &aligner, ModelId::OmniAsrCtc300M),
            ("demucs", &demucs, ModelId::HtdemucsFt),
            ("sensevoice", &sensevoice, ModelId::SenseVoiceSmall),
        ] {
            assert_eq!(value["models"][key]["id"], id.as_str(), "{key}");
            assert_eq!(
                value["models"][key]["dir"],
                dir.display().to_string(),
                "{key}"
            );
            assert_eq!(value["models"][key]["ready"], true, "{key}");
            assert_eq!(
                value["models"][key]["error"],
                serde_json::Value::Null,
                "{key}: 就绪时 error 必须是 null"
            );
        }
        assert!(report.can_start_ok(), "{:?}", report.can_start_reason);
        assert_eq!(value["can_start"]["ok"], true);
        assert_eq!(value["can_start"]["reason"], serde_json::Value::Null);

        // catalog：8 条全来自 ModelId::ALL，dir = {models_root}/{id}，kind 是契约值。
        let catalog = value["catalog"].as_array().expect("catalog 必须是数组");
        assert_eq!(catalog.len(), 8);
        assert_eq!(ModelId::ALL.len(), 8, "catalog 清单只有一处");
        let kinds: Vec<&str> = catalog
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            kinds,
            [
                "asr",
                "asr",
                "asr",
                "asr",
                "aligner",
                "aligner",
                "demucs",
                "sensevoice"
            ]
        );
        for (entry, id) in catalog.iter().zip(ModelId::ALL) {
            assert_eq!(entry["id"], id.as_str());
            assert_eq!(
                entry["dir"],
                models_root.join(id.as_str()).display().to_string()
            );
            // 假目录过不了安装布局的精确校验：说明 ready 真去查了文件，不是抄的目录名。
            assert_eq!(entry["ready"], false, "{} 不该就绪", id.as_str());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `status --json` 的 `media_exts` 就是 core 那一份表（全小写、不带点、无重复）：
    /// 壳拿它拼文件过滤器，不再自己维护第二份正则。
    ///
    /// 断骨锚点：把 `StatusReport::to_json` 里的 `"media_exts"` 字段删掉，
    /// 本用例立刻变红（`value["media_exts"]` 是 null，不是数组）。
    #[test]
    fn status_json_exposes_core_media_exts() {
        let (root, asr, aligner, demucs, sensevoice) = fake_model_dirs("media-exts");
        let settings = settings_with_all_models(&asr, &aligner, &demucs, &sensevoice);
        let report = status_report(&settings, &root, &root, &root.join("models"));

        let value = report.to_json();
        let exts: Vec<&str> = value["media_exts"]
            .as_array()
            .expect("media_exts 必须是数组")
            .iter()
            .map(|v| v.as_str().expect("扩展名必须是字符串"))
            .collect();
        assert_eq!(exts, oneasr_core::media::supported_exts());
        assert!(exts.contains(&"wav"), "{exts:?}");
        assert!(exts.contains(&"mp4"), "{exts:?}");
        for ext in &exts {
            assert_eq!(*ext, ext.to_ascii_lowercase(), "必须全小写: {ext}");
            assert!(!ext.starts_with('.'), "必须不带点: {ext}");
        }
        let transcript_exts: Vec<&str> = value["transcript_exts"]
            .as_array()
            .expect("transcript_exts 必须是数组")
            .iter()
            .map(|v| v.as_str().expect("扩展名必须是字符串"))
            .collect();
        assert_eq!(
            transcript_exts,
            oneasr_core::transcript::TRANSCRIPT_EXTS,
            "壳的配对 / 过滤器读这张表"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 缺模型：`ready=false` + 非空 `error`，`can_start.ok=false` + 双语原因。
    #[test]
    fn status_reports_missing_models_with_a_reason_and_can_start_false() {
        let root =
            env::temp_dir().join(format!("oneasr-cli-status-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let settings = Settings {
            asr_model_dir: root.join("no-asr"),
            aligner_model_dir: root.join("no-aligner"),
            demucs_model_dir: root.join("no-demucs"),
            sensevoice_model_dir: root.join("no-sensevoice"),
            ui_language: "zh".into(),
            ..Settings::default()
        };
        let report = status_report(&settings, &root, &root, &root.join("models"));
        let value = report.to_json();

        assert_eq!(value["models"]["asr"]["ready"], false);
        let reason = value["models"]["asr"]["error"]
            .as_str()
            .expect("未就绪必须给原因");
        assert!(!reason.trim().is_empty());
        assert_eq!(value["can_start"]["ok"], false);
        assert!(
            value["can_start"]["reason"]
                .as_str()
                .is_some_and(|r| !r.trim().is_empty()),
            "can_start 的 reason 就是它的错误串"
        );
        assert!(!report.can_start_ok());
    }

    /// 逐行开关：给到哪几个就只叠哪几个，再问 `Settings::can_start()` —— 全局
    /// 关着人声分离、这一行开着，Demucs 缺失就必须拦；只导 TXT 则不必有对齐模型。
    ///
    /// 断骨锚点：把 `apply_status_overrides` 里三个输出格式旗标改成「只导点名的
    /// 那些」（`--txt` 顺手关掉 SRT），第三条断言立刻变红。
    #[test]
    fn status_overrides_answer_for_this_row() {
        let (root, asr, aligner, demucs, sensevoice) = fake_model_dirs("overrides");
        let _ = (&aligner, &demucs, &sensevoice);
        // 只留 ASR 在盘上，对齐器与人声分离都缺。
        let base = Settings {
            asr_model_dir: asr.clone(),
            aligner_model_dir: root.join("no-aligner"),
            demucs_model_dir: root.join("no-demucs"),
            sensevoice_model_dir: root.join("no-sensevoice"),
            ui_language: "zh".into(),
            ..Settings::default()
        };
        assert!(base.can_start().is_err(), "默认（SRT 开着）要有对齐模型");

        // 只导 TXT：--txt --no-srt → 对齐器不必存在。
        let mut txt_only = base.clone();
        apply_status_overrides(
            &mut txt_only,
            &["--txt".to_string(), "--no-srt".to_string()],
        )
        .unwrap();
        assert!(txt_only.can_start().is_ok(), "{:?}", txt_only.can_start());

        // 与 transcribe 逐字同义：只给 --txt（settings 里 SRT 还开着）就还得要对齐模型。
        let mut txt_with_srt = base.clone();
        apply_status_overrides(&mut txt_with_srt, &["--txt".to_string()]).unwrap();
        assert!(txt_with_srt.output_srt, "--txt 不许顺手关掉 SRT");
        assert!(
            txt_with_srt.can_start().is_err(),
            "SRT 还开着就必须有对齐模型，否则门禁会放行一个跑不起来的任务"
        );

        // 人声分离：全局关着，这一行开着 → Demucs 缺失就得拦（逐行门禁问的正是这个）。
        let mut separating = base.clone();
        apply_status_overrides(&mut separating, &["--vocal-separation".to_string()]).unwrap();
        assert!(separating.vocal_separation);
        assert!(separating.can_start().is_err());

        // 语言门禁：非中英的任务不接热词，空组 / 缺粗转模型都不该拦。
        // 用 TXT-only 的副本，免得「对齐模型缺失」先一步把门禁拦下、盖住热词这条。
        let txt_base = Settings {
            output_srt: false,
            output_txt: true,
            ..base.clone()
        };
        let mut ja = txt_base.clone();
        ja.hotword_run = Some(hotword_run("甲词"));
        apply_status_overrides(&mut ja, &["--language".to_string(), "ja".to_string()]).unwrap();
        assert!(ja.can_start().is_ok(), "{:?}", ja.can_start());
        let mut zh = txt_base.clone();
        zh.hotword_run = Some(hotword_run("甲词"));
        apply_status_overrides(&mut zh, &["--language".to_string(), "zh".to_string()]).unwrap();
        assert!(zh.can_start().is_err(), "中文 + 热词组但缺粗转模型必须拦");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// align 的产物必须带时间轴：SRT 或 ASS 至少一个。
    ///
    /// `--no-srt --txt`（只要纯文本）没有时间轴产物可言 —— 文稿匹配跑不出
    /// 字幕，与其在管线里失败，不如在用法这一层就拒掉。
    ///
    /// 断骨锚点：把 `check_align_outputs` 里的 `timed_output_selected` 换成
    /// `true`，下面两条 `is_err` 断言立刻变红（该拒的组合被放行）。
    #[test]
    fn align_needs_a_timestamped_output() {
        // 默认：写 SRT —— 合法。
        assert!(check_align_outputs(false, false).is_ok());
        // 只要 ASS（`--no-srt --ass`）：ASS 也是带时间轴的 —— 合法。
        assert!(check_align_outputs(true, true).is_ok());
        // 只要 TXT（`--no-srt --txt`）：没有时间轴产物 —— 拒。
        assert!(check_align_outputs(true, false).is_err());
        // 什么也不要（`--no-srt`）：同样没有时间轴产物 —— 拒。
        assert!(check_align_outputs(true, false).is_err());
        // 拒绝的理由要说清怎么改：两种语言的文案都点出 SRT / ASS。
        let err = check_align_outputs(true, false).unwrap_err();
        assert!(err.contains("SRT") && err.contains("ASS"), "{err}");
    }

    /// `align --no-srt --txt` 是一条**用法错**，不是「模型没就绪」。
    ///
    /// 这条命令以前能跑（门禁问的是 `can_start()`），改用文稿门禁后必须
    /// 明确地拒掉，而不是报一个「模型未就绪」让人去装根本用不上的权重。
    ///
    /// 断骨锚点：把 `cmd_align` 里 `check_align_outputs` 那段删掉，这条命令
    /// 会继续往下走（钉数据目录 → 找 ffmpeg），退出码变成 1，断言立刻变红。
    #[test]
    fn align_rejects_a_txt_only_output_before_anything_else() {
        let dir = env::temp_dir().join("oneasr-cli-align-output-gate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("clip.mp4");
        let text = dir.join("clip.txt");
        std::fs::write(&audio, b"not really a video").unwrap();
        std::fs::write(&text, "第一行。\n第二行。\n").unwrap();

        let args = |extra: &[&str]| -> Vec<String> {
            let mut v = vec![
                "--audio".to_string(),
                audio.to_string_lossy().to_string(),
                "--text".to_string(),
                text.to_string_lossy().to_string(),
            ];
            v.extend(extra.iter().map(|s| s.to_string()));
            v
        };

        // 只要 TXT：没有时间轴产物 —— 用法错，退出码 2。
        assert_eq!(cmd_align(&args(&["--no-srt", "--txt"])).unwrap_err(), 2);
        // 什么也不要：同样没有时间轴产物 —— 用法错，退出码 2。
        assert_eq!(cmd_align(&args(&["--no-srt"])).unwrap_err(), 2);
        // （`--no-srt --ass` 是合法的：它由上面那条纯函数用例盖住。这里不跑
        //   它——放行之后命令会继续往下走，钉住进程级的数据目录，污染别的用例。）

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GUI 的输出模式：默认 headless 落在 `{data-root}/output`；壳传
    /// `--save-next-to-source` / `--output-dir` 后必须把这两件事交还给核心。
    ///
    /// 断骨锚点：删掉 `if save_next_to_source` 分支，第一条断言变红；删掉
    /// `--output-dir` 赋值，第二条断言变红。
    #[test]
    fn output_flags_restore_the_gui_output_mode() {
        let dir = env::temp_dir().join("oneasr-cli-output-mode");
        let mut next_to_source = Settings {
            output_dir: env::temp_dir().join("headless-output"),
            save_next_to_source: false,
            ..Settings::default()
        };
        apply_output_flags(&mut next_to_source, Some(dir.clone()), true);
        assert_eq!(next_to_source.output_dir, dir);
        assert!(next_to_source.save_next_to_source);

        let mut fixed_dir = Settings {
            save_next_to_source: false,
            ..Settings::default()
        };
        apply_output_flags(&mut fixed_dir, Some(dir.clone()), false);
        assert_eq!(fixed_dir.output_dir, dir);
        assert!(!fixed_dir.save_next_to_source);
    }

    #[test]
    fn transcript_json_reports_stats_and_rate() {
        let t = oneasr_core::transcript::parse_transcript(
            oneasr_core::transcript::TranscriptKind::Plain,
            "第一行。\n第二行。",
        )
        .unwrap();
        let v = transcript_json(&t, Some(4.0));
        assert_eq!(v["lines"], 2);
        assert_eq!(v["chars"], t.char_count());
        assert_eq!(v["dropped_timecodes"], false);
        assert_eq!(v["speech_rate"]["spaced"], false, "中文按字算");
        assert!(
            v["speech_rate"]["per_second"].as_f64().is_some(),
            "给了时长就必须有语速"
        );
    }

    #[test]
    fn transcript_split_json_keeps_the_source_kind() {
        let t = oneasr_core::transcript::parse_transcript(
            oneasr_core::transcript::TranscriptKind::SubRip,
            "1\n00:00:01,000 --> 00:00:03,000\n第一行。\n第二行。\n",
        )
        .unwrap();
        let text = oneasr_core::sentence_boundary::break_long_lines(&t.text, "zh", "short");
        let v = transcript_text_json(&text, t.kind, None);
        assert_eq!(v["kind"], "subrip");
        assert_eq!(v["dropped_timecodes"], true);
        assert_eq!(v["text"], text);
    }

    /// 壳问文稿门禁时会点名 CTC：设置里选着 Qwen 也不能让门禁去看 Qwen。
    #[test]
    fn status_aligner_override_selects_ctc() {
        let mut s = Settings::default();
        apply_status_overrides(&mut s, &["--aligner".into(), "ctc".into()]).unwrap();
        assert_eq!(
            s.aligner_model,
            oneasr_core::ModelId::OmniAsrCtc300M.as_str()
        );
    }

    /// 目录解析：与 transcribe 同一条规则 —— 点名的型号赢过扫描，root 布局照常生效。
    ///
    /// 断骨锚点：把 `resolve_status_paths` 传的 `AsrPick::Exact` 改回 `None`，
    /// 第二条断言立刻变红（盘上的 0.6B 会把选中的 1.7B 压回去 —— 门禁看错模型的假绿）。
    #[test]
    fn resolve_status_paths_keeps_the_selected_models() {
        let root = env::temp_dir().join(format!("oneasr-cli-status-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::Qwen3Asr17BInt8,
            ModelId::OmniAsrCtc300M,
        ] {
            std::fs::create_dir_all(root.join("models").join(id.as_str())).unwrap();
        }
        let selected = Settings {
            asr_model: ModelId::Qwen3Asr17B.as_str().into(),
            aligner_model: ModelId::OmniAsrCtc300M.as_str().into(),
            ..Settings::default()
        };
        let resolved = resolve_status_paths(selected, &root, &root);
        assert_eq!(resolved.asr_model, ModelId::Qwen3Asr17B.as_str());
        assert_eq!(
            resolved.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17B.as_str()),
            "点了 1.7B 就必须是 1.7B，哪怕盘上还躺着 0.6B"
        );
        assert_eq!(resolved.aligner_model, ModelId::OmniAsrCtc300M.as_str());

        // 对照组：同样的盘、不带点名去扫描，就会压回 0.6B。
        let mut scanned = Settings {
            asr_model: ModelId::Qwen3Asr17B.as_str().into(),
            ..Settings::default()
        };
        apply_app_root_paths(&mut scanned, &root, &root, None, None);
        assert_eq!(scanned.asr_model, ModelId::Qwen3Asr06B.as_str());

        // 每个尺寸手选的目录（`asr_dirs`）是 GUI 的功能：命令行 transcribe 从
        // `Settings::default()` 起手，一次都读不到它。status 报的是命令行会用的
        // 目录，所以手选记录必须被丢掉。
        // 断骨锚点：把 `resolve_status_paths` 里那两行 clear 删掉，本段变红。
        let mut with_pick = Settings::default();
        with_pick.asr_dirs.insert(
            ModelId::Qwen3Asr06B.as_str().to_string(),
            root.join("picked-elsewhere"),
        );
        let resolved = resolve_status_paths(with_pick, &root, &root);
        assert_eq!(
            resolved.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr06B.as_str()),
            "手选目录不参与命令行的路径解析"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 阶段编号：条件阶段不进分母，pct = index / total 四舍五入。
    ///
    /// 断骨锚点：把 `transcribe_stage_plan` 里 `Separating` / `RoughTranscribing` /
    /// 对齐两段的 if 去掉（无条件全列），前两条断言立刻变红。
    #[test]
    fn stage_plan_counts_only_the_stages_this_run_will_have() {
        let srt = Settings {
            output_srt: true,
            ..Settings::default()
        };
        let plain = transcribe_stage_plan(&srt, &ProcessExportOptions::default());
        assert_eq!(
            plain,
            vec![
                AsrStage::Converting,
                AsrStage::Planning,
                AsrStage::LoadingAsr,
                AsrStage::Transcribing,
                AsrStage::LoadingAligner,
                AsrStage::Aligning,
                AsrStage::Exporting,
            ],
            "没人声分离、没热词组：两个条件阶段都不进计划"
        );
        let progress = stage_progress(&plain, AsrStage::Transcribing).expect("在计划里");
        assert_eq!((progress.index, progress.total, progress.pct), (4, 7, 57));

        // 人声分离 + 热词组都在：9 个阶段，转写第 6 —— 与契约样例的 6/9 一致。
        let full = Settings {
            vocal_separation: true,
            language: "zh".into(),
            hotword_run: Some(hotword_run("你好")),
            ..Settings::default()
        };
        let full_plan = transcribe_stage_plan(&full, &ProcessExportOptions::default());
        assert_eq!(full_plan.len(), 9, "{full_plan:?}");
        assert_eq!(full_plan.first(), Some(&AsrStage::Separating), "分离先发");
        assert_eq!(full_plan.last(), Some(&AsrStage::Exporting));
        let progress = stage_progress(&full_plan, AsrStage::Transcribing).expect("在计划里");
        assert_eq!((progress.index, progress.total), (6, 9));
        assert_eq!(progress.pct, 67, "6/9 四舍五入是 67");

        // 计划外阶段没有编号可给（调用方兜底），最后一个阶段是 100%。
        assert_eq!(stage_progress(&plain, AsrStage::RoughTranscribing), None);
        let three = [
            AsrStage::Converting,
            AsrStage::Exporting,
            AsrStage::Planning,
        ];
        assert_eq!(
            stage_progress(&three, AsrStage::Exporting).map(|p| (p.index, p.pct)),
            Some((2, 67))
        );
        assert_eq!(
            stage_progress(&three, AsrStage::Planning).map(|p| (p.index, p.pct)),
            Some((3, 100))
        );

        // TXT-only：对齐阶段整段不进计划（也不需要对齐模型）。
        let txt_only = Settings {
            output_srt: false,
            output_txt: true,
            ..Settings::default()
        };
        let export = ProcessExportOptions {
            words_json: None,
            timeline_json: false,
        };
        let plan = transcribe_stage_plan(&txt_only, &export);
        assert!(!plan.contains(&AsrStage::LoadingAligner));
        assert!(!plan.contains(&AsrStage::Aligning));

        // align：没有识别、没有分段规划，也没有热词。
        assert_eq!(
            align_stage_plan(&Settings::default()),
            vec![
                AsrStage::Converting,
                AsrStage::LoadingAligner,
                AsrStage::Aligning,
                AsrStage::Exporting,
            ]
        );
        let separating = Settings {
            vocal_separation: true,
            ..Settings::default()
        };
        assert_eq!(
            align_stage_plan(&separating).first(),
            Some(&AsrStage::Separating)
        );
    }

    /// 事件 JSON 的形状：字段名是契约（壳按名取值），一个都不能改名。
    ///
    /// 断骨锚点：把 `stage_event` 里任一键改名（例如 `pct` → `percent`），
    /// 本用例立刻变红。
    #[test]
    fn event_json_shapes_match_the_contract() {
        let stage = stage_event(
            AsrStage::Transcribing,
            StageProgress {
                index: 6,
                total: 9,
                pct: 67,
            },
            None,
        );
        assert_eq!(stage["event"], "stage");
        assert_eq!(stage["key"], "transcribing");
        assert!(stage["label"].as_str().is_some_and(|l| !l.is_empty()));
        assert_eq!(stage["index"], 6);
        assert_eq!(stage["total"], 9);
        assert_eq!(stage["pct"], 67);
        // 没有分段时不带 chunk 字段，免得壳把「没有」和「0 段」混在一起。
        assert!(stage.get("chunk").is_none());
        assert!(stage.get("chunkTotal").is_none());

        let chunked = stage_event(
            AsrStage::Transcribing,
            StageProgress {
                index: 6,
                total: 9,
                pct: 67,
            },
            Some((2, 5)),
        );
        assert_eq!(chunked["chunk"], 2);
        assert_eq!(chunked["chunkTotal"], 5);
        // 分段数字不占用阶段计划的 index/total。
        assert_eq!(chunked["index"], 6);
        assert_eq!(chunked["total"], 9);
        // 只有一段时不报 1/1。
        let single = stage_event(
            AsrStage::Aligning,
            StageProgress {
                index: 1,
                total: 1,
                pct: 100,
            },
            Some((1, 1)),
        );
        assert!(single.get("chunk").is_none());
        assert!(single.get("chunkTotal").is_none());

        let timing = timing_events(&TaskTiming {
            stages: vec![oneasr_core::StageTiming {
                stage: AsrStage::Transcribing,
                elapsed_ms: 587,
            }],
            total_ms: 12_828,
        });
        assert_eq!(timing.len(), 2, "每个阶段一条 + 一条 total");
        assert_eq!(timing[0]["event"], "timing");
        assert_eq!(timing[0]["ms"], 587);
        assert!(timing[0]["label"].as_str().is_some_and(|l| !l.is_empty()));
        // 耗时明细卡按 key 出界面语言的文案（label 跟随 CLI 进程语言）。
        assert_eq!(timing[0]["key"], AsrStage::Transcribing.key());
        assert_eq!(timing[0]["key"], "transcribing");
        assert_eq!(timing[1]["event"], "total");
        assert_eq!(timing[1]["ms"], 12_828);

        let output = Path::new("output").join("x.srt");
        let done = done_event(&output);
        assert_eq!(done["event"], "done");
        assert_eq!(done["output"], output.display().to_string());

        let error = error_event("模型未就绪：…");
        assert_eq!(error["event"], "error");
        assert_eq!(error["message"], "模型未就绪：…");
    }

    /// 同一阶段的分段进度要逐条发出：转写 1/5 然后 2/5 是两条事件。
    /// 只有一段、或同阶段没有新分段（警告）时不重发。
    ///
    /// 断骨锚点：把 `stage_note` 改回「同一阶段直接 return」，2/5 那条
    /// `expect` 立刻变红（事件是 None）。把 `chunk_mark` 的 `total > 1` 去掉，
    /// 「只有一段不带 chunk」那两条 `is_none` 也变红。
    #[test]
    fn stage_events_repeat_when_the_chunk_changes() {
        let mut plan = vec![
            AsrStage::Converting,
            AsrStage::Transcribing,
            AsrStage::Aligning,
        ];
        let mut last = None;

        let (first, next) = stage_note(
            &mut plan,
            last,
            &StageUpdate::with_chunk(AsrStage::Transcribing, 1, 5),
        );
        let first = first.expect("第一段要发");
        assert_eq!(first["key"], "transcribing");
        assert_eq!(first["chunk"], 1);
        assert_eq!(first["chunkTotal"], 5);
        // 2/5 是分段，不是阶段计划：转写在这条计划里仍是第 2 个阶段、共 3 个。
        assert_eq!(first["index"], 2);
        assert_eq!(first["total"], 3);
        last = next;

        let (same, next) = stage_note(
            &mut plan,
            last,
            &StageUpdate::with_chunk(AsrStage::Transcribing, 1, 5),
        );
        assert!(same.is_none(), "同一段同一进度不重复发");
        last = next;

        let (second, next) = stage_note(
            &mut plan,
            last,
            &StageUpdate::with_chunk(AsrStage::Transcribing, 2, 5),
        );
        let second = second.expect("2/5 要再发一条");
        assert_eq!(second["chunk"], 2);
        assert_eq!(second["chunkTotal"], 5);
        assert_eq!(second["key"], "transcribing");
        assert_eq!(second["index"], 2);
        last = next;

        // 打轴只有一段：换阶段要发，但不带 1/1，再来一次也不重复。
        let (one, next) = stage_note(
            &mut plan,
            last,
            &StageUpdate::with_chunk(AsrStage::Aligning, 1, 1),
        );
        let one = one.expect("换阶段要发");
        assert_eq!(one["key"], "aligning");
        assert!(one.get("chunk").is_none());
        assert!(one.get("chunkTotal").is_none());
        let (repeat, _) = stage_note(
            &mut plan,
            next,
            &StageUpdate::with_chunk(AsrStage::Aligning, 1, 1),
        );
        assert!(repeat.is_none(), "只有一段时不发 1/1，也不重复");

        // 人声分离已经报到 3/10，接着一条没分段的警告不能把进度清掉。
        let mut sep_plan = vec![AsrStage::Separating];
        let (progress, cursor) = stage_note(
            &mut sep_plan,
            None,
            &StageUpdate::with_chunk(AsrStage::Separating, 3, 10),
        );
        assert_eq!(progress.expect("分离进度要发")["chunk"], 3);
        let (warn, kept) = stage_note(
            &mut sep_plan,
            cursor,
            &StageUpdate::new(AsrStage::Separating).with_warning("已改用 CPU"),
        );
        assert!(warn.is_none(), "同阶段没有新分段时不重发");
        assert_eq!(kept, cursor, "游标留在 3/10，下一次分段还能接着比");
    }

    /// `--events` 只认 `json`；末尾孤零零一个旗标也不许当没给。
    #[test]
    fn events_flag_accepts_only_json() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!events_json_arg(&args(&["--input", "x.mp4"])).unwrap());
        assert!(events_json_arg(&args(&["--events=json"])).unwrap());
        assert!(events_json_arg(&args(&["--events", "JSON"])).unwrap());
        assert_eq!(events_json_arg(&args(&["--events=xml"])).unwrap_err(), 2);
        assert_eq!(events_json_arg(&args(&["--events"])).unwrap_err(), 2);
    }

    /// `probe`：普通文件给 size/kind，不存在走错误分支；判不出的扩展名给 `other`。
    ///
    /// 断骨锚点：把 `probe_info` 里 `fs::metadata` 的 `?` 改成 `unwrap_or_default()`
    /// （读不到就给 0），第一条断言立刻变红；把 `media_kind` 的 `Other` 分支
    /// 改回 `Audio`，`.txt` 与没扩展名那两条也是红的。
    #[test]
    fn probe_reports_size_and_kind_and_errors_on_a_missing_file() {
        let root = env::temp_dir().join(format!("oneasr-cli-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        assert!(
            probe_info(&root.join("nope.mp4")).is_err(),
            "不存在的文件必须走错误分支"
        );

        let audio = root.join("track.mp3");
        std::fs::write(&audio, vec![7u8; 1234]).unwrap();
        let info = probe_info(&audio).expect("普通文件要给 size/kind");
        assert_eq!(info.size_bytes, 1234);
        assert_eq!(info.kind, oneasr_core::MediaKind::Audio);
        assert_eq!(info.to_json()["input"], audio.display().to_string());
        assert_eq!(info.to_json()["size_bytes"], 1234);
        assert_eq!(info.to_json()["kind"], "audio");

        // 视频容器大小写不敏感；两张表都不在的扩展名是 other，不再是「音频」。
        let video = root.join("CLIP.MP4");
        std::fs::write(&video, vec![0u8; 8]).unwrap();
        assert_eq!(
            probe_info(&video).unwrap().kind,
            oneasr_core::MediaKind::Video
        );
        let txt = root.join("notes.txt");
        std::fs::write(&txt, b"not media").unwrap();
        let info = probe_info(&txt).expect("普通文件要给 size/kind");
        assert_eq!(info.kind, oneasr_core::MediaKind::Other);
        assert_eq!(info.to_json()["kind"], "other");
        let no_ext = root.join("plain");
        std::fs::write(&no_ext, b"x").unwrap();
        assert_eq!(
            probe_info(&no_ext).unwrap().kind,
            oneasr_core::MediaKind::Other
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
