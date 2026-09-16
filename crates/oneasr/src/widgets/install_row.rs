//! Model download / component install rows and their action button.

use gpui::{
    div, prelude::*, App, ClickEvent, Window,
};
use oneasr_core::{DownloadProgress, DownloadState, ModelKind};

/// The live state of one download / install row.
///
/// Kept as a single value because these five always come from the same probe
/// and progress snapshot and always travel to the same builder — five
/// positional arguments never said which boolean was which.
pub struct ComponentRow<'a> {
    /// Element id of the action button.
    pub id: &'static str,
    /// Element id of the cancel button, shown while busy.
    pub cancel_id: &'static str,
    /// Installed / downloaded and ready to use.
    pub ready: bool,
    /// A download is in flight.
    pub busy: bool,
    /// Live progress, once a download has reported.
    pub progress: Option<&'a DownloadProgress>,
}

/// Compact download progress bar under a model path field.
pub fn model_download_row(
    row: ComponentRow<'_>,
    kind: ModelKind,
    on_download: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    download_action_row(
        row,
        ActionCopy {
            ready_status: "已就绪",
            idle_status: "未下载",
            busy_btn: "下载中…",
            idle_btn: "下载模型",
            ready_btn: "重新下载",
            kind,
        },
        on_download,
        on_cancel,
    )
}

/// CUDA / native runtime components → `{exe}/dll/`, button「安装组件」.
pub fn component_install_row(
    row: ComponentRow<'_>,
    on_install: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    download_action_row(
        row,
        ActionCopy {
            ready_status: "已安装",
            idle_status: "未安装",
            busy_btn: "安装中…",
            idle_btn: "安装组件",
            ready_btn: "重新安装",
            kind: ModelKind::CudaRuntime,
        },
        on_install,
        on_cancel,
    )
}

struct ActionCopy {
    ready_status: &'static str,
    idle_status: &'static str,
    busy_btn: &'static str,
    idle_btn: &'static str,
    ready_btn: &'static str,
    kind: ModelKind,
}

fn download_action_row(
    row: ComponentRow<'_>,
    copy: ActionCopy,
    on_action: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    // Short status only — no filesystem path (model path is already above).
    let status_text = if let Some(p) = row.progress {
        p.label_for_kind(copy.kind)
    } else if row.ready {
        copy.ready_status.to_string()
    } else {
        copy.idle_status.to_string()
    };
    let status_color = if row.ready && !row.busy {
        crate::theme::ACCENT
    } else if row.busy {
        crate::theme::WARN
    } else if row.progress.is_some_and(|p| p.state == DownloadState::Failed) {
        crate::theme::DANGER
    } else {
        crate::theme::MUTED
    };
    let busy_label = copy.busy_btn;
    let action_label = if row.ready { copy.ready_btn } else { copy.idle_btn };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(status_color)
                .whitespace_normal()
                .line_clamp(2)
                .child(status_text),
        )
        .child(
            div().flex().items_center().gap_1p5().children({
                let mut kids: Vec<gpui::AnyElement> = Vec::new();
                if row.busy {
                    kids.push(
                        div()
                            .id(row.cancel_id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(crate::theme::LINE)
                            .bg(crate::theme::BG)
                            .text_xs()
                            .text_color(crate::theme::MUTED)
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(crate::theme::DANGER_SOFT)
                                    .text_color(crate::theme::DANGER)
                                    .border_color(crate::theme::DANGER)
                            })
                            .on_click(on_cancel)
                            .child("取消")
                            .into_any_element(),
                    );
                    kids.push(
                        div()
                            .id(row.id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(crate::theme::LINE)
                            .bg(crate::theme::BG)
                            .text_xs()
                            .text_color(crate::theme::MUTED)
                            .opacity(0.7)
                            .child(busy_label)
                            .into_any_element(),
                    );
                } else {
                    kids.push(
                        div()
                            .id(row.id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(crate::theme::ACCENT)
                            .bg(crate::theme::ACCENT_SOFT)
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(crate::theme::ACCENT)
                            .cursor_pointer()
                            .hover(|s| s.bg(crate::theme::ACCENT).text_color(gpui::rgb(0xffffff)))
                            .on_click(on_action)
                            .child(action_label)
                            .into_any_element(),
                    );
                }
                kids
            }),
        )
}
