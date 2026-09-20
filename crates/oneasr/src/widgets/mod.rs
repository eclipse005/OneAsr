//! Shared UI building blocks: buttons, icons, tooltips, popovers, install rows.
//!
//! Split by kind; the public paths are re-exported below so callers keep using
//! `crate::widgets::*`.

mod button;
mod install_row;
mod logo;
mod popover;

pub use button::{
    BtnKind, IconKind, NameTooltip, btn, btn_cta, caption_btn, icon_btn, pill,
    settings_gear_btn,
};
pub use install_row::{ComponentRow, model_download_row};
pub use logo::app_logo;
pub use popover::{
    floating_lang_menu, popover_dismiss_layer, popover_menu_shadow,
    timing_breakdown_popover,
};
