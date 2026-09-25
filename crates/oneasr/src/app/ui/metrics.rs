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
/// Actions column: play + delete (+ open/copy when done).
pub(crate) const ACTIONS_COL_PX: f32 = 120.;
/// Language chip column (fixed so status changes never shove it).
pub(crate) const LANG_COL_PX: f32 = 64.;
/// Per-task vocal-separation toggle column (right of the language chip).
pub(crate) const SEPARATE_COL_PX: f32 = 76.;
/// Status pill column — wide enough for「人声分离 1800/1800」「转写中 99/99」.
pub(crate) const STATUS_COL_PX: f32 = 132.;
/// List language menu width (absolute panel under the chip).
pub(crate) const LANG_MENU_W: f32 = 168.;
/// Floating language menu max height before it scrolls.
pub(crate) const LANG_MENU_MAX_H: f32 = 280.;
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
