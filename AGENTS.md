# AGENTS.md

OneAsr —— 本地、离线的音视频转字幕桌面应用（Qwen3-ASR + ForcedAligner，Rust + gpui/wgpu），
支持 Windows / Linux / macOS（Apple Silicon）。识别全程在本机跑，不依赖在线 API。

## 常用命令

- 开发编译：`cargo build -p oneasr`
- 发版编译：`cargo build -p oneasr --release`
- 检查：`cargo check -p oneasr` —— 目标是 **0 警告**，不要新增 warning
- 测试：`cargo test -p oneasr-core`、`cargo test -p oneasr`（改动引擎或设置模型时必跑）
- 打包（Windows）：`./scripts/pack-release.ps1 -Version 1.0.1`
- 打包（Linux / macOS）：`./scripts/pack-unix.sh --version 1.0.1 --os macos --arch arm64`
- 跑起来看效果：`target/release/oneasr.exe`（app_root 会自动向上找含 `bin/ffmpeg` 的目录）

## 项目结构

- `crates/oneasr-core/` —— 引擎：模型目录解析、VAD、识别管线、字幕切分、设置模型、统计
- `crates/oneasr/` —— GUI：gpui 界面、任务列表、设置抽屉、耗时弹卡与动画
- `crates/oneasr-core/src/i18n.rs` —— 双语机制（`UiLang` / `Str` / `t` / 系统语言探测）
  + 管线自产文案（错误、阶段名、下载进度）的双语表
- `crates/oneasr/src/i18n.rs` —— GUI 界面标签的双语表（复用 core 机制）
- `vendor/gpui/` —— 打过补丁的 gpui；每处改动都要登记进 `vendor/gpui/PATCHES.md`
- `installer/` —— Windows Inno Setup 脚本；`installer/macos/` 是 macOS 的安装说明与安装脚本源文件
- `scripts/` —— 打包脚本与图标生成
- `docs/release-notes/` —— 每个版本的发布说明（见下）
- 运行期布局：`bin/ffmpeg`、`models/`、`output/`、`runs/` 都在 app_root 下

## 代码风格

- 提交信息用 conventional commits + 中文描述，例如 `fix(settings): 每个 ASR 尺寸各记一个模型目录`
- 提交正文写清"为什么"和用户能感知的影响，不写"优化代码"这类空话
- 注释与文档用中文，术语跟 README 保持一致；`.rs` 在仓库里是 CRLF（Windows 检出）
- 新增行为要带单测，`crates/oneasr-core/src/settings.rs`、`crates/oneasr/src/app/rows.rs` 有现成的纯函数单测可参照
- **界面文案必须双语**：新增用户可见文案一律成对写入双语表——GUI 标签进
  `crates/oneasr/src/i18n.rs` 的 `L` 表（`t(L::X)`），管线自产消息用
  `crates/oneasr-core/src/i18n.rs` 的助手；**不许硬编码单语字符串**。
  带参数的句子写成按语言的 helper，两种语言的语序差异在 helper 内消化；
  纯格式化函数显式接收 `UiLang` 参数（单测不读全局），渲染/动作边界才用 `ui_lang()`
- 术语表（zh → en）：任务 task · 队列 queue · 转写 transcribe · 断句 sentence segmentation ·
  打轴/对齐 alignment · 人声分离 vocal separation · 字幕 subtitle · 分段时长 segment length ·
  量化 quantization(int8) · 推理后端 inference backend

## 发布流程

主分支是 `wgpu`（GitHub 上的默认分支；本地 `origin/HEAD` 可能陈旧地指向 `master`，别被它带偏）。
直接提交到 `wgpu`，不走过 PR 流程。

1. 改 `Cargo.toml` 的 `[workspace.package] version`（应用标题栏与日志都读它），跑一次 `cargo check` 刷新 `Cargo.lock`
2. 更新 **两份** README 顶部的当前版本号与三平台下载链接：`README.md`（英文）与 `README.zh-CN.md`（中文），内容保持同构
3. **写发布说明** `docs/release-notes/v<版本>.md`（格式见下，这一步不能省）
4. `git commit` → `git push origin wgpu` → `git tag v<版本>` → `git push origin v<版本>`
5. tag 触发 `.github/workflows/release.yml`：三平台打包 + 自动建 release 并挂上全部产物
6. 验证：`gh release view v<版本>`，确认 5 个产物都在、说明正文正确

注意：`workflow_dispatch` 只出 artifact、**不建 release**；只有推 tag 才会发布。

### 发布说明要写成什么形式

用户明确要求的形式（面向使用者，不是变更清单；**双语**，英文在前、中文在后，
工作流会把整个文件原样贴进 GitHub Release 正文，两个受众一次满足）：

- 双语 Markdown，各占一半：英文二级标题用 `## New` / `## Fixed` / `## macOS Install` / `## Download`，
  中文用 `## 新增` / `## 修复` / `## macOS 安装` / `## 下载`；没有内容的小节两边都直接不写
- 每条一到两行，讲"用户能感觉到什么变化"；**不要复述提交号、不要把提交原文搬上去**
- 只写提交里体现的事实，不许推测、不许编造功能，不写"提升了体验""优化了性能"这类空话
- 涉及单个平台的改动要点明平台（macOS / Windows / Linux），两种语言都点
- macOS 那一节要写明未签名与两种安装方式（右键 / 终端 `bash`），两种语言都写
- 结尾附对比链接 `https://github.com/eclipse005/OneAsr/compare/<上个 tag>...<本 tag>`（两种语言各一次）
- 单语时代的样例是 `docs/release-notes/v1.0.1.md`（条目写法可参照）；
  双语从国际化版本起生效，按上面的结构写
- 工作流有兜底：这个文件不存在时会自动列提交清单（`## Changes` + 各条提交主题），
  比空白强但远不如正经说明，所以别让它缺失

### 发版注意

- macOS 包未签名（没有 Apple 开发者账号），DMG 里必须带 `安装说明.txt` + `安装 OneAsr.command`；
  `pack-unix.sh` 缺这两个文件会直接报错退出，这是有意设计。两份文件都已双语
  （说明文件英文在前中文在后；脚本弹窗按系统语言自动切换），
  **文件名保持中文**（英文用户靠说明文件开头一行指引）
- macOS 15 起右键打开被隔离的 `.command` 会被系统拦住（弹框只剩「完成」），
  所以安装说明里主推终端方式：`bash` 加空格，把脚本拖进终端窗口，回车
- 打包前确认 `cargo check -p oneasr` 是 0 警告（CI 会把所有 warning 打进日志）

## 不要提交的东西

- `tmp/`、`forum-post*.md` —— 本地草稿与临时产物，**已在 `.gitignore` 里**，别用 `-f` 强加
- `models/`、`output/`、`runs/`、`release/`、`dist/`、`bin/` —— 权重、产物、中间文件（同样已忽略）
- 任何密钥：CI 不依赖任何 secret，需要 AI 能力时由使用者显式配置
