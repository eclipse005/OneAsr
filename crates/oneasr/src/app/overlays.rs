//! Transient overlays and their animation state: the language menus, the timing
//! popover, the settings drawer slide, and the single sound gate.

use crate::app::prelude::*;

impl OneAsrApp {
    /// The one sound gate (「提示音」): interaction taps AND the run reminder
    /// together.
    ///
    /// Never attach this to hover: it fires tens of times a second and is the
    /// fastest way to make an app feel noisy.
    pub(crate) fn play_ui(&self, kind: sfx::Sfx) {
        if self.settings.sound {
            sfx::play(kind);
        }
    }

    /// True when the task-row language panel can actually paint for `lang_menu`.
    pub(crate) fn task_lang_menu_active(&self) -> bool {
        let Some(id) = self.lang_menu.as_ref() else {
            return false;
        };
        self.tasks.iter().any(|t| {
            &t.id == id
                && !t.status.locks_row_actions()
                && !self.exiting.contains_key(&t.id)
        })
    }

    /// Drop select flags that cannot render a panel (stale id / locked / exiting).
    pub(crate) fn sync_lang_select_state(&mut self) {
        if self.lang_menu.is_some() && !self.task_lang_menu_active() {
            self.lang_menu = None;
        }
        // Settings select only while the drawer is open or animating.
        if self.settings_lang_open && !self.settings_open && self.settings_progress() < 0.01 {
            self.settings_lang_open = false;
        }
    }

    /// Clear both language selects (no notify — caller owns the frame).
    pub(crate) fn close_lang_selects(&mut self) {
        self.lang_menu = None;
        self.settings_lang_open = false;
    }

    /// Language menus and the timing card share MENU_Z — only one floating surface.
    pub(crate) fn close_floating_overlays(&mut self) {
        self.close_lang_selects();
        self.close_timing_popover();
    }

    /// Eased 0..=1 progress for the processing-time popover.
    pub(crate) fn timing_popover_progress(&self) -> f32 {
        let t = (self.timing_pop_t0.elapsed().as_secs_f32() / TIMING_POP_ANIM_SECS).min(1.0);
        let e = ease_out_cubic(t);
        self.timing_pop_from + (self.timing_pop_to - self.timing_pop_from) * e
    }

    /// Whether the popover still paints (open or closing animation).
    pub(crate) fn timing_popover_visible(&self) -> bool {
        self.timing_popover.is_some() && self.timing_popover_progress() > 0.01
    }

    pub(crate) fn open_timing_popover(&mut self, id: &str) {
        if self.timing_popover.as_deref() == Some(id) && self.timing_pop_to >= 1.0 {
            return;
        }
        let cur = self.timing_popover_progress();
        self.timing_popover = Some(id.to_string());
        self.timing_pop_from = cur;
        self.timing_pop_to = 1.0;
        self.timing_pop_t0 = Instant::now();
    }

    pub(crate) fn close_timing_popover(&mut self) {
        self.timing_hover_since = None;
        self.timing_leave_since = None;
        if self.timing_popover.is_none() && self.timing_pop_to <= 0.0 {
            return;
        }
        let cur = self.timing_popover_progress();
        self.timing_pop_from = cur;
        self.timing_pop_to = 0.0;
        self.timing_pop_t0 = Instant::now();
        // Keep id until anim finishes so exit still paints the right card.
        if cur < 0.01 {
            self.timing_popover = None;
        }
    }

    /// Hover entered the timing chip / card for `id`.
    pub(crate) fn timing_hover_enter(&mut self, id: &str, cx: &mut Context<Self>) {
        // Do not stack under an open language menu.
        if self.lang_menu.is_some() || self.settings_lang_open {
            return;
        }
        // Cancel pending leave.
        self.timing_leave_since = None;
        match &self.timing_hover_since {
            Some((hid, _)) if hid == id => {}
            _ => {
                self.timing_hover_since = Some((id.to_string(), Instant::now()));
            }
        }
        // Already open: keep to=1.
        if self.timing_popover.as_deref() == Some(id) {
            if self.timing_pop_to < 1.0 {
                let cur = self.timing_popover_progress();
                self.timing_pop_from = cur;
                self.timing_pop_to = 1.0;
                self.timing_pop_t0 = Instant::now();
            }
            cx.notify();
            return;
        }
        // Open delay handled in `tick_timing_popover`.
        cx.notify();
    }

    /// Hover left chip/card; schedule close after a short grace.
    pub(crate) fn timing_hover_leave(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .timing_hover_since
            .as_ref()
            .is_some_and(|(hid, _)| hid == id)
        {
            self.timing_hover_since = None;
        }
        if self.timing_popover.as_deref() == Some(id) || self.timing_pop_to > 0.0 {
            self.timing_leave_since = Some((id.to_string(), Instant::now()));
        }
        cx.notify();
    }

    /// Promote delayed hover → open; honor leave grace; clear id when closed.
    pub(crate) fn tick_timing_popover(&mut self) {
        if let Some((id, since)) = self.timing_hover_since.clone()
            && since.elapsed() >= Duration::from_millis(TIMING_HOVER_DELAY_MS)
            && self.timing_popover.as_deref() != Some(id.as_str())
        {
            self.open_timing_popover(&id);
        }
        if let Some((id, since)) = self.timing_leave_since.clone()
            && since.elapsed() >= Duration::from_millis(TIMING_LEAVE_DELAY_MS)
            && self.timing_hover_since.is_none()
        {
            if self.timing_popover.as_deref() == Some(id.as_str()) {
                self.close_timing_popover();
            } else {
                self.timing_leave_since = None;
            }
        }
        if self.timing_pop_to <= 0.0
            && self.timing_popover.is_some()
            && self.timing_popover_progress() < 0.01
        {
            self.timing_popover = None;
        }
    }

    /// Open / close the per-task language dropdown (closes the other select).
    pub(crate) fn toggle_lang_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        let locked = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .is_some_and(|t| t.status.locks_row_actions() || self.exiting.contains_key(id));
        if locked {
            self.close_floating_overlays();
            self.flash_hint("处理中的任务不能改语言", cx);
            return;
        }
        let was_open = self.lang_menu.as_deref() == Some(id);
        self.close_floating_overlays();
        if !was_open {
            self.lang_menu = Some(id.to_string());
        }
        cx.notify();
    }

    /// Open / close settings default-language dropdown (closes list select).
    pub(crate) fn toggle_settings_lang(&mut self, cx: &mut Context<Self>) {
        let was_open = self.settings_lang_open;
        self.close_floating_overlays();
        if !was_open {
            self.settings_lang_open = true;
        }
        cx.notify();
    }

    /// Apply a source-language pick and close every language select.
    pub(crate) fn pick_source_language(
        &mut self,
        target: LangSelectTarget,
        language: &str,
        cx: &mut Context<Self>,
    ) {
        match target {
            LangSelectTarget::Task(id) => {
                let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
                    self.close_floating_overlays();
                    cx.notify();
                    return;
                };
                if task.status.locks_row_actions() {
                    self.close_floating_overlays();
                    self.flash_hint("处理中的任务不能改语言", cx);
                    return;
                }
                task.set_language(language);
            }
            LangSelectTarget::Settings => {
                self.settings.language = language.into();
                self.mark_settings_dirty(cx);
            }
        }
        // Single exit boundary: any successful (or abandoned) pick leaves no open select.
        self.close_floating_overlays();
        cx.notify();
    }

    /// Flip per-task vocal separation (row pill). Blocked while the row is
    /// processing; enabling needs the Demucs weights to be installed.
    pub(crate) fn toggle_task_separation(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if task.status.locks_row_actions() {
            self.flash_hint("处理中的任务不能修改人声分离", cx);
            return;
        }
        let next = !task.vocal_separation;
        if next && !self.demucs_ready {
            self.flash_hint("请先在设置中下载人声分离模型", cx);
            return;
        }
        task.set_vocal_separation(next);
        self.play_ui(sfx::Sfx::Click);
        self.flash_hint(
            if next {
                "本任务已开启人声分离"
            } else {
                "本任务已关闭人声分离"
            },
            cx,
        );
        cx.notify();
    }

    /// Close language selects (Escape / click-outside scrim). Timing is hover-only.
    pub(crate) fn dismiss_menus(&mut self, cx: &mut Context<Self>) {
        // Escape closes the stats panel too. It lives on MENU_Z with the menus,
        // so it must honour the same key — even though its own dismiss layer,
        // not a key, is what normally closes it. Behaviour when it is closed is
        // byte-for-byte what it was.
        let closed_stats = self.stats_open;
        if closed_stats {
            self.stats_open = false;
            self.stats_hover_day = None;
        }
        if self.lang_menu.is_none() && !self.settings_lang_open {
            if closed_stats {
                cx.notify();
            }
            return;
        }
        self.close_lang_selects();
        cx.notify();
    }

    /// Whether a language panel is live (scrim / Escape target).
    pub(crate) fn any_menu_open(&self) -> bool {
        self.task_lang_menu_active() || self.settings_lang_open
    }

    pub(crate) fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        let open = !self.settings_open;
        // Closing with unsaved edits → auto-save (desktop-tool default).
        if !open && self.is_settings_dirty(cx) {
            self.save_settings(cx);
        }
        let cur = self.settings_progress();
        self.settings_from = cur;
        self.settings_to = if open { 1.0 } else { 0.0 };
        self.settings_anim_t0 = Instant::now();
        self.settings_open = open;
        // Drawer chrome shares the list surface — drop any floating overlays.
        self.close_floating_overlays();
        // The stats panel shares MENU_Z with those overlays; the drawer wins.
        self.stats_open = false;
        self.stats_hover_day = None;
        if open {
            self.play_ui(sfx::Sfx::Click);
        }
        cx.notify();
    }

    /// Eased 0..=1 drawer progress (works mid-animation when retoggled).
    pub(crate) fn settings_progress(&self) -> f32 {
        let t = (self.settings_anim_t0.elapsed().as_secs_f32() / DRAWER_ANIM_SECS).min(1.0);
        let e = ease_out_cubic(t);
        self.settings_from + (self.settings_to - self.settings_from) * e
    }

    pub(crate) fn animations_active(&self) -> bool {
        // Drawer slide + row enter/exit + empty-wave hover. Processing rows are
        // deliberately excluded: their visuals are static, and a multi-hour ASR
        // run must not force an animation frame for the whole window.
        let drawer = (self.settings_progress() - self.settings_to).abs() > 0.002;
        let row_anim = !self.exiting.is_empty() || !self.entering.is_empty();
        // Keep RAF while amp eases out after mouse leaves (smooth collapse to flat).
        let empty_wave =
            self.tasks.is_empty() && (self.empty_wave_hover || self.empty_wave_amp > 0.008);
        let timing_pop = (self.timing_popover_progress() - self.timing_pop_to).abs() > 0.002
            || self.timing_hover_since.is_some()
            || self.timing_leave_since.is_some();
        drawer || row_anim || empty_wave || timing_pop
    }

    /// Advance empty-wave smoothing (cursor follow + amp ease). Call once per frame while active.
    pub(crate) fn tick_empty_wave(&mut self) {
        let target_amp = if self.empty_wave_hover { 1.0 } else { 0.0 };
        // Snappy but not instant — ~120–180ms feel at 60fps.
        self.empty_wave_amp += (target_amp - self.empty_wave_amp) * 0.18;
        if self.empty_wave_amp < 0.004 && !self.empty_wave_hover {
            self.empty_wave_amp = 0.0;
        }
        self.empty_wave_smooth_x +=
            (self.empty_wave_cursor_x - self.empty_wave_smooth_x) * 0.22;
    }
}
