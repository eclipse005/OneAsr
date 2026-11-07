//! Native Shell 界面。颜色沿用原来的浅色青绿，控件是 NSH 的视图。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use nsh_base::{button, progress, switch, tab, tabs};
use nsh_core::desktop::TitleBarStyle;
use nsh_core::element::LayoutAnimation;
use nsh_core::element::desktop::{set_title_bar, title_bar};
use nsh_core::file_dialog::{FileFilter, OpenDialog};
use nsh_core::prelude::*;
use oneasr_core::model::ModelId;
use oneasr_core::{
    CHUNK_TARGET_PRESETS, Settings, TextScript, invalidate_all_model_checks, source_language_by_id,
};

use crate::i18n::{self, L};
use crate::session::{self, Ev, Session, Status, Task, bytes_label, clock, file_uri_to_path, pump};

const BG: u32 = 0xf4f6f8;
const PANEL: u32 = 0xffffff;
const LINE: u32 = 0xe2e8f0;
const TEXT: u32 = 0x0f172a;
const MUTED: u32 = 0x64748b;
const ACCENT: u32 = 0x0d9488;
const ACCENT_SOFT: u32 = 0xccfbf1;
const LOGO: u32 = 0x0f766e;
const DANGER: u32 = 0xdc2626;
const WARN: u32 = 0xd97706;

/// 界面信号。任务列表和设置页各自保持节点，进度刷新只改读到的文字。
#[derive(Clone)]
struct Shell {
    session: Rc<RefCell<Session>>,
    tasks: Signal<Vec<Task>>,
    hint: Signal<String>,
    ready: Signal<bool>,
    /// 设置草稿。控件只创建一次，选中态和路径读它。
    draft: Signal<Settings>,
    unsaved: Signal<bool>,
    lang: Signal<u8>,
    /// 当前目的地：0 队列，1 模型，2 字幕，3 偏好。只活在界面上。
    place: Signal<u8>,
}

impl Shell {
    fn boot(session: Rc<RefCell<Session>>) -> Self {
        let shell = Self {
            session,
            tasks: signal(Vec::new()),
            hint: signal(String::new()),
            ready: signal(false),
            draft: signal(Settings::default()),
            unsaved: signal(false),
            lang: signal(0),
            place: signal(0),
        };
        shell.sync(false);
        shell
    }

    /// 把会话抄进信号。只有行数变了才做布局动画，阶段文字的刷新不动布局。
    fn sync(&self, layout: bool) {
        let session = self.session.borrow();
        let next = session.tasks.clone();
        let count_changed = self.tasks.with_untracked(|tasks| tasks.len()) != next.len();
        drop(session);
        if layout && count_changed {
            animate_layout(LayoutAnimation::EASE_IN_EASE_OUT);
        }
        let session = self.session.borrow();
        self.tasks.set(next);
        self.hint.set(session.hint.clone());
        self.ready.set(session.model_ready());
        self.draft.set(session.settings.clone());
        self.unsaved.set(session.dirty);
        self.lang.set(oneasr_core::i18n::ui_lang() as u8);
    }
}

fn install(shell: Shell, rx: Receiver<Ev>) {
    let rx = Rc::new(RefCell::new(rx));
    set_interval(Duration::from_millis(80), move || {
        let batch: Vec<Ev> = rx.borrow_mut().try_iter().collect();
        if batch.is_empty() {
            return;
        }
        let mut follow = false;
        {
            let mut session = shell.session.borrow_mut();
            for ev in batch {
                follow |= session.apply(ev);
            }
        }
        if follow {
            pump(&shell.session);
        }
        shell.sync(follow);
    });
}

pub fn root(session: Rc<RefCell<Session>>, rx: Receiver<Ev>) -> impl IntoElement {
    set_title_bar(TitleBarStyle::Hidden, Some(px(44.0)));
    let shell = Shell::boot(session);
    install(shell.clone(), rx);
    let dropped = shell.clone();
    view()
        .flex_1()
        .bg(rgb(BG))
        .text_color(rgb(TEXT))
        .on_file_drop(move |event: &FileDropEvent| {
            let paths = event
                .uris
                .iter()
                .filter_map(|uri| file_uri_to_path(uri))
                .collect();
            accept_paths(&dropped, paths);
        })
        .child(title_bar_row())
        .child(screen(shell))
}

fn title_bar_row() -> impl IntoElement {
    view()
        .h(px(44.0))
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .bg(rgb(PANEL))
        .window_drag(true)
        .pl(move || title_bar().left + px(14.0))
        .pr(move || title_bar().right + px(8.0))
        .child(mark())
        .child(
            text("OneAsr")
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD),
        )
        .child(
            text(env!("CARGO_PKG_VERSION"))
                .text_size(px(12.0))
                .text_color(rgb(MUTED)),
        )
}

fn mark() -> impl IntoElement {
    view()
        .size(px(22.0))
        .rounded(px(6.0))
        .bg(rgb(LOGO))
        .items_center()
        .justify_center()
        .window_drag(false)
        .child(
            text("A")
                .text_size(px(13.0))
                .font_weight(FontWeight::BOLD)
                .text_color(white()),
        )
}

fn screen(shell: Shell) -> impl IntoElement {
    let here = shell.clone();
    let there = shell.clone();
    view()
        .flex_1()
        .flex_row()
        .child(rail(shell))
        .child(view().w(px(1.0)).self_stretch().bg(rgb(LINE)))
        .child(
            view()
                .flex_1()
                .child(destination(here))
                .child(view().h(px(1.0)).bg(rgb(LINE)))
                .child(status_bar(there)),
        )
}

fn destination(shell: Shell) -> impl IntoElement {
    let queue = shell.clone();
    let models = shell.clone();
    let subtitles = shell.clone();
    let prefs = shell.clone();
    when(move || queue.place.get() == 0, {
        let shell = queue.clone();
        move || queue_pane(shell.clone())
    })
    .otherwise({
        let models = models.clone();
        let subtitles = subtitles.clone();
        let prefs = prefs.clone();
        move || {
            when(move || models.place.get() == 1, {
                let shell = models.clone();
                move || work_page(shell.clone(), L::MODELS, 1)
            })
            .otherwise({
                let subtitles = subtitles.clone();
                let prefs = prefs.clone();
                move || {
                    when(move || subtitles.place.get() == 2, {
                        let shell = subtitles.clone();
                        move || work_page(shell.clone(), L::SUBTITLES, 2)
                    })
                    .otherwise({
                        let shell = prefs.clone();
                        move || work_page(shell.clone(), L::PREFERENCES, 3)
                    })
                }
            })
        }
    })
}

/// 左边是目的地，不是一排和旧界面同构的工具按钮。官方 Tabs 的指示条自己滑到当前项。
fn rail(shell: Shell) -> impl IntoElement {
    let queue = shell.clone();
    let settings = shell.clone();
    tabs()
        .flex_col()
        .w(px(148.0))
        .bg(rgb(PANEL))
        .px(px(8.0))
        .py(px(12.0))
        .gap(px(4.0))
        .indicator_class("absolute left-1 top-2 bottom-2 w-[3px] rounded-full bg-teal-600")
        .child(place_tab(queue, L::QUEUE, 0))
        .child(place_tab(settings.clone(), L::MODELS, 1))
        .child(place_tab(settings.clone(), L::SUBTITLES, 2))
        .child(place_tab(settings, L::PREFERENCES, 3))
}

fn place_tab(shell: Shell, copy: oneasr_core::i18n::Str, id: u8) -> impl IntoElement {
    let lang = shell.lang;
    let place = shell.place;
    let ink = shell.place;
    tab()
        .selected(move || place.get() == id)
        .on_press(move |_: &()| {
            if shell.place.get() == id {
                return;
            }
            animate_layout(LayoutAnimation::EASE_IN_EASE_OUT);
            shell.place.set(id);
        })
        .self_stretch()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .child(
            text(move || {
                let _ = lang.get();
                i18n::t(copy).to_string()
            })
            .text_size(px(13.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || {
                if ink.get() == id {
                    rgb(LOGO)
                } else {
                    rgb(MUTED)
                }
            }),
        )
}

fn work_page(shell: Shell, title: oneasr_core::i18n::Str, section: u8) -> impl IntoElement {
    let enter = motion().entering(MotionPreset::FadeInUp);
    let fields = shell.clone();
    let footer = shell.clone();
    let label = shell.lang;
    // 标题、保存和字段共用一列，并在内容区里居中。保存不再贴在窗口最右、字段却停在左边。
    view().flex_1().bind_motion(&enter).child(
        view().flex_1().w_full().items_center().child(
            view()
                .w_full()
                .max_w(px(640.0))
                .flex_1()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(24.0))
                        .py(px(12.0))
                        .child(
                            text(move || {
                                let _ = label.get();
                                i18n::t(title).to_string()
                            })
                            .text_size(px(18.0))
                            .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(view().flex_1())
                        .child(save_footer(footer)),
                )
                .child(view().h(px(1.0)).bg(rgb(LINE)))
                .child(
                    scroll().flex_1().child(
                        view()
                            .w_full()
                            .px(px(24.0))
                            .py(px(16.0))
                            .child(settings_page(fields, section)),
                    ),
                ),
        ),
    )
}

fn queue_pane(shell: Shell) -> impl IntoElement {
    let enter = motion().entering(MotionPreset::FadeInUp);
    let listed = shell.clone();
    view()
        .flex_1()
        .bind_motion(&enter)
        .child(when(move || !listed.tasks.with(Vec::is_empty), {
            let shell = listed.clone();
            move || queue_actions(shell.clone())
        }))
        .child(task_layer(shell))
}

fn queue_actions(shell: Shell) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
        .child(primary_button(phrase(shell.lang, L::ADD), {
            let shell = shell.clone();
            move || pick_media(shell.clone())
        }))
        .child(ghost_button(phrase(shell.lang, L::START_ALL), {
            let shell = shell.clone();
            move || {
                {
                    let mut session = shell.session.borrow_mut();
                    if session.queue_all() == 0 && !session.model_ready() {
                        session.hint = i18n::t(L::START_ALL_BLOCKED).to_string();
                    }
                }
                pump(&shell.session);
                shell.sync(true);
            }
        }))
        .child(ghost_button(phrase(shell.lang, L::CLEAR), {
            let shell = shell.clone();
            move || {
                shell.session.borrow_mut().clear_idle();
                shell.sync(true);
            }
        }))
}

fn task_layer(shell: Shell) -> impl IntoElement {
    let empty = shell.clone();
    let listed = shell.clone();
    scroll().flex_1().child(
        when(move || empty.tasks.with(Vec::is_empty), {
            let shell = empty.clone();
            move || empty_state(shell.clone())
        })
        .otherwise({
            let shell = listed.clone();
            move || {
                each(shell.tasks, |task: &Task| task.id, {
                    let shell = shell.clone();
                    move |task, index| task_row(shell.clone(), task, index)
                })
            }
        }),
    )
}

fn phrase(lang: Signal<u8>, copy: oneasr_core::i18n::Str) -> impl Fn() -> String + 'static {
    move || {
        let _ = lang.get();
        i18n::t(copy).to_string()
    }
}

fn empty_state(shell: Shell) -> impl IntoElement {
    let enter = motion().entering(MotionPreset::FadeInUp);
    let breathe = motion();
    breathe.play(MotionPreset::Breathe);
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(12.0))
        .px(px(32.0))
        .py(px(48.0))
        .bind_motion(&enter)
        .child(
            view()
                .bind_motion(&breathe)
                .size(px(64.0))
                .rounded_full()
                .bg(rgb(ACCENT_SOFT))
                .items_center()
                .justify_center()
                .child(text("♪").text_size(px(26.0)).text_color(rgb(LOGO))),
        )
        .child(
            text(phrase(shell.lang, L::WELCOME_TITLE))
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_center(),
        )
        .child(
            text(phrase(shell.lang, L::WELCOME_STEP2))
                .max_w(px(280.0))
                .text_size(px(13.0))
                .text_color(rgb(MUTED))
                .text_center(),
        )
        .child(primary_button(phrase(shell.lang, L::ADD_FILES), {
            move || pick_media(shell.clone())
        }))
}

fn task_row(shell: Shell, task: ReadSignal<Task>, index: ReadSignal<usize>) -> impl IntoElement {
    let delay = (index.get_untracked().min(6) as f32) * 42.0;
    let enter = motion().entering(
        MotionPreset::FadeInUp
            .spec()
            .timing(|timing| timing.delay(delay)),
    );
    let id = task.with_untracked(|task| task.id);
    view()
        .bind_motion(&enter)
        .flex_row()
        .bg(rgb(PANEL))
        .rounded(px(12.0))
        .shadow_sm()
        .overflow_hidden()
        .mx(px(16.0))
        .my(px(4.0))
        .child(
            view()
                .w(px(3.0))
                .bg(move || rgb(status_color(task.with(|task| task.status))))
                .transition_colors()
                .duration(220.0)
                .ease_out(),
        )
        .child(
            view()
                .flex_1()
                .px(px(14.0))
                .py(px(12.0))
                .gap(px(8.0))
                .child(
                    text(move || task.with(|task| task.name.clone()))
                        .text_size(px(14.0))
                        .font_weight(FontWeight::MEDIUM)
                        .truncate(),
                )
                .child(
                    text(move || task_meta(&task.with(|task| task.clone())))
                        .text_size(px(12.0))
                        .text_color(rgb(MUTED)),
                )
                .child(when(
                    move || task.with(|task| task.status == Status::Running),
                    || {
                        progress()
                            .indeterminate(true)
                            .self_stretch()
                            .track_class("mt-1 h-1 w-full rounded-full bg-slate-200")
                            .indicator_class("h-1 w-1/3 rounded-full bg-teal-600")
                    },
                ))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(live_chip(
                            move || {
                                source_language_by_id(&task.with(|task| task.language.clone()))
                                    .map(|lang| lang.short.to_string())
                                    .unwrap_or_else(|| "?".into())
                            },
                            || true,
                            move || !task.with(|task| task.status.locks()),
                            {
                                let shell = shell.clone();
                                move || {
                                    shell.session.borrow_mut().cycle_language(id);
                                    shell.sync(false);
                                }
                            },
                        ))
                        .child(live_chip(
                            move || {
                                let _ = shell.lang.get();
                                i18n::t(L::SEP_CHIP).to_string()
                            },
                            move || task.with(|task| task.vocal),
                            move || !task.with(|task| task.status.locks()),
                            {
                                let shell = shell.clone();
                                move || {
                                    shell.session.borrow_mut().toggle_vocal(id);
                                    shell.sync(false);
                                }
                            },
                        ))
                        .child(
                            when(
                                move || task.with(|task| task.status == Status::Running),
                                || {
                                    view()
                                        .size(px(7.0))
                                        .rounded_full()
                                        .bg(rgb(ACCENT))
                                        .animate_pulse()
                                },
                            )
                            .otherwise(|| view().size(px(7.0)).rounded_full().bg(rgb(LINE))),
                        )
                        .child(
                            text(move || {
                                let _ = shell.lang.get();
                                task.with(|task| task.status.label()).to_string()
                            })
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .flex_1(),
                        )
                        .child(ghost_button(
                            move || {
                                let _ = shell.lang.get();
                                let open = task.with(|task| {
                                    task.status == Status::Done && task.output.is_some()
                                });
                                i18n::t(if open {
                                    L::TIP_OPEN_OUTPUT
                                } else {
                                    L::TIP_START
                                })
                                .to_string()
                            },
                            {
                                let shell = shell.clone();
                                move || {
                                    let output = task.with(|task| task.output.clone());
                                    if let Some(path) = output {
                                        spawn(async move {
                                            let _ = reveal_path(path).await;
                                        });
                                        return;
                                    }
                                    if shell.session.borrow_mut().queue_one(id) {
                                        pump(&shell.session);
                                    }
                                    shell.sync(true);
                                }
                            },
                        ))
                        .child(ghost_button(phrase(shell.lang, L::TIP_DELETE), {
                            let shell = shell.clone();
                            move || {
                                shell.session.borrow_mut().remove(id);
                                shell.sync(true);
                            }
                        })),
                ),
        )
}

fn task_meta(task: &Task) -> String {
    let mut meta = bytes_label(task.bytes);
    if let Some(sec) = task.duration {
        meta.push_str(" · ");
        meta.push_str(&clock(sec));
    }
    if !task.stage.is_empty() {
        meta.push_str(" · ");
        meta.push_str(&task.stage);
    } else if let Some(err) = &task.error {
        meta.push_str(" · ");
        meta.push_str(err);
    }
    meta
}

fn status_color(status: Status) -> u32 {
    match status {
        Status::Running => ACCENT,
        Status::Failed => DANGER,
        Status::Done => LOGO,
        Status::Queued => WARN,
        Status::Pending => LINE,
    }
}

fn on_draft(
    draft: Signal<Settings>,
    f: impl Fn(&Settings) -> bool + 'static,
) -> Box<dyn Fn() -> bool> {
    Box::new(move || f(&draft.get()))
}

fn settings_page(shell: Shell, section: u8) -> impl IntoElement {
    let draft = shell.draft;
    let mut page = view().w_full().gap(px(16.0)).items_start();
    if section == 3 {
        page = page.child(group(
            i18n::t(L::UI_LANGUAGE),
            view()
                .flex_row()
                .gap(px(6.0))
                .child(choice(
                    i18n::t(L::UI_LANG_SYSTEM),
                    move || draft.get().ui_language == oneasr_core::i18n::UI_LANGUAGE_SYSTEM,
                    {
                        let shell = shell.clone();
                        move || set_ui_choice(&shell, oneasr_core::i18n::UI_LANGUAGE_SYSTEM)
                    },
                ))
                .child(choice(
                    i18n::t(L::UI_LANG_ZH),
                    move || draft.get().ui_language == "zh",
                    {
                        let shell = shell.clone();
                        move || set_ui_choice(&shell, oneasr_core::i18n::UI_LANGUAGE_ZH)
                    },
                ))
                .child(choice(
                    i18n::t(L::UI_LANG_EN),
                    move || draft.get().ui_language == "en",
                    {
                        let shell = shell.clone();
                        move || set_ui_choice(&shell, oneasr_core::i18n::UI_LANGUAGE_EN)
                    },
                )),
        ));
    }
    if section == 1 {
        page = page.child(group(
            i18n::t(L::ASR_MODEL),
            view()
                .gap(px(8.0))
                .child(model_row(draft))
                .child(choice_row(
                    ModelId::ASR_CHOICES
                        .into_iter()
                        .map(|id| {
                            let on = on_draft(draft, move |settings| {
                                settings.selected_asr_id().asr_base() == id
                            });
                            let shell = shell.clone();
                            (
                                id.short_label().to_string(),
                                on,
                                Box::new(move || {
                                    edit(&shell, |session| {
                                        let quant =
                                            session.settings.selected_asr_id().is_quantized();
                                        session.settings.select_asr_model(id.with_quant(quant));
                                    });
                                }) as Box<dyn Fn()>,
                            )
                        })
                        .chain(std::iter::once({
                            let on = on_draft(draft, |settings| {
                                settings.selected_asr_id().is_quantized()
                            });
                            let shell = shell.clone();
                            (
                                i18n::t(L::QUANT).to_string(),
                                on,
                                Box::new(move || {
                                    edit(&shell, |session| {
                                        let next =
                                            !session.settings.selected_asr_id().is_quantized();
                                        session.settings.set_asr_quantized(next);
                                    });
                                }) as Box<dyn Fn()>,
                            )
                        })),
                ))
                .child(folder_row(
                    i18n::t(L::DLG_ASR_DIR),
                    move || draft.get().asr_model_dir.display().to_string(),
                    {
                        let shell = shell.clone();
                        move || {
                            pick_folder(shell.clone(), i18n::t(L::DLG_ASR_DIR), |session, dir| {
                                session.settings.set_asr_dir(dir);
                                invalidate_all_model_checks();
                            })
                        }
                    },
                )),
        ));

        page = page.child(group(
            i18n::t(L::ALIGNER_MODEL),
            view()
                .gap(px(8.0))
                .child(choice_row(ModelId::ALIGNER_CHOICES.into_iter().map(|id| {
                    let on = on_draft(draft, move |settings| settings.selected_aligner_id() == id);
                    let shell = shell.clone();
                    (
                        id.short_label().to_string(),
                        on,
                        Box::new(move || {
                            edit(&shell, |session| {
                                session.settings.select_aligner_model(id);
                            });
                        }) as Box<dyn Fn()>,
                    )
                })))
                .child(folder_row(
                    i18n::t(L::DLG_ALIGNER_DIR),
                    move || draft.get().aligner_model_dir.display().to_string(),
                    {
                        let shell = shell.clone();
                        move || {
                            pick_folder(
                                shell.clone(),
                                i18n::t(L::DLG_ALIGNER_DIR),
                                |session, dir| {
                                    session.settings.set_aligner_dir(dir);
                                    invalidate_all_model_checks();
                                },
                            )
                        }
                    },
                )),
        ));
    }
    if section == 1 {
        page = page.child(group(
            i18n::t(L::BACKEND),
            choice_row(
                [
                    ("auto", i18n::t(L::BACKEND_AUTO)),
                    ("gpu", i18n::t(L::BACKEND_GPU)),
                    ("cpu", i18n::t(L::BACKEND_CPU)),
                ]
                .into_iter()
                .map(|(id, label)| {
                    let on = on_draft(draft, move |settings| settings.backend == id);
                    let shell = shell.clone();
                    (
                        label.to_string(),
                        on,
                        Box::new(move || {
                            edit(&shell, |session| {
                                session.settings.backend = id.to_string();
                            });
                        }) as Box<dyn Fn()>,
                    )
                }),
            ),
        ));

        page = page.child(group(
            i18n::t(L::CHUNK_DURATION),
            choice_row(CHUNK_TARGET_PRESETS.iter().map(|&(sec, label)| {
                let on = on_draft(draft, move |settings| settings.chunk_target_seconds == sec);
                let shell = shell.clone();
                (
                    label.to_string(),
                    on,
                    Box::new(move || {
                        edit(&shell, |session| {
                            session.settings.chunk_target_seconds = sec;
                        });
                    }) as Box<dyn Fn()>,
                )
            })),
        ));
    }
    if section == 2 {
        page = page.child(group(
            i18n::t(L::SUBTITLE_LENGTH),
            choice_row(
                [
                    ("short", i18n::t(L::LEN_SHORT)),
                    ("standard", i18n::t(L::LEN_STANDARD)),
                    ("loose", i18n::t(L::LEN_LOOSE)),
                ]
                .into_iter()
                .map(|(id, label)| {
                    let on = on_draft(draft, move |settings| settings.subtitle_length_preset == id);
                    let shell = shell.clone();
                    (
                        label.to_string(),
                        on,
                        Box::new(move || {
                            edit(&shell, |session| {
                                session.settings.subtitle_length_preset = id.to_string();
                            });
                        }) as Box<dyn Fn()>,
                    )
                }),
            ),
        ));

        page = page.child(group(
            i18n::t(L::OUTPUT_FORMAT),
            choice_row(
                [
                    ("srt", on_draft(draft, |settings| settings.output_srt)),
                    ("txt", on_draft(draft, |settings| settings.output_txt)),
                    ("ass", on_draft(draft, |settings| settings.output_ass)),
                ]
                .into_iter()
                .map(|(id, on)| {
                    let shell = shell.clone();
                    (
                        id.to_uppercase(),
                        on,
                        Box::new(move || toggle_output(&shell, id)) as Box<dyn Fn()>,
                    )
                }),
            ),
        ));

        page = page.child(group(
            i18n::t(L::CHINESE_OUTPUT),
            choice_row(TextScript::ALL.into_iter().map(|item| {
                let on = on_draft(draft, move |settings| {
                    TextScript::from_id(&settings.text_script) == item
                });
                let shell = shell.clone();
                (
                    item.label(oneasr_core::i18n::ui_lang()).to_string(),
                    on,
                    Box::new(move || {
                        edit(&shell, |session| {
                            session.settings.text_script = item.id().to_string();
                        });
                    }) as Box<dyn Fn()>,
                )
            })),
        ));

        page = page.child(switch_line(
            i18n::t(L::SUBTITLE_BEAUTIFY),
            bool_chips(
                i18n::t(L::SUBTITLE_BEAUTIFY),
                move || draft.get().subtitle_beautify,
                {
                    let shell = shell.clone();
                    move |on| {
                        edit(&shell, move |session| {
                            session.settings.subtitle_beautify = on;
                        });
                    }
                },
            ),
        ));
    }

    if section == 1 {
        page = page.child(
            view()
                .self_stretch()
                .gap(px(8.0))
                .child(switch_line(
                    i18n::t(L::VOCAL_SEPARATION),
                    bool_chips(
                        i18n::t(L::VOCAL_SEPARATION),
                        move || draft.get().vocal_separation,
                        {
                            let shell = shell.clone();
                            move |on| {
                                edit(&shell, move |session| {
                                    session.settings.vocal_separation = on;
                                });
                            }
                        },
                    ),
                ))
                .child(folder_row(
                    i18n::t(L::DLG_DEMUCS_DIR),
                    {
                        move || {
                            draft
                                .get()
                                .resolved_demucs_model_dir()
                                .display()
                                .to_string()
                        }
                    },
                    {
                        let shell = shell.clone();
                        move || {
                            pick_folder(
                                shell.clone(),
                                i18n::t(L::DLG_DEMUCS_DIR),
                                |session, dir| {
                                    session.settings.demucs_model_dir = dir;
                                    invalidate_all_model_checks();
                                    if check_demucs_off(&session.settings)
                                        && session.settings.vocal_separation
                                    {
                                        session.settings.vocal_separation = false;
                                    }
                                },
                            )
                        }
                    },
                )),
        );
    }

    if section == 2 {
        page = page.child(group(
            i18n::t(L::OUTPUT_LOCATION),
            view()
                .gap(px(8.0))
                .child(choice_row(
                    [
                        (true, i18n::t(L::NEXT_TO_VIDEO)),
                        (false, i18n::t(L::CUSTOM_DIR)),
                    ]
                    .into_iter()
                    .map(|(beside, label)| {
                        let on = on_draft(draft, move |settings| {
                            settings.save_next_to_source == beside
                        });
                        let shell = shell.clone();
                        (
                            label.to_string(),
                            on,
                            Box::new(move || {
                                edit(&shell, |session| {
                                    session.settings.save_next_to_source = beside;
                                });
                            }) as Box<dyn Fn()>,
                        )
                    }),
                ))
                .child(folder_row(
                    i18n::t(L::DLG_OUTPUT_DIR),
                    move || draft.get().output_dir.display().to_string(),
                    {
                        let shell = shell.clone();
                        move || {
                            pick_folder(
                                shell.clone(),
                                i18n::t(L::DLG_OUTPUT_DIR),
                                |session, dir| {
                                    session.settings.output_dir = dir;
                                    session.settings.save_next_to_source = false;
                                },
                            )
                        }
                    },
                )),
        ));
    }

    if section == 3 {
        page = page.child(switch_line(
            i18n::t(L::SOUND),
            bool_chips(i18n::t(L::SOUND), move || draft.get().sound, {
                let shell = shell.clone();
                move |on| {
                    edit(&shell, move |session| {
                        session.settings.sound = on;
                    });
                }
            }),
        ));
    }

    page
}

fn save_footer(shell: Shell) -> impl IntoElement {
    let unsaved = shell.unsaved;
    let lang = shell.lang;
    view()
        .self_stretch()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(when(
            move || unsaved.get(),
            move || {
                let lang = lang;
                text(move || {
                    let _ = lang.get();
                    i18n::t(L::UNSAVED).to_string()
                })
                .text_size(px(12.0))
                .text_color(rgb(WARN))
            },
        ))
        .child(primary_button(i18n::t(L::SAVE_SETTINGS), {
            let shell = shell.clone();
            move || save_settings(&shell)
        }))
        .child(ghost_button(i18n::t(L::RESET), {
            let shell = shell.clone();
            move || {
                edit(&shell, |session| {
                    let welcome = session.settings.welcome_done;
                    session.settings = Settings::default();
                    session.settings.welcome_done = welcome;
                    invalidate_all_model_checks();
                });
            }
        }))
}

fn status_bar(shell: Shell) -> impl IntoElement {
    view()
        .h(px(36.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(16.0))
        .bg(rgb(PANEL))
        .child(
            view()
                .size(px(8.0))
                .rounded_full()
                .bg(move || {
                    if shell.ready.get() {
                        rgb(ACCENT)
                    } else {
                        rgb(WARN)
                    }
                })
                .transition_colors()
                .duration(200.0),
        )
        .child(
            text(move || {
                let _ = shell.lang.get();
                let hint = shell.hint.get();
                if hint.is_empty() {
                    i18n::t(if shell.ready.get() {
                        L::MODEL_READY
                    } else {
                        L::MODEL_NOT_READY
                    })
                    .to_string()
                } else {
                    hint
                }
            })
            .text_size(px(12.0))
            .text_color(rgb(MUTED))
            .flex_1(),
        )
        .child(
            text(move || {
                let _ = shell.lang.get();
                i18n::n_tasks(
                    oneasr_core::i18n::ui_lang(),
                    shell.tasks.with(|tasks| tasks.len()),
                )
            })
            .text_size(px(12.0))
            .text_color(rgb(MUTED)),
        )
}

fn section_title(label: &str) -> impl IntoElement {
    text(label.to_string())
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
}

fn group(title: &str, body: impl IntoElement) -> impl IntoElement {
    view()
        .self_stretch()
        .gap(px(6.0))
        .child(section_title(title))
        .child(body)
}

fn switch_line(title: &str, control: impl IntoElement) -> impl IntoElement {
    view()
        .self_stretch()
        .flex_row()
        .items_center()
        .min_h(px(28.0))
        .child(view().flex_1().child(section_title(title)))
        .child(control)
}

fn path_line(path: impl Fn() -> String + 'static) -> impl IntoElement {
    text(path)
        .text_size(px(12.0))
        .text_color(rgb(MUTED))
        .truncate()
}

/// 路径和选择按钮合成一条。按钮贴在条的末端，路径放不下再缩短。
fn folder_row(
    name: &str,
    path: impl Fn() -> String + 'static,
    pick: impl Fn() + 'static,
) -> impl IntoElement {
    let name = name.to_string();
    view()
        .self_stretch()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .bg(rgb(BG))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgb(LINE))
        .pl(px(10.0))
        .pr(px(4.0))
        .py(px(4.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(path_line(path)),
        )
        .child(ghost_labeled(i18n::t(L::CHOOSE_FOLDER), Some(name), pick))
}

fn bool_chips(
    name: &str,
    on: impl Fn() -> bool + 'static,
    set: impl Fn(bool) + 'static,
) -> impl IntoElement {
    // checked: 跟官方 Switch 的选中态走，拇指用过渡滑过去。不要按当前值换成两套 class。
    switch()
        .accessibility_label(name.to_string())
        .checked(on)
        .on_change(move |next: &bool| set(*next))
        .track_class("h-5 w-9 rounded-full bg-slate-300 transition checked:bg-teal-600")
        .thumb_class("m-0.5 size-4 rounded-full bg-white transition checked:translate-x-4")
        .hit_slop(8.0)
}

fn choice_row(
    items: impl IntoIterator<Item = (String, Box<dyn Fn() -> bool>, Box<dyn Fn()>)>,
) -> impl IntoElement {
    let mut row = view().flex_row().flex_wrap().gap(px(6.0));
    for (label, on, press) in items {
        row = row.child(choice(label, on, press));
    }
    row
}

fn choice(
    label: impl Into<String>,
    on: impl Fn() -> bool + 'static,
    press: impl Fn() + 'static,
) -> impl IntoElement {
    let label = label.into();
    let on = Rc::new(on);
    let on_bg = on.clone();
    let on_border = on.clone();
    let on_ink = on.clone();
    button()
        .self_start()
        .px(px(10.0))
        .py(px(5.0))
        .rounded_full()
        .bg(move || if on_bg() { rgb(ACCENT_SOFT) } else { rgb(BG) })
        .border_1()
        .border_color(move || if on_border() { rgb(ACCENT) } else { rgb(LINE) })
        .selected(move || on())
        .transition_colors()
        .duration(180.0)
        .ease_out()
        .on_press(move |_: &()| press())
        .active(|style| style.scale(0.96))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if on_ink() { rgb(LOGO) } else { rgb(TEXT) }),
        )
}

fn live_chip(
    label: impl Fn() -> String + 'static,
    on: impl Fn() -> bool + 'static,
    enabled: impl Fn() -> bool + 'static,
    press: impl Fn() + 'static,
) -> impl IntoElement {
    let on = Rc::new(on);
    let on_border = on.clone();
    let on_ink = on.clone();
    view()
        .self_start()
        .px(px(10.0))
        .py(px(5.0))
        .rounded_full()
        .bg(move || if on() { rgb(ACCENT_SOFT) } else { rgb(PANEL) })
        .border_1()
        .border_color(move || if on_border() { rgb(ACCENT) } else { rgb(LINE) })
        .transition_colors()
        .duration(180.0)
        .ease_out()
        .cursor_pointer()
        .on_press(move || {
            if enabled() {
                press();
            }
        })
        .active(|style| style.scale(0.96))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if on_ink() { rgb(LOGO) } else { rgb(TEXT) }),
        )
}

fn primary_button(label: impl ButtonLabel, press: impl Fn() + 'static) -> impl IntoElement {
    let label = label.paint();
    // 对齐交给父级。self_start 会盖过列上的 items_center，空状态的按钮就贴到左边。
    button()
        .px(px(14.0))
        .py(px(7.0))
        .rounded(px(8.0))
        .bg(rgb(ACCENT))
        .transition()
        .duration(160.0)
        .ease_out()
        .on_press(move |_: &()| press())
        .hover(|style| style.opacity(0.92))
        .active(|style| style.scale(0.97))
        .child(
            text(label)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(white()),
        )
}

fn ghost_button(label: impl ButtonLabel, press: impl Fn() + 'static) -> impl IntoElement {
    ghost_labeled(label, None, press)
}

fn ghost_labeled(
    label: impl ButtonLabel,
    name: Option<String>,
    press: impl Fn() + 'static,
) -> impl IntoElement {
    let label = label.paint();
    let mut control = button();
    if let Some(name) = name {
        control = control.accessibility_label(name);
    }
    control
        .px(px(12.0))
        .py(px(7.0))
        .rounded(px(8.0))
        .bg(rgb(PANEL))
        .border_1()
        .border_color(rgb(LINE))
        .transition()
        .duration(160.0)
        .ease_out()
        .on_press(move |_: &()| press())
        .hover(|style| style.bg(rgb(BG)))
        .active(|style| style.scale(0.97))
        .child(text(label).text_size(px(13.0)).text_color(rgb(TEXT)))
}

trait ButtonLabel {
    fn paint(self) -> Box<dyn Fn() -> String>;
}

impl ButtonLabel for &str {
    fn paint(self) -> Box<dyn Fn() -> String> {
        let label = self.to_string();
        Box::new(move || label.clone())
    }
}

impl<F> ButtonLabel for F
where
    F: Fn() -> String + 'static,
{
    fn paint(self) -> Box<dyn Fn() -> String> {
        Box::new(self)
    }
}

fn edit(shell: &Shell, f: impl FnOnce(&mut Session)) {
    {
        let mut session = shell.session.borrow_mut();
        f(&mut session);
        session.dirty = session.settings != Settings::load();
    }
    shell.sync(false);
}

fn set_ui_choice(shell: &Shell, value: &str) {
    let value = value.to_string();
    edit(shell, move |session| {
        session.settings.ui_language = value;
        session.hint = i18n::t(L::UI_LANGUAGE_APPLY_ON_SAVE).to_string();
    });
}

fn toggle_output(shell: &Shell, which: &str) {
    let which = which.to_string();
    edit(shell, move |session| {
        let settings = &mut session.settings;
        match which.as_str() {
            "srt" => settings.output_srt = !settings.output_srt,
            "txt" => settings.output_txt = !settings.output_txt,
            "ass" => settings.output_ass = !settings.output_ass,
            _ => {}
        }
        if !settings.output_srt && !settings.output_txt && !settings.output_ass {
            match which.as_str() {
                "txt" => settings.output_txt = true,
                "ass" => settings.output_ass = true,
                _ => settings.output_srt = true,
            }
            session.hint = i18n::t(L::KEEP_ONE_FORMAT).to_string();
        }
    });
}

fn save_settings(shell: &Shell) {
    {
        let mut session = shell.session.borrow_mut();
        match session.settings.save() {
            Ok(_) => {
                oneasr_core::i18n::set_ui_lang(session.settings.resolved_ui_language());
                session.dirty = false;
                session.hint = if session.model_ready() {
                    i18n::t(L::SETTINGS_SAVED_READY).to_string()
                } else {
                    i18n::t(L::SETTINGS_SAVED_NOT_READY).to_string()
                };
            }
            Err(err) => session.hint = err,
        }
    }
    shell.sync(false);
}

fn accept_paths(shell: &Shell, paths: Vec<PathBuf>) {
    let ids = {
        let mut session = shell.session.borrow_mut();
        let (ids, _) = session.add_paths(paths);
        ids
    };
    let tx = shell.session.borrow().tx.clone();
    let pending: Vec<(u64, PathBuf)> = {
        let session = shell.session.borrow();
        ids.into_iter()
            .filter_map(|id| {
                session
                    .tasks
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| (id, t.path.clone()))
            })
            .collect()
    };
    for (id, path) in pending {
        session::spawn_probe(tx.clone(), id, path);
    }
    if shell.place.get() != 0 {
        animate_layout(LayoutAnimation::EASE_IN_EASE_OUT);
        shell.place.set(0);
    }
    shell.sync(true);
}

fn pick_media(shell: Shell) {
    let exts: Vec<String> = oneasr_core::AUDIO_EXTS
        .iter()
        .chain(oneasr_core::VIDEO_EXTS)
        .map(|ext| (*ext).to_string())
        .collect();
    let dialog = OpenDialog::files()
        .multiple(true)
        .title(i18n::t(L::DLG_ADD_MEDIA))
        .button_label(i18n::t(L::ADD))
        .filter(FileFilter::new(i18n::t(L::ADD_FILES), exts));
    spawn(async move {
        match pick_paths(dialog).await {
            Ok(Some(paths)) => accept_paths(&shell, paths),
            Ok(None) => {}
            Err(err) => {
                shell.session.borrow_mut().hint = err;
                shell.sync(false);
            }
        }
    });
}

fn pick_folder(shell: Shell, title: &str, apply: impl Fn(&mut Session, PathBuf) + 'static) {
    let dialog = OpenDialog::folders().title(title.to_string());
    spawn(async move {
        match pick_paths(dialog).await {
            Ok(Some(paths)) => {
                if let Some(dir) = paths.into_iter().next() {
                    edit(&shell, |session| apply(session, dir));
                }
            }
            Ok(None) => {}
            Err(err) => {
                shell.session.borrow_mut().hint = err;
                shell.sync(false);
            }
        }
    });
}

fn model_row(draft: Signal<Settings>) -> impl IntoElement {
    text(move || {
        draft
            .get()
            .selected_asr_id()
            .label(oneasr_core::i18n::ui_lang())
            .to_string()
    })
    .text_size(px(12.0))
    .text_color(rgb(MUTED))
}

fn check_demucs_off(settings: &Settings) -> bool {
    oneasr_core::check_demucs_model_dir(&settings.resolved_demucs_model_dir()).is_err()
}
