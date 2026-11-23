//! OS shell helpers: show a file in the system file manager.

use oneasr_core::i18n::{UiLang, ui_lang};

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

fn parent_dir(file: &Path) -> Option<PathBuf> {
    file.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
}

pub fn open_containing_folder(file: &Path) -> Result<(), String> {
    if file.exists() {
        let target = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        return launch(reveal_plan(host(), &target));
    }
    match parent_dir(file) {
        Some(parent) => launch(open_dir_plan(host(), &parent)),
        None => Err(file_not_found(&file.display().to_string())),
    }
}

enum Platform {
    Windows,
    MacOs,
    Linux,
}

// cfg! 是编译期常量，三个变体都真的被构造过，`-D warnings` 下不会报 dead_code。
fn host() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

fn reveal_plan(platform: Platform, file: &Path) -> Vec<Vec<OsString>> {
    match platform {
        Platform::Windows => vec![command(
            "explorer",
            [format!("/select,\"{}\"", file.display()).into()],
        )],
        // `open -R` 选中文件本身；Finder 不认就退回打开所在目录。
        Platform::MacOs => {
            let mut plan = vec![command(
                "/usr/bin/open",
                ["-R".into(), file.as_os_str().to_owned()],
            )];
            if let Some(parent) = parent_dir(file) {
                plan.push(command("/usr/bin/open", [parent.into_os_string()]));
            }
            plan
        }
        // Linux 没有可移植的「选中」，打开目录即可。
        Platform::Linux => match parent_dir(file) {
            Some(parent) => open_dir_plan(Platform::Linux, &parent),
            None => Vec::new(),
        },
    }
}

fn open_dir_plan(platform: Platform, dir: &Path) -> Vec<Vec<OsString>> {
    let dir = dir.as_os_str().to_owned();
    match platform {
        Platform::Windows => vec![command(
            "explorer",
            [format!("\"{}\"", dir.to_string_lossy()).into()],
        )],
        Platform::MacOs => vec![command("/usr/bin/open", [dir])],
        Platform::Linux => vec![
            command("xdg-open", [dir.clone()]),
            command("gio", [OsString::from("open"), dir]),
        ],
    }
}

fn command(program: &str, args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    std::iter::once(OsString::from(program))
        .chain(args)
        .collect()
}

fn launch(mut plan: Vec<Vec<OsString>>) -> Result<(), String> {
    if plan.is_empty() {
        return Err(resolve_output_failed());
    }
    let first = plan.remove(0);
    match spawn(&first) {
        Ok(child) => {
            watch(child, first, plan);
            Ok(())
        }
        // 只有「没这个命令」值得换下一个候选。
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if plan.is_empty() {
                Err(open_folder_failed(&e))
            } else {
                launch(plan)
            }
        }
        Err(e) => Err(open_folder_failed(&e)),
    }
}

fn spawn(argv: &[OsString]) -> std::io::Result<Child> {
    let mut command = Command::new(&argv[0]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let line = describe(&argv[1..]);
        if !line.is_empty() {
            command.raw_arg(line);
        }
    }
    #[cfg(not(windows))]
    command.args(&argv[1..]);
    command.spawn()
}

#[cfg(windows)]
fn watch(child: Child, _argv: Vec<OsString>, _rest: Vec<Vec<OsString>>) {
    // explorer.exe 成功也返回 1，退出码在这里没有含义。
    drop(child);
}

#[cfg(not(windows))]
fn watch(mut child: Child, argv: Vec<OsString>, rest: Vec<Vec<OsString>>) {
    std::thread::spawn(move || {
        let line = describe(&argv);
        match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                crate::crashlog::log_warn(format!("{line} exited with {status}"));
                if rest.is_empty() {
                    return;
                }
                if let Err(e) = launch(rest) {
                    crate::crashlog::log_warn(format!("{line} fallback failed: {e}"));
                }
            }
            Err(e) => crate::crashlog::log_warn(format!("{line}: wait failed: {e}")),
        }
    });
}

fn describe(argv: &[OsString]) -> String {
    argv.iter()
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn file_not_found(path: &str) -> String {
    match ui_lang() {
        UiLang::Zh => format!("文件不存在: {path}"),
        UiLang::En => format!("File not found: {path}"),
    }
}

fn resolve_output_failed() -> String {
    match ui_lang() {
        UiLang::Zh => "无法解析输出目录".into(),
        UiLang::En => "Cannot resolve the output folder".into(),
    }
}

fn open_folder_failed(e: &std::io::Error) -> String {
    match ui_lang() {
        UiLang::Zh => format!("无法打开文件夹: {e}"),
        UiLang::En => format!("Failed to open folder: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PathBuf {
        std::env::temp_dir().join("oneasr-shell").join("clip.srt")
    }

    fn render(plan: &[Vec<OsString>]) -> Vec<Vec<String>> {
        plan.iter()
            .map(|argv| {
                argv.iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn macos_reveals_the_file_in_finder() {
        let file = sample();
        let parent = file.parent().unwrap().display().to_string();
        let plan = render(&reveal_plan(Platform::MacOs, &file));
        assert_eq!(
            plan[0],
            ["/usr/bin/open", "-R", file.display().to_string().as_str()]
        );
        assert_eq!(plan[1], ["/usr/bin/open", parent.as_str()]);
        assert!(plan.iter().flatten().all(|arg| arg != "xdg-open"));
    }

    #[test]
    fn linux_reveal_opens_the_containing_folder_with_a_fallback() {
        let file = sample();
        let parent = file.parent().unwrap().display().to_string();
        let plan = render(&reveal_plan(Platform::Linux, &file));
        assert_eq!(plan[0], ["xdg-open", parent.as_str()]);
        assert_eq!(plan[1], ["gio", "open", parent.as_str()]);
    }

    #[test]
    fn windows_selects_the_file_in_explorer() {
        let file = sample();
        let plan = render(&reveal_plan(Platform::Windows, &file));
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0],
            [
                "explorer",
                format!("/select,\"{}\"", file.display()).as_str()
            ]
        );
    }

    #[test]
    fn open_dir_uses_the_platform_opener() {
        let dir = sample();
        assert_eq!(
            render(&open_dir_plan(Platform::MacOs, &dir))[0][0],
            "/usr/bin/open"
        );
        assert_eq!(
            render(&open_dir_plan(Platform::Windows, &dir))[0][0],
            "explorer"
        );
        assert_eq!(
            render(&open_dir_plan(Platform::Linux, &dir))[0][0],
            "xdg-open"
        );
    }
}
