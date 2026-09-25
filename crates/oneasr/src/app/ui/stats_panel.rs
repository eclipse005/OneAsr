//! The floating stats panel: the assembled panel plus the element builders it
//! is made of.
//!
//! All the arithmetic and the wording live in [`super::stats_grid`]; this file
//! only turns those values into a tree.

use crate::app::prelude::*;
use crate::app::ui::stats_grid::*;
use crate::app::OneAsrApp;

impl OneAsrApp {
    /// Floating stats panel, anchored above the status bar (`MENU_Z`).
    ///
    /// Purely additive: nothing here touches task state. Every number comes
    /// from the cached [`StatsSummary`], refreshed on open and on each finished
    /// task — never per frame.
    pub(super) fn render_stats_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let stats = &self.stats;
        let saved = stats.saved_sec();
        let speed = stats.avg_speed();
        let lang = ui_lang();

        let today = crashlog::local_day_ymd();
        let range_start = stats_range_start(stats, &today);
        let year: i64 = today[..4].parse().unwrap_or(0);
        let grid = build_stats_year_grid(
            year,
            &range_start,
            &today,
            &stats.per_day,
            self.stats_hover_day.as_deref(),
        );
        // Hover card payload, resolved once here instead of per cell.
        let hover = self
            .stats_hover_day
            .as_deref()
            .map(|day| stats_hover_text(stats, day, lang));
        let early_caption = stats_early_caption(stats, &today, &range_start, lang);

        let mut panel = div()
            .absolute()
            .left(px(14.))
            .bottom(px(34.))
            .w(px(STATS_PANEL_W))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(LINE)
            .bg(PANEL)
            .shadow(popover_menu_shadow())
            .occlude()
            .child(
                div()
                    .text_sm()
                    .text_color(TEXT)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(t(L::STATS)),
            );

        if stats.is_empty() {
            // An empty panel must not be a wall of zeros — that reads as an
            // accusation, not an invitation.
            panel = panel.child(
                div()
                    .py_2()
                    .text_xs()
                    .text_color(MUTED)
                    .child(t(L::STATS_EMPTY)),
            );
        } else {
            panel = panel
                .child(stats_hero(saved, lang))
                .child(stats_totals(stats, speed, lang))
                .child(stats_year_section(&grid, hover, early_caption, cx))
                .child(stats_rows_section(stats, lang));
        }

        deferred(panel).with_priority(MENU_Z)
    }
}

// ── Element builders (free functions: none of them need app state) ───────────

/// Hero line: the cumulative duration saved, and what it buys.
///
/// Cumulative by design and never following a time filter — the accumulation IS
/// the value — and there is no "0 分" until a claim can actually be made.
fn stats_hero(saved: Option<f64>, lang: UiLang) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(MUTED)
                .child(t(L::STATS_SAVED_TOTAL)),
        )
        .child(
            div()
                .text_size(px(20.))
                .text_color(TEXT)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(match saved {
                    Some(s) => oneasr_core::stats::format_span_secs(lang, s),
                    None => "—".to_string(),
                }),
        )
        // The number is a duration; this is what it buys. Kept to the unit the
        // user can feel, never a second figure.
        .when_some(saved.and_then(|s| saved_tale(s, lang)), |el, tale| {
            el.child(div().text_xs().text_color(MUTED_SOFT).child(tale))
        })
}

/// The three aggregate columns under the hero.
fn stats_totals(stats: &StatsSummary, speed: Option<f64>, lang: UiLang) -> impl IntoElement {
    div().flex().gap_3().child(
        div()
            .flex()
            .flex_1()
            .gap_3()
            .child(stats_metric(
                t(L::STATS_MEDIA_TOTAL),
                if stats.media_sec > 0.0 {
                    oneasr_core::stats::format_span_secs(lang, stats.media_sec)
                } else {
                    "—".into()
                },
            ))
            .child(stats_metric(
                t(L::STATS_PROCESS_TOTAL),
                if stats.process_ms > 0 {
                    oneasr_core::stats::format_span_secs(lang, stats.process_ms as f64 / 1000.0)
                } else {
                    "—".into()
                },
            ))
            .child(stats_metric(
                t(L::STATS_AVG_SPEED),
                speed.map_or_else(|| "—".into(), crate::i18n::rtfx),
            )),
    )
}

/// The heat grid: month labels, day cells, one shared hover card, and the
/// "where the record starts" caption.
fn stats_year_section(
    grid: &StatsYearGrid,
    hover: Option<String>,
    early_caption: Option<String>,
    cx: &mut Context<OneAsrApp>,
) -> impl IntoElement + use<> {
    let mut month_labels: Vec<AnyElement> = Vec::with_capacity(grid.month_cols.len());
    for (m, first_col) in &grid.month_cols {
        month_labels.push(
            div()
                .absolute()
                .left(px(*first_col as f32 * (STATS_CELL + STATS_GAP)))
                .text_xs()
                .text_color(MUTED)
                .whitespace_nowrap()
                .child(crate::i18n::month_label(*m))
                .into_any_element(),
        );
    }

    let mut cells: Vec<AnyElement> = Vec::with_capacity(grid.cells.len());
    for cell in &grid.cells {
        // Frame vs data: a day the ledger cannot speak about still holds its
        // place in the grid, but it never claims "nothing was done".
        let color = if !cell.in_range {
            STATS_GHOST
        } else if cell.media_sec > 0.0 {
            stats_level_color(cell.media_sec)
        } else {
            STATS_L0
        };
        let el = div()
            .absolute()
            .left(px(cell.col as f32 * (STATS_CELL + STATS_GAP)))
            .top(px(cell.row as f32 * (STATS_CELL + STATS_GAP)))
            .size(px(STATS_CELL))
            // Explicit radius: `rounded_sm` resolves larger than half of a 7px
            // box, which turns the cell into a circle — dot-matrix, not GitHub.
            .rounded(px(1.5))
            .bg(color);
        cells.push(if cell.in_range {
            let day = cell.day.clone();
            el
                // Stateful: on_hover in this gpui lives on stateful elements
                // only. Day strings are unique per cell.
                .id(SharedString::from(format!("stat-cell-{day}")))
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    let next = over.then(|| day.clone());
                    if this.stats_hover_day != next {
                        this.stats_hover_day = next;
                        cx.notify();
                    }
                }))
                .into_any_element()
        } else {
            el.into_any_element()
        });
    }

    let grid_w = grid.grid_w();
    let cols = grid.col_cursor;

    // One shared hover card for the whole grid: 371 per-cell tooltips would
    // destroy/recreate on every cell boundary and flicker.
    let hover_card = hover.zip(grid.hover_cell).map(|(text, (col, row))| {
        // Right-edge columns flip the card so it cannot leave the panel.
        let right_align = col + 16 >= cols;
        div()
            .absolute()
            .when(right_align, |el| {
                el.right(px((cols.saturating_sub(1 + col) * 9) as f32))
            })
            .when(!right_align, |el| el.left(px((col * 9) as f32)))
            .when(row == 0, |el| el.top(px(STATS_CELL + 4.0)))
            .when(row > 0, |el| {
                el.bottom(px(STATS_GRID_H - row as f32 * 9.0 + 4.0))
            })
            .rounded_md()
            .px_2()
            .py_1()
            .bg(hsla(0.0, 0.0, 0.13, 0.94))
            .text_xs()
            .text_color(hsla(0.0, 0.0, 1.0, 0.96))
            .whitespace_nowrap()
            .child(text)
    });

    div()
        .border_t_1()
        .border_color(LINE)
        .pt_3()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .text_color(TEXT)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(t(L::STATS_DAILY)),
                )
                .child(stats_legend()),
        )
        // Absolute children inside a fixed-size relative box: the pitch (9px)
        // is the single source of truth for cells, labels, and the hover card
        // alike.
        .child(
            div()
                .relative()
                .h(px(14.))
                .w(px(grid_w))
                .children(month_labels),
        )
        .child(
            div()
                .relative()
                .w(px(grid_w))
                .h(px(STATS_GRID_H))
                .children(cells)
                .children(hover_card),
        )
        .when_some(early_caption, |el, caption| {
            el.child(div().text_xs().text_color(MUTED_SOFT).child(caption))
        })
}

/// The bottom tally: task count, output lines, languages, separation, records.
fn stats_rows_section(stats: &StatsSummary, lang: UiLang) -> impl IntoElement {
    let mut rows: Vec<AnyElement> = Vec::new();
    rows.push(stats_row(
        t(L::STATS_TASKS),
        if stats.tasks_err == 0 {
            crate::i18n::tasks_all_ok(lang, stats.tasks_ok)
        } else {
            crate::i18n::tasks_split(stats.tasks_total(), stats.tasks_ok, stats.tasks_err)
        },
    ));
    if stats.cues > 0 {
        rows.push(stats_row(
            t(L::STATS_OUTPUT),
            crate::i18n::n_lines(stats.cues),
        ));
    }
    if !stats.langs.is_empty() {
        let lang_label = |id: &str| {
            source_language_by_id(id)
                .map(|l| l.label.to_string())
                .unwrap_or_else(|| id.to_string())
        };
        let mut line = format!("{} {}", lang_label(&stats.langs[0].0), stats.langs[0].1);
        if let Some((id, n)) = stats.langs.get(1) {
            line.push_str(&format!(" · {} {}", lang_label(id), n));
        }
        if stats.langs.len() > 2 {
            line.push_str(&crate::i18n::langs_more(stats.langs.len()));
        }
        rows.push(stats_row(t(L::STATS_LANGS), line));
    }
    if stats.sep_tasks > 0 {
        rows.push(stats_row(
            t(L::STATS_SEPARATION),
            crate::i18n::n_tasks(lang, stats.sep_tasks),
        ));
    }
    if let Some(m) = stats.longest_media_sec {
        rows.push(stats_row(
            t(L::STATS_LONGEST),
            oneasr_core::stats::format_span_secs(ui_lang(), m),
        ));
    }
    if let Some(f) = stats.fastest_speed {
        rows.push(stats_row(t(L::STATS_FASTEST), crate::i18n::rtfx(f)));
    }

    div()
        .border_t_1()
        .border_color(LINE)
        .pt_2()
        .flex()
        .flex_col()
        .children(rows)
}

/// One small labelled figure in the totals row.
fn stats_metric(label: &'static str, value: String) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(div().text_xs().text_color(MUTED).child(label))
        .child(
            div()
                .text_sm()
                .text_color(TEXT)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .truncate()
                .child(value),
        )
        .into_any_element()
}

/// One `label … value` line in the bottom tally.
fn stats_row(label: &'static str, value: String) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .py_0p5()
        .child(div().text_xs().text_color(MUTED).child(label))
        .child(
            div()
                .text_xs()
                .text_color(TEXT)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(value),
        )
        .into_any_element()
}

/// `少 ▢▢▢▢▢ 多` — the heat scale's key.
fn stats_legend() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .text_color(MUTED)
        .child(t(L::LEGEND_LESS))
        .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L0))
        .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L1))
        .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L2))
        .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L3))
        .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L4))
        .child(t(L::LEGEND_MORE))
}
