# OneAsr

本地音视频批量转字幕。亮色列表 UI + 真实 MOSS Rust ASR。

## 运行

准备：将 `ffmpeg.exe` 放到 `bin/ffmpeg.exe`（应用启动时从该目录解析）。

```powershell
cd D:\OneAsr
cargo run -p oneasr
```

## 输出

成功转写后自动写入安装目录：

```text
{app_root}/output/{视频名}.srt
```

例如 `D:\OneAsr\output\ja.srt`。中间产物在 `runs/`。

## 流程

启动加载模型 → 添加文件 → 全部开始（顺序）  
→ `bin/ffmpeg` 转 16k → `AsrInference`（raw）  
→ **解析引擎** `TranscriptEngine`：raw → 时间/说话人/正文  
→ 导出 SRT / TXT → `output/{stem}.srt`

### 解析引擎（最佳实践）

```text
MOSS raw  [t][Sxx]text[t]…
    ↓  TranscriptEngine::parse_moss_compact
TranscriptDocument { segments: [{start,end,speaker,text}, …] }
    ↓  to_srt / to_vtt / to_json / to_plain
多格式字幕
```

ASR 只负责出 raw；格式导出只读结构化 `Segment`，不重新抠 raw。

## 不做

- 长音频 VAD 分窗合并（仅设计，未实现）
- 进度百分比
- 字幕编辑器

## 测试

```powershell
cargo test -p oneasr-core
cargo test -p oneasr-core --test pipeline_contract
cargo test -p oneasr-core --test real_or_honest
cargo build -p oneasr
```
