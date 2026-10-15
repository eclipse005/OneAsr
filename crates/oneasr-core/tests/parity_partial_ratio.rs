//! `partial_ratio` / `partial_ratio_alignment` 与 rapidfuzz 的对拍（黄金语料回放）。
//!
//! 两份语料由上游 `asr-hotword` 项目的 `tools/dump_python.py`（conda `myenv` 里的
//! rapidfuzz）生成，随本仓库归档在 `tests/golden/`：
//!
//! - `pr_exhaustive.json`：字母表 {0,1}、长度 1..4 的**全部 900 个序列对**；
//! - `partial_ratio.json`：长度 1..40 / 1..18 的 2000 组随机用例（含 cutoff）。
//!
//! 它们是**外部**判据：`src/hotword/partial_ratio.rs` 是按 rapidfuzz-cpp 逐行复刻的，
//! 这几条测试就是「复刻得像不像」的硬证据——而不是拿自己的输出当自己的标准。
//! 要重新生成语料得回到上游项目跑那个脚本（rapidfuzz 不在本仓库的依赖里）。
//!
//! 注意 cutoff 以下 rapidfuzz 返回 **0.0 而非 None**，且低于阈值时不返回 alignment。

use oneasr_core::hotword::partial_ratio::{Code, partial_ratio, partial_ratio_alignment};

/// 序列中出现的最大符号 + 1，即 `num_symbols`。
fn num_symbols(cases: &[serde_json::Value]) -> usize {
    let mut max = 0usize;
    for c in cases {
        for key in ["a", "b", "haystack", "needle"] {
            if let Some(arr) = c.get(key).and_then(|v| v.as_array()) {
                for x in arr {
                    if let Some(i) = x.as_u64() {
                        max = max.max(i as usize + 1);
                    }
                }
            }
        }
    }
    (max + 1).max(1)
}

fn to_codes(v: &serde_json::Value) -> Vec<Code> {
    v.as_array()
        .expect("期望数组")
        .iter()
        .map(|x| x.as_u64().expect("期望整数") as Code)
        .collect()
}

fn load(name: &str) -> Vec<serde_json::Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", path.display()));
    let data: serde_json::Value = serde_json::from_str(&raw).expect("黄金数据不是合法 JSON");
    data["cases"].as_array().expect("缺少 cases 数组").clone()
}

/// 穷举域：全 900 对的分数与对齐四元组都必须一致。
#[test]
fn exhaustive_900_pairs_match_rapidfuzz() {
    let cases = load("pr_exhaustive.json");
    let ns = num_symbols(&cases);
    assert_eq!(cases.len(), 900, "语料应当是全 900 对（长度 1..4）");

    let mut score_bad: Vec<String> = Vec::new();
    let mut align_bad: Vec<String> = Vec::new();
    let mut align_total = 0usize;

    for c in &cases {
        let a = to_codes(&c["a"]);
        let b = to_codes(&c["b"]);
        let exp_score = c["score"].as_f64().unwrap();

        let got = partial_ratio_alignment(&a, &b, 0.0, ns);

        if (got.score - exp_score).abs() > 1e-9 {
            keep(
                &mut score_bad,
                format!("a={a:?} b={b:?} 期望={exp_score} 实际={}", got.score),
            );
        }

        if let Some(al) = c["align"].as_array() {
            align_total += 1;
            let want = (
                al[0].as_u64().unwrap() as usize,
                al[1].as_u64().unwrap() as usize,
                al[2].as_u64().unwrap() as usize,
                al[3].as_u64().unwrap() as usize,
            );
            let have = (got.src_start, got.src_end, got.dest_start, got.dest_end);
            if want != have {
                keep(
                    &mut align_bad,
                    format!("a={a:?} b={b:?} 期望={want:?} 实际={have:?}"),
                );
            }
        }
    }

    assert_eq!(align_total, cases.len(), "每条用例都该有对齐数据");
    assert!(
        score_bad.is_empty(),
        "{} 个分数不一致：\n{}",
        score_bad.len(),
        score_bad.join("\n")
    );
    assert!(
        align_bad.is_empty(),
        "{} 个对齐不一致：\n{}",
        align_bad.len(),
        align_bad.join("\n")
    );
}

/// 随机域：分数必须一致；rapidfuzz 给出 alignment 的用例上，对齐也必须一致；
/// 没给的（低于 cutoff / 0 分）我们这边必须是 0.0。
#[test]
fn random_2000_cases_match_rapidfuzz() {
    let cases = load("partial_ratio.json");
    let ns = num_symbols(&cases);
    assert_eq!(cases.len(), 2000, "语料应当是 2000 组随机用例");

    let mut score_bad: Vec<String> = Vec::new();
    let mut align_bad: Vec<String> = Vec::new();
    let mut none_bad: Vec<String> = Vec::new();
    let mut align_total = 0usize;
    // 语料里有对齐数据的那些用例，一条都不能被跳过——否则「对齐全一致」可能只是
    // 因为一条都没比（假绿）。
    let expected_align = cases
        .iter()
        .filter(|c| c.get("src_start").is_some())
        .count();

    for c in &cases {
        let hay = to_codes(&c["haystack"]);
        let ned = to_codes(&c["needle"]);
        let cutoff = c["cutoff"].as_f64().unwrap();
        // rapidfuzz 在 cutoff 以下返回 0.0（不是 None）
        let exp_score = c["partial_ratio"].as_f64().unwrap();

        let got = partial_ratio_alignment(&hay, &ned, cutoff, ns);

        if (got.score - exp_score).abs() > 1e-9 {
            keep(
                &mut score_bad,
                format!(
                    "hay={hay:?} ned={ned:?} cutoff={cutoff} 期望={exp_score} 实际={}",
                    got.score
                ),
            );
            continue;
        }

        // 分数一致后，再比对对齐：rapidfuzz 的绑定层对低于 cutoff / 0 分返回 None。
        if c.get("src_start").is_some() {
            align_total += 1;
            let want = (
                c["src_start"].as_u64().unwrap() as usize,
                c["src_end"].as_u64().unwrap() as usize,
                c["dest_start"].as_u64().unwrap() as usize,
                c["dest_end"].as_u64().unwrap() as usize,
            );
            let have = (got.src_start, got.src_end, got.dest_start, got.dest_end);
            if want != have {
                keep(
                    &mut align_bad,
                    format!("hay={hay:?} ned={ned:?} cutoff={cutoff} 期望={want:?} 实际={have:?}"),
                );
            }
        } else if got.score != 0.0 {
            keep(
                &mut none_bad,
                format!(
                    "hay={hay:?} ned={ned:?} cutoff={cutoff} 但得到分数={}",
                    got.score
                ),
            );
        }
    }

    assert!(
        score_bad.is_empty(),
        "{} 个分数不一致：\n{}",
        score_bad.len(),
        score_bad.join("\n")
    );
    assert_eq!(
        align_total, expected_align,
        "带对齐数据的用例必须逐条比对过（{expected_align} 条）"
    );
    assert!(
        align_bad.is_empty(),
        "{} 个对齐不一致：\n{}",
        align_bad.len(),
        align_bad.join("\n")
    );
    assert!(
        none_bad.is_empty(),
        "{} 个「不该有分数」的用例出现了分数：\n{}",
        none_bad.len(),
        none_bad.join("\n")
    );
}

/// `partial_ratio()` 与 `partial_ratio_alignment().score` 必须同源一致。
#[test]
fn score_function_agrees_with_alignment() {
    let cases = load("pr_exhaustive.json");
    let ns = num_symbols(&cases);
    for c in cases.iter().take(300) {
        let a = to_codes(&c["a"]);
        let b = to_codes(&c["b"]);
        assert_eq!(
            partial_ratio(&a, &b, 0.0, ns),
            partial_ratio_alignment(&a, &b, 0.0, ns).score
        );
    }
}

/// 只留前几条样本：断言消息要能直接贴进日志，不该被上千行淹掉。
fn keep(samples: &mut Vec<String>, line: String) {
    const MAX: usize = 8;
    if samples.len() < MAX {
        samples.push(line);
    }
}
