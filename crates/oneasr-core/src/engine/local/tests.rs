//! Adapter and backend-policy tests.

use super::backend::{is_forced_gpu, ComputeBackend};
use super::demucs::{separator_backend, DemucsSeparatorAdapter};
use super::provider::LocalEngineProvider;
use crate::settings::Settings;

#[test]
fn separator_backend_follows_compute() {
    assert_eq!(separator_backend(ComputeBackend::Cpu).tag(), "cpu");
    assert_eq!(separator_backend(ComputeBackend::Gpu).tag(), "gpu:auto");
    assert!(is_forced_gpu("gpu"));
    assert!(is_forced_gpu(" GPU "));
    assert!(!is_forced_gpu("auto"));
    assert!(!is_forced_gpu("cpu"));
}

#[test]
fn missing_weights_is_a_clear_error() {
    let dir = std::env::temp_dir().join(format!("oneasr_sep_missing_{}", std::process::id()));
    // 断言中文文案：错误消息按进程语言构造，而单测并行共享全局语言，
    // 必须固定语言窗口（其他测试可能正在 with_ui_lang 里切来切去）。
    let err = crate::i18n::with_ui_lang(crate::i18n::UiLang::Zh, || {
        match DemucsSeparatorAdapter::load(&dir, ComputeBackend::Cpu, false, |_| {}) {
            Ok(_) => panic!("missing weights must fail to load"),
            Err(e) => e.to_string(),
        }
    });
    assert!(err.contains("人声分离模型不存在"), "{err}");
}

#[test]
fn provider_resolves_cpu_when_asked() {
    let settings = Settings {
        backend: "cpu".into(),
        ..Settings::default()
    };
    let provider = LocalEngineProvider::from_settings(&settings).unwrap();
    assert_eq!(provider.resolved_backend(), "cpu");
}

/// GPU smoke: exercise the real adapter probe used by backend resolution.
/// Skips gracefully on machines without a wgpu device/driver.
#[test]
fn probe_gpu_device_smoke() {
    match super::backend::probe_gpu_device() {
        Ok(p) => eprintln!("probe: {}", p.description),
        Err(e) => eprintln!("no usable GPU: {e}"),
    }
}
