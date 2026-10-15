//! Layout metrics for the window: drawer, row columns, menus and popovers.
//!
//! Values are kept in one place so the list, the settings drawer and the
//! floating menus cannot drift apart. Read by both the render methods and the
//! animation state in [`crate::app`].

/// Settings drawer width (overlay, does not shrink the list). Wide enough that
/// a model path bar + its download button share one row without wrapping.
pub(crate) const SETTINGS_W: f32 = 450.;
pub(crate) const DRAWER_ANIM_SECS: f32 = 0.34;
/// Row add fade-in duration (seconds).
pub(crate) const ROW_ENTER_SECS: f32 = 0.30;
/// Row delete / clear fade-out duration (seconds).
pub(crate) const ROW_EXIT_SECS: f32 = 0.22;
// ── 任务行控制区列宽 ──────────────────────────────────────────────────────
//
// 这些列宽曾经普遍比内容宽出 20~50px，于是每颗 chip 坐在自己那一格的正中间，
// 左右各空一截——列与列之间看上去隔了 60px，而控件本身只有 45px。收进一个紧凑
// flex 只解决了「控件之间」那一半，**每列内部那圈空白还在**。下面每个数字都按
// 「实际内容 + 少量余量」重新算过（`text_xs` = 12px，2 个汉字 ≈ 24px，
// 药丸内边距 px_2 = 8×2 = 16px）。
//
// 状态药丸曾经也在这一组里，用一列 132px 的固定宽度去对抗抖动（「待处理」
// →「人声分离 1800/1800」时不至于把右边整排控件推走）。代价是空闲时那列左右
// 各空 40px，视觉间距 46px，夹在 10~14px 的邻居中间格外刺眼；而且按中文最长
// 标签预留的宽度，英文 "Separating vocals 1800/1800" 反而装不下会溢出去。
// 它已经搬到左侧信息行（那边本来就随内容伸缩，抖动无从发生），这里只留下限
// 宽度 `STATUS_PILL_MAX_W`。

/// Actions column: play + delete. `icon_btn` 是 32×32，两颗 + gap_2(8) = 72，
/// 刚好。再宽就纯粹是空白。
pub(crate) const ACTIONS_COL_PX: f32 = 72.;

/// Language chip column (fixed so switching language on one row never shoves
/// the chips behind it).
/// 最宽的短标签是「中文」/「粵語」两个汉字：24 + 4(间距) + 8(箭头) + 16(内边距) = 52。
pub(crate) const LANG_COL_PX: f32 = 56.;

/// 任务行「热词组」chip 列（语言 chip 右侧）的**下限**。
///
/// 这一列的内容是用户自己起的组名，长度不可预测，所以它**跟着内容走**，这里
/// 只给一个下限，把短组名的行对齐、把整排控件的基线稳住。
///
/// 设成固定宽度行不通：未绑定的标签是「热词：无」——4 个全角字 ≈ 48px，加
/// 箭头与药丸内边距约 77px。任何 ≤77px 的固定值都会把它切掉；而更大的固定值
/// 又会在组名很短时留出一截空白（chip 被拉满整列，右边空着——这正是之前
/// 那版看着「这么宽」的原因）。
pub(crate) const HOTWORD_COL_MIN_PX: f32 = 64.;

/// 同一列的**上限**：组名再长也不许把整排控件往左顶。扣掉药丸内边距约 122px，
/// 够放 10 个汉字或 20 个拉丁字符，再长就省略（完整名字在 tooltip 与下拉里）。
pub(crate) const HOTWORD_COL_MAX_PX: f32 = 140.;

/// 抽屉里那排热词组 chip 的单颗宽度上限。抽屉宽 450px，chip 是 `flex_wrap`
/// 排布的：一个失控的组名会把整颗撑到抽屉外，把右边框顶出可视区。留 260px
/// 还能一行并排两三颗，又不至于被一条超长名字带偏。
pub(crate) const HOTWORD_GROUP_CHIP_MAX_W: f32 = 260.;

/// Per-task vocal-separation toggle column (right of the language chip).
/// 「分离」两字 + 内边距 ≈ 46。
pub(crate) const SEPARATE_COL_PX: f32 = 56.;

/// Transcript chip column, between the separation chip and the actions: it is a
/// one-button two-state control, and it sits next to the start button whose
/// behaviour it decides. Just wide enough for「+ 文稿」— the counts live in the
/// card, not on the chip.
pub(crate) const TRANSCRIPT_COL_PX: f32 = 60.;
/// Hover-to-open delay for the transcript card. Same feel as the 用时 card: long
/// enough that sweeping the pointer across the row does not flash it.
pub(crate) const TRANSCRIPT_HOVER_DELAY_MS: u64 = 420;
/// Grace after leaving the chip or card. Generous, because the pointer has to
/// cross the gap between the two.
pub(crate) const TRANSCRIPT_LEVER_GRACE_MS: u64 = 260;
/// The card's fade/rise duration.
pub(crate) const TRANSCRIPT_CARD_ANIM_SECS: f32 = 0.18;
/// The card's width. Anchored to the chip's **right** edge and growing leftward,
/// because the chip sits in the right-hand control cluster — a left-anchored
/// card would run off the window.
pub(crate) const TRANSCRIPT_CARD_W: f32 = 280.;
/// 状态药丸的宽度**上限**（不是固定列宽）。
///
/// 药丸原本是控制簇里一列 132px 的固定宽度，按最长标签「人声分离 1800/1800」
/// 预留。现在它住在左侧信息行、宽随内容走，就不再需要预留——但带 `N/M` 计数的
/// 阶段标签长短不定（`asr.rs` 里带计数的是 Separating / Transcribing / Aligning），
/// 所以留一个上限兜底：超出就省略，而不是把文件名列挤没。
///
/// 180 的来历：英文最长的 "Separating vocals 1800/1800" 实测约 155px，加上药丸
/// 内边距 16px ≈ 171px——刚好放得下，于是这条上限在英文下是**保险**而不是常态
/// 截断。中文最长「人声分离 1800/1800」约 111px + 16 = 127px，宽裕。
pub(crate) const STATUS_PILL_MAX_W: f32 = 180.;
/// List language menu width (absolute panel under the chip).
pub(crate) const LANG_MENU_W: f32 = 168.;
/// Floating language menu max height before it scrolls.
pub(crate) const LANG_MENU_MAX_H: f32 = 280.;
/// 任务行「热词组」下拉宽度（组名可能比语言名长）。
pub(crate) const HOTWORD_MENU_W: f32 = 200.;
/// Processing-time popover enter/exit (seconds).
pub(crate) const TIMING_POP_ANIM_SECS: f32 = 0.18;
/// Hover delay before opening the timing card (ms).
pub(crate) const TIMING_HOVER_DELAY_MS: u64 = 100;
/// Leave grace so pointer can travel chip → card without flicker (ms).
pub(crate) const TIMING_LEAVE_DELAY_MS: u64 = 120;
/// Timing breakdown card width.
pub(crate) const TIMING_POP_W: f32 = 220.;

// ── Popover select layer model (GPUI has no built-in Select) ─────────────────
// Two deferred layers, sorted by priority (higher paints + hit-tests on top):
//
//   MENU_DISMISS_Z  full-window scrim on the app root, `occlude`
//                   → click-outside closes (toolbar / list / status bar)
//   MENU_Z          floating panel, `occlude`
//                   → owns hits above the scrim so option `on_click` completes
//
// Deferred escapes the list's overflow stack. `occlude` is required so the
// scrim is not also "hovered" under the panel (GPUI hit-test walks every
// Normal hitbox under the cursor until it hits BlockMouse).
//
// Invariant: `lang_menu` / `settings_lang_open` only count as "open" when a
// panel can actually render (`task_lang_menu_active` / settings drawer). Scrim
// visibility follows that derived state, never a stale flag alone.
pub(crate) const MENU_DISMISS_Z: usize = 5;
pub(crate) const MENU_Z: usize = 10;

// ── 指南面板（首启欢迎卡 / 状态栏「指南」chip）──────────────────────────────
// 与统计面板同一套浮层：MENU_Z + 全窗 dismiss 层，点击面板外 / Escape 收起，
// 非模态、不挡下面的界面。与统计面板互斥展开（同一块位置）。
/// 面板宽：中文三步文案一行放得下（380 − 内边距 32 − 序号徽 26 ≈ 322px）。
pub(crate) const WELCOME_PANEL_W: f32 = 380.;
