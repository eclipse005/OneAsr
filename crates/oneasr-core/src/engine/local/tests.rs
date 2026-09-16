//! Adapter and backend-policy tests.

use super::cuda::is_forced_cuda;
#[cfg(feature = "cuda")]
use super::cuda::{cuda_probe_reject_reason, probe_cuda_device};
use super::demucs::{DemucsSeparatorAdapter, preferred_separator_backend};
use super::provider::LocalEngineProvider;
use crate::settings::Settings;

#[test]
fn separator_backend_pref_picks_the_first_attempt() {
    use demucs_core_native::Backend;
    assert_eq!(preferred_separator_backend("cpu"), Backend::Cpu);
    assert_eq!(preferred_separator_backend(" CPU "), Backend::Cpu);
    #[cfg(feature = "cuda")]
    {
        assert_eq!(preferred_separator_backend("auto"), Backend::Cuda);
        assert_eq!(preferred_separator_backend("nonsense"), Backend::Cuda);
    }
    #[cfg(not(feature = "cuda"))]
    {
        assert_eq!(preferred_separator_backend("auto"), Backend::Cpu);
        assert_eq!(preferred_separator_backend("nonsense"), Backend::Cpu);
    }
    assert!(is_forced_cuda("cuda"));
    assert!(is_forced_cuda(" CUDA "));
    assert!(!is_forced_cuda("auto"));
    assert!(!is_forced_cuda("cpu"));
}

#[test]
fn missing_weights_is_a_clear_error() {
    let dir = std::env::temp_dir().join(format!("oneasr_sep_missing_{}", std::process::id()));
    let err = match DemucsSeparatorAdapter::load(&dir, "cpu", |_| {}) {
        Ok(_) => panic!("missing weights must fail to load"),
        Err(e) => e.to_string(),
    };
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

/// GPU smoke: exercise the real device probe used by backend resolution.
/// Skips gracefully on machines without a CUDA device/driver.
#[cfg(feature = "cuda")]
#[test]
fn probe_cuda_device_smoke() {
    match probe_cuda_device() {
        Ok(p) => {
            eprintln!(
                "probe: {} sm_{}{} vram={:.1}GB reject={:?}",
                p.name,
                p.cc.0,
                p.cc.1,
                p.total_vram as f64 / 1e9,
                cuda_probe_reject_reason(p)
            );
        }
        Err(e) => eprintln!("no usable CUDA device: {e}"),
    }
}
