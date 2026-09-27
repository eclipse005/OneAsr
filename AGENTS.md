# AGENTS.md

OneAsr —— 本地、离线的音视频转字幕桌面应用（Qwen3-ASR + ForcedAligner，Rust + gpui/wgpu），
支持 Windows / Linux / macOS（Apple Silicon）。转写全程在本机跑，不依赖在线 API。

## 常用命令

- 开发编译：`cargo build -p oneasr`
- 发版编译：`cargo build -p oneasr --release`
- 检查（三条都要干净，CI 会拦）：`cargo check -p oneasr` 目标 **0 警告**、
  `cargo clippy --locked -p oneasr --all-targets -- -D warnings`、`cargo fmt --all --check`
- 测试：`cargo test -p oneasr-core`、`cargo test -p oneasr`（改动引擎或设置模型时必跑）
- 版本一致性：`python3 scripts/verify-version.py`（加 `--tag v<版本>` 可一并断言 tag）
- 打包（Windows）：`./scripts/pack-release.ps1` —— 版本号从 `Cargo.toml` 读，**没有 `-Version` 参数了**
  （以前会写回 `Cargo.toml`，等于两处真相）；开关只有 `-SkipBuild` / `-SkipInstaller`
- 打包（Linux / macOS）：`./scripts/pack-unix.sh --os macos --arch arm64` —— 同样没有 `--version`，可加 `--skip-build`
- 工具链：`rust-toolchain.toml` 钉死版本，本地与 CI 都以它为准，workflow 里不写第二个版本号
- 跑起来看效果：`target/release/oneasr.exe`（app_root 会自动向上找含 `bin/ffmpeg` 的目录）

## 项目结构

- `crates/oneasr-core/` —— 引擎：模型目录解析、VAD、转写管线、字幕切分、设置模型、统计
- `crates/oneasr/` —— GUI：gpui 界面、任务列表、设置抽屉、耗时弹卡与动画
- `crates/oneasr-core/src/i18n.rs` —— 双语机制（`UiLang` / `Str` / `t` / 系统语言探测）
  + 管线自产文案（错误、阶段名、下载进度）的双语表
- `crates/oneasr/src/i18n.rs` —— GUI 界面标签的双语表（复用 core 机制）
- `vendor/gpui/` —— 打过补丁的 gpui；每处改动都要登记进 `vendor/gpui/PATCHES.md`
- `installer/` —— Windows Inno Setup 脚本；`installer/macos/` 是 macOS 的安装说明与安装脚本源文件
- `scripts/` —— 打包脚本与图标生成，外加三道校验：`verify-version.py`（版本一致性）、
  `verify-ffmpeg-manifest.py`（ffmpeg 哈希清单）、`ffmpeg-checksums.txt`（清单本体）
- `.github/workflows/` —— `ci.yml`（每次提交的门禁：格式 / clippy+测试 / 版本 / 供应链）
  与 `release.yml`（tag 触发的打包发版）
- `rust-toolchain.toml` —— 工具链的唯一来源；`deny.toml` —— 依赖漏洞与来源规则
- `docs/release-notes/` —— 每个版本的发布说明（见下）
- 运行期布局：`bin/ffmpeg`、`models/`、`output/`、`runs/` 都在 app_root 下

## 代码风格

- 提交信息用 conventional commits + 中文描述，例如 `fix(settings): 每个 ASR 尺寸各记一个模型目录`
- 提交正文写清"为什么"和用户能感知的影响，不写"优化代码"这类空话
- 注释与文档用中文，术语跟 README 保持一致；`.rs` 在仓库里是 CRLF（Windows 检出）
- 新增行为要带单测，`crates/oneasr-core/src/settings.rs`、`crates/oneasr/src/app/rows.rs` 有现成的纯函数单测可参照
- **单测必须跨平台**（CI 只在 ubuntu 上跑，本地是 Windows，两边不一致时以 CI 为准）：
  - 路径一律用 `join` 构造，**不许写死 `r"D:\..."` 当期望值**。`Path::join` 在 Linux
    用的是 `/`，写死字面量的话断言比的是分隔符、不是被测的规则
  - 喂给 `file_name()` / `parent()` / `ends_with()` 的路径必须用 `join` 构造：`r"D:\m\x"`
    在 Linux 上整体被当成文件名，解析必然失败
  - 断言平台串要**大小写不敏感**：`consts::OS` 是小写 `linux`，而 UA 里是 `X11; Linux x86_64`
  - 不要断言本机环境（仓库里有没有 `bin/ffmpeg`、`models/` 在不在）。要测就把逻辑抽成
    吃参数的纯函数、用临时目录树测；`#[ignore]` 比静默 early-return 好——
    读不到文件就 return 的测试在 CI 上是**假绿**
  - 改完只在 Windows 上跑过不算数，`git push` 后看 CI 结果
- **界面文案必须双语**：新增用户可见文案一律成对写入双语表——GUI 标签进
  `crates/oneasr/src/i18n.rs` 的 `L` 表（`t(L::X)`），管线自产消息用
  `crates/oneasr-core/src/i18n.rs` 的助手；**不许硬编码单语字符串**。
  带参数的句子写成按语言的 helper，两种语言的语序差异在 helper 内消化；
  纯格式化函数显式接收 `UiLang` 参数（单测不读全局），渲染/动作边界才用 `ui_lang()`
- 术语表（zh → en）：任务 task · 队列 queue · 转写 transcribe · 断句 sentence segmentation ·
  打轴/对齐 alignment · 人声分离 vocal separation · 字幕 subtitle · 分段时长 segment length ·
  量化 quantization(int8) · 推理后端 inference backend

## 发布流程

主分支只有 `master` 一个，也是 GitHub 上的默认分支，直接提交，不走过 PR 流程。
（历史上曾用 `wgpu` 作默认分支，两条线从未分叉，已快进合并并删除；
远端与本地都只剩 `master`，CI 的 `on.push.branches` 也已改成 `[master]`。）

版本号只有一个出处：`Cargo.toml` 的 `[workspace.package] version`（应用标题栏与日志都读它）。
打包脚本不再接收、也不再写回版本号；tag 与两份 README 必须跟它一致，CI 负责断言。

1. 改 `Cargo.toml` 的 `[workspace.package] version`；若动了依赖，跑一次 `cargo check` 刷新 `Cargo.lock`
   （发布构建一律 `--locked`，锁文件没跟上的话 CI 与打包都会失败）
2. 更新 **两份** README 顶部的当前版本号与三平台下载链接：`README.md`（英文）与 `README.zh-CN.md`（中文），内容保持同构
3. **写发布说明** `docs/release-notes/v<版本>.md`（格式见下，这一步不能省）
4. 本地自检全绿再推：`cargo fmt --all --check`、
   `cargo clippy --locked -p oneasr --all-targets -- -D warnings`、
   `cargo test --locked -p oneasr-core -p oneasr`、`python3 scripts/verify-version.py --tag v<版本>`
5. `git commit` → `git push origin master` → `git tag v<版本>` → `git push origin v<版本>`
6. tag 触发 `.github/workflows/release.yml`：三平台打包 + 自动建 release 并挂上全部产物
   （打包前先跑同一个 `verify-version.py --tag`，tag 与 `Cargo.toml` 不一致就直接失败）
7. 验证：`gh release view v<版本>`，确认 5 个产物都在、说明正文正确

注意：`workflow_dispatch` 只出 artifact、**不建 release**；只有推 tag 才会发布。
CI 只在 `push: master` 与 PR 上跑，直接提交到 `master` 同样会拿到门禁结果。

### 发布说明要写成什么形式

用户明确要求的形式（面向使用者，不是变更清单；**双语**，英文在前、中文在后，
工作流会把整个文件原样贴进 GitHub Release 正文，两个受众一次满足）：

- 双语 Markdown，各占一半：文档标题各用一级 `#`（英文标题在前、中文标题在后），
  英文二级标题用 `## New` / `## Fixed` / `## macOS Install` / `## Download`，
  中文用 `## 新增` / `## 修复` / `## macOS 安装` / `## 下载`；没有内容的小节两边都直接不写
- 每条一到两行，讲"用户能感觉到什么变化"；**不要复述提交号、不要把提交原文搬上去**
- 只写提交里体现的事实，不许推测、不许编造功能，不写"提升了体验""优化了性能"这类空话
- 涉及单个平台的改动要点明平台（macOS / Windows / Linux），两种语言都点
- macOS 那一节要写明未签名与两种安装方式（终端 `bash` 在前、右键打开作兜底），两种语言都写
- 结尾附对比链接 `https://github.com/eclipse005/OneAsr/compare/<上个 tag>...<本 tag>`（两种语言各一次）
- 双语从 **v1.1.0 之后的第一个版本**起生效：国际化的提交（`91b0d16`）晚于 v1.1.0 的
  发布提交（`1986387`，tag `v1.1.0`），所以 v1.1.0 之后发的那一版必须写双语说明。
  `docs/release-notes/v1.1.0.md` 已按上面的结构补写成双语，可以当样板；但它线上
  Release 的正文仍是发布当时贴出去的旧版单语文本，除非重推这个 tag 才会更新
- `v1.0.3` 及更早的发布说明是**单语历史件**（条目写法可参照 `v1.0.1.md`），不再回填双语
- 工作流有兜底：这个文件不存在时会自动列提交清单（`## Changes` + 各条提交主题），
  比空白强但远不如正经说明，所以别让它缺失

### 发版注意

- macOS 包未签名（没有 Apple 开发者账号），DMG 里必须带安装说明与安装脚本。**仓库里的源文件用英文名**：
  `installer/macos/install-notes.txt` 与 `installer/macos/fix-and-open.command`（`.gitattributes` 锁 LF）；
  `scripts/pack-unix.sh` 校验的正是这两个名字，缺一个就直接报错退出，这是有意设计。
  中文名只出现在 **DMG 内**——打包时复制成 `安装说明.txt` 与 `安装 OneAsr.command`，
  英文用户靠说明文件开头一行指引。两份文件都已双语（说明文件英文在前中文在后；
  脚本弹窗按系统语言自动切换）
- macOS 15 起右键打开被隔离的 `.command` 会被系统拦住（弹框只剩「完成」），
  所以安装说明里**主推终端方式**（`install-notes.txt` 的「方式一 / Method 1」）：
  `bash` 加空格，把脚本拖进终端窗口，回车；右键打开降级为旧版本上的
  「方式二 / Method 2」兜底。`docs/release-notes/v1.1.0.md` 与两份 README 的
  macOS 安装段落口径与之一致（终端方式排在前）
- ffmpeg 来自**上游** `Tyrrrz/FFmpegBin` 的版本化 tag（tag 名就是 ffmpeg 版本），**不再用本仓库
  那个 `tools` release**——供应链少一个自建环节。打包脚本下的是**归档**，进产物前强制校验
  `scripts/ffmpeg-checksums.txt` 里那个归档的 sha256（不匹配直接失败），再只解出需要的 ffmpeg。
  换 ffmpeg 只改清单里的 `base-url`（tag）一行，再跑
  `python3 scripts/verify-ffmpeg-manifest.py --print-manifest` 更新哈希，**不要**改脚本里的 URL。
  为什么钉 tag 之外还要钉哈希：GitHub 的 release 资产默认仍可被重传，信任锚点是内容不是 URL
- 老的 `tools` release **留在原地别删**：≤v1.1.0 的打包脚本指向它，删掉历史版本就没法重新打包了。
  新脚本不再读它
- 上面这套「来源声明 + 哈希」在 `scripts/ffmpeg-checksums.txt` 里是**单一出处**（`# base-url <URL>`
  一行机器可读），两个打包脚本与 CI 的 `verify-ffmpeg-manifest.py` 都从那里读
- 已知待办：这批 ffmpeg 的 configure 行带 `--enable-nonfree`，FFmpeg 官方 LICENSE.md 写明这样的
  产物不可再分发。换 LGPL 构建**只动清单一个文件**（base-url + 哈希），不用改打包脚本
- `scripts/pack-release.ps1` 必须带 UTF-8 BOM：Windows PowerShell 5.1 没有 BOM 时按 ANSI 代码页
  读文件，中文**字符串**会被误解码并可能凭空多出一个引号把脚本解析搞崩（`release.yml` 用 pwsh，
  所以这个坑只在本地显形）。`.gitattributes` 里已登记，别把 BOM 去掉
- 依赖来源与漏洞由 `deny.toml` + CI 的 `cargo deny check advisories sources` 把关：
  git 依赖只许来自白名单里的仓库、且必须带 tag/rev。已知的 6 条「已停维护」通告都是传递依赖，
  记在配置注释里；真实漏洞仍然是硬错误
- 打包前确认 `cargo check -p oneasr` 是 0 警告——这件事现在由 CI 保证，不再靠人记

## 不要提交的东西

- `tmp/`、`forum-post*.md` —— 本地草稿与临时产物，**已在 `.gitignore` 里**，别用 `-f` 强加
- `models/`、`output/`、`runs/`、`release/`、`dist/`、`bin/` —— 权重、产物、中间文件（同样已忽略）
- 任何密钥：CI 不依赖任何 secret，需要 AI 能力时由使用者显式配置
