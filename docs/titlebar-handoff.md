# OneAsr 标题栏问题交接（给接手 AI）

> 本文只陈述现状、反馈、代码事实与已尝试记录。**不含**推荐方案或结论性判断。  
> 最后提交：`1e18f46`（`feat: allow custom SRT output directory in settings`）。  
> 标题栏相关试验代码已从工作区还原，当前 `main` 工作树无标题栏 WIP。

---

## 1. 产品与技术栈

| 项 | 内容 |
|----|------|
| 项目 | OneAsr — Windows 本地离线批量音视频转 SRT |
| GUI crate | `crates/oneasr`，二进制 `oneasr` |
| UI 框架 | **GPUI 0.2.2**（crates.io：`gpui = "0.2.2"`） |
| 系统目标 | Windows 10 / 11 x64 |
| 相关文档站 | https://gpui.rs/ |
| 社区标题栏示例讨论 | https://github.com/zed-industries/zed/discussions/45012 |
| 第三方组件实现参考 | https://longbridge.github.io/gpui-component/docs/components/title-bar （`gpui-component` 的 TitleBar 源码） |

---

## 2. 用户反馈（按时间）

### 2.1 早期（开发者未在本机复现）

- 系统：反馈方 **Windows 10**；开发者本机 **Windows 11** 与另一台 **Windows 10** 曾报告「一切正常」。
- 反馈内容：
  1. 窗口似乎不能按住标题栏移动；不能点关闭按钮关闭窗口。
  2. 容易闪退，尤其添加文件后；再点设置，操作几次总能遇到闪退。
- 关闭方式：任务栏图标右键 → 关闭。

### 2.2 后续更具体的标题栏描述

- 点标题栏会变灰。
- 没有：拖拽移动、最小化、最大化、关闭。
- 关闭只能：Windows 任务栏右键选关闭。

### 2.3 在本机尝试改标题栏之后（开发者本机）

- 「完全没用，我的电脑上都用不了了。」
- 再改之后：「可以点，但是全屏（最大化）就卡住，关都关不掉。」
- 再改之后：「依旧没用，全屏之后整个界面点哪都没用。」
- 要求：对照 https://gpui.rs/ 与最佳实践；随后要求**停止改代码、还原、写交接文档**。

---

## 3. 当前代码里窗口是怎么开的（已还原后的 HEAD）

文件：`crates/oneasr/src/main.rs` 的 `main()`。

- `WindowOptions`：
  - `window_bounds: Some(WindowBounds::Windowed(bounds))`，默认约 `860×560`。
  - `titlebar: Some(TitlebarOptions { title: Some("OneAsr - dabao005 - www.52pojie.cn"), ..Default::default() })`
  - 即 **`appears_transparent` 为默认 `false`**（`bool` 默认值）。
- **没有**应用内自绘标题栏。
- **没有**使用 `WindowControlArea`（Drag / Min / Max / Close）。
- 界面顶部是业务 **toolbar**（logo、添加、全部开始、设置等），不是系统 caption 控件。

GPUI 0.2.2 Windows 实现中（registry 源码，只作事实引用）：

- 建窗样式大致为 `WS_SYSMENU` +（可缩放时）`WS_THICKFRAME | WS_MAXIMIZEBOX` +（可最小化时）`WS_MINIMIZEBOX`；**未在建窗时显式设置 `WS_CAPTION`**。
- `hide_title_bar = titlebar.appears_transparent`（有 titlebar 时）；`titlebar` 为 `None` 时默认为 `true`。
- `appears_transparent == false` 时：不自定义 `WM_NCCALCSIZE` 客户区，hit-test 注释写「若 OS 画标题栏则不必处理」。
- `WindowControlArea` 在 `WM_NCHITTEST` 中映射为 `HTCAPTION` / `HTMINBUTTON` / `HTMAXBUTTON` / `HTCLOSE`。
- `start_window_move()` 在 Windows 平台 trait 默认实现为空；Linux 等有实现。
- 最大化相关：`zoom()` 在 Windows 上对可见窗口调用 `ShowWindowAsync(..., SW_MAXIMIZE)`；NC 路径上 min/max/close 在 `handle_nc_mouse_up_msg` 里用 `ShowWindowAsync` / `PostMessage(WM_CLOSE)`。

---

## 4. 试验过但已还原的改动（勿当作当前代码）

以下均**不在**当前 `HEAD` 工作树中，仅记录曾做过什么与现象。

### 4.1 试验 A：自绘标题栏 + `appears_transparent: true` + 仅 `WindowControlArea`

- 窗口：`TitlebarOptions { title, appears_transparent: true, ... }`
- 客户区顶部自绘条：拖拽区 `WindowControlArea::Drag`；按钮 `Min` / `Max` / `Close`，**无** `on_click`。
- 现象（开发者）：「完全没用 / 本机都用不了了」（具体是否含拖不动、点不动，以用户原话为准）。

### 4.2 试验 B：自绘标题栏 + 自写 Win32（`win_chrome`）

- 新增过 `crates/oneasr/src/win_chrome.rs`（已删除）。
- 拖动：`ReleaseCapture` + `PostMessage(WM_NCLBUTTONDOWN, HTCAPTION)`。
- 最小化 / 最大化：先用过同步 `ShowWindow(SW_*)`，后改成 `PostMessage(WM_SYSCOMMAND, SC_*)`；关闭用过 `remove_window` 与 `SC_CLOSE`/`WM_CLOSE`。
- 按钮用 GPUI `on_click` / 拖拽区 `on_mouse_down`。
- 现象（开发者）：
  - 按钮「可以点」；
  - **最大化（用户称「全屏」）后整窗卡住**，点哪里都没反应，关不掉；
  - 改成异步 `PostMessage` 后用户仍反馈：**最大化后整个界面点哪都没用**。

### 4.3 试验 C：准备改回「仅 WindowControlArea」官方示例形态

- 进行中被用户叫停；随后要求还原并写文档。
- 还原操作：`git checkout HEAD -- crates/oneasr/src/main.rs Cargo.lock crates/oneasr/Cargo.toml`，删除 `win_chrome.rs`。

---

## 5. 与标题栏无关、但同会话涉及的其它事实

### 5.1 已合并的功能（`1e18f46`）

- 设置项 **字幕输出目录** `settings.output_dir`，默认 `{app}/output`。
- GUI 设置面板可浏览选目录（交互类似模型目录）。
- CLI：`apply_app_root_paths` 将 `output_dir` 绑到 `--app-root/output`。
- `words-json`：SRT 先写，侧导出失败不阻断主产物。
- 相对路径 `output_dir` 在 `normalize` 中会相对 app root 绝对化。

### 5.2 本地 review 曾提到、未作为标题栏修复项

- `examples/run_pipeline.rs` 可能未绑定 `output_dir`（suggestion）。
- CLI 在 words-json 非致命失败时仍可能打印成功路径（nit）。

### 5.3 闪退反馈

- 有用户称：添加文件后点设置、操作几次易闪退。
- 开发者本机 Win10/Win11 **未复现**。
- 相关代码点（事实）：`add_files_dialog` / 选模型目录使用 `thread::spawn` + `rfd::FileDialog`。

### 5.4 工作区未跟踪内容

- `testdata/`（体积大，`.gitignore` 已忽略部分子目录）。
- `.review-tmp/`（本地 review 产物）。

---

## 6. 官方 / 社区材料中的相关写法（摘录事实，非推荐）

### 6.1 gpui.rs 站点

- Hello World 使用 `WindowOptions { window_bounds, ..Default::default() }`，未展示自定义标题栏。
- 指向 Zed 内 `crates/gpui` 文档与 examples：`window.rs`、`window_shadow.rs` 等。

### 6.2 Zed Discussion #45012

- 提问：自定义标题栏下 `zoom_window` 只能最大化不能还原；`window_control_area` 单独使用时除 Drag 外其它不灵；并混用了 `on_mouse_down` + `zoom_window`。
- 回复示例：自绘 titlebar，**Min/Max/Close 仅** `.window_control_area(...)`，**没有** `on_mouse_down`；Drag 区 `.window_control_area(WindowControlArea::Drag)`；窗口侧需能配合自定义栏（示例未完整贴 `WindowOptions`）。

### 6.3 gpui-component `TitleBar`（longbridge）

- `title_bar_options()`：`appears_transparent: true`。
- 注释大意：Windows 上控件**不需要**实现 click；点在 bounds 内由 window event 触发。
- Windows：控件 `.window_control_area(...)`。
- Linux：用 `on_click` 调 `minimize_window` / `zoom_window` / `remove_window`。
- 另有 `start_window_move` 等用于 Linux 客户端装饰路径。

---

## 7. 接手时建议先核对的文件

| 路径 | 说明 |
|------|------|
| `crates/oneasr/src/main.rs` | `open_window`、`Render` 根布局、toolbar |
| `crates/oneasr/Cargo.toml` | `gpui = "0.2.2"` |
| `%USERPROFILE%\.cargo\registry\src\*\gpui-0.2.2\src\platform\windows\window.rs` | 建窗样式、`hide_title_bar` |
| `%USERPROFILE%\.cargo\registry\src\*\gpui-0.2.2\src\platform\windows\events.rs` | `WM_NCHITTEST`、`WM_NCCALCSIZE`、NC 按钮 |
| `crates/oneasr-core/src/settings.rs` / `paths.rs` | 输出目录（与标题栏无关） |

---

## 8. 复现 / 验证时可用的命令

```powershell
# 开发运行
cargo run -p oneasr --release

# 产物
# target\release\oneasr.exe
```

验证项（与用户反馈对应）：

1. 能否拖动移动窗口  
2. 最小化 / 最大化（还原）/ 关闭是否可用  
3. 最大化后界面是否仍可点击、能否关闭  
4. （次要）添加文件 → 设置多次操作是否闪退  

---

## 9. 当前仓库状态（写文档时）

- 分支：`master`
- 最新提交：`1e18f46 feat: allow custom SRT output directory in settings`
- 标题栏试验：**已还原**，与 `1e18f46` 一致（无自绘标题栏、无 `win_chrome`）
- 未提交：仅 untracked `.review-tmp/`、`testdata/`（若本地仍在）

---

## 10. 用户明确要求（对接手方）

- 先不要继续在上一任基础上「判断式」乱改；看清问题。
- 需要对照 **https://gpui.rs/** 及 GPUI 体系内文档 / 示例 / 社区实现。
- 标题栏问题在部分用户与开发者改坏后的本机上均出现过严重表现（无法用系统 caption、或最大化后整窗无响应）。

---

## 11. 解决记录（2026-07-22）

### 根因（两层）

1. **GPUI 0.2.2 建窗从不带 `WS_CAPTION`**（`platform/windows/window.rs` 建窗样式只有
   `WS_SYSMENU | WS_THICKFRAME | WS_MAXIMIZEBOX | WS_MINIMIZEBOX`）。`appears_transparent: false`
   时 `WM_NCHITTEST` 直接交给 OS（注释：*"If the OS draws the title bar..."*）。所谓「系统标题栏」
   实际是 DWM 兜底画的，给不给、能不能用取决于机器环境（DWM/主题/GPU），所以同一台 Win10
   正常、另一台 Win10「标题栏一点就灰、不能拖/没有按钮」。
2. **自绘标题栏 + `WindowControlArea` 在本机也无效**的根因：根节点 `track_focus` 的
   MouseDown 监听器每次点击都 `prevent_default()`（div.rs 焦点转移逻辑），而 GPUI 0.2.2 的
   `handle_nc_mouse_down_msg` 把 `default_prevented` 当作「应用已处理」直接吞掉
   `WM_NCLBUTTONDOWN`，`DefWindowProc` 收不到 → 拖拽、最小化/最大化/关闭、边框缩放全死。
   **上游已修**：Zed PR **#48330**（去掉 `|| result.default_prevented`），但 crates.io 最新
   0.2.2 未包含。

### 修复内容

- `Cargo.toml`：`[patch.crates-io] gpui = { path = "vendor/gpui" }`，`vendor/gpui` 为 0.2.2
  源码 + #48330 一行修复（`events.rs` `handle_nc_mouse_down_msg`，带注释）。上游发新版后
  可还原。
- `crates/oneasr/src/main.rs`：`TitlebarOptions.appears_transparent: true`；新增
  `render_titlebar()`（拖拽区 + 最小/最大/关闭按钮，**仅** `WindowControlArea`，无 `on_click`）
  与 `caption_btn()`。双击拖拽区可切换最大化（DefWindowProc 行为）。
- `crates/oneasr/src/assets.rs` + `assets/icons/win-{min,max,close}.svg`：三个 caption 图标。
- 试验 B 的「最大化后卡死」未再出现（当时为自写 Win32 与 GPUI 事件循环冲突；现纯走 GPUI 路径）。

### 本机自动化验证（Win11，真实输入注入）

拖拽位移精确 +150/+100；最小化 `IsIconic=True`；最大化 `IsZoomed=True`；最大化后 UI 仍可
交互并成功还原；点击关闭进程退出。`WM_NCHITTEST` 在四个区域分别返回
`HTCAPTION/HTMINBUTTON/HTMAXBUTTON/HTCLOSE`（测试脚本在 `target/tmp/`，不入库）。

### 待用户侧确认

- 反馈用户的 Win10 机器上验证同样五项（理论上已不再依赖 DWM caption，与环境无关）。
- 「添加文件后易闪退」为另一独立问题，本次未涉及。
