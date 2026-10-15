//! Shared UI building blocks: buttons, icons, tooltips, popovers, install rows.
//!
//! Split by kind; the public paths are re-exported below so callers keep using
//! `crate::widgets::*`.

mod button;
mod install_row;
mod logo;
mod popover;
mod switch;
mod text_field;

pub use button::{
    BtnKind, CaptionAction, IconKind, NameTooltip, btn, btn_cta, caption_btn, icon_btn, pill,
    settings_gear_btn, ui_language_btn,
};
pub use install_row::{ComponentRow, model_download_row, model_path_bar};
pub use logo::app_logo;
pub use popover::{
    floating_hotword_menu, floating_lang_menu, popover_dismiss_layer, popover_menu_shadow,
    timing_breakdown_popover,
};
pub use switch::switch;
pub use text_field::{FieldKind, Rows, TextField};
