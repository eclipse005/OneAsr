# AGENTS.md

OneAsr —— 本地、离线的音视频转字幕桌面应用（Qwen3-ASR，Rust 引擎 + Electron/React 界面），
支持 Windows / Linux / macOS（Apple Silicon）。转写全程在本机跑，不依赖在线 API。
对齐默认走 CTC（omniASR-CTC-300M-v2），Qwen3-ForcedAligner 仅作兼容老档保留。

> **分支约定**：`master` 是 GPUI 线；`feat/electron-ui` 是 Electron 线，已删掉
> gpui 壳（`crates/oneasr` 与 `vendor/gpui`）。两条线**各自都能发版**：
> master 走 GPUI 打包，electron-ui 走 Electron 打包；合并前互不影响。
> 合并后 Electron 产物取代 GPUI，但这是显式的发版链路切换。

## 常用命令

- 引擎 / CLI 开发编译：`cargo build -p oneasr-core --bin oneasr-cli`
- 引擎 / CLI 发版编译：`cargo build -p oneasr-core --release --bin oneasr-cli`
- Electron 开发：`cd apps/electron && npm install && npm run dev`（Node 24；Electron 44 要求 >=22.12）
- Electron 构建：`cd apps/electron && npm run build`
- Electron 全量门禁：`cd apps/electron && npm run verify`（typecheck + vitest + build）
- 模型选择与目录：以 `settings.json` 为唯一来源，**每个模型各记各的路径**（`asr_dirs` /
  `aligner_dirs` / `demucs_model_dir` / `sensevoice_model_dir`）；CLI 的
  `transcribe` / `align` / `status` / `download` 都读它，显式旗标（`--asr-model-dir` 等）可覆盖。
- 文稿匹配：GUI 把同名 `.txt` / `.srt` 与媒体一起拖入，或把文稿拖到某一行；
  CLI 用 `align --audio <媒体> --text <文稿>`，摘要 / 智能断句用
  `transcript inspect|split --input <文稿> --json`（都走 core，壳不复制规则）
- 检查（三条都要干净，CI 会拦）：`cargo check --locked --workspace --all-targets` 目标 **0 警告**、
  `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`
  （两条 cargo 命令都必须带 `--workspace --all-targets`：只写 `-p oneasr-core` 时，
  core 的 tests / examples 根本进不了 lint 视野）
- 测试：`cargo test --locked -p oneasr-core`（改动引擎或设置模型时必跑）；
  Electron 侧 `cd apps/electron && npm test`
- 版本一致性：`python3 scripts/verify-version.py`（加 `--tag v<版本>` 可一并断言 tag）
- Rust 管线文案双语：`python3 scripts/verify-i18n.py`（改 core 文案后跑，CI 会拦）
- Electron 文案双语：`cd apps/electron && npm test`（`i18n/gate.test.ts`：
  zh/en 键一致、值非空、占位符成对、渲染层无硬编码中文）
- 双语门禁的自测：`python3 scripts/test_verify_i18n.py`（改了门禁或它的测试就跑；CI 也跑）
- Windows 本机：商店的「应用执行别名」会抢走 `python3`（跑起来是 "Python was not found"、
  退出码 9009），而 Python 官方安装器与 conda 只装 `python`。仓库的文档、脚本提示与 CI
  都只有 `python3` 一种拼写，所以在本机补一个转发：`pwsh -File scripts/ensure-python3.ps1`
  —— 它挑一个 3.11+ 的解释器（`py -3` 可能还是 3.10，缺 tomllib），在它旁边写一个
  `python3.cmd` 按名字转发（conda 切环境也跟着走）；幂等，撤销就是删掉那个文件
- 门禁的断骨验证：`python3 scripts/bonebreak-verify-i18n.py` —— 把每处检查逐个还原，
  确认对应用例真的变红。**改过 `verify-i18n.py` 之后必跑**；它不进 CI（锚点是源码
  片段，实现一重构工具就会自己过期），是维护者手工用的
- 打包（Electron 线）：`./scripts/pack-electron.ps1`（Windows）/
  `./scripts/pack-electron.sh --os linux --arch x64` /
  `./scripts/pack-electron.sh --os macos --arch arm64`。版本号从 `Cargo.toml` 读，
  `apps/electron/package.json` 必须与它一致；Electron 应用、CLI、校验过的 ffmpeg
  一起进产物。Windows 出 setup.exe + portable zip，Linux 出 deb + tar.gz，macOS 出 dmg
- 打包（master / GPUI 线）：`./scripts/pack-release.ps1` / `./scripts/pack-unix.sh --os macos --arch arm64`
  —— 版本号同样从 `Cargo.toml` 读，没有版本参数
- 漏洞检查（钉住的公告库快照，先铺缓存再跑）：`pwsh -File scripts/prepare-advisory-db.ps1` 然后
  `cargo deny check advisories sources` —— 首次还需要 `git submodule update --init vendor/advisory-db`；
  更新公告库是显式动作，没有定时任务（见「发版注意」）
- 工具链：`rust-toolchain.toml` 钉死版本，本地与 CI 都以它为准，workflow 里不写第二个版本号
- 跑起来看效果：先 `cargo build -p oneasr-core --bin oneasr-cli`，再
  `cd apps/electron && npm run dev`（CLI 会从 app root / `target/release` / `target/debug` 查找）

## 项目结构

- `crates/oneasr-core/` —— 引擎：模型目录解析、VAD、转写管线、字幕切分、设置模型、统计、
  热词召回（`hotword/`：文本→音素、粗筛、`hotword.json` 存储；含短语词典资产）；
  以及 `src/bin/oneasr-cli.rs`（Electron 与脚本共用的命令行入口）
- `apps/electron/` —— Electron 界面：`src/main`（窗口、队列、CLI 子进程、下载）、
  `src/preload`、`src/renderer`（React + Vite）、`src/shared`（IPC 类型）
- `crates/oneasr-core/src/i18n.rs` —— Rust 侧双语机制（`UiLang` / `Str` / `t` / 系统语言探测）
  + 管线自产文案（错误、阶段名、下载进度）的双语表
- `apps/electron/src/renderer/src/i18n/index.ts` —— 渲染层 zh/en 双语文案表
  （`apps/electron/src/main/i18n.ts` 是主进程托盘等系统文案）
- `vendor/gpu-allocator/` —— 钉住 `windows` 0.62 的 gpu-allocator 补丁（wgpu-hal 需要）；
  改动登记在 `vendor/gpu-allocator/PATCHES.md`
- `installer/` —— Windows Inno Setup 脚本；`installer/macos/` 是 macOS 的安装说明与安装脚本源文件
  （两者目前仍属于 master 的 GPUI 打包链路）
- `scripts/` —— 打包脚本与图标生成，外加四道校验与它们的自测：
  `verify-version.py`（版本一致性）、`verify-i18n.py`（Rust 管线文案双语）、
  `verify-ffmpeg-manifest.py`（ffmpeg 哈希清单）、`ffmpeg-checksums.txt`（清单本体）；
  以及 `test_verify_i18n.py`（Rust 门禁自己的单测）、
  `bonebreak-verify-i18n.py`（门禁的断骨验证）、`_verify_common.py`
  （三个校验脚本共用的输出编码处理 —— 同一件事只写一份，改一次三处都跟着）、
  `export-phrases.py`（热词短语词典资产 `phrases.bin` 的生成器：要一份装了 pypinyin
  的 Python，不进 CI；`--check` 只核对不写盘。改了资产要连 manifest 一起提交，
  单测会拿 sha256 对账）
- `.github/workflows/` —— `ci.yml`（格式 / clippy+core 测试 / Electron 检查 / 版本+双语 / 供应链）
  与 `release.yml`（tag 触发：本分支打 Electron 包，master 打 GPUI 包）
- `rust-toolchain.toml` —— 工具链的唯一来源；`deny.toml` —— 依赖漏洞与来源规则
- `docs/release-notes/` —— 每个版本的发布说明（见下）
- `docs/lessons.md` —— 从已合并的改动里学到的规则：每条带**为什么**与**证据**
  （哪次提交、哪个 issue、哪天），并记 Seen N×；第 3 次被撞上的才迁进本文，
  两边都要读。改代码前先读它——那是本文这些规矩的出处
- 运行期布局：`bin/ffmpeg`、`models/`、`output/`、`runs/` 都在 app_root 下

## 代码风格

下面带 **粗体** 的几条规矩不是审美偏好，每一条底下都有一次具体的翻车；
来历与证据在 `docs/lessons.md`，动笔前值得先扫一眼。

- 提交信息用 conventional commits + 中文描述，例如 `fix(settings): 每个 ASR 尺寸各记一个模型目录`
- 提交正文写清"为什么"和用户能感知的影响，不写"优化代码"这类空话
- 大的改动（新行为、跨模块）在正文里记一行**验证边界**：跑了哪些检查、
  手动试了什么、什么没试（例：`验证：cargo test -p oneasr-core 全绿、
  apps/electron 的 npm run verify 全绿；未验：macOS 打包路径（本地无环境）`）。
  事后 review 或 CI 红了，一眼知道当时验到哪——没验的明说，别让"假绿"变成
  "没人知道"。跟 `#[ignore]` 比静默 early-return 好是同一条原则
- 注释与文档用中文，术语跟 README 保持一致；`.rs` 在仓库里是 CRLF（Windows 检出）
- 新增行为要带单测：Rust 侧可参照 `crates/oneasr-core/src/settings.rs` 的纯函数单测；
  Electron 侧可参照 `apps/electron/src/main/queue.test.ts` 与
  `apps/electron/src/renderer/src/i18n/gate.test.ts`
- **测试要断骨验证**：新测试写完，把被测的修复还原（或手动破坏它守护的行为）跑一遍，
  **测试必须变红**才算有效；在旧代码上照样绿的测试等于没写。别只写"加了测试"，
  说清哪个测试在坏代码上失败、失败信息是什么
- **修一类，不修一个**：改 bug 前先 grep 同构的兄弟位置，一次修完。本仓库的高发区：
  `crates/oneasr-core/src/settings.rs` 的同构纯函数、core i18n 与 Electron zh/en
  字典的同款文案、`crates/oneasr-core/src/asr/model_check.rs` 与
  `crates/oneasr-core/src/model/download.rs`
  里按 ASR 尺寸×模型角色展开的逻辑——
  这些形状都是"修一个漏一串"。只修报告里那一个，用户下一个场景就会带着同一个 bug 回来
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
- **界面文案必须双语**：新增用户可见文案一律成对写入双语表——Electron 渲染层进
  `apps/electron/src/renderer/src/i18n/index.ts` 的 zh/en 字典（主进程系统文案进
  `apps/electron/src/main/i18n.ts`），Rust 管线自产消息用
  `crates/oneasr-core/src/i18n.rs` 的助手；**不许硬编码单语字符串**。
  带参数的句子写成按语言的 helper，两种语言的语序差异在 helper 内消化；
  纯格式化函数显式接收语言参数（Rust 单测不读全局，Electron helper 显式收 `UiLang`），
  渲染/动作边界才读当前语言。Electron 的 vitest 门禁会扫描渲染层源码里的硬编码中文。
- 术语表（zh → en）：任务 task · 队列 queue · 转写 transcribe · 断句 sentence segmentation ·
  打轴/对齐 alignment · 人声分离 vocal separation · 字幕 subtitle · 分段时长 segment length ·
  量化 quantization(int8) · 推理后端 inference backend

## 发布流程

主分支只有 `master` 一个，也是 GitHub 上的默认分支，直接提交，不走过 PR 流程。
（历史上曾用 `wgpu` 作默认分支，两条线从未分叉，已快进合并并删除；
远端与本地都只剩 `master`。另有长期并行的 `feat/electron-ui` 分支，CI 的
`on.push.branches` 已包含它。）

> **两条线都能发版，但要各打各的 tag**：`master` 的 `release.yml` 仍打 GPUI 包；
> `feat/electron-ui` 的 `release.yml` 打 Electron 包（`pack-electron.*`）。
> 标签是全局的，不要用同一个版本号在两条线各发一次；Electron 线发版时先升
> `Cargo.toml` 与两份 README，再打一个只属于 Electron 版本的 tag。
>
> **v2.0.0 是 Electron 线的预览版**：`release.yml` 把它标成 prerelease，并且
> 不更新仓库的 "Latest" 标记（GitHub 首页仍指向稳定的 GPUI 版本）。其余 tag
> 自动按正式版发布。验证通过后可以在 GitHub 上手动取消 pre-release 标记转正。

版本号只有一个出处：`Cargo.toml` 的 `[workspace.package] version`（CLI 与日志都读它）。
`apps/electron/package.json` 的版本必须与它一致（`verify-version.py` 会断言；
electron-builder 用这个版本生成安装包和目录名）。
打包脚本不再接收、也不再写回版本号；tag 与两份 README 必须跟它一致，CI 负责断言。

1. 改 `Cargo.toml` 的 `[workspace.package] version`；若动了依赖，跑一次 `cargo check` 刷新 `Cargo.lock`
   （发布构建一律 `--locked`，锁文件没跟上的话 CI 与打包都会失败）
2. 更新 **两份** README 顶部的当前版本号与三平台下载链接：`README.md`（英文）与 `README.zh-CN.md`（中文），内容保持同构
3. **写发布说明** `docs/release-notes/v<版本>.md`（格式见下，这一步不能省）
4. 本地自检全绿再推（**与 ci.yml 的实际门禁逐条对齐**，少跑一条就等于给自己少一道）：
   `cargo fmt --all --check`、
   `cargo clippy --locked --workspace --all-targets -- -D warnings`、
   `cargo test --locked -p oneasr-core`、
   `cd apps/electron && npm run verify`、
   `python3 scripts/test_verify_i18n.py`、
   `python3 scripts/verify-version.py --tag v<版本>`、`python3 scripts/verify-i18n.py`；
   Electron 线还要跑 `cd apps/electron && npm ci`（打包脚本依赖已安装的 electron-builder）
5. `git commit` → 推当前线（`git push origin master` 或 `git push origin feat/electron-ui`）
   → `git tag v<版本>` → `git push origin v<版本>`
6. tag 触发当前线的 `.github/workflows/release.yml`：三平台打包 + 自动建 release 并挂上全部产物
   （打包前先跑同一个 `verify-version.py --tag`，tag 与 `Cargo.toml` 不一致就直接失败）
7. 验证：`gh release view v<版本>`，确认 5 个产物都在、说明正文正确

注意：`workflow_dispatch` 只出 artifact、**不建 release**；只有推 tag 才会发布。
CI 在 `push: master` / `push: feat/electron-ui` 与 PR 上跑，直接提交到主分支同样会拿到门禁结果。

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
  两个打包脚本（`pack-unix.sh` / `pack-electron.sh`）校验的正是这两个名字，缺一个就直接报错退出，这是有意设计。
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
  一行机器可读），GPUI 的 `pack-*`、Electron 的 `fetch-ffmpeg.mjs` 与 CI 的 `verify-ffmpeg-manifest.py` 都从那里读
- 已知待办：这批 ffmpeg 的 configure 行带 `--enable-nonfree`，FFmpeg 官方 LICENSE.md 写明这样的
  产物不可再分发。换 LGPL 构建**只动清单一个文件**（base-url + 哈希），不用改打包脚本
- `scripts/pack-release.ps1` 必须带 UTF-8 BOM：Windows PowerShell 5.1 没有 BOM 时按 ANSI 代码页
  读文件，中文**字符串**会被误解码并可能凭空多出一个引号把脚本解析搞崩（`release.yml` 用 pwsh，
  所以这个坑只在本地显形）。`.gitattributes` 里已登记，别把 BOM 去掉
- 依赖来源与漏洞由 `deny.toml` + CI 的 `cargo deny check advisories sources` 把关：
  git 依赖只许来自白名单里的仓库、且必须带 tag/rev。已知的 4 条「已停维护」通告都是传递依赖，
  记在配置注释里；真实漏洞仍然是硬错误
- 门禁的**输入也钉死**：漏洞库是 `vendor/advisory-db` 这个 submodule（钉在某个 commit），
  CI 与本地读同一份快照，上游 main 新增通告不会再把无关提交弄红（2026-10-16 的 zip yank
  与 10-17 的三条通告就是这么进来的）。跑检查前先铺缓存：
  `pwsh -File scripts/prepare-advisory-db.ps1`（CI 的 supply-chain job 自动调它）；
  首次 clone 需要 `git submodule update --init vendor/advisory-db`。更新公告库是**显式动作**
  （`git -C vendor/advisory-db fetch origin` → `checkout <新 commit>` → `git add`），
  **没有定时任务**，也不跟随上游
- runner 镜像同样写死版本（`ubuntu-24.04` / `windows-2022` / `macos-15`），不用 `*-latest`：
  2026-10-19 起 `ubuntu-latest` 迁往 26.04，同一份提交会换一套 glibc / apt 源
- 打包前确认 `cargo check --locked --workspace --all-targets` 是 0 警告——这件事现在由 CI 保证，不再靠人记

## 不要提交的东西

- `tmp/`、`forum-post*.md` —— 本地草稿与临时产物，**已在 `.gitignore` 里**，别用 `-f` 强加
- `models/`、`output/`、`runs/`、`release/`、`dist/`、`bin/` —— 权重、产物、中间文件（同样已忽略）
- 任何密钥：CI 不依赖任何 secret，需要 AI 能力时由使用者显式配置
