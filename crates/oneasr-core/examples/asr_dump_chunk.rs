//! Dump ASR text for one time range (diagnose hallucination).
//!
//! Prefer: `oneasr-cli asr-chunk ...` (same options).
//!
//! ```powershell
//! cargo run -p oneasr-core --release --bin oneasr-cli --features cuda -- `
//!   asr-chunk --wav "D:\OneAsr\runs\...\input_16k.wav" --start 722.75 --end 842.75 --language zh
//!
//! cargo run -p oneasr-core --release --example asr_dump_chunk --features cuda -- `
//!   --wav "D:\OneAsr\runs\...\input_16k.wav" --start 722.75 --end 842.75 --language zh
//! ```

use std::env;
use std::path::PathBuf;

use oneasr_core::init_native_library_path;
use oneasr_core::media::slice_wav;
use oneasr_core::{resolve_cuda_runtime_dir, Settings};
use qwen3_asr::{AsrInference, Backend as AsrBackend, TranscribeOptions};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let wav = PathBuf::from(arg(&args, "--wav").expect("--wav required"));
    let start: f32 = arg(&args, "--start")
        .and_then(|s| s.parse().ok())
        .expect("--start required");
    let end: f32 = arg(&args, "--end")
        .and_then(|s| s.parse().ok())
        .expect("--end required");
    let language = arg(&args, "--language").unwrap_or_else(|| "zh".into());
    let out = arg(&args, "--out").map(PathBuf::from);

    if let Some(dll) = resolve_cuda_runtime_dir() {
        register_dll_dir(&dll);
    }
    init_native_library_path();

    let mut settings = Settings::default();
    let asr_dir = PathBuf::from(r"D:\OneAsr\models\Qwen3-ASR-0.6B");
    if asr_dir.is_dir() {
        settings.asr_model_dir = asr_dir;
    }
    settings.language = language;
    settings.normalize();

    let tmp = env::temp_dir().join(format!(
        "oneasr_dump_{}_{}.wav",
        start as u32, end as u32
    ));
    slice_wav(&wav, start, end, &tmp).expect("slice_wav");
    eprintln!(
        "slice {:.3}-{:.3} ({:.1}s) -> {}",
        start,
        end,
        end - start,
        tmp.display()
    );

    let asr = AsrInference::load(&settings.asr_model_dir, AsrBackend::Cuda)
        .expect("load ASR");
    let lang = oneasr_core::lang::to_qwen_language_label(&settings.language);
    let opts = TranscribeOptions::default()
        .with_max_new_tokens(settings.max_new_tokens)
        .with_language(lang);
    let path_str = tmp.to_str().expect("utf8 path");
    let report = asr.transcribe(path_str, opts).expect("transcribe");
    let text = report.text.trim();
    let chars = text.chars().count();
    eprintln!("chars={chars} bytes={}", text.len());
    eprintln!("--- ASR TEXT BEGIN ---");
    println!("{text}");
    eprintln!("--- ASR TEXT END ---");

    if let Some(p) = out {
        std::fs::write(&p, text.as_bytes()).expect("write");
        eprintln!("wrote {}", p.display());
    }

    let _ = std::fs::remove_file(&tmp);
}

fn register_dll_dir(dll_dir: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = dll_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe extern "system" {
            fn SetDllDirectoryW(path: *const u16) -> i32;
        }
        unsafe {
            let _ = SetDllDirectoryW(wide.as_ptr());
        }
    }
}
