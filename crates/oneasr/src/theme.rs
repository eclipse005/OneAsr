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
/// Soft row hover / zebra
pub const ROW_HOVER: Rgba = rgba_hex(0xf8fafc);
/// Danger
pub const DANGER: Rgba = rgba_hex(0xdc2626);
/// Danger soft
pub const DANGER_SOFT: Rgba = rgba_hex(0xfee2e2);
/// Warning / loading
pub const WARN: Rgba = rgba_hex(0xd97706);
/// Logo mark fill
pub const LOGO: Rgba = rgba_hex(0x0f766e);
