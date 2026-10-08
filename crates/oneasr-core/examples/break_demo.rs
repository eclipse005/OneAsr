//! 评测用例子：把一份文稿按「智能断句」的口径断好行，原样打到 stdout。
//!
//! 评测脚本（`tmp/align-eval/break_measure.py`）靠它拿到真实输出：断句逻辑是
//! `break_long_lines`，本文件只负责把文件读进来、把结果写出去，不掺任何判断。

use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("用法: break_demo <文稿> [语言] [预设]")?;
    let lang = args.next().unwrap_or_else(|| "zh".into());
    let preset = args.next().unwrap_or_else(|| "standard".into());

    let raw = std::fs::read_to_string(&path)?;
    let broken = oneasr_core::sentence_boundary::break_long_lines(&raw, &lang, &preset);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    out.write_all(broken.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}
