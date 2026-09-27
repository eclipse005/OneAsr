//! 路径策略：**安装目录**与**数据目录**分开判定。
//!
//! - 安装目录 = 含 `bin/ffmpeg` 的目录（[`resolve_app_root`](crate::media::resolve_app_root)），
//!   只读也必须能用；ffmpeg 永远从那里找。
//! - 数据目录 = `models/`、`output/`、`runs/`、`settings.json` 的父亲。
//!
//! 上半部分是**纯策略**：判定所需的「安装目录是否可写」「平台环境变量」都由调用方
//! 注入，所以「只读安装目录 → 用户数据目录」这条分支和三条平台规则在任何平台上
//! 都能单测。末尾是薄薄一层 I/O 适配：把环境读进来、探测一次可写性，并把结果在
//! 进程内缓存（`OnceLock`）——[`data_root`] 是全进程唯一的解析入口。
//!
//! 判定链（优先级从高到低）：
//! 1. 显式覆盖：`ONEASR_DATA_DIR`，或 CLI `--data-root`；
//! 2. 安装目录可写 → 就用安装目录（**便携语义**：zip / tar.gz 解压到可写目录时，
//!    数据待在应用旁边，升级、搬家都不用重下模型）；
//! 3. 平台用户数据目录：Windows `%LOCALAPPDATA%\OneAsr`、
//!    macOS `$HOME/Library/Application Support/OneAsr`、
//!    Linux/BSD `$XDG_DATA_HOME/oneasr`（未设置或为空则 `$HOME/.local/share/oneasr`）。
//!
//! 这里**没有**「退临时目录」这一条：模型是几 GB 的文件，落到 `%TEMP%` 是错的。
//! 平台用户目录也探测不到时 [`resolve_data_root`] 返回 [`DataRootError`]，由写方
//! （保存设置、下载权重）给出清晰错误；只有日志 / 统计这类"丢了不致命"的兜底才
//! 允许调用方在 `Err` 时退临时目录。
//!
//! 系统包管理器把应用装到 `/opt/OneAsr`（root 所有、755）时，安装目录不可写，
//! 数据目录自动落到用户目录——不需要在 postinst 里 chmod/chown。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 数据目录里的固定子目录名。
pub const MODELS_DIR: &str = "models";
/// 见 [`MODELS_DIR`]。
pub const OUTPUT_DIR: &str = "output";
/// 见 [`MODELS_DIR`]。
pub const RUNS_DIR: &str = "runs";
/// 数据目录里的设置文件名。
pub const SETTINGS_FILE: &str = "settings.json";

/// Windows / macOS 的用户数据子目录名（与既有 `%LOCALAPPDATA%\OneAsr` 保持一致，
/// 不再造第二套用户目录）。
const APP_DIR_NAME: &str = "OneAsr";
/// XDG 规范要求小写目录名。
const APP_DIR_NAME_XDG: &str = "oneasr";

/// 显式覆盖数据目录的环境变量。
pub const DATA_ROOT_ENV: &str = "ONEASR_DATA_DIR";

// ─── 纯策略（无 I/O，跨平台可测） ─────────────────────────────────────

/// 平台目录规则的种类（注入式，保证三条规则在任意平台上都能单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    /// Linux / BSD / 其它类 Unix：跟随 XDG。
    Xdg,
}

impl Platform {
    /// 运行平台。
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Xdg
        }
    }
}

/// 平台目录规则要读的环境变量（注入，避免单测读进程全局）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataEnv {
    /// Windows：`%LOCALAPPDATA%`。
    pub local_app_data: Option<OsString>,
    /// Linux / BSD：`$XDG_DATA_HOME`。
    pub xdg_data_home: Option<OsString>,
    /// `$HOME`（Windows 上通常只有 `%USERPROFILE%`）——类 Unix 规则与日志脱敏共用。
    pub home: Option<OsString>,
}

impl DataEnv {
    /// 读一次当前进程的环境。
    pub fn from_process() -> Self {
        Self {
            local_app_data: env_var("LOCALAPPDATA"),
            xdg_data_home: env_var("XDG_DATA_HOME"),
            home: env_var("HOME").or_else(|| env_var("USERPROFILE")),
        }
    }
}

/// 平台用户数据目录（应用子目录已包含在内）。
///
/// Windows `%LOCALAPPDATA%\OneAsr`；macOS `$HOME/Library/Application Support/OneAsr`；
/// Linux/BSD `$XDG_DATA_HOME/oneasr`，未设置或为空则 `$HOME/.local/share/oneasr`。
/// 基址缺失时返回 `None`（由 [`resolve_data_root`] 变成清晰错误）。
pub fn user_data_base(platform: Platform, env: &DataEnv) -> Option<PathBuf> {
    match platform {
        Platform::Windows => {
            nonempty(&env.local_app_data).map(|base| PathBuf::from(base).join(APP_DIR_NAME))
        }
        Platform::MacOs => nonempty(&env.home).map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(APP_DIR_NAME)
        }),
        Platform::Xdg => {
            let base = match nonempty(&env.xdg_data_home) {
                Some(xdg) => PathBuf::from(xdg),
                None => PathBuf::from(nonempty(&env.home)?)
                    .join(".local")
                    .join("share"),
            };
            Some(base.join(APP_DIR_NAME_XDG))
        }
    }
}

/// 数据目录来自哪里（写进日志，用户报障时一眼看懂）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataRootSource {
    /// `ONEASR_DATA_DIR` / CLI `--data-root`。
    Explicit,
    /// 安装目录本身可写，数据待在应用旁边。
    Portable,
    /// 平台用户数据目录。
    UserData,
}

impl DataRootSource {
    /// 日志里用的短标签（`data_root=... (portable)`）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Portable => "portable",
            Self::UserData => "user-data",
        }
    }
}

/// 解析出来的数据目录 + 它的来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    pub path: PathBuf,
    pub source: DataRootSource,
}

/// 既没有可写的安装目录、也探测不到平台用户目录时的错误。
///
/// 这里刻意不给"临时目录兜底"：模型是几 GB 的文件，`%TEMP%` 是错的地方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRootError(String);

impl DataRootError {
    fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }

    /// 英文技术原因（日志用；面向用户的句子由 i18n 包装）。
    pub fn message(&self) -> &str {
        &self.0
    }

    fn missing_base(platform: Platform) -> Self {
        let base = match platform {
            Platform::Windows => "%LOCALAPPDATA%",
            Platform::MacOs => "$HOME",
            Platform::Xdg => "neither $XDG_DATA_HOME nor $HOME",
        };
        Self::new(format!(
            "the install directory is not writable and {base} is not set, so there is nowhere to keep models/, output/ or runs/"
        ))
    }
}

impl std::fmt::Display for DataRootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DataRootError {}

/// 数据目录判定链（优先级从高到低，见模块文档）。
///
/// `install_writable` 由 I/O 层用 [`probe_writable`](crate::model::probe_writable)
/// 实测后传入——把"可写性"变成参数，这条分支在任何平台上都能单测。
pub fn resolve_data_root(
    install_dir: &Path,
    install_writable: bool,
    explicit: Option<&Path>,
    platform: Platform,
    env: &DataEnv,
) -> Result<DataRoot, DataRootError> {
    if let Some(dir) = explicit {
        return Ok(DataRoot {
            path: dir.to_path_buf(),
            source: DataRootSource::Explicit,
        });
    }
    if install_writable {
        return Ok(DataRoot {
            path: install_dir.to_path_buf(),
            source: DataRootSource::Portable,
        });
    }
    match user_data_base(platform, env) {
        Some(path) => Ok(DataRoot {
            path,
            source: DataRootSource::UserData,
        }),
        None => Err(DataRootError::missing_base(platform)),
    }
}

/// 单个模型目录的落点（纯规则，供 [`crate::model::resolve_model_dir`] 使用）。
///
/// 数据目录里已有这个模型 → 用它；否则安装目录里有 → 沿用（只读安装目录里
/// 已下好的权重照旧可用，不做 GB 级搬迁）；两处都没有 → 落数据目录，也就是
/// 下次下载的落点（一定可写）。显式指定数据目录时只看数据目录——那是用户
/// 明确要求的隔离。
pub fn choose_model_dir(
    data_root: &Path,
    install_dir: &Path,
    source: DataRootSource,
    model_name: &str,
    data_has_model: bool,
    install_has_model: bool,
) -> PathBuf {
    let data_model = data_root.join(MODELS_DIR).join(model_name);
    if data_has_model || source == DataRootSource::Explicit || !install_has_model {
        return data_model;
    }
    install_dir.join(MODELS_DIR).join(model_name)
}

/// `{root}/output`（输出目录的通用构造；数据目录的默认值见 [`default_output_dir`]）。
pub fn output_dir_under(root: &Path) -> PathBuf {
    root.join(OUTPUT_DIR)
}

/// File stem for a media path (`video.mp4` → `video`). Falls back to `"out"`.
pub fn media_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "out".into())
}

/// User-facing deliverable: `{output_dir}/{media_stem}.srt`.
pub fn output_srt_path(output_dir: &Path, media_stem: &str) -> PathBuf {
    output_path(output_dir, media_stem, "srt")
}

/// `{output_dir}/{media_stem}.{ext}` (`ext` without dot).
pub fn output_path(output_dir: &Path, media_stem: &str, ext: &str) -> PathBuf {
    let stem = if media_stem.is_empty() {
        "out"
    } else {
        media_stem
    };
    output_dir.join(format!("{stem}.{ext}"))
}

/// 把 `home` 前缀替换成 `~`（日志脱敏；纯函数，便于单测）。
pub fn redact_prefix(text: &str, home: &Path) -> String {
    let native = home.display().to_string();
    if native.is_empty() {
        return text.to_string();
    }
    let mut out = text.replace(&native, "~");
    // Windows 日志里同一条路径可能出现正斜杠写法（ffmpeg 回话、用户手输）。
    let slashed = native.replace('\\', "/");
    if slashed != native {
        out = out.replace(&slashed, "~");
    }
    out
}

fn env_var(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

fn nonempty(value: &Option<OsString>) -> Option<&OsString> {
    value.as_ref().filter(|v| !v.is_empty())
}

// ─── I/O 适配（薄层 + 进程内缓存） ───────────────────────────────────

/// CLI `--data-root` 的显式覆盖。
///
/// 必须在**第一次解析之前**调用（解析结果进程内只算一次，之后改不生效）。
pub fn set_data_root_override(dir: impl Into<PathBuf>) {
    let _ = DATA_ROOT_OVERRIDE.set(dir.into());
}

/// 显式覆盖：CLI `--data-root` 优先，其次 `ONEASR_DATA_DIR`。
fn explicit_data_dir() -> Option<PathBuf> {
    DATA_ROOT_OVERRIDE
        .get()
        .cloned()
        .or_else(|| env_var(DATA_ROOT_ENV).map(PathBuf::from))
}

/// 全进程唯一的解析入口：探测安装目录可写性一次，结果缓存。
pub fn data_root() -> &'static Result<DataRoot, DataRootError> {
    static ROOT: OnceLock<Result<DataRoot, DataRootError>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let install = crate::model::resolve_app_root_dir();
        let explicit = explicit_data_dir();
        // 显式覆盖时不必碰安装目录：只有"靠安装目录可写性决定"的那条分支才探测。
        let writable = explicit.is_none() && crate::model::probe_writable(&install).is_ok();
        resolve_data_root(
            &install,
            writable,
            explicit.as_deref(),
            Platform::current(),
            &DataEnv::from_process(),
        )
    })
}

/// 数据目录本体。
///
/// 解析失败（既不可写又没有平台用户目录）时退回安装目录，让**读**路径仍然工作；
/// **写**路径请先看 [`data_root`]，别在这里悄悄改写目标。
pub fn data_dir() -> PathBuf {
    match data_root() {
        Ok(root) => root.path.clone(),
        Err(_) => crate::model::resolve_app_root_dir(),
    }
}

/// `{data}/models`（模型根的默认落点；单个模型目录的选点见 [`choose_model_dir`]）。
pub fn models_root() -> PathBuf {
    data_dir().join(MODELS_DIR)
}

/// `{data}/runs`（流水线每次运行的临时目录都开在这里）。
pub fn runs_dir() -> PathBuf {
    data_dir().join(RUNS_DIR)
}

/// 默认字幕输出目录：`{data}/output`。
pub fn default_output_dir() -> PathBuf {
    data_dir().join(OUTPUT_DIR)
}

/// `settings.json` 的**写**路径（数据目录里）。
pub fn settings_path() -> PathBuf {
    data_dir().join(SETTINGS_FILE)
}

/// 老布局的 `settings.json`（安装目录里）——只在数据目录里读不到时兜底读一次，
/// 保证老用户升级后设置不被重置；写入永远走 [`settings_path`]。
pub fn install_settings_path() -> PathBuf {
    crate::model::resolve_app_root_dir().join(SETTINGS_FILE)
}

/// 日志 / 统计的兜底基址：数据目录；连平台用户目录都探测不到时返回 `None`
/// （这类"丢了不致命"的文件才允许调用方退临时目录）。
pub fn log_fallback_base() -> Option<PathBuf> {
    data_root().as_ref().ok().map(|root| root.path.clone())
}

/// `data_root=... (portable|user-data|explicit)`，供启动日志。
pub fn data_root_log_line() -> String {
    match data_root() {
        Ok(root) => format!("{} ({})", root.path.display(), root.source.label()),
        Err(e) => format!("<unresolved: {}>", e.message()),
    }
}

/// 当前用户主目录（日志脱敏用；读一次并缓存）。
pub fn user_home() -> Option<PathBuf> {
    static HOME: OnceLock<Option<PathBuf>> = OnceLock::new();
    HOME.get_or_init(|| DataEnv::from_process().home.map(PathBuf::from))
        .clone()
}

/// 写日志前把用户主目录前缀换成 `~`（用户会把日志贴到公开 issue）。
pub fn redact_home(text: &str) -> String {
    match user_home() {
        Some(home) => redact_prefix(text, &home),
        None => text.to_string(),
    }
}

/// CLI `--data-root` 的进程级覆盖（见 [`set_data_root_override`]）。
static DATA_ROOT_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(local: Option<&str>, xdg: Option<&str>, home: Option<&str>) -> DataEnv {
        DataEnv {
            local_app_data: local.map(OsString::from),
            xdg_data_home: xdg.map(OsString::from),
            home: home.map(OsString::from),
        }
    }

    #[test]
    fn explicit_override_beats_everything() {
        let install = PathBuf::from(r"D:\app");
        let env = env_of(Some(r"C:\Users\u\AppData\Local"), None, None);
        // 安装目录可写、用户目录也有，显式覆盖仍然优先。
        let root = resolve_data_root(
            &install,
            true,
            Some(Path::new(r"D:\data")),
            Platform::Windows,
            &env,
        )
        .unwrap();
        assert_eq!(root.path, PathBuf::from(r"D:\data"));
        assert_eq!(root.source, DataRootSource::Explicit);
    }

    #[test]
    fn writable_install_dir_keeps_data_portable() {
        let install = PathBuf::from(r"D:\OneAsr");
        let env = env_of(Some(r"C:\Users\u\AppData\Local"), None, None);
        let root = resolve_data_root(&install, true, None, Platform::Windows, &env).unwrap();
        assert_eq!(root.path, install);
        assert_eq!(root.source, DataRootSource::Portable);
    }

    #[test]
    fn read_only_install_dir_falls_back_to_user_data() {
        // 系统包管理器把应用装到 /opt/OneAsr（root 所有）：数据必须落用户目录。
        let install = PathBuf::from("/opt/OneAsr");
        let env = env_of(None, None, Some("/home/u"));
        let root = resolve_data_root(&install, false, None, Platform::Xdg, &env).unwrap();
        assert_eq!(root.path, PathBuf::from("/home/u/.local/share/oneasr"));
        assert_eq!(root.source, DataRootSource::UserData);
    }

    #[test]
    fn user_data_rules_per_platform() {
        let win = env_of(
            Some(r"C:\Users\u\AppData\Local"),
            Some("/xdg"),
            Some("/home/u"),
        );
        assert_eq!(
            user_data_base(Platform::Windows, &win).unwrap(),
            PathBuf::from(r"C:\Users\u\AppData\Local\OneAsr")
        );
        let mac = env_of(None, None, Some("/Users/u"));
        assert_eq!(
            user_data_base(Platform::MacOs, &mac).unwrap(),
            PathBuf::from("/Users/u/Library/Application Support/OneAsr")
        );
        // XDG_DATA_HOME 存在时优先。
        let xdg = env_of(None, Some("/xdg/data"), Some("/home/u"));
        assert_eq!(
            user_data_base(Platform::Xdg, &xdg).unwrap(),
            PathBuf::from("/xdg/data/oneasr")
        );
        // 未设置则 $HOME/.local/share/oneasr。
        let plain = env_of(None, None, Some("/home/u"));
        assert_eq!(
            user_data_base(Platform::Xdg, &plain).unwrap(),
            PathBuf::from("/home/u/.local/share/oneasr")
        );
        // 空字符串等于未设置。
        let empty = env_of(Some(""), Some(""), Some("/home/u"));
        assert_eq!(
            user_data_base(Platform::Windows, &empty),
            None,
            "empty %LOCALAPPDATA% must not become a relative path"
        );
        assert_eq!(
            user_data_base(Platform::Xdg, &empty).unwrap(),
            PathBuf::from("/home/u/.local/share/oneasr")
        );
    }

    #[test]
    fn missing_user_base_is_a_clear_error() {
        let env = DataEnv::default();
        let err = resolve_data_root(Path::new("/opt/OneAsr"), false, None, Platform::Xdg, &env)
            .unwrap_err();
        assert!(
            err.message().contains("$XDG_DATA_HOME"),
            "error must name the missing base: {err}"
        );
        let win_err = resolve_data_root(Path::new(r"C:\app"), false, None, Platform::Windows, &env)
            .unwrap_err();
        assert!(win_err.message().contains("%LOCALAPPDATA%"), "{win_err}");
        // 文案要完整列出数据目录里的三类内容（models/、output/、runs/）：
        // 只写 models/ 就收尾读起来像被截断。
        for dir in [MODELS_DIR, OUTPUT_DIR, RUNS_DIR] {
            let name = format!("{dir}/");
            assert!(
                err.message().contains(&name),
                "error must list {name}: {err}"
            );
        }
    }

    #[test]
    fn model_dir_prefers_data_then_install() {
        let data = PathBuf::from(r"D:\data");
        let install = PathBuf::from(r"/opt/OneAsr");
        // 数据目录里有 → 用数据目录。
        assert_eq!(
            choose_model_dir(&data, &install, DataRootSource::UserData, "m", true, true),
            data.join("models").join("m")
        );
        // 数据目录里没有、安装目录里有 → 沿用老位置（只读安装目录仍可用）。
        assert_eq!(
            choose_model_dir(&data, &install, DataRootSource::UserData, "m", false, true),
            install.join("models").join("m")
        );
        // 两处都没有 → 数据目录（下次下载的落点）。
        assert_eq!(
            choose_model_dir(&data, &install, DataRootSource::UserData, "m", false, false),
            data.join("models").join("m")
        );
        // 显式覆盖 → 不回头找安装目录。
        assert_eq!(
            choose_model_dir(&data, &install, DataRootSource::Explicit, "m", false, true),
            data.join("models").join("m")
        );
    }

    #[test]
    fn redaction_hides_the_home_prefix() {
        let home = Path::new(r"C:\Users\someone");
        let line = r"media: C:\Users\someone\Videos\a.mp4 (log C:/Users/someone/x.log)";
        let out = redact_prefix(line, home);
        assert!(!out.contains("someone"), "{out}");
        assert!(out.contains(r"~\Videos\a.mp4"), "{out}");
        assert!(out.contains("~/x.log"), "{out}");
    }

    #[test]
    fn media_stem_from_video_name() {
        assert_eq!(media_stem(Path::new(r"D:\clips\my_video.mp4")), "my_video");
        assert_eq!(media_stem(Path::new("demo.wav")), "demo");
    }

    #[test]
    fn output_srt_under_chosen_dir() {
        let dir = PathBuf::from(r"D:\OneAsr\output");
        let p = output_srt_path(&dir, "my_video");
        assert_eq!(p, PathBuf::from(r"D:\OneAsr\output\my_video.srt"));
        assert_eq!(
            output_path(&dir, "my_video", "txt"),
            PathBuf::from(r"D:\OneAsr\output\my_video.txt")
        );
    }

    #[test]
    fn empty_stem_falls_back() {
        let dir = PathBuf::from("/app/output");
        assert_eq!(
            output_srt_path(&dir, ""),
            PathBuf::from("/app/output/out.srt")
        );
    }

    #[test]
    fn output_dir_is_root_output() {
        let root = PathBuf::from(r"D:\OneAsr");
        assert_eq!(output_dir_under(&root), PathBuf::from(r"D:\OneAsr\output"));
    }
}
