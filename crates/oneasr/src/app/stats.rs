//! The panel's side of the ledger: open/close, and when to re-fold the summary
//! from `stats.jsonl`.

use crate::app::prelude::*;

impl OneAsrApp {
    /// Ledger root — the app folder, next to `settings.json`.
    pub(crate) fn stats_root() -> PathBuf {
        resolve_app_root_dir()
    }

    /// Re-aggregate the ledger into the cached summary.
    ///
    /// Called at startup, on panel open, and after each finished task — **never
    /// per frame**: the panel repaints on every cell hover, and folding the
    /// whole ledger per paint is exactly the mistake the settings panel's probe
    /// cache exists to avoid.
    pub(crate) fn reload_stats(&mut self) {
        self.stats = oneasr_core::stats::summarize(&oneasr_core::stats::load(&Self::stats_root()));
    }

    /// Append one finished task, then refresh the cache.
    ///
    /// Failure is logged and swallowed: a usage ledger must never be able to
    /// break a transcription run, and must never surface an error the user
    /// cannot act on.
    pub(crate) fn record_stats(&mut self, rec: &StatsRecord) {
        if let Err(e) = oneasr_core::stats::append(&Self::stats_root(), rec) {
            crashlog::log_warn(format!("stats append failed: {e}"));
            return;
        }
        self.reload_stats();
    }

    pub(crate) fn toggle_stats(&mut self, cx: &mut Context<Self>) {
        if self.stats_open {
            self.stats_open = false;
            self.stats_hover_day = None;
        } else {
            // Fresh numbers on open, and only one floating surface at a time
            // (they share MENU_Z).
            self.reload_stats();
            self.stats_hover_day = None;
            self.close_lang_selects();
            self.close_timing_popover();
            // Navigation, same rule as the gear: one tap on open.
            self.play_ui(sfx::Sfx::Click);
            self.stats_open = true;
        }
        cx.notify();
    }
}
