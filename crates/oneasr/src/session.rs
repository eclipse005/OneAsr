//! 任务队列。界面只改这里的状态；转写在独立线程里调用 `oneasr-core`。

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use oneasr_core::i18n::UiLang;
use oneasr_core::{
    Settings, StageUpdate, check_demucs_model_dir, demote_current_thread, is_audio_ext,
    is_video_ext, normalize_source_language, probe_duration_sec, process_media_file_with_progress,
};

use crate::i18n::{self, L};

/// 一行任务的生命周期。同一时刻最多一个 [`Status::Running`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Queued,
    Running,
    Done,
    Failed,
}

impl Status {
    /// 排队或正在跑的行不能改语言、分离开关，也不能删。
    pub fn locks(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => i18n::t(L::ST_PENDING),
            Self::Queued => i18n::t(L::ST_QUEUED),
            Self::Running => i18n::t(L::ST_RUNNING),
            Self::Done => i18n::t(L::ST_DONE),
            Self::Failed => i18n::t(L::ST_FAILED),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    pub duration: Option<f64>,
    pub language: String,
    pub vocal: bool,
    pub status: Status,
    pub stage: String,
    pub output: Option<PathBuf>,
    pub error: Option<String>,
}

/// 工作线程送回界面的事件。界面线程每 80ms 排空一次。
#[derive(Debug)]
pub enum Ev {
    Duration {
        id: u64,
        seconds: Option<f64>,
    },
    Stage {
        id: u64,
        label: String,
    },
    Finished {
        id: u64,
        output: Result<PathBuf, String>,
    },
}

/// 一次真正开跑所需的副本。设置按这一行的语言和人声分离改过。
pub struct Job {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub settings: Settings,
    pub ui_lang: UiLang,
}

pub struct Session {
    pub tasks: Vec<Task>,
    pub settings: Settings,
    pub dirty: bool,
    pub hint: String,
    next_id: u64,
    pub tx: Sender<Ev>,
}

impl Session {
    /// 读 `settings.json`，并把界面语言切到里面那一档。
    pub fn boot() -> (Self, Receiver<Ev>) {
        let (tx, rx) = mpsc::channel();
        let settings = Settings::load();
        oneasr_core::i18n::set_ui_lang(settings.resolved_ui_language());
        let session = Self {
            tasks: Vec::new(),
            settings,
            dirty: false,
            hint: String::new(),
            next_id: 1,
            tx,
        };
        (session, rx)
    }

    /// 只给队列单测用：不读盘。
    #[cfg(test)]
    fn blank() -> Self {
        let (tx, _rx) = mpsc::channel();
        Self {
            tasks: Vec::new(),
            settings: Settings::default(),
            dirty: false,
            hint: String::new(),
            next_id: 1,
            tx,
        }
    }

    /// 把拖进来或选中的路径收成任务。返回新行的 id，以及有没有被扩展名拒绝的路径。
    pub fn add_paths(&mut self, paths: Vec<PathBuf>) -> (Vec<u64>, bool) {
        let mut ids = Vec::new();
        let mut skipped = false;
        for path in paths {
            if !is_media(&path) {
                skipped = true;
                continue;
            }
            if self
                .tasks
                .iter()
                .any(|t| t.path == path && t.status.locks())
            {
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            self.tasks.push(Task {
                id,
                path,
                name,
                bytes,
                duration: None,
                language: self.settings.language.clone(),
                vocal: self.settings.vocal_separation,
                status: Status::Pending,
                stage: String::new(),
                output: None,
                error: None,
            });
            ids.push(id);
        }
        if skipped && ids.is_empty() {
            self.hint = i18n::t(L::NOT_MEDIA).to_string();
        }
        (ids, skipped)
    }

    pub fn set_duration(&mut self, id: u64, seconds: Option<f64>) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.duration = seconds;
        }
    }

    pub fn cycle_language(&mut self, id: u64) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if task.status.locks() {
            self.hint = i18n::t(L::LOCKED_LANG).to_string();
            return;
        }
        let langs = oneasr_core::SOURCE_LANGUAGES;
        let ix = langs
            .iter()
            .position(|l| l.id == task.language)
            .unwrap_or(0);
        task.language = langs[(ix + 1) % langs.len()].id.to_string();
    }

    pub fn toggle_vocal(&mut self, id: u64) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if task.status.locks() {
            self.hint = i18n::t(L::LOCKED_SEPARATION).to_string();
            return;
        }
        if !task.vocal
            && check_demucs_model_dir(&self.settings.resolved_demucs_model_dir()).is_err()
        {
            self.hint = i18n::t(L::SEPARATION_NEEDS_MODEL).to_string();
            return;
        }
        task.vocal = !task.vocal;
        self.hint = if task.vocal {
            i18n::t(L::SEPARATION_ON).to_string()
        } else {
            i18n::t(L::SEPARATION_OFF).to_string()
        };
    }

    pub fn remove(&mut self, id: u64) {
        let Some(task) = self.tasks.iter().find(|t| t.id == id) else {
            return;
        };
        if task.status.locks() {
            self.hint = i18n::t(L::LOCKED_DELETE).to_string();
            return;
        }
        self.tasks.retain(|t| t.id != id);
    }

    /// 等待和失败的行进入队列。正在跑的不动。
    pub fn queue_all(&mut self) -> usize {
        mark_queued(&mut self.tasks)
    }

    pub fn queue_one(&mut self, id: u64) -> bool {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        if !matches!(task.status, Status::Pending | Status::Failed) {
            return false;
        }
        task.status = Status::Queued;
        task.error = None;
        task.stage.clear();
        true
    }

    /// 没有正在跑的任务时，把第一条排队的标成运行中并交出任务书。
    pub fn kick(&mut self) -> Option<Job> {
        let id = take_next(&mut self.tasks)?;
        let task = self.tasks.iter().find(|t| t.id == id)?;
        let mut settings = self.settings.clone();
        settings.language = normalize_source_language(&task.language);
        settings.vocal_separation = task.vocal;
        settings.hotword_run = None;
        if let Err(reason) = settings.can_start() {
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                task.status = Status::Failed;
                task.error = Some(reason.clone());
            }
            self.hint = reason;
            return None;
        }
        let task = self.tasks.iter().find(|t| t.id == id)?;
        Some(Job {
            id,
            path: task.path.clone(),
            name: task.name.clone(),
            settings,
            ui_lang: oneasr_core::i18n::ui_lang(),
        })
    }

    /// 清掉不在跑的行。有任务在跑时返回 true（队列清了，这一则继续）。
    pub fn clear_idle(&mut self) -> bool {
        let running = clear_idle(&mut self.tasks);
        self.hint = if running {
            i18n::t(L::CLEARED_KEEP_RUNNING).to_string()
        } else if self.tasks.is_empty() {
            i18n::t(L::LIST_EMPTY).to_string()
        } else {
            String::new()
        };
        running
    }

    pub fn apply(&mut self, ev: Ev) -> bool {
        match ev {
            Ev::Duration { id, seconds } => {
                self.set_duration(id, seconds);
                false
            }
            Ev::Stage { id, label } => {
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                    task.stage = label;
                }
                false
            }
            Ev::Finished { id, output } => {
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                    match output {
                        Ok(path) => {
                            task.status = Status::Done;
                            task.output = Some(path);
                            task.stage.clear();
                            task.error = None;
                        }
                        Err(err) => {
                            task.status = Status::Failed;
                            task.error = Some(err);
                            task.stage.clear();
                        }
                    }
                }
                true
            }
        }
    }

    pub fn model_ready(&self) -> bool {
        self.settings.can_start().is_ok()
    }
}

/// 等待和失败的行改成排队。返回改了几行。
pub fn mark_queued(tasks: &mut [Task]) -> usize {
    let mut n = 0;
    for task in tasks.iter_mut() {
        if matches!(task.status, Status::Pending | Status::Failed) {
            task.status = Status::Queued;
            task.error = None;
            task.stage.clear();
            n += 1;
        }
    }
    n
}

/// 已经有一行在跑就什么都不做。否则把第一条排队的标成运行中。
pub fn take_next(tasks: &mut [Task]) -> Option<u64> {
    if tasks.iter().any(|t| t.status == Status::Running) {
        return None;
    }
    let id = tasks.iter().find(|t| t.status == Status::Queued)?.id;
    if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
        task.status = Status::Running;
        task.stage.clear();
        task.error = None;
    }
    Some(id)
}

/// 只留下正在跑的行。返回清之前是否有一行在跑。
pub fn clear_idle(tasks: &mut Vec<Task>) -> bool {
    let running = tasks.iter().any(|t| t.status == Status::Running);
    tasks.retain(|t| t.status == Status::Running);
    running
}

/// 没有任务在跑时启动下一条。门禁失败的行标成失败，继续看队列里的下一则。
pub fn pump(session: &RefCell<Session>) {
    loop {
        let job = session.borrow_mut().kick();
        if let Some(job) = job {
            let tx = session.borrow().tx.clone();
            spawn_job(tx, job);
            return;
        }
        let guard = session.borrow();
        let running = guard
            .tasks
            .iter()
            .any(|task| task.status == Status::Running);
        let queued = guard.tasks.iter().any(|task| task.status == Status::Queued);
        drop(guard);
        if running || !queued {
            return;
        }
    }
}

pub fn spawn_job(tx: Sender<Ev>, job: Job) {
    let _ = thread::Builder::new()
        .name("oneasr-run".into())
        .spawn(move || {
            demote_current_thread();
            let id = job.id;
            let tx_stage = tx.clone();
            let lang = job.ui_lang;
            let result = catch_unwind(AssertUnwindSafe(|| {
                let data_root = oneasr_core::paths::data_dir();
                process_media_file_with_progress(
                    &job.path,
                    &job.name,
                    &job.settings,
                    &data_root,
                    |update: StageUpdate| {
                        let _ = tx_stage.send(Ev::Stage {
                            id,
                            label: update.label(lang),
                        });
                    },
                )
                .map_err(|e| e.to_string())
            }));
            let output = match result {
                Ok(Ok(path)) => Ok(path),
                Ok(Err(err)) => Err(err),
                Err(payload) => Err(panic_message(payload)),
            };
            let _ = tx.send(Ev::Finished { id, output });
        });
}

pub fn spawn_probe(tx: Sender<Ev>, id: u64, path: PathBuf) {
    let _ = thread::Builder::new()
        .name("oneasr-probe".into())
        .spawn(move || {
            let seconds = probe_duration_sec(&path);
            let _ = tx.send(Ev::Duration { id, seconds });
        });
}

fn is_media(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    is_audio_ext(ext) || is_video_ext(ext)
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        i18n::unknown_panic().into()
    }
}

pub fn clock(sec: f64) -> String {
    let total = sec.round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn bytes_label(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1} GB", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0} KB", n as f64 / 1_000.0)
    } else {
        format!("{n} B")
    }
}

/// `file:///D:/a%20b.mp4` → `D:\a b.mp4`。已经是本地路径的原样收下。
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file:")?;
    let rest = rest.trim_start_matches('/');
    let rest = rest.strip_prefix("localhost/").unwrap_or(rest);
    let decoded = percent_decode(rest);
    if decoded.is_empty() {
        return None;
    }
    Some(PathBuf::from(decoded))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2]))
        {
            out.push((h << 4) | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
fn task(id: u64, status: Status) -> Task {
    Task {
        id,
        path: PathBuf::from(format!("clip-{id}.mp3")),
        name: format!("clip-{id}.mp3"),
        bytes: 0,
        duration: None,
        language: "zh".into(),
        vocal: false,
        status,
        stage: String::new(),
        output: None,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_next_runs_one_at_a_time() {
        let mut tasks = vec![task(1, Status::Queued), task(2, Status::Queued)];
        assert_eq!(take_next(&mut tasks), Some(1));
        assert_eq!(tasks[0].status, Status::Running);
        assert_eq!(
            take_next(&mut tasks),
            None,
            "已经有一行在跑时，下一条必须留在队列里"
        );
        assert_eq!(tasks[1].status, Status::Queued);
    }

    #[test]
    fn mark_queued_skips_done_and_running() {
        let mut tasks = vec![
            task(1, Status::Pending),
            task(2, Status::Done),
            task(3, Status::Running),
            task(4, Status::Failed),
        ];
        assert_eq!(mark_queued(&mut tasks), 2);
        assert_eq!(tasks[0].status, Status::Queued);
        assert_eq!(tasks[1].status, Status::Done);
        assert_eq!(tasks[2].status, Status::Running);
        assert_eq!(tasks[3].status, Status::Queued);
    }

    #[test]
    fn clear_idle_keeps_the_running_row() {
        let mut tasks = vec![
            task(1, Status::Pending),
            task(2, Status::Queued),
            task(3, Status::Running),
            task(4, Status::Done),
        ];
        assert!(clear_idle(&mut tasks));
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, 3);
    }

    #[test]
    fn clear_idle_on_an_idle_list_empties_it() {
        let mut tasks = vec![task(1, Status::Pending), task(2, Status::Done)];
        assert!(!clear_idle(&mut tasks));
        assert!(tasks.is_empty());
    }

    #[test]
    fn file_uri_decodes_spaces() {
        let path = file_uri_to_path("file:///D:/media/a%20b.mp4").expect("path");
        assert_eq!(path, PathBuf::from("D:/media/a b.mp4"));
    }

    #[test]
    fn add_paths_rejects_a_text_file() {
        let mut session = Session::blank();
        let dir = std::env::temp_dir().join(format!("oneasr-nsh-add-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let media = dir.join("clip.mp3");
        let note = dir.join("notes.txt");
        std::fs::write(&media, b"x").unwrap();
        std::fs::write(&note, b"x").unwrap();
        let (ids, skipped) = session.add_paths(vec![note, media]);
        assert!(skipped);
        assert_eq!(ids.len(), 1);
        assert_eq!(session.tasks[0].language, session.settings.language);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
