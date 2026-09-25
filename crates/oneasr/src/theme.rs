//! Light theme tokens for OneAsr.

use gpui::Rgba;

const fn rgba_hex(hex: u32) -> Rgba {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    Rgba { r, g, b, a: 1.0 }
}

/// App background
pub const BG: Rgba = rgba_hex(0xf4f6f8);
/// Cards / header / panels
pub const PANEL: Rgba = rgba_hex(0xffffff);
/// Borders
pub const LINE: Rgba = rgba_hex(0xe2e8f0);
/// Softer border for inputs (less harsh on white)
pub const LINE_SOFT: Rgba = rgba_hex(0xecf0f4);
/// Primary text
pub const TEXT: Rgba = rgba_hex(0x0f172a);
/// Secondary text
pub const MUTED: Rgba = rgba_hex(0x64748b);
/// Disabled / very soft secondary (still readable on white)
pub const MUTED_SOFT: Rgba = rgba_hex(0x94a3b8);
/// Brand / success
pub const ACCENT: Rgba = rgba_hex(0x0d9488);
/// Accent soft fill
pub const ACCENT_SOFT: Rgba = rgba_hex(0xccfbf1);
/// Very soft accent for empty-state / drop zone icons
pub const ACCENT_MIST: Rgba = rgba_hex(0xe6fffa);
/// Cool slate plate for media icons (less “toy” than accent mist)
pub const MEDIA_PLATE: Rgba = rgba_hex(0xf1f5f9);
/// Soft row hover
pub const ROW_HOVER: Rgba = rgba_hex(0xf8fafc);
/// Alternating list row
pub const ZEBRA: Rgba = rgba_hex(0xfafbfc);
/// Danger
pub const DANGER: Rgba = rgba_hex(0xdc2626);
/// Danger soft
pub const DANGER_SOFT: Rgba = rgba_hex(0xfee2e2);
/// Warning / loading
pub const WARN: Rgba = rgba_hex(0xd97706);
/// Warning soft fill (status pill / hint)
pub const WARN_SOFT: Rgba = rgba_hex(0xfff7ed);
/// Logo mark fill
pub const LOGO: Rgba = rgba_hex(0x0f766e);
/// 滑动开关关闭态轨道：介于 LINE 与 MUTED_SOFT 之间的冷灰，白卡片上一眼可辨。
pub const SWITCH_OFF: Rgba = rgba_hex(0xcbd5e1);
/// 滑动开关关闭态悬停：再暗一档，让"这里可以点"更明显。
pub const SWITCH_OFF_HOVER: Rgba = rgba_hex(0xb6c2d2);

// ─── stats year-grid intensity ramp ─────────────────────────────────
//
// Four levels plus an empty swatch, all absolute (see `oneasr_core::stats`):
// a shade that means a different duration on every screen would be a lie, so
// these are picked once and never normalised against the current maximum.

/// A day outside the window the ledger can speak about — before it began, or
/// still ahead. Deliberately lighter than [`STATS_L0`] so the three states stay
/// legible at 7px: *not yours yet* (this), *yours but idle* (`L0`), *worked*.
/// Without it the year frame is white space and reads as a panel that failed to
/// draw; with it the frame reads as the canvas the year fills in.
pub const STATS_GHOST: Rgba = rgba_hex(0xf1f5f9);
/// Nothing recorded on a day inside the rendered window. Dark enough to read
/// as a cell on the white panel (GitHub's empty swatch is `#ebedf0`); `#f1f5f9`
/// here was invisible against `#ffffff` — it is the frame colour above instead.
pub const STATS_L0: Rgba = rgba_hex(0xe2e8f0);
/// Level 1 — up to 15 minutes.
pub const STATS_L1: Rgba = rgba_hex(0xccfbf1);
/// Level 2 — up to 1 hour.
pub const STATS_L2: Rgba = rgba_hex(0x99f6e4);
/// Level 3 — up to 3 hours.
pub const STATS_L3: Rgba = rgba_hex(0x2dd4bf);
/// Level 4 — 3 hours or more.
pub const STATS_L4: Rgba = rgba_hex(0x0d9488);
