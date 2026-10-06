# 用 ctc-forced-aligner-wgpu 替换 Qwen3-ForcedAligner —— 采纳分析

> 分析日期 2026-10-06 · 对比基线：OneAsr 现役 Qwen3-ForcedAligner-0.6B（qwen3-aligner-wgpu）
> 素材：韩语真实综艺（266.6s，59 条人工 SRT 参考）+ FLEURS en/ja/zh 拼接音频（20 片段、片段时间已知）
> 原始数据：`D:\OneAsr\tmp\segtest\`（`bakeoff.py` / `bakeoff_align.json` / `out/ctc_*.json`）

## 一句话结论

**建议替换**。CTC 对齐器在真实难素材上的打轴精度显著更好（词头中位数 45–60ms vs
Qwen 的 951ms 中位数 + 21/59 条字幕无法可靠对齐），输出 token 形态与现管线**逐位同构**
（即插即用），架构上单调 DP 不会全局失步（Qwen 在歌声段整体丢同步），词表覆盖全部
11 种界面语言，wgpu 版本一致（30.0.1）零依赖冲突，OOV/短文本/emoji 边界行为全部优雅。
代价主要是两处管线逻辑要适配（标点恢复步骤、模型目录校验）和 3 个语言的精度待验证。

## 1. 它是什么

- [ctc-forced-aligner-wgpu](https://github.com/eclipse005/ctc-forced-aligner-wgpu)：纯 Rust +
  wgpu 的 CTC 强制对齐，吃 Meta Omnilingual 家族的 **omniASR-CTC-300M-v2** 权重
  （wav2vec2，300M，16kHz，时间戳 20ms 栅格），算法移植自 Python 参考（MahmoudAshraf97）
  并在该 checkpoint 上做了 10 处带测量依据的修正（见其 README「Deliberate departures」）。
- 权重上游 Apache-2.0（facebook/omniASR-CTC-300M）；镜像 aadel4/omniASR-CTC-300M-v2
  的许可证字段**发版前需上页面核实**。
- 本机已有权重：`D:\omnilingual-asr\models\omniASR-CTC-300M-v2-hf`（vocab 10288 条）。
- 库 API 极小：`Aligner::load_on(dir, DeviceSelector)` + `align(wav, text, window, context)`
  → `AlignOutput { tokens: Vec<TokenAlignment>, segments, … }`，
  `TokenAlignment { piece, start, end, start_frame, end_frame, word_id, score }` +
  `space_before` / `inferred`。与 OneAsr 的 `Aligner` port（`AlignRequest{wav,text,language}`
  → `Vec<AlignedToken{text,start,end}>`）一一对应，`language` 参数 CTC 不需要（忽略即可）。

## 2. 实测对比

### 2.1 韩语真实综艺（人声+音乐+多人，人工 SRT 为参考，59 条 cue）

对齐方法对两个对齐器完全一致：输出字符流与参考 cue 做单调 Levenshtein 对齐，
每条 cue 取命中字符的 [min.start, max.max] 与人工时间求差。

| 指标 | Qwen（喂 ASR 转写） | CTC（喂参考原文） | CTC（喂 ASR 转写＝生产等价） |
|---|---|---|---|
| 可靠匹配 cue | 34/59（**21 条 >5s 失步**） | 59/59 | 54/59 |
| start MAE / 中位数 | 508ms / 65ms | **110ms / 45ms** | **215ms / 60ms** |
| start ≤200ms | 21/34 | 55/59 | 40/54 |
| end 中位数 | 235ms | 376ms | 325ms |

Qwen 的 21 条失步集中在《月亮代表我的心》歌声段：ASR 转写与演唱内容 diverge 后，
其两段式（先转写后对齐）词流整体跑偏，后续 cue 全部锚错位置。**CTC 的单调 DP
对整段 (音频，文本) 做全局对齐，文本错误只会局部插值（`inferred` 标记），
结构上不可能全局失步**——这是两个架构的本质差别，也是替换的最强理由。

### 2.2 干净语音一致性（FLEURS 拼接，en/ja/zh）

两个对齐器输出字符流互相单调对齐后的 start 差：
**en 中位数 30ms、zh 65ms、ja 80ms**（p95 160/180/320ms）。干净素材上两者高度一致，
即：替换在"本来就没问题"的素材上不会带来可感知的退化。

### 2.3 输出形态（决定即插即用）

| | Qwen（管线内经标点恢复后） | CTC（原生） |
|---|---|---|
| zh | `此 / 外，/ 在 / 革 / 命 / 之后，…`（逐字+标点随前字） | **完全相同** |
| en | `As / light / pollution / …`（逐词） | **完全相同** |

CTC 的分词规则（空格语言→词、CJK→字、标点零时长搭在前字上）与 Qwen 走完
`attach_transcript_punctuation` + `normalize_word_tokens` 之后的产物**逐位同构**。
且 CTC 保证"输入文本的每个字符都按序出现在 tokens 里"（OOV 字插值保留），
比 Qwen"对齐器丢标点、管线再从转写贴回去"更简单也更不易丢字符。

### 2.4 速度与资源（266.6s 韩语，P104-100）

| | 加载 | 对齐 | 合计 | 实时倍率 |
|---|---|---|---|---|
| CTC（整文件一次） | 4.8s | 4.7s+0.02s | ≈9.5s | **56×** |
| Qwen（6 块逐块） | 2.6s | 4.6s | ≈7.2s | ≈37× |

速度同一量级——对齐本来就不是管线瓶颈（ASR 转写 6.2s、断句导出毫秒级）。
换 CTC 的收益在质量与稳健，不在速度；副产品是显存减半（300M vs 0.6B），
且支持 ASS 卡拉 OK 输出（已做，见 §6：本仓库的 `subtitle::ass` 从时间轴 IR 渲染，
**两个对齐器通用**，不读 CTC 的 token）。

### 2.5 边界行为（管线退化路径相关，全部实测）

- 短文本（1–4 字）：正常。
- 词表外罕见汉字（曌/彧）：**插值保留**（`inferred: true`，时间取邻域中点），不丢字、不崩溃。
- emoji：并入所在词单元，`inferred: true`。
- 单字+标点：正常（标点零时长搭在前字）。

### 2.6 词表覆盖（OneAsr 全部 11 语言）

en/fr/de/es/it/pt 全部拉丁变音符、zh 简体、yue 繁体（嘅咗唔哋喺乜嘢等粤字）、
ja 假名（含拗音/促音/长音）、ko 谚文、全角标点、数字：**全部在词表内**；
常用 3500 汉字抽检 100/100。注意"字符在词表"≠"该语言精度已验证"——
仓库自己的评测覆盖 en/es/fr/de/ja/zh + 韩语广播；**yue/it/pt 精度需补测**
（FLEURS 有 yue 参考集，可直接测）。

## 3. 与 OneAsr 管线的兼容性

管线：`convert → VAD → ASR(Qwen) → align → normalize_word_tokens → sentence_boundary → SRT/TXT`。

**集成后对齐逻辑与 Qwen 完全一致**（逐块对齐、块偏移、退化回退），仅两处分发：
标点恢复只走 Qwen 路径；CTC 适配器在每次 `align()` 前重置 GPU scratch（见下）。

| 环节 | 影响 |
|---|---|
| `Aligner` port（engine trait） | 签名不变；新 adapter 忽略 `language`，tokens 1:1 映射 |
| `attach_transcript_punctuation` | **仅 Qwen 路径需要**（CTC token 原生带标点），按所选模型分发 |
| `normalize_word_tokens` / 断句 / SRT / TXT / words_json | **零改动**（输出形态同构已验证） |
| `MIN_ALIGN_SEC` / `has_alignable_word` 退化路径 | 两条路径一致保留 |
| 后端选择 | CTC crate 自带同款 `DeviceSelector`；已接入 `ONEASR_DEVICE` |
| words_json | schema 不变；可选增加 `score`/`inferred` 字段（见 §5） |

### 集成中发现并修复的 crate bug（上游已修）

管线按块对齐暴露了一个独立 CLI 从未触发的问题：**同一 `Aligner` 实例跨文件复用时，
第二个文件的对齐整体漂移**（韩语实测：块 3 文本在块内系统性偏 +9.9s；最小复现 =
复用实例先对齐 51s 再对齐 60s，第二个文件首 token 晚 4.4s，全新实例则帧级精准）。
根因在 GPU 塔的激活 scratch 缓存：录制 dispatch 图按「输入长度+帧数+读回模式」复用，
跨文件命中后重放的 dispatch 不会重写窗口 1 前面的零填充 context 区，上一文件的
样本泄漏进本文件的第一个窗口。已在 crate 修复（`GpuModel::reset_scratch()`，
`align()` 每文件入口调用；文件内窗口照常共享 scratch，性能不损；
`examples/reuse_probe.rs` 断言该不变量），rev `e9faaf5`。

修复前后同条件对比（ko 真实综艺 · CTC 管线逐块对齐 vs 人工 SRT）：

| | 修复前 | 修复后 |
|---|---|---|
| 可靠匹配 cue | 20/59（35 条失步） | **54/59** |
| start MAE / 中位数 | 926ms / 80ms | **215ms / 52ms** |

## 4. 集成改动清单（并存可选，估算）

**定位（已定）**：CTC 对齐器与 Qwen3-ForcedAligner **并存可选**，与 ASR 的 0.6B/1.7B
选择模式同构——设置里选对齐模型，**默认 CTC**（按钮顺序也是 CTC 在前），下载列表新增 CTC 条目。
对用户来说就是"对齐模型多了一个可选项"。

> 首启决策落在 `settings::decide_default_aligner`：**磁盘上只有 Qwen 权重时退 Qwen，
> 其余（含什么都没装）一律 CTC**。所以已装 Qwen 的老用户升级后仍停在 Qwen，
> 新装用户拿到的默认是 CTC。

1. `Cargo.toml`：`ctc-forced-aligner-wgpu = { git = "...", rev = "<钉死>" }`；
   **`deny.toml` 的 git 来源白名单要加这个仓库**（CI 会拦）。
2. `model/catalog.rs`：`ModelId::OmniAsrCtc300M` + `ALIGNER_CHOICES = [OmniAsrCtc300M, QwenAlign06B]`
   + `model_def("eclipse005", "omniASR-CTC-300M-v2", files)` ——ModelScope 仓库已传好
   （1304MB fp32 权重 + config/preprocessor/special_tokens/tokenizer_config/vocab.json，
   走现有 `modelscope.cn/.../resolve/master/` 下载通道，零新基建）。
   可选：补传 f16 权重（~652MB）作下载变体，crate 会自动 widen。
3. `settings.rs`：`aligner_model: String`（catalog id，照 `asr_model` 模式：normalize、
   目录解析 `aligner_dir_for`、按模型各记目录）；旧 settings 无此字段 → 走上面的
   在盘决策。`Settings::default()` 自身必须**自洽**（id 与目录同一个模型），
   且「没读到配置 / 解析失败」三条回退分支都要过一遍 `normalize`。
4. `engine/local/ctc_aligner.rs`：新 adapter（`Aligner::load_on` + `align(wav, text, window, context)`）。
   ⚠️ **读 `AlignOutput::words`，不要读 `tokens`**：`tokens` 是逐 CTC target，而这个
   checkpoint 的词表是字符级的，所以每个脚本都是逐字符。crate 的 `words` 才是成品
   对齐单元——不带空格的书写系统逐字、其余整词、标点零时长搭在前面的单元上，
   判断在 crate 的 `spans::unit_runs` 里一处完成（`words` 与它自己的断句器读同一份）。
   直传 `tokens` 的后果是 `sentence_boundary::util::join_tokens` 逐 token 插空格，
   把 `Whisper` 渲染成 `W h i s p e r`。`language` 参数 CTC 不需要，忽略。
5. `engine/local/device.rs`：加 `ctc_selector_from_env()`（ONEASR_DEVICE 第三处接线）。
6. `asr.rs` 对齐阶段按所选模型分发：`attach_transcript_punctuation` 仅 Qwen 路径；
   CTC 路径 tokens 直通（标点原生在 token 里）。
7. 模型目录校验：新增 `check_ctc_model_dir`（`config.json + vocab.json + model.safetensors`），
   照 `check_aligner_model_dir` 的缓存模式。
8. GUI 设置抽屉：「对齐模型」照「语音识别模型」的**分段按钮**样式加 `[CTC] [Qwen]`
   两个选项（**CTC 在前、默认 CTC**——用户已定；不做下拉），目录行跟随所选模型、
   各自记忆目录；下载列表加 CTC 条目；i18n 新文案成对写入双语表。
9. 单测：catalog URL 断言（照现有 `url.contains("models/eclipse005/...")` 模式）、
   settings normalize 兼容、adapter 映射、目录校验——路径一律 `join` 构造，跨平台。

## 5. 风险与机会

**风险**
1. **end 边界**：CTC 的中点规则让词尾倾向伸进后续静音（有 1.0s 上界；韩语实测 end
   中位数 325–376ms，比 Qwen 的 235ms 略晚）。断句阶段本就参考 VAD 语音段收口，
   预期影响有限，但**换完后要过一遍真实素材的观感检查**。
2. **yue/it/pt 精度未验证**：字符在词表 ≠ 精度达标；上线前用 FLEURS-yue 等参考集补测。
3. **权重许可证**：上游 Apache-2.0，但镜像页（aadel4）的 license 字段与随包 LICENSE
   要在打进下载目录前核实；下载来源按供应链规则钉 tag/哈希。
4. CTC 无语言条件输入：语言提示少了一个信息源（实测影响不显著，仅记录）。

**机会**
1. `score`（逐 token 帧均分）：**第一次有了打轴置信度**——可做"低置信段提示/重对齐"，
   也能给"ASR 崩坏块"（本次分段测试中 ko_c180 那种）提供检测信号。
2. `inferred` 标记：OOV 率可统计，罕见字字幕不再"消失"。
3. ASS 卡拉 OK 输出：**已做**。crate 内建 `--format ass`（本仓库未直接用：它的断行
   是 crate 自己的），本仓库的 `subtitle::ass` 从 `timeline.json` 那份 IR 渲染，
   粒度（CJK 逐字、空格语言逐词）由对齐器已经决定的 token 粒度继承而来，
   所以 CTC 与 Qwen 共用同一个渲染器。
4. 整文件一次对齐（window=None）可行——未来甚至可以把各 ASR 块的转写拼接后一次对齐。

## 6. 实施状态与路线

**已完成（本次）**：

1. crate 修复（scratch 跨文件污染，rev `e9faaf5`，已推送）+ OneAsr rev 钉新提交。
2. OneAsr 并存集成全量落地：catalog（ModelScope `eclipse005/omniASR-CTC-300M-v2`，
   6 文件 size+sha256+revision 钉死）、settings（`aligner_model` + 逐模型目录记忆 +
   首次运行决策纯函数）、engine adapter、`ONEASR_DEVICE` 接线、模型目录校验、
   GUI 设置抽屉 `[CTC] [Qwen]` 按钮（CTC 在前、默认 CTC）、CLI `--aligner` 旗标、
   i18n 双语。门禁全绿（check 0 警告 / clippy / fmt / 360 测试）。
3. 真机 E2E：ko 真实综艺 CTC 管线 54/59 匹配、词头 MAE 215ms/中位 52ms（Qwen：
   34/59、508ms/65ms）；zh CTC 与 Qwen 均 8.92% CER（文本一致，对齐器不影响转写）。
4. 时间轴边界（`timeline.rs`）：测量与呈现之间落一个 `{stem}.timeline.json`，
   Phase B 抽成纯函数，`oneasr-cli render` 脱离模型重渲染（真实素材逐字节一致）。
5. ASS 卡拉OK输出（issue #14 第一项）：设置「输出格式」第三个按钮、CLI `--ass`、
   `subtitle::ass` 渲染器。两个对齐器各出一份真实素材，扫光合计逐行闭合、
   剥掉 markup 后与 SRT 逐字相同。

**待办**：

1. 补测 yue/it/pt 精度 + 核实权重 license 字段（半天）。
2. 「文稿对齐」功能（issues #2/#14 的第二、三项）——交互设计已成稿
   （`docs/transcript-alignment-design.md`），按切片实施；对齐任务天然使用
   所选对齐器，CTC 是该功能的首选引擎，ASS 渲染器已是现成的输出末端。
   ⚠️ 设计文档里「跳过 VAD」只对 `--line-breaks text` 成立：选 `auto` 时必须跑
   VAD，否则自动断句会丢掉全部「静音处硬切」。
3. GUI 观感检查 end 边界后发版。

## 附录：复现

```powershell
cd D:\OneAsr\tmp\segtest
D:\ctc-forced-aligner-wgpu\target\release\align.exe --audio "C:\Users\ADMIN\Videos\《Veiled Cup》\ko_4m.wav" `
  --text "C:\Users\ADMIN\Videos\《Veiled Cup》\ko_4m.txt" `
  --model D:\omnilingual-asr\models\omniASR-CTC-300M-v2-hf --format json --output out\ctc_ko.json
python bakeoff.py     # 两对齐器 vs 参考（SRT cue / FLEURS 片段边界）
```

数据文件：`bakeoff_align.json`（逐 cue 差值明细）、`out/ctc_{ko,en,ja,zh}.json`、
`out/ctc_ko_asrtext.json`（生产等价条件）、Qwen 侧用矩阵实验的 `out/{lang}_c60.words.json`。
