# OneAsr 全项目结构调整方案

> 基线：2026-09-16，`D:\OneAsr`。统计口径排除 `target/` 与 `vendor/`。
> 本文取代早先只覆盖 `main.rs` 的草案，范围扩到**两个 crate 的全部 59 个源文件 + 仓库目录布局**。
> 所有结论都有可复现的统计数据支撑，脚本见文末附录。
>
> **实施状态：阶段 0–3 已完成并验证（见 §11）。阶段 4（core 中大型文件拆分）尚余 3 项。**

---

## 零、结论摘要

按「同一关注点的代码应放在一起、且放在正确的 crate / 目录」这个标准，全项目排查出 **4 类问题、共 17 处**：

| 类别 | 数量 | 严重度 | 说明 |
|---|---|---|---|
| **A. 跨 crate 层级错位** | 2 | 高 | `job.rs`、`ui_labels.rs` 住在 core，但 core 内部**零消费者**，只服务 GUI |
| **B. crate 内目录错位** | 2 | 中 | `subtitle_length.rs`（→ `sentence_boundary/`）；`text_script.rs` 经核实**应留在根部**，理由见 §2 |
| **C. 过度拆分** | 3 | 中 | `timing.rs`(14 行)、`words.rs`(25 行)、`text.rs`(仅 1 个生产函数) 应合并 |
| **D. 职责混合的中大型文件** | 5 | 中高 | `main.rs`(4925)、`asr.rs`(1449)、`download.rs`(874)、`local.rs`(613)、`stats.rs`(637) |
| **E. 命名与文档不一致** | 5 | 低 | `lang.rs` 与 `sentence_boundary/language.rs` 重名；13 个文件缺 `//!` 文档；1 个文件用中文写文档而其余用英文 |

**先说一件好事**：`oneasr-core` 的**依赖方向是干净的、无环的**。`sentence_boundary` → `subtitle` 单向（7 处 import），`subtitle` 不反向依赖任何模块。`sentence_boundary/mod.rs` 是编排器、子模块是流水线各阶段，这是全项目结构最好的模块。**本次调整的核心是"归位"，不是"重构"。**

另外：仓库根目录的 `settings.json` / `stats.jsonl` / `oneasr-error.log` / `output/` / `dist/` / `release/` 看着凌乱，但 `.gitignore` 已**完整覆盖**，不构成仓库污染。唯一真正的错位是 `output/sfx-lab/`（把开发实验材料放进了产品输出目录），见 §7。

---

## 一、A 类：跨 crate 层级错位（优先做，收益最高）

判据是**core 内部是否有人消费**。全项目 `Task`/`TaskStatus`/`accept_input_path`/`next_queue_seq` 的引用只出现在 `job.rs` 自身与 `lib.rs` 的再导出里；`ui_labels.rs` 同理。也就是说这两个模块是**纯粹为一个下游消费者存在的库模块**——这是 crate 边界画错的典型症状。

### A1. `crates/oneasr-core/src/job.rs`（231 行）→ `crates/oneasr/src/app/task.rs`

模块文档自己写着：**"Batch task model for the list UI."** 内容是 `Task` / `TaskStatus`（含 `locks_row_actions`）/ `DurationState` / `accept_input_path` / `next_queue_seq`。GUI 消费其中 5 个符号，CLI 一个都不用。

留在 core 的代价：core 作为"无界面 ASR 流水线"库里，却出现"列表行""队列序号""文件对话框过滤器"这类表达层概念。移走后 core 彻底干净。

### A2. `crates/oneasr-core/src/ui_labels.rs`（86 行）→ `crates/oneasr/src/ui/text.rs`

文档头："**Pure UI label helpers** (no GPUI, unit-testable)"。4 个函数全是界面措辞：`empty_state_title`、`empty_state_subtitle`、`format_batch_progress`、`format_queue_status`。

它的存在理由（"没有 GPUI 所以可单测"）是**正当的**——错的是它住在 core 而不是 GUI 的 `ui/` 层。移过去后，正好和 GUI 自身散落在 `main.rs` / `widgets.rs` 里的中文文案（合计 160 处）汇合，成为统一的文案归属地。

> **同步动作**：从 `lib.rs` 删除这两个 `pub mod` 与对应 `pub use`。GUI 侧的 `use oneasr_core::{Task, TaskStatus, ...}` 改为 `use crate::app::task::{...}`。core 与 CLI 无需其他改动。

---

## 二、B 类：crate 内目录错位

### B1. `subtitle_length.rs`（31 行，crate 根）→ `sentence_boundary/preset.rs`

名字极具误导性。实际引用统计：`SubtitleLengthPreset` 的消费者**全部在 `sentence_boundary/` 内部**——`language.rs`（9 个语言档 × 2 个方法）、`subtitle_layout.rs`、`watchability_merge.rs`、`mod.rs`、`tests.rs`。

文件自己的文档也承认了归属：*"Subtitle length presets (limits live on each language profile)"* —— 档位限制定义在 `sentence_boundary/language.rs` 里。而 `settings.rs` 里存的是**字符串**（`settings.subtitle_length_preset`），由 `sentence_boundary/mod.rs:61` 解析，所以 `settings` 不依赖这个类型。

结论：它是**切句层的预设枚举**，不是字幕输出层的东西。归入 `sentence_boundary/` 后，这个模块对自己的配置类型拥有完全所有权。

### B2. `text_script.rs`（222 行，crate 根）→ **建议更正：留在 crate 根**

> ⚠️ **本节结论在实施时被推翻。** 初稿建议移到 `subtitle/script.rs`，实际改完编译后发现了两个问题：
>
> 1. `script.rs` 内部 `use crate::sentence_boundary::SourceSentences;`（`convert_sentences` 的入参就是切句层的输出类型）。而 `sentence_boundary` 已经依赖 `subtitle`（`beautify` / `segmenter` / `srt` / `text_rules` 共 7 处）。把它放进 `subtitle/` 会立刻形成**双向依赖环**，破坏 §0 里那个"依赖无环"的优点。
> 2. 退一步放进 `lang/` 同样不行：`lang.rs` 当前**零依赖**（纯数据目录），让它反向依赖切句引擎属于分层倒置。
>
> **修正后的结论**：`text_script.rs` 与 `lang.rs` 是同类——都是"语言 / 文字的**选项目录**"（源语言 vs 输出字形），二者都是被 `settings` / `asr` / CLI / GUI 消费的配置型模块。放在 crate 根是**正确的**，保持原位。
>
> 记录这条是因为它说明了一件事：**归位判断不能只看"概念像谁"，必须看依赖方向**。

`TextScript`（原文 / 简体 / 繁体）+ `applies_to_language` + `convert` / `convert_sentences`，在 `asr.rs:1113` 的输出阶段作用于字幕 cue 文本。消费者：`asr.rs`、`settings.rs`、CLI、GUI。

---

## 三、C 类：过度拆分（3 个小文件应合并）

`sentence_boundary/` 有 16 个文件。整体按流水线阶段划分得很好，但有 3 个是"为了一个函数开一个文件"：

| 文件 | 行数 | 生产内容 | 现状 |
|---|---|---|---|
| `timing.rs` | **14** | `gap_ms`、`seconds_to_ms` | 2 个毫秒换算函数 |
| `words.rs` | **25** | `to_core_words`、`from_core_words` | DTO ↔ 内部类型的双向转换 |
| `text.rs` | 106 | `join_words`（1 个函数，其余全是测试） | 词序列拼接 |

三者都是**无状态的纯小工具**，与 `types.rs`（87 行，7 个类型定义）职责同源：都是"类型与它们的转换"。

**建议**：合并进 `sentence_boundary/types.rs`，或新建 `sentence_boundary/util.rs`。推荐前者——`timing`/`words` 操作的对象正是 `types.rs` 里定义的类型，放一起可读性最好；`join_words` 也随之归入。

注意：`text.rs` 目前有 6 个测试挂在它上面，`timing.rs` / `words.rs` 是 0 个。合并后测试要同步迁移，**别丢测试**。

---

## 四、D 类：职责混合的中大型文件

### D0. `crates/oneasr/src/main.rs` — 4,925 行（最严重）

占全项目 23,266 行的 **21.2%**、GUI crate 6,744 行的 **73%**。全项目 12 个超过 100 行的函数里 **8 个在它这儿**；最长的三个（`render_settings` 1,038 / `render_list` 646 / `render_stats_panel` 406）也都在。

内部分区：

| 行区间 | 行数 | 内容 |
|---|---|---|
| 1–171 | 171 | 模块声明、`use`、3 个枚举、`fn main()` |
| 173–274 | 102 | `struct OneAsrApp`：**58 个字段**，横跨 7 个互不相关的领域 |
| 276–2,035 | **1,760** | `impl OneAsrApp` 第一块：**55 个方法，全是逻辑** |
| 2,035–2,238 | 204 | `TaskRowView`、`impl Render`（`render` 106 行）、47 行布局常量 |
| 2,240–4,724 | **2,485** | `impl OneAsrApp` 第二块：**仅 7 个方法，全是渲染** |
| 4,725–4,925 | 201 | 纯辅助函数 + 2 个枚举 + 面板常量 |

`self.` 引用 456 次、`cx.listener` 44 次、**内联测试 0 处**。近 30 次涉及 `.rs` 的提交中有 **20 次**改了它（第二名 `settings.rs` 14 次）。

一个 `impl` 块被物理拆成两处（276 与 2,240），本身就是"装不下"的信号——现在的分区是按书写顺序，不是按职责。

职责与目标模块的对应关系见 §5。

### D1. `oneasr-core/src/asr.rs` — 1,449 行

结构不差（`Pipeline` + 6 个 `stage_*` 方法，最长函数只有 70 行的 `stage_vad_plan`），但混了 4 类东西：

| 行区间 | 内容 | 建议去处 |
|---|---|---|
| 52–300 | `AsrError` / `AsrStage` / `StageUpdate` / `StageTiming` / `TaskTiming` / `StageClock` | `asr/mod.rs` |
| 300–560 | `check_asr/aligner/demucs_model_dir` + `ModelRole` 缓存 + safetensors 索引与权重校验 | `asr/model_check.rs` |
| 565–1,150 | `Pipeline` + `run` + 6 个 `stage_*` | `asr/pipeline.rs` |
| 1,074–1,270 | `stage_export`（197 行，全文件最长）+ 导出辅助 | `asr/export.rs` |

**其中 `model_check` 那块（约 260 行）与流水线零关系**，是最容易摘出去的部分，也是本文件里唯一"确实住错了模块"的内容——它其实和 `model/` 更亲。

### D2. `oneasr-core/src/model/download.rs` — 874 行

35 个顶层项，明显是两个主题拼在一起：

- **就绪与可写探测**（L179–264）：`is_model_ready`、`is_cuda_runtime_ready`、`resolve_cuda_runtime_dir`、`cuda_runtime_search_dirs`、`cuda_runtime_ready_in`、`file_meets_ready_threshold`、`probe_writable` → `model/ready.rs`
- **下载状态机**（L265–778）：`download_model`、`FileDownloadCtx`、`download_one_file`（**131 行，本文件最长**）、`finish_part_file`、`sha256_matches`、`format_speed`、`truncate` → 留在 `model/download.rs`
- **ModelScope HTTP 细节**（L671–778）：`download_client`、`modelscope_request`、`content_length_total`、`content_range_start`、`initial_bytes`、`is_success`、`retry_backoff`、`sleep_cancellable`、`discard_part` → `model/http.rs`

拆分依据：`download_one_file` 里同时在做 HTTP 协议细节（Range 续传、Content-Length 解析、退避重试）和文件落盘状态机，这是它膨胀到 131 行的原因。把 HTTP 细节推到一个模块后，它有机会自然缩短。

### D3. `oneasr-core/src/engine/local.rs` — 613 行

26 个顶层项，**内部已经按适配器分好了，只是没切开**：

| 行区间 | 内容 | 建议去处 |
|---|---|---|
| L28–198 | `ComputeBackend`、`CudaProbe`、`probe_cuda_device`、`resolve_compute_backend`（66 行）、`cuda_load_failure_msg` | `engine/cuda.rs` |
| L199–286 | `LocalEngineProvider` + `impl EngineProvider` | `engine/provider.rs` |
| L287–320 | `QwenAsrAdapter` + `impl AsrEngine` | `engine/qwen_asr.rs` |
| L321–361 | `QwenAlignerAdapter` + `impl Aligner` | `engine/aligner.rs` |
| L362–549 | `DemucsSeparatorAdapter`、`read_stereo_f32`、`write_stereo_f32`、`preferred_separator_backend` | `engine/demucs.rs` |

> ⚠️ 源码里有 `crate::engine::local::` 这种**深路径内联引用**（`asr.rs` 里就有）。改名时要用编辑器全局替换，别漏掉非 `use` 形式的引用。

### D4. `oneasr-core/src/stats.rs` — 637 行

台账逻辑和通用日历数学混在一起，文件里自己都画了分节线：

- **台账**（L29–206）：`LEDGER_FILE` / `LEDGER_VERSION`、`StatsRecord`、`StatsSummary` + `impl`、`ledger_path`、`fallback_ledger_path`、`append` / `append_at`、`load` / `load_from` / `read_ledger`、`summarize` → `stats/mod.rs` + `stats/ledger.rs`
- **日历数学**（L251–342，注释明写 `calendar (no dependency; proleptic Gregorian)`）：`days_from_civil`、`civil_from_days`、`parse_ymd`、`format_ymd`、`monday_of`、`shift_days`、`month_of`、`days_in_month`、`days_between` → `stats/calendar.rs`
- **格式化**：`format_day_cn`、`format_span_secs`

日历那块是**可复用、可独立测试的纯函数组**（当前 20 个测试里有一批正是测它的），单独成文件后能脱离台账独立演进。

> **关于 `stats.rs` 是否该跟着 `job.rs` 一起搬去 GUI**：**不该**。它虽然只被 GUI 消费，但内容是 I/O + 聚合的领域逻辑，零 GPUI 依赖、自带 20 个测试，属于"可测试的领域服务"。判断标准是**有无 UI 依赖**，不是"谁在调用"。

---

## 五、目标结构（全项目）

### 5.1 `crates/oneasr-core/`

```
src/
├── lib.rs                   扁平再导出（公开 API 保持不变，这是拆分不出问题的前提）
├── asr/
│   ├── mod.rs               ← asr.rs 的类型部分
│   ├── model_check.rs       ← asr.rs 的模型目录校验 + ModelRole 缓存（约 260 行）
│   ├── pipeline.rs          ← asr.rs 的 Pipeline + run + stage_prepare/separate/plan/transcribe/align
│   └── export.rs            ← asr.rs 的 stage_export + 导出辅助
├── engine/
│   ├── mod.rs               端口（trait）—— 保持不变
│   ├── provider.rs          ← local.rs 的 LocalEngineProvider
│   ├── cuda.rs              ← local.rs 的 ComputeBackend / CudaProbe / 探测与错误文案
│   ├── qwen_asr.rs          ← local.rs 的 QwenAsrAdapter
│   ├── aligner.rs           ← local.rs 的 QwenAlignerAdapter
│   ├── demucs.rs            ← local.rs 的 Demucs 适配器 + 立体声 PCM 读写
│   └── testing.rs           保持
├── model/
│   ├── mod.rs               保持
│   ├── catalog.rs           保持
│   ├── path.rs              保持
│   ├── ready.rs             ← download.rs 的就绪 / 可写探测
│   ├── http.rs              ← download.rs 的 ModelScope HTTP 细节
│   └── download.rs          ← 下载状态机
├── sentence_boundary/
│   ├── mod.rs               编排（build_source_sentences_from_words）+ WordTokenDto
│   ├── preset.rs            ← subtitle_length.rs 归位
│   ├── profile.rs           ← language.rs 改名（见 §6）
│   ├── types.rs             ← 合并 timing.rs + words.rs + text.rs
│   ├── digit_glue.rs        保持
│   ├── semantic.rs          保持（补 //! 文档）
│   ├── punkt_map.rs         保持（文档语言统一，见 §6）
│   ├── boundary_rules.rs    保持
│   ├── subtitle_layout.rs   保持
│   ├── vad_align.rs         保持
│   ├── watchability_merge.rs 保持
│   ├── assembly.rs          保持（补 //! 文档）
│   └── tests.rs             保持 + 接收 mod.rs 里 3 个测试专用函数
├── subtitle/
│   ├── mod.rs
│   ├── script.rs            ← text_script.rs 归位
│   ├── segmenter.rs / srt.rs / text_rules.rs / beautify.rs / alignment.rs   保持
├── stats/
│   ├── mod.rs               记录与聚合
│   ├── ledger.rs            ← 台账读写
│   └── calendar.rs          ← 日历数学（约 110 行）
├── settings.rs              保持（补 //! 文档）
├── media.rs / paths.rs / vad.rs / lang.rs / runtime.rs / diagnostics.rs   保持
└── bin/oneasr-cli.rs        保持
```

**移出本 crate**：`job.rs` → GUI `app/task.rs`；`ui_labels.rs` → GUI `ui/text.rs`。

### 5.2 `crates/oneasr/`（GUI）

```
src/
├── main.rs                  ~120 行：仅 init 顺序 + Application / open_window
├── app/                     状态与逻辑
│   ├── mod.rs               OneAsrApp 58 字段（按 6 组分节注释）+ new() 装配
│   ├── task.rs              ← 从 core 迁入的 job.rs
│   ├── worker.rs            WorkerMsg / AsrJob + 7 个 handler + tick
│   ├── downloads.rs         四路下载状态机（start/cancel/busy/progress/handle）
│   ├── run_control.rs       批次与队列编排
│   ├── rows.rs              行快照、进出场动画
│   ├── settings.rs          持久化、dirty、路径与文件选择
│   ├── overlays.rs          语言菜单、计时气泡、波形 tick
│   └── stats.rs             台账的 UI 侧读写
├── ui/                      渲染
│   ├── mod.rs               impl Render（组装 6 块）
│   ├── text.rs              ← 从 core 迁入的 ui_labels.rs（+ 收拢散落的中文文案）
│   ├── theme.rs             ← theme.rs 归位
│   ├── metrics.rs           全部布局常量
│   ├── util.rs              ← 尾部纯展示函数（saved_tale / format_thousands /
│   │                           stats_level_color / status_border_color / row_accent /
│   │                           media_type_icon / is_video_format / count_output_lines /
│   │                           ease_out_cubic）—— 全部可单测
│   ├── chrome.rs            render_titlebar + render_toolbar + render_status_bar
│   ├── task_list.rs         列表骨架 + 空态
│   ├── task_row.rs          单行视图（从 render_list 的 554 行闭包提炼）
│   ├── settings_drawer.rs   抽屉骨架 + 固定 footer
│   ├── settings_sections.rs S1–S10 十个分区
│   ├── stats_panel.rs       统计面板
│   ├── empty_wave.rs        空态波形 + 波形常量
│   └── widgets/             ← widgets.rs (839) 拆开
│       ├── mod.rs
│       ├── button.rs        btn / btn_cta / caption_btn / settings_gear_btn / icon_btn / pill
│       ├── popover.rs       popover_dismiss_layer / popover_menu_shadow /
│       │                    floating_lang_menu / timing_breakdown_popover
│       ├── install_row.rs   model_download_row / component_install_row / download_action_row
│       └── logo.rs          app_logo / IconKind / icon_svg_path
└── infra/                   宿主环境交互（都不做 GPUI 渲染）
    ├── mod.rs
    ├── assets.rs            ← assets.rs
    ├── sfx.rs               ← sfx.rs
    ├── ui_font.rs           ← ui_font.rs
    ├── shell.rs             ← shell.rs
    └── crashlog.rs          ← crashlog.rs
```

### 5.3 `main.rs` 的 7 类职责 → 目标模块

| 职责 | 主要方法 | 约行数 | 目标 | 状态字段 |
|---|---|---|---|---|
| 1 | 进程入口 | `main` | 60 | `main.rs` | — |
| 2 | worker 协议与消息泵 | `poll_worker`、`WorkerMsg`、`AsrJob` | ~335 | `app/worker.rs` | `tx` `rx` `job_tx` `worker_channel_dead` `active_stage` |
| 3 | 四路下载状态机 | `start_model_download`、`download_busy` 等 9 个 | ~179 | `app/downloads.rs` | 4×`*_download` + 4×`*_dl_handle` |
| 4 | 批次与队列编排 | `launch_task`、`start_all`、`start_one`、`try_start_next`、`end_batch_if_idle`、`delete_task`、`clear_all` | ~444 | `app/run_control.rs` | `tasks` `batch_*` `busy` |
| 5 | 设置持久化与路径选择 | `save_settings`、`add_paths`、`add_files_dialog`、4×`pick_*` | ~175 | `app/settings.rs` | `settings` `settings_dirty` |
| 6 | 覆盖层菜单与动画 | `toggle_lang_menu`、`timing_*`、`dismiss_menus`、`tick_empty_wave` 等 ~21 个 | ~315 | `app/overlays.rs` | 悬停/弹层/波形 12 个 |
| 7 | 行快照与进出场 | `snapshot_task_rows`、`drain_row_anims` | ~70 | `app/rows.rs` | `entering` `exiting` `hover_row` |
| 8 | 统计台账 UI 侧 | `reload_stats`、`record_stats`、`toggle_stats` | ~30 | `app/stats.rs` | `stats` `stats_open` `stats_hover_day` |
| 9 | 全部渲染 | `render_*` × 7 | **2,485** | `ui/*` | — |

### 5.4 `render_settings` 的 10 个分区

| 分区 | 内容 | 约行数 |
|---|---|---|
| S1 | 默认语言 + 字幕长度（并排平分） | 135 |
| S2 | 分段时长（30–180s 预设） | 64 |
| S3 | 输出格式 + 中文输出 | 128 |
| S4 | 字幕输出位置 | 91 |
| S5 | 语音识别模型 | 126 |
| S6 | 对齐模型 | 91 |
| S7 | 人声分离（HTDemucs） | 124 |
| S8 | 推理后端（auto / cuda） | 50 |
| S9 | CUDA 运行库 | 45 |
| S10 | 音效与运行提醒 | 48 |
| — | 抽屉骨架 + 34 行"一次性 clone 40 个局部变量"的前置采集 | ~137 |

拆完后那个 40 个 `let` 的头部会自然消失：每个分区只取自己需要的 2–5 个值。

---

## 六、E 类：命名与文档一致性

1. **`lang.rs` 与 `sentence_boundary/language.rs` 重名**。二者职责不同（前者是源语言目录 `SOURCE_LANGUAGES`，后者是切句用的语言档 `LanguageProfile`），互不引用（已核实无环）。同一 crate 里两个"语言模块"必然误导。
   → 建议 `sentence_boundary/language.rs` 改名为 **`sentence_boundary/profile.rs`**，语义直接对应它导出的 `profile_for_lang`。`lang.rs` 保持不动（它是公开 API）。

2. **13 个文件缺 `//!` 模块文档**，而项目另有 15 个文件的文档头写得相当好（`asr.rs`、`engine/mod.rs`、`model/download.rs`、`stats.rs` 都是范本）。缺口集中在：`settings.rs`（801 行！）、`sentence_boundary/mod.rs`（编排主体）、`sentence_boundary/` 的 `assembly`/`semantic`/`text`/`timing`/`types`/`words`、`subtitle/` 的全部 5 个文件。
   → 拆分时顺手补齐，成本极低、长期收益明确。

3. **`sentence_boundary/punkt_map.rs` 用中文写文档**，其余全项目用英文。
   → 统一为英文（或全项目统一为中文，但那是更大的决定；跟随现状 = 英文）。

4. **`sentence_boundary/mod.rs` 混入 3 个测试专用函数**（L150–161，`#[cfg(test)] fn build_deterministic_sentence_spans` 等）。`#[cfg(test)]` 门控的生产文件函数是常见但不理想的做法。
   → 移入 `tests.rs`。

5. **数字/缩写 token 重组分散在两个顶层模块**：`subtitle/segmenter.rs`（`should_merge_*_pair` ×10、`merge_*_tokens` ×5）与 `sentence_boundary/digit_glue.rs`（`glue_asr_split_digits`）。
   **经核实这不是重复实现**——`normalize_word_tokens` 在 `asr.rs:1085` 先跑（对齐后立刻规范化），`glue_asr_split_digits` 在 `sentence_boundary/mod.rs:52` 后跑，两者是流水线前后两阶段，且 `digit_glue` 的文档明确划了界（*"Decimal / thousand separators (`1.` `4`, `1,` `000`) are left alone"*，而那正是 `segmenter` 的 `should_merge_decimal_pair` / `should_merge_thousands_pair` 的职责）。
   → **不改结构，但两处文档应互相交叉引用**，把这条流水线显式写出来，否则下一个人很容易误以为其中一份是死代码。

---

## 七、仓库目录布局（低优先级）

| 项 | 现状 | 评价 |
|---|---|---|
| `settings.json` / `stats.jsonl` / `oneasr-error.log` | 落在源码根 | `.gitignore` 已覆盖；开发时以仓库根为 app_root 运行所致。**可不动**，若想更整洁可指向 `dev-data/` |
| `output/sfx-lab/`（3 个 .py + 1 wav + 1 html） | 产品输出目录里放开发实验材料 | **确实错位**，移到 `scripts/` 或 `.workbuddy/` |
| `release/`（8 个版本 × portable+setup，16 个文件） | 历史发布包全平铺 | 建议按版本子目录归档或定期清理，否则无限增长 |
| `dist/` vs `release/` | 两者命名接近 | 职责不同（`dist` = 当前构建的安装布局，`release` = 已发布产物）。可考虑把 `release/` 改名 `releases/` 降低混淆 |
| `assets/` / `bin/` / `dll/` / `models/` | 与 `crates/` 平级的运行时资源 | 布局清晰、分工明确（`assets` = 编译期内嵌源，其余为运行时资源）。**保持** |
| `docs/` 只有 1 个文件 | — | 本文档落在这里，合适 |
| `forum-post.md` | 仓库根 | 已是草稿性质，移入 `docs/` 更整齐 |

---

## 八、实施路线

原则：**每阶段只做"剪切 → 粘贴 → 补 use"，不改任何行为逻辑**；每步编译通过即验证成功。GPUI 的借用检查器会替你抓出搬移错误。

### 阶段 0 — 跨 crate 归位（独立可先做，零风险）
1. `core/job.rs` → `oneasr/src/app/task.rs`
2. `core/ui_labels.rs` → `oneasr/src/ui/text.rs`
3. 从 `core/lib.rs` 移除这两个模块与再导出，修 GUI 的 `use`

验收：`cargo check` 两个 crate 通过；`cargo build -p oneasr-core --bin oneasr-cli` 通过（**证明 CLI 确实不依赖它们**）；`cargo test -p oneasr-core` 全绿。

### 阶段 1 — core 内归位与合并（零行为变化）
4. `subtitle_length.rs` → `sentence_boundary/preset.rs`
5. `text_script.rs` → `subtitle/script.rs`
6. `sentence_boundary/language.rs` → `profile.rs`
7. `timing.rs` + `words.rs` + `text.rs` → 合并进 `types.rs`（含测试迁移）
8. `mod.rs` 的 3 个 `#[cfg(test)]` 函数 → `tests.rs`

验收：`cargo test -p oneasr-core` 的 **271 个测试一个不少地全绿**——纯搬移阶段，测试数与通过率是硬指标。

### 阶段 2 — core 的中大型文件拆分（不改逻辑）
9. `asr.rs` → `asr/`（mod / model_check / pipeline / export）
10. `engine/local.rs` → `engine/`（provider / cuda / qwen_asr / aligner / demucs）
11. `model/download.rs` → `ready.rs` + `http.rs` + 本体
12. `stats.rs` → `stats/`（mod / ledger / calendar）

验收：测试全绿 + `oneasr-cli` 对一个真实媒体文件跑通一次完整流水线（覆盖 VAD → ASR → align → export）。

### 阶段 3 — GUI 视图层外提
13. `widgets.rs` → `widgets/`（button / popover / install_row / logo）
14. `theme.rs` / `assets.rs` / `sfx.rs` / `ui_font.rs` / `shell.rs` / `crashlog.rs` → `ui/` 与 `infra/`
15. `main.rs` 的 `impl` 第二块（2,485 行）→ `ui/*`；`render_settings` 拆 10 个分区；`render_list` 拆骨架 + `task_row`；尾部纯函数 → `ui/util.rs`

**`main.rs` 4,925 → 约 380 行。**

### 阶段 4 — GUI 逻辑层外提
16. `impl` 第一块（1,760 行）→ `app/*`；`poll_worker` 按 `WorkerMsg` 的 7 个变体拆 handler + 一个 `tick()`

**`main.rs` → 约 120 行。**

### 阶段 5 — 文档与可测化
17. 补 13 个 `//!` 文档；统一文档语言；补 §6.5 的交叉引用
18. 把 `app/run_control.rs` / `app/rows.rs` 里**不依赖 gpui 的纯状态迁移**抽成自由函数并加单测

> 关于第 18 条：GUI crate 现在只有 7 个测试（`crashlog` 2 / `sfx` 3 / `ui_font` 2），`main.rs` 是 **0**——而它承担了全部状态机。这是本次调整最有价值的长期回报。

### 每阶段统一验收
1. `cargo check` / `cargo clippy` 零新增告警
2. `cargo test` 全绿（core 271 个用例数不下滑）
3. 手工回归：加文件 → 起批 → 中途删行 → 跑完 → 打开输出位置 → 切语言 → 改设置保存 → 下载模型 → 开统计面板 → Esc 关菜单
4. `pack-release.ps1` 产物与 `dist/OneAsr/` 现有体积同量级（LTO + 单 codegen unit 已配置，拆分不应改变产物大小）
5. `oneasr-error.log` 无新增 panic

---

## 九、不建议做的事

1. **不要为了行数拆 `boundary_rules.rs`（1,047）与 `sentence_boundary/profile.rs`（926）的规则表。** 前者 19 个方法最长 37 行，后者 131 个方法最长 27 行——逻辑已足够扁平，问题是数据密度高而非结构差。真要切只能按语言族切，**且仅在确有多语言扩展计划时才值得**。
2. **不要把 58 个字段的 `OneAsrApp` 拆成多个 struct。** `tasks` / `batch_*` / `entering` / `exiting` / `busy` 是强耦合状态机，`active_stage` 与 `hover_row` 也与 `tasks` 联动。58 个字段里真正能独立成组的只有**下载**（8 个）与**统计面板**（3 个）。拆开会把 456 处 `self.` 变成更长的路径，收益为负。
3. **不要把散落的中文 `label()` 集中到一处。** `AsrStage::label` / `ModelId::label` / `TextScript::label` 等紧贴枚举定义是 Rust 惯例，好处是新增枚举变体时**编译器会强制你补文案**（match 穷尽性）。集中到 `ui_labels.rs` 反而会丢了这个保障。§0 里要搬的是 `ui_labels.rs` 这个**模块**，不是要统一所有文案。
4. **不要把 `stats.rs` 搬去 GUI。** 判据是有无 UI 依赖，不是谁在调用——见 §4 D4。
5. **不要把 `vad.rs` 与 `sentence_boundary/vad_align.rs` 合并。** 经核实两者角色正交：`vad.rs::run_vad` 是 VAD 推理的**生产者**（产出语音段），`vad_align.rs::SpeechSegmentIndex` 是**纯查询器**（接收调用方归一化好的语音段，做容差判定），无重复推理。当前分居两处反映了各自的领域归属，只是**可发现性差**——建议在两者文档里互相引用即可。
6. **不要为 `runs/` / `output/` / `release/` 的本地产物设计与迁移**，`.gitignore` 已完整覆盖，收益低。§7 里唯一值得动的是 `output/sfx-lab/`。
7. **不要一次搬完再编译。** `main.rs` 是 GPUI 借用最密集的地方，攒下几十个借用错误再定位，成本远高于分批。

---

## 十、拆分时必须注意的技术约束

这几条不注意会在中途撞墙：

1. **私有字段可见性决定 `ui/` 的落位。** Rust 私有项对**定义模块及其子孙模块**可见。`OneAsrApp` 放 `app/mod.rs` 时，`app/worker.rs` 等子模块可直接读写私有字段（零改动）；但兄弟模块 `ui/*` 不行，需要 `pub(crate)`。
   → 方案 A：视图放 `app/ui/`（子模块，完全不碰可见性）；方案 B：`ui/` 与 `app/` 平级，给约 30 个被视图读取的字段标 `pub(crate)`。**二选一，不要混用。**

2. **`render_list` 的行闭包必须提为自由函数，不能提为 `&mut self` 方法。** 那 554 行闭包能工作，靠的是先 `snapshot_task_rows()` 拿到 **owned 的 `Vec<TaskRowView>`**，且闭包全程不捕获 `self`（用 `cx.listener(move |this, ...|)` 里的 `this` 改状态）。提成方法后 `.children(items.into_iter().map(|r| self.render_row(r, cx)))` 会立刻撞上 `self` 的可变借用冲突。
   → 正确签名：`fn task_row(row: &TaskRowView, ctx: &RowCtx<'_>, cx: &mut Context<OneAsrApp>) -> impl IntoElement`。

3. **`render_settings` 的 10 个分区可以安全各自提为方法**：它们挂在同一个 `div()` 上顺序 `.child(section({...}))`，每个 `{}` 块独立求值，借用即释放，不构成对 `self` 的重叠借用。

4. **`widgets.rs` 的耦合要先解。** 它现在通过根模块的 `use` 再导出引用调色板（`crate::ACCENT`、`crate::LINE`… 20+ 处），并直接用 `Context<crate::OneAsrApp>`。必须改为 `crate::ui::theme::ACCENT` 与 `crate::app::OneAsrApp`，否则 `main.rs` 永远瘦不下来。
   → 注意：`crate::ACCENT` 能写通，是因为 `main.rs` 顶部有 `use theme::{...}` 把常量导入了根命名空间。这是隐含耦合，拆文件时会集体爆发。

5. **`engine/local.rs` 有深路径内联引用**（`crate::engine::local::`）。改名/搬移时用全局替换，别只改 `use`。

---

## 附：统计方法与脚本

全部统计脚本在 `scripts/`（该目录按 `.gitignore` 为本地专用，不入库），需在仓库根目录执行：

```bash
python scripts/rs-fn-stats.py        # 全项目函数体量排行
python scripts/rs-fn-map.py          # main.rs 函数边界地图（行号 + 行数）
python scripts/rs-module-audit.py    # 每个文件的 //! 文档头 + 公开 API 清单
python scripts/rs-dep-map.py         # 模块 × 消费者（core/GUI/CLI/tests）矩阵
python scripts/rs-gui-consumers.py   # GUI 实际消费的 core 符号 → 归属模块
python scripts/rs-outline.py         # 指定文件的顶层项结构
python scripts/rs-ui-strings.py      # 中文字面量分布
```

**口径提醒（踩过的坑）**：
- 函数体量**必须**用大括号配对测算。用「下一个函数起始 − 本函数起始」会把函数之后的自由函数与结构体定义算进来——本项目曾据此把 `asr.rs::finish`（真实 12 行）误报成 376 行、`stats.rs::is_empty` 误报成 260 行。
- 「内联测试行」= `#[cfg(test)]` 首次出现处到文件末尾。
- 「零消费者」的判定基于全仓库符号检索，已排除 `lib.rs` 的再导出本身。
- 提交热度取自 `git log -30 --name-only -- "*.rs"` 的文件出现次数。

---

## 十一、实施记录（2026-09-16）

### 已完成并验证

权威验证基线：**271 个测试**（core `lib` 252 + `pipeline_behaviour` 4 + `real_or_honest` 2；GUI 13）。
每个阶段结束时 `cargo check --workspace --all-targets` **零错误、零告警**，测试数不降。

| 阶段 | 内容 | 结果 |
|---|---|---|
| **0** | 跨 crate 归位：`core/job.rs` → `oneasr/src/app/task.rs`；`core/ui_labels.rs` → `oneasr/src/app/ui/text.rs`；从 `core/lib.rs` 移除两个模块与再导出 | core 测试 258 → 252，GUI 7 → 13，**总数 271 不变**（6 个用例随代码迁移） |
| **1** | crate 内归位与合并：`subtitle_length.rs` → `sentence_boundary/preset.rs`；`language.rs` → `profile.rs`；`timing.rs` + `words.rs` + `text.rs` → `sentence_boundary/util.rs`；`mod.rs` 的 3 个测试专用函数移入 `tests.rs` | 271 全绿 |
| **2** | GUI 视图层外提：布局常量 → `app/ui/metrics.rs`；展示辅助 → `app/ui/util.rs`；7 个 `render_*` → `app/ui/{chrome,task_list,settings_drawer,stats_panel,empty_wave}.rs`；`impl Render` → `app/ui/mod.rs` | **`main.rs` 4929 → 2088 行** |
| **3** | GUI 逻辑层外提：`impl OneAsrApp` 的 62 个方法按职责分入 `app/{downloads,worker,settings,overlays,rows,run_control,stats}.rs`；`OneAsrApp` / 枚举 / `TaskRowView` / `new()` → `app/mod.rs`；worker 协议 → `app/worker.rs` | **`main.rs` → 90 行** |
| **3+** | `render_list` 的行闭包（553 行）提为自由函数 `task_row_view` + `RowCtx` → `app/ui/task_row.rs` | `render_list` **646 → 103 行** |
| **4** | `render_settings`（1,038 行）拆为抽屉骨架 + 10 个分区方法：33 个前置采集值收进 `SettingsFormState`（一次计算，按引用共享）；分区移入 `app/ui/settings_sections.rs` | **`render_settings` 1,038 → 95 行**；骨架 221 行 + 10 个方法（53–146 行） |
| **5** | `poll_worker`（298 行）拆为调度器 + 10 个 handler + `tick_status_hint` | **`poll_worker` 298 → 30 行** |
| **6** | `core/engine/local.rs`（613）→ `engine/local/{mod,cuda,provider,qwen_asr,aligner,demucs,tests}.rs`，`mod.rs` 作 facade 保持 `engine::local::*` 路径 | 7 个文件，最大 198 行 |
| **7** | `core/model/download.rs`（874）→ `{download,ready,http}.rs`，`model/mod.rs` 的再导出来源同步更新 | 680 + 94 + 126 |
| **8** | `core/asr.rs`（1,449）→ `asr.rs` + `asr/{model_check,export}.rs`；`check_*_model_dir` 在 `asr.rs` 里重新导出，`crate::asr::*` 路径不变 | `asr.rs` 1,109 + 261 + 105；**模型校验 248 行彻底离开流水线文件** |
| **9** | `widgets.rs`（839）→ `widgets/{button,install_row,logo,popover,mod}.rs` | 5 个文件，最大 342 行 |

### 实施后结构（第六轮结束时实测）

**GUI crate `crates/oneasr/src/` — 8,237 行**

```
main.rs                      90   仅进程入口（init + open_window）
app/
  mod.rs                    435   OneAsrApp 58 字段 + new() + 共享类型
  prelude.rs                 50   视图与逻辑层的共享导入
  worker.rs                 386   WorkerMsg / AsrJob / 按变体拆分的 handler
  run_control.rs            513   批次与队列编排              +6 纯函数单测
  overlays.rs               332   菜单 / 计时气泡 / 动画状态
  rows.rs                   238   行快照 / 进出场 / 队列排名    +8 纯函数单测
  task.rs                   234   ← 从 core 的 job.rs 迁入
  downloads.rs              202   四路下载状态机
  settings.rs               195   持久化与路径选择
  stats.rs                   52   台账 UI 侧
  ui/
    settings_sections.rs   1065   10 个抽屉分区方法（各 53–146 行）
    task_row.rs             581   单行视图（自由函数 + RowCtx）
    stats_panel.rs          378   统计面板组装与元素构建
    stats_grid.rs           352   年历布局与文案（纯数据层）    +11 单测
    status_bar.rs           263   状态栏                      +3 单测
    settings_drawer.rs      224   抽屉骨架
    chrome.rs               170   标题栏 + 工具栏
    util.rs                 147   纯展示函数
    mod.rs                  129   impl Render
    task_list.rs            114   列表骨架 + 空态
    empty_wave.rs           100   空态波形
    text.rs                  89   界面文案 ← 从 core 的 ui_labels.rs 迁入
    metrics.rs               51   布局常量
widgets/                    848   button 340 / popover 252 / install_row 185 / logo 71 / mod 20
（crate 根未动）assets 119 · crashlog 284 · sfx 243 · shell 61 · theme 72 · ui_font 200
```

**core crate `crates/oneasr-core/src/` — 15,686 行**

```
asr.rs                    1,105   流水线主体（阶段已是一函数一阶段）
  asr/model_check.rs        257   模型目录校验 ← 原本混在流水线里，与它零关系
  asr/export.rs             101   输出写入（SRT / TXT / words JSON）
sentence_boundary/        6,008   切句层；tests 1,510 / boundary_rules 1,047 / profile 925
  util.rs                   156   ← text.rs + timing.rs + words.rs 合并
  preset.rs                  29   ← subtitle_length.rs（改名 + 归位）
subtitle/                 1,363   segmenter 752 / text_rules 379 / alignment 302 / srt 144 / beautify 82
stats/                      666   ← 原 stats.rs 637 拆为 mod 125 + ledger 310 + calendar 231
model/                    1,343   download 679 / catalog 425 / http 121 / path 100 / ready 94
engine/                   1,181   local/ 654（cuda 172 · demucs 197 · provider 105 · tests 69 · aligner 50 · qwen_asr 41 · mod 20）+ testing 227 + mod 132
media 571 · settings 830 · vad 389 · lang 187 · text_script 222 · 其余 < 100
```

> `text_script.rs` 留在 crate 根是**有意的**（见 §2 B2 的更正）：它依赖 `sentence_boundary`，而后者已依赖 `subtitle`，搬进 `subtitle/` 会成环。

### 实施中发现的几个技术要点

1. **`impl OneAsrApp` 跨模块后必须放宽方法可见性。** 方法定义在 `app::downloads` 而在 `app::overlays` 调用，私有方法不可见。统一加 `pub(crate)`。
2. **视图层放在 `app/ui/` 而不是平级的 `ui/`，是为了零可见性改动。** `OneAsrApp` 定义在 `app/mod.rs`，其私有字段对**子孙模块**可见；`app/ui/*` 是子孙，因此能直接读写 58 个字段中的绝大多数，不必给字段加任何标注。若放平级的 `ui/`，则要给约 30 个字段加 `pub(crate)`。
3. **`task_row_view` 必须用 `impl IntoElement + use<>`。** edition 2024 的 `impl Trait` 默认捕获所有在作用域内的生命周期，导致返回值"借用"了局部变量 `row` / `ctx`，编译报 `captured variable cannot escape FnMut closure body`。精确捕获语法 `+ use<>` 声明不捕获任何生命周期后通过。
4. **`widgets.rs` 曾通过根命名空间的再导出引用调色板**（`crate::ACCENT` 等 13 个常量，依赖 `main.rs` 顶部的 `use theme::{...}`）。这些引用在拆分时集体失效，已全部改为 `crate::theme::*`；同理 `crate::LangMenuLayout` → `crate::app::LangMenuLayout`、`crate::Render` → `gpui::Render`。**这是拆分 `main.rs` 的前置条件**，也是 §10 约束 4 的实证。
5. **一个必须记录的代码删除**：`sentence_boundary/preset.rs` 的 `SubtitleLengthPreset::as_str` 被删除。它从未被调用过——此前因为 `subtitle_length` 是 `pub mod`，公开可达性掩盖了 `dead_code` 告警；模块转为私有后告警暴露。经全仓库检索确认无任何调用点，删除不影响行为（该类型也不在 crate 公开 API 中——`settings` 存的是字符串）。

### 自动化搬移脚本的三个坑（都实际踩到并修正了）

在剩下的大批量拆分里（`asr.rs` / `engine/local.rs` / `model/download.rs` / `widgets.rs`）用了脚本做机械搬移，遇到三个会**静默丢代码**的问题，记录在此：

1. **文档注释与属性行必须与条目一起搬。** 只在条目所在行向前扫描 `///` 是不够的：`/// 说明` + `#[derive(..)]` + `enum X` 这种排列下，`#[derive]` 挡在中间，扫描会停在它前面，**把 derive 和文档一起丢掉**（本次在 `ComputeBackend` 上真实发生，表现为 `Cell<ComputeBackend>::get()` 因缺少 `Copy` 而报错）。正确做法是向前同时接受 `//` 与 `#[..]` 两种行。
2. **导入块提取要能跨行。** `use crate::engine::{\n  A, B,\n};` 是多行语句；若用「下一行必须以 `use ` 开头」来扫描，会在此截断，生成的每个文件都留下一个未闭合的 `{`（表现为 `this file contains an unclosed delimiter`）。
3. **按「首尾条目区间」切片会把区间内未列出的条目一起带走。** 本次 `sha256_matches` / `finish_part_file` 夹在 `http.rs` 的两个目标条目之间被顺带搬走，之后又在原文件里补了一份 → 重复定义。**切片后必须逐条核对归属。**

配套的验收手段（缺一不可）：

- `cargo check --workspace --all-targets` 零告警；
- `cargo test --workspace` 用例数不降（本次全程 **271**）；
- **声明名集合比对**：`git show HEAD:<file>` 与拆分后各文件的顶层声明取集合，`MISSING` 必须为空；
- **大括号平衡**：每个生成文件的 `{`/`}` 净值必须为 0（上面第 2 个坑就是靠它定位的）。

脚本：`scripts/rs-split-generic.py`（含上述三项修正）、`scripts/rs-check-integrity.py`、`scripts/dbg-balance.py`。

### 第五轮：剩余项清零（2026-09-16 晚）

上一轮列出的 7 项待办中，第 1–5、7 项全部完成；第 6 项（按语言族再切规则表）保留为「有需求再做」。

| 项 | 前 → 后 |
|---|---|
| `render_stats_panel` | **405 → 74 行**。纯数据层独立成 `app/ui/stats_grid.rs`（年历布局 + 文案，**11 个单测**），渲染拆为 6 个自由函数 |
| `app/mod.rs::new()` | **212 → 72 行**。worker 线程 spawn → `spawn_asr_worker()`，settings 加载与 CUDA 回退 → `load_settings_or_fallback()`，环境快照 → `log_environment_snapshot()`；余下主要是**无法拆分**的 58 字段结构体字面量 |
| `render_status_bar` | 整段迁至 `app/ui/status_bar.rs`；`tally_tasks()`（**3 个单测**）承担「淡出中的行不计入统计」这条规则。`chrome.rs` 326 → 168 行，只管标题栏与工具栏 |
| `core/stats.rs` 637 | → `stats/{mod 125, ledger 310, calendar 231}`，`mod.rs` 作 facade，`stats::…` 路径不变；20 个测试按归属分入两个子模块 |
| 模块文档 | 全部源文件都有 `//!`（只剩 `build.rs` 不需要）；`punkt_map.rs` 中文文档改英文。顺带修掉 11 个 rustdoc 警告 → `cargo doc --no-deps` **零警告** |
| 状态机可测化 | `rows.rs` 提出 `queue_ranks` / `row_fade` / `finished_exits`（**8 个单测**）；`run_control.rs` 提出 `no_start_reason` / `single_start_blocker` / `next_queued_id` / `batch_summary`（**6 个单测**） |

测试 **271 → 299**（GUI 13 → 41），全程 `cargo check --workspace --all-targets` 零错误零告警。

#### 第一次被写下来的规则（现在有测试兜着）

- **淡出中的行不算数。** `exiting` 里的墓碑行不进状态栏计数（`tally_tasks`）、不进队列排名（`queue_ranks` 只数 `Queued`）、不参与「下一个跑谁」（`next_queued_id` 跳过）。删除后它立刻从用户可见的计数里消失，却仍在 `tasks` 里直到淡出结束——列表因此不会跳。
- **队列排名是稳定排序。** 同 `queue_seq` 的行保持列表顺序，帧与帧之间不会互换位次；未分配序号的排队行排最后（`u64::MAX`），不会插队。
- **一条行有三种画法。** 淡出中 = 不透明度递减且**不可点**；淡入中 = 递增且可点；两者都不是 = 完全不透明、可点。删除发生在淡入期间时，**淡出赢**。
- **批次结束语永不报 0/0。** `batch_summary` 在 `done == 0 && err == 0` 时返回 `None`，调用方直接 return——空总结读起来像指责，不像战报。

#### 当前最长函数（全部 ≤ 140 行）

`cmd_transcribe` 140（CLI）· `align_text_to_timestamps` 127 · `render` 105 · `cmd_asr_chunk` 105（CLI）· `render_list` 103 · `launch_task` 101 · `render_settings` 95 · `render_toolbar` 83

GUI 侧已无超过 105 行的函数。起点曾是 **4929 行的文件 + 1038 行的函数**。CLI 的两个子命令与示例的 `main` 是「命令式入口」，100–140 行仍可读，不建议机械拆。

#### 剩下的大文件为什么不必再拆

| 文件 | 行数 | 理由 |
|---|---|---|
| `sentence_boundary/tests.rs` | 1,511 | 纯测试语料 |
| `asr.rs` | 1,109 | 流水线本体 + 内联测试；阶段已是一函数一阶段 |
| `boundary_rules.rs` / `profile.rs` / `subtitle_layout.rs` | 1,046 / 925 / 799 | 语言规则表，19–131 个方法中最长 37 行；再切只能按语言族，收益取决于是否扩语种 |
| `settings_sections.rs` | 1,029 | 10 个抽屉分区方法（53–146 行），已是分区粒度 |
| `settings.rs` | 804 | 设置模型 + 331 行内联测试 |
| `subtitle/segmenter.rs` | 752 | 数字/缩写合并规则链 + 386 行内联测试 |

#### 本轮验收

`cargo check --workspace --all-targets` 零错误零告警 · `cargo test --workspace` **299 全绿** · `cargo doc --no-deps --workspace` 零警告 · 函数名集合比对**零丢失**。

除「提取纯函数」外仍是纯搬移：行为逻辑未变，变的只是函数边界。新增的语义面是**测试**——上面那几条规则第一次被写下来并被验证。

### 第六轮：lint 与仓库卫生清零（2026-09-16 晚）

结构拆完之后，把「机器能查出来的问题」也清到零。

| 项 | 结果 |
|---|---|
| **clippy** | 51 个警告 → **0**（`cargo clippy --workspace --all-targets`） |
| **rustdoc** | 11 个警告 → **0**（`cargo doc --no-deps --workspace`） |
| **行尾** | 94 个 `.rs` 统一 CRLF，新增 `.gitattributes` 固定约定（此前 35 个纯 LF、56 个 CRLF、3 个混合） |
| **测试** | 299 全绿（本轮不动任何语义） |

#### clippy 清掉的六类

1. **collapsible_if ×14** → Rust 2024 let-chains（`if let Some(x) = a && b`）。
   **警告：`cargo clippy --fix` 只删行、不重排缩进**，14 处里 9 处的 `&&` 留下了阶梯缩进、body 与 `}` 也停在旧的嵌套层级。已用新增的 `scripts/rs-fix-letchain-indent.py` 规范化（42 行 / 9 文件）：`&&` 对齐到 `if_indent + 4`、body 到 `+4`、`}` 回到 `if_indent`。脚本幂等（复跑 0 改动），以后再用 `clippy --fix` 应当接着跑它。
2. **field_reassign_with_default ×16** → struct update 语法
   （`Settings { language, ..Default::default() }`）。集中在 `settings.rs` 测试（13）、CLI（2）与示例（1）。转换后 5 处 `mut` 变成多余，交给 `cargo fix` 去掉。
3. **too_many_arguments ×3** → 两处按真实概念打包：
   - `install_row.rs` 的下载 / 安装行构建器：5 个「这一行的状态」参数 → **`ComponentRow`**（`model_download_row` 8→4、`download_action_row` 8→4、`component_install_row` 7→3）；
   - `subtitle_layout.rs` 的 DP 入口：4 个「一次运行的预算」参数 → **`SpanBudget`**（9→6），顺带把 `limit <= 0.0` 的早退封装成 `SpanBudget::for_profile() -> Option<Self>`。
   两处都是真打包，**没有留下任何 `#[allow]`**。
4. **doc list without indentation ×2** → `vad.rs` 的 `1) 2) 3)` 改成散文。
5. **索引循环 → 迭代器 / 手写 char 比较 / 可化简的布尔表达式 / 多余的引号** 各 1–2 处。
6. **`items after a test module`** → 测试模块挪到文件末尾。

#### 两个值得记下的判断

- **`field_reassign_with_default` 值得改，不是为了讨好 linter**：`Settings { language, ..Default::default() }` 一眼看出「偏离默认值的是哪几个字段」，13 行零散赋值做不到。
- **`too_many_arguments` 要分两类**：能对应真实概念的（一行的状态、一次运行的预算）就打包；只是「参数就是多」的才谈豁免。本轮两处都属前者。

#### 顺带发现并修掉的仓库隐患

行尾在项目里本来就是混的（35 LF / 56 CRLF / 3 混合），而 `core.autocrlf=true` 期望工作区是 CRLF。任何按行读写的脚本都会把行尾带进新文件，久而久之 `git status` 里会冒出"只改了行尾"的假 diff。已统一为 CRLF 并加 `.gitattributes`（`*.rs text eol=crlf` + 二进制类型声明）。规范化后 `git diff --shortstat` 与之前**完全一致**，证明没有引入假 diff。

#### 建议纳入日常流程

```sh
cargo clippy --workspace --all-targets   # 应零警告
cargo test  --workspace                  # 应 299 通过
cargo doc   --no-deps --workspace        # 应零警告
```

前两条适合做提交前检查；第三条能挡住"文档里的链接指向已删掉的项"这类腐化。
