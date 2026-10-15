//! Model download rows: path bar + icon action button.
//!
//! 下载中时进度直接覆盖在路径栏上（同一块区域）：青色填充 + 百分比 / 速度
//! 暂时代替路径文字，下载完成自动变回路径——零位移、零挤压，卡片高度恒定。
//! 三态按钮共用同一中性样式（白底灰边灰图标），区别只在图标与提示：
//! - 未安装：下载图标（箭头 + 托盘）
//! - 已安装：重新下载（未闭环圆圈箭头）
//! - 下载中：停止图标，点击取消

use gpui::{
    App, ClickEvent, ClipboardItem, SharedString, Window, div, prelude::*, px, relative, svg,
};
use oneasr_core::DownloadProgress;

use super::button::NameTooltip;
use crate::i18n::L;
use crate::theme::{ACCENT, ACCENT_SOFT, LINE, LINE_SOFT, MUTED, PANEL, TEXT};

/// 一条路径栏：左边可点的路径文字（**点击即复制**），右边「选文件夹」按钮。
///
/// 这块东西原本在设置抽屉里抄了四遍、热词页第五遍，连 id / 文字 / 回调都只差几个
/// 字符。抄五份的代价不是行数，是**行为**：改一次复制交互要改五处，漏一处就出现
/// 「ASR 那行能复制、Demucs 那行不能」。所以抽成一个构件，复制只写一次。
///
/// 点击复制而不是加「在资源管理器中打开」：前者跨平台、跨 RDP（远程时本机
/// 资源管理器根本开不了）、而且用户自己粘到地址栏里也就三步。后者只省两步，
/// 换来三平台各一条命令和一条只在桌面端有效的路径。
pub fn model_path_bar(
    // 元素 id 前缀（同一窗内唯一）。
    id: &'static str,
    // 完整路径（复制的就是它，不是显示被截断的那一截）。
    path: SharedString,
    // 悬停提示：完整路径 + 「点击复制」两行。
    tip: SharedString,
    // 最多显示几行（输出目录会长，模型目录一行够）。
    lines: usize,
    // 变暗（输出目录在「保存到源文件旁边」打开时不用）。
    dimmed: bool,
    // 复制之后的回执提示。构件只管写剪贴板（那是通用的），提示是界面自己的事，
    // 由调用方接上——这样这个构件不必知道 OneAsrApp 的存在。
    on_copy: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_browse: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let copy_path = path.clone();
    let path_id: SharedString = format!("{id}-path").into();
    let browse_id: SharedString = format!("{id}-browse").into();

    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(LINE)
        .bg(PANEL)
        .overflow_hidden()
        .opacity(if dimmed { 0.45 } else { 1.0 })
        .hover(|s| s.border_color(ACCENT))
        .child(
            div()
                .id(path_id)
                .flex_1()
                .min_w_0()
                .px_2p5()
                .py_1p5()
                .text_xs()
                .text_color(TEXT)
                .whitespace_normal()
                .line_clamp(lines)
                .cursor_pointer()
                // 点击复制：本地 AI 玩家拿到权重目录的第一件事就是自己往里拷文件，
                // 让用户把路径抄走比让他记住在哪看更省事。
                .on_click(move |click, window, app| {
                    app.write_to_clipboard(ClipboardItem::new_string(copy_path.to_string()));
                    on_copy(click, window, app);
                })
                .child(path)
                .tooltip(move |_, cx| cx.new(|_| NameTooltip { text: tip.clone() }).into()),
        )
        .child(
            div()
                .id(browse_id)
                .flex_shrink_0()
                .px_2p5()
                .py_1p5()
                .border_l_1()
                .border_color(LINE_SOFT)
                .cursor_pointer()
                .hover(|s| s.bg(ACCENT_SOFT))
                .child(
                    svg()
                        .size(px(15.))
                        .path("icons/folder.svg")
                        .text_color(MUTED),
                )
                .on_click(on_browse),
        )
}

/// The live state of one download / install row.
///
/// Kept as a single value because these always come from the same probe
/// and progress snapshot and always travel to the same builder.
pub struct ComponentRow<'a> {
    /// Element id of the action button.
    pub id: &'static str,
    /// Installed / downloaded and ready to use.
    pub ready: bool,
    /// A download is in flight.
    pub busy: bool,
    /// Live progress, once a download has reported.
    pub progress: Option<&'a DownloadProgress>,
}

/// 路径栏 + 图标动作按钮一行；下载中路径栏原位变为进度显示。
pub fn model_download_row(
    row: ComponentRow<'_>,
    path_bar: impl IntoElement,
    on_download: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let busy = row.busy;
    let on_click = move |click: &ClickEvent, window: &mut Window, app: &mut App| {
        if busy {
            on_cancel(click, window, app);
        } else {
            on_download(click, window, app);
        }
    };

    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .relative()
                .child(path_bar)
                // 下载中：路径栏原位覆盖进度（同尺寸同圆角，卡片高度恒定）。
                // 填充块自带与路径组件一致的圆角，边缘不会露出直角。
                .when(busy, |el| {
                    let (frac, speed) = progress_of(row.progress);
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_md()
                            .bg(crate::theme::PANEL)
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left_0()
                                    .rounded_md()
                                    .bg(crate::theme::ACCENT_SOFT)
                                    .w(relative(frac.clamp(0.0, 1.0))),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .px_2p5()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(crate::theme::ACCENT)
                                            .child(format!("{:.0}%", frac * 100.0)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(crate::theme::MUTED)
                                            .child(speed),
                                    ),
                            ),
                    )
                }),
        )
        .child(download_button(&row, on_click))
}

fn progress_of(progress: Option<&DownloadProgress>) -> (f32, String) {
    match progress.filter(|p| p.total_bytes > 0) {
        Some(p) => {
            let frac = p.downloaded_bytes as f32 / p.total_bytes as f32;
            (frac, fmt_speed(p.speed_bytes_per_sec))
        }
        None => (0.0, crate::i18n::t(L::PREPARING).to_string()),
    }
}

/// 三态图标按钮：统一样式，图标与提示不同；下载中悬停转红色取消态。
fn download_button(
    row: &ComponentRow<'_>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (tip, icon) = if row.busy {
        (crate::i18n::t(L::TIP_CANCEL_DOWNLOAD), "icons/stop.svg")
    } else if row.ready {
        (crate::i18n::t(L::TIP_REDOWNLOAD), "icons/redownload.svg")
    } else {
        (crate::i18n::t(L::TIP_DOWNLOAD_MODEL), "icons/download.svg")
    };
    let tip: SharedString = tip.into();
    let icon: SharedString = icon.into();
    // State-suffixed ids so gpui remounts when the icon swaps.
    let element_id: SharedString = if row.busy {
        format!("{}-cancel", row.id).into()
    } else if row.ready {
        format!("{}-re", row.id).into()
    } else {
        format!("{}-dl", row.id).into()
    };
    let hover_group: SharedString = format!("{}-hover", row.id).into();

    div()
        .id(element_id)
        .size(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(crate::theme::PANEL)
        .border_1()
        .border_color(crate::theme::LINE)
        .cursor_pointer()
        .group(hover_group.clone())
        .tooltip(move |_, cx| cx.new(|_| NameTooltip { text: tip.clone() }).into())
        .hover(|s| {
            if row.busy {
                s.bg(crate::theme::DANGER_SOFT)
                    .border_color(crate::theme::DANGER)
            } else {
                s.bg(crate::theme::ACCENT_SOFT)
                    .border_color(crate::theme::ACCENT)
            }
        })
        .child(
            svg()
                .size(px(15.))
                .path(icon)
                .text_color(crate::theme::MUTED)
                .group_hover(hover_group, |s| {
                    if row.busy {
                        s.text_color(crate::theme::DANGER)
                    } else {
                        s.text_color(crate::theme::ACCENT)
                    }
                }),
        )
        .on_click(on_click)
}

fn fmt_speed(bps: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let x = bps as f64;
    if x >= MB {
        format!("{:.1} MB/s", x / MB)
    } else if x >= KB {
        format!("{:.0} KB/s", x / KB)
    } else if bps > 0 {
        format!("{bps} B/s")
    } else {
        crate::i18n::t(L::PREPARING).to_string()
    }
}
