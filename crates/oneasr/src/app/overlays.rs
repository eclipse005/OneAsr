//! Transient overlays and their animation state: the language menus, the timing
//! popover, the settings drawer slide, and the single sound gate.

use crate::app::prelude::*;

/// 悬停满 `delay`、且当前没开着这张卡片 → 该打开它（返回卡片 id）。
///
/// 「用时」和「文稿」两张卡片是照着彼此写的，所以判据只留这一份：它们曾经各写
/// 一份，而文稿那份前面还多了一个「卡片还没开就 return」的守卫——悬停刚进来时
/// 卡片本来就是 `None`，于是那条路永远走不到，卡片从来没被打开过。
///
/// 纯函数：不碰 app 状态，所以「延迟没到 / 已经开着 / 没有悬停」三种不该打开的
/// 情况都能在没有 GPUI 上下文的地方钉住。
fn hover_promotes(
    hover: Option<&(String, Instant)>,
    open: Option<&str>,
    now: Instant,
    delay: std::time::Duration,
) -> Option<String> {
    let (id, since) = hover?;
    if open == Some(id.as_str()) || now.saturating_duration_since(*since) < delay {
        return None;
    }
    Some(id.clone())
}

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
            &t.id == id && !t.status.locks_row_actions() && !self.exiting.contains_key(&t.id)
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

    // ── 文稿卡片（纯悬停）────────────────────────────────────────────
    //
    // 和「用时」卡片是**同一套机制**，逐条对齐：悬停延时打开、移开有宽限期、
    // 卡片自己挂 `on_hover`（所以指针能移上去按按钮）、`animations_active()`
    // 里带它、只在 `progress > 0.01` 时才进渲染树。
    //
    // 没有「点击粘住」：那个是当初自己加的，理由是「纯悬停的卡片上按按钮难受」。
    // 实际上宽限期已经让指针有足够时间移过去，用时卡片也是这么用的。
    pub(crate) fn transcript_hover_enter(&mut self, id: &str, cx: &mut Context<Self>) {
        self.transcript_card_hovered = false;
        self.transcript_leave_since = None;
        if self
            .transcript_hover_since
            .as_ref()
            .is_some_and(|(hid, _)| hid == id)
        {
            return;
        }
        self.transcript_hover_since = Some((id.to_string(), Instant::now()));
        cx.notify();
    }

    /// 指针**真的**落在卡片上。
    ///
    /// 卡片刚被画出来时指针通常在**芯片**上，不在它身上——而 gpui 会给刚进入
    /// 渲染树的元素派发一次 `hover(false)`。不把这两种「离开」分开，芯片的悬停
    /// 就会被这一次假的离开取消：卡片出现 → 卡片关掉 → 再出现，鼠标一动一轮。
    /// 芯片上 `hover(false)` 是真的离开，卡片上要先确认指针确实来过。
    pub(crate) fn transcript_card_hover_enter(&mut self, id: &str, cx: &mut Context<Self>) {
        self.transcript_card_hovered = true;
        self.transcript_hover_enter(id, cx);
    }

    pub(crate) fn transcript_card_hover_leave(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.transcript_card_hovered {
            return;
        }
        self.transcript_card_hovered = false;
        self.transcript_hover_leave(id, cx);
    }

    pub(crate) fn transcript_hover_leave(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .transcript_hover_since
            .as_ref()
            .is_some_and(|(hid, _)| hid == id)
        {
            self.transcript_hover_since = None;
            self.transcript_leave_since = Some((id.to_string(), Instant::now()));
        }
        cx.notify();
    }

    /// Promote delayed hover → open; honour the leave grace; clear on close.
    pub(crate) fn tick_transcript_card(&mut self) {
        // 这里**不能**先 `if self.transcript_card.is_none() { return }`：
        // 悬停刚进来时卡片本来就是 None、只有 `hover_since` 有值，那个提前返回
        // 会让「悬停 → 打开」永远走不到。开着的时候才需要守 leave grace，
        // 所以判断本身而不是状态决定要不要往下走。
        if let Some(id) = hover_promotes(
            self.transcript_hover_since.as_ref(),
            self.transcript_card.as_deref(),
            Instant::now(),
            Duration::from_millis(TRANSCRIPT_HOVER_DELAY_MS),
        ) {
            self.open_transcript_card(&id);
        }
        if let Some((id, since)) = self.transcript_leave_since.clone()
            && since.elapsed() >= Duration::from_millis(TRANSCRIPT_LEVER_GRACE_MS)
        {
            if self.transcript_card.as_deref() == Some(id.as_str()) {
                self.close_transcript_card();
            } else {
                self.transcript_leave_since = None;
            }
        }
    }

    /// 打开一张文稿卡片。**这是 `transcript_card` 唯一的赋值入口。**
    ///
    /// 只写 `transcript_card = Some(id)` 而不启动动画，`progress()` 停在初值 0，
    /// 卡片会以 0 透明度画出来——渲染了，但看着跟没渲染一样（曾被当成「点击没
    /// 反应」）。收敛到一处就没有「忘了启动动画」这种可能了。
    pub(crate) fn open_transcript_card(&mut self, id: &str) {
        self.transcript_card_from = self.transcript_card_progress();
        self.transcript_card_to = 1.0;
        self.transcript_card_anim_t0 = Instant::now();
        self.transcript_card = Some(id.to_string());
    }

    pub(crate) fn close_transcript_card(&mut self) {
        self.transcript_card = None;
        self.transcript_card_hovered = false;
        self.transcript_hover_since = None;
        self.transcript_leave_since = None;
        self.transcript_card_note = None;
    }

    /// Eased 0..=1 progress for the card's fade/rise.
    pub(crate) fn transcript_card_progress(&self) -> f32 {
        if self.transcript_card.is_none() {
            return 0.0;
        }
        let t = (self.transcript_card_anim_t0.elapsed().as_secs_f32() / TRANSCRIPT_CARD_ANIM_SECS)
            .min(1.0);
        let e = ease_out_cubic(t);
        self.transcript_card_from + (self.transcript_card_to - self.transcript_card_from) * e
    }

    /// 卡片开着吗（含淡入淡出）。**渲染门也用它**：不在渲染树里的元素不参与
    /// 命中测试，所以一个 `progress == 0` 的卡片不会变成一块看不见却吞点击的
    /// 遮挡层（带 `occlude()` 时尤其致命）。这正是「用时」卡片一直在做的事。
    pub(crate) fn transcript_card_visible(&self) -> bool {
        self.transcript_card_progress() > 0.01
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
        if let Some(id) = hover_promotes(
            self.timing_hover_since.as_ref(),
            self.timing_popover.as_deref(),
            Instant::now(),
            Duration::from_millis(TIMING_HOVER_DELAY_MS),
        ) {
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
            self.flash_hint(t(L::LOCKED_LANG), cx);
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
                    self.flash_hint(t(L::LOCKED_LANG), cx);
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
            self.flash_hint(t(L::LOCKED_SEPARATION), cx);
            return;
        }
        let next = !task.vocal_separation;
        if next && !self.demucs_ready {
            self.flash_hint(t(L::SEPARATION_NEEDS_MODEL), cx);
            return;
        }
        task.set_vocal_separation(next);
        self.play_ui(sfx::Sfx::Click);
        self.flash_hint(
            if next {
                t(L::SEPARATION_ON)
            } else {
                t(L::SEPARATION_OFF)
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
        if open {
            // Snapshot on open. Closing the drawer is a cancel: unsaved edits
            // roll back to whatever was last persisted, never auto-saved.
            self.settings_snapshot = Some(self.settings.clone());
        } else if self.is_settings_dirty(cx) {
            if let Some(snap) = self.settings_snapshot.take() {
                self.settings = snap;
                self.settings_dirty = false;
                // Probes followed the staged edits — re-run them on the
                // restored paths so the status dots tell the truth again.
                self.refresh_model_probe();
            }
        } else {
            self.settings_snapshot = None;
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
        // 文稿卡片和用时卡片是同一套浮层机制，所以这三项必须一起在：少了它们，
        // 悬停延时到期后没有任何东西请求下一帧，`tick_transcript_card` 也就再也不会
        // 被调用——卡片只能靠点击（一个真实输入事件）才开得起来。
        let transcript_card = (self.transcript_card_progress() - self.transcript_card_to).abs()
            > 0.002
            || self.transcript_hover_since.is_some()
            || self.transcript_leave_since.is_some();
        drawer || row_anim || empty_wave || timing_pop || transcript_card
    }

    /// Advance empty-wave smoothing (cursor follow + amp ease). Call once per frame while active.
    pub(crate) fn tick_empty_wave(&mut self) {
        let target_amp = if self.empty_wave_hover { 1.0 } else { 0.0 };
        // Snappy but not instant — ~120–180ms feel at 60fps.
        self.empty_wave_amp += (target_amp - self.empty_wave_amp) * 0.18;
        if self.empty_wave_amp < 0.004 && !self.empty_wave_hover {
            self.empty_wave_amp = 0.0;
        }
        self.empty_wave_smooth_x += (self.empty_wave_cursor_x - self.empty_wave_smooth_x) * 0.22;
    }
}

#[cfg(test)]
mod tests {
    use super::hover_promotes;
    use std::time::{Duration, Instant};

    const D: Duration = Duration::from_millis(300);

    /// 悬停经过了 `elapsed`，返回可以传给 `hover_promotes` 的借用值。
    fn hover_for(elapsed: Duration) -> Option<(String, Instant)> {
        Some(("task-1".to_string(), Instant::now() - elapsed))
    }

    /// 没到延迟不开——指针扫过一行不该弹卡片。
    #[test]
    fn a_hover_shorter_than_the_delay_opens_nothing() {
        let hover = hover_for(Duration::from_millis(100));
        assert_eq!(
            hover_promotes(hover.as_ref(), None, Instant::now(), D),
            None
        );
    }

    /// 到点就开，**哪怕此刻一张卡片都还没开着**。这条就是「悬停打不开卡片」
    /// 那个 bug 的正面写法：它之前被一个「没开卡片就别往下走」的守卫挡死了。
    #[test]
    fn a_hover_past_the_delay_opens_a_card_that_is_not_yet_open() {
        let hover = hover_for(Duration::from_millis(400));
        assert_eq!(
            hover_promotes(hover.as_ref(), None, Instant::now(), D),
            Some("task-1".to_string())
        );
    }

    /// 已经开着就别再开一遍。
    #[test]
    fn an_already_open_card_is_not_promoted_again() {
        let hover = hover_for(Duration::from_millis(400));
        assert_eq!(
            hover_promotes(hover.as_ref(), Some("task-1"), Instant::now(), D),
            None
        );
    }

    /// 指针根本没在芯片上时，没有任何事发生。
    #[test]
    fn no_hover_opens_nothing() {
        assert_eq!(hover_promotes(None, None, Instant::now(), D), None);
    }
}
