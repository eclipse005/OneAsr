//! 粗筛检索器（FastRAG）：批量扫描 + 掩码剥离多位置召回。
//!
//! 对齐 Python `hotword/rag_fast_batch.py`。Python 版靠 `rapidfuzz.process.extract`
//! 在 C++ 层一次性粗筛；Rust 版 `rapidfuzz` crate 没有 `process` 模块，
//! 这里改为显式遍历 + 自研位并行 [`crate::hotword::partial_ratio`]。
//!
//! **行为等价性说明**：`process.extract` 返回的顺序会影响
//! `_find_matches` 里 `seen_targets` 的插入顺序，进而影响最终 `similars` 的排列。
//! 本实现按「构建顺序 + 分数稳定降序」复现该顺序，并由对拍测试锁定。

use std::collections::HashMap;

use rayon::prelude::*;

use crate::hotword::encoder::{Code, EncoderView, PhonemeEncoder};
use crate::hotword::partial_ratio::{
    BlockPattern, MASKED, PartialRatioScratch, ScoreAlignment, alignment_text_pattern,
};
use crate::hotword::phoneme::{Lang, Phoneme};

/// 检索结果：`(热词, 分数, 结束位置)`。
pub type FastHit = (String, f64, usize);

/// 每个 rayon 工作线程私有的临时区，避免在热路径上反复分配。
#[derive(Default)]
struct MatchScratch {
    pr: PartialRatioScratch,
    /// 掩码剥离用的输入副本
    remaining: Vec<Code>,
}

struct Entry {
    /// 热词名在其所属目标词表中的下标（去重键用，Python 侧以字符串为键）
    name_idx: usize,
    hw: String,
    /// 在 `arena` 中的编码区间
    start: usize,
    len: usize,
    /// `int(hw_len * (1 - threshold))`
    osa_cutoff: usize,
    /// 该热词自己的粗筛门槛（百分制）。
    ///
    /// 字母名展开后音素序列的重复度远高于字符（`ay`/`iy`/`eh` 高频出现），
    /// `partial_ratio` 的分数分布整体上移：为字符级校准的同一 cutoff 在英文上
    /// 会放进一个数量级的假阳性候选，而第二阶段（逐候选对齐 + OSA 距离）正是
    /// 耗时大头。实测 1200 英文热词 × 1710 音素输入的纯噪声场景：
    /// cutoff 0.55 -> 1155 个假候选，cutoff 0.70 -> 111 个。
    /// 中文/数字路径沿用 `threshold`，互不影响。
    pr_cutoff: f64,
    /// 预编译的位并行掩码表。构建期算好，查询期只读，因此 `search` 无需加锁。
    block: BlockPattern,
}

/// 批量粗筛检索器。构建完成后索引不可变，`search` 取 `&self`（无锁并发读）。
pub struct FastRag {
    threshold: f64,
    en_threshold: f64,
    encoder: EncoderView,
    /// 所有热词音素编码的扁平存储
    arena: Vec<Code>,
    entries: Vec<Entry>,
    /// `(热词, 编码序列)` → entry 下标，用于复现 Python dict 的「首次插入位置保留」
    by_key: HashMap<(usize, Vec<Code>), usize>,
    /// 热词名 → entry 下标列表
    by_name: HashMap<String, Vec<usize>>,
    /// 非英文 entry 的下标，按构建顺序（Python `_non_en` 桶）
    non_en_entries: Vec<usize>,
    /// 英文 entry 的下标，按构建顺序（Python `_en` 桶）
    en_entries: Vec<usize>,
    hotword_count: usize,
}

impl FastRag {
    /// 以默认的英文门槛构造（`max(threshold, 0.70)`）。
    pub fn new(threshold: f64) -> Self {
        Self::with_en_threshold(threshold, threshold.max(0.70))
    }

    pub fn with_en_threshold(threshold: f64, en_threshold: f64) -> Self {
        Self {
            threshold,
            en_threshold,
            encoder: PhonemeEncoder::new().freeze(),
            arena: Vec::new(),
            entries: Vec::new(),
            by_key: HashMap::new(),
            by_name: HashMap::new(),
            non_en_entries: Vec::new(),
            en_entries: Vec::new(),
            hotword_count: 0,
        }
    }

    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    pub fn en_threshold(&self) -> f64 {
        self.en_threshold
    }

    pub fn hotword_count(&self) -> usize {
        self.hotword_count
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 批量添加热词。
    ///
    /// 遍历顺序必须与 Python 的 `for hw, phoneme_lists in hotwords.items()` 一致，
    /// 因为它决定了音素编码的分配顺序，而编码顺序会进入位并行掩码。
    pub fn add_hotwords<'a, I>(&mut self, hotwords: I)
    where
        I: IntoIterator<Item = (&'a str, &'a [Vec<Phoneme>])>,
    {
        let mut encoder = PhonemeEncoder::new();
        // 先按 Python 顺序把编码分配完，保证与参考实现一致。
        //
        // 名称 → 名称**首次出现**时的下标（Python dict 的键序），查表而不是每
        // 条线性扫一遍词表：上千条热词时那是 O(n²) 次字符串比较，而这张表顺手
        // 就是键序本身。键借用迭代器，不需要额外 clone 一份名字。
        let mut name_idx: HashMap<&str, usize> = HashMap::new();
        let mut staged: Vec<(String, Vec<Code>, bool)> = Vec::new();

        for (hw, phoneme_lists) in hotwords {
            if !name_idx.contains_key(hw) {
                let next = name_idx.len();
                name_idx.insert(hw, next);
            }
            for phonemes in phoneme_lists {
                if phonemes.is_empty() {
                    continue;
                }
                let codes: Vec<Code> = phonemes.iter().map(|p| encoder.encode(&p.value)).collect();
                // 以首个音素的语言判定该热词走哪条粗筛路径
                let is_en = phonemes[0].lang == Lang::En;
                staged.push((hw.to_string(), codes, is_en));
                self.hotword_count += 1;
            }
        }

        self.encoder = encoder.freeze();
        let num_symbols = self.encoder.num_symbols();

        for (hw, codes, is_en) in staged {
            let key = (
                *name_idx.get(hw.as_str()).expect("名称已登记"),
                codes.clone(),
            );
            if self.by_key.contains_key(&key) {
                // Python dict 重复键保留原位置，仅计数增加
                continue;
            }
            let idx = self.entries.len();
            let start = self.arena.len();
            let len = codes.len();
            let name_idx = key.0;
            let block = BlockPattern::new(&codes, num_symbols);
            // 英文走更高的粗筛门槛（见 `Entry::pr_cutoff`）
            let pr_cutoff = if is_en {
                self.threshold.max(self.en_threshold) * 100.0
            } else {
                self.threshold * 100.0
            };
            self.arena.extend_from_slice(&codes);
            self.entries.push(Entry {
                name_idx,
                hw: hw.clone(),
                start,
                len,
                osa_cutoff: (len as f64 * (1.0 - self.threshold)) as usize,
                pr_cutoff,
                block,
            });
            if is_en {
                self.en_entries.push(idx);
            } else {
                self.non_en_entries.push(idx);
            }
            self.by_key.insert(key, idx);
            if let Some(v) = self.by_name.get_mut(&hw) {
                v.push(idx);
            } else {
                self.by_name.insert(hw, vec![idx]);
            }
        }
    }

    /// 某热词的 entry 下标列表（别名顺序）。
    pub fn entries_of(&self, hw: &str) -> &[usize] {
        self.by_name.get(hw).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// 粗筛扫描一个语言桶，等价于一次
    /// `process.extract(input, bucket, scorer=partial_ratio, score_cutoff=<各自门槛>, limit=None)`。
    ///
    /// 内部按分数**稳定**降序，因此并列分数的相对次序由构建顺序决定——
    /// 这正是 `process.extract` 在单次调用内的返回顺序语义。
    fn scan_group(
        &self,
        group: &[usize],
        input: &[Code],
        text_block: &BlockPattern,
        input_counts: &[u32],
    ) -> Vec<(usize, f64)> {
        let mut matches: Vec<(usize, f64)> = group
            .par_iter()
            .map_init(PartialRatioScratch::default, |scratch, &idx| {
                let e = &self.entries[idx];
                let pr_cutoff = e.pr_cutoff;
                // 必要条件不成立时必然召回不到，直接跳过昂贵的窗口搜索
                if !e
                    .block
                    .can_reach_cutoff(input.len(), input_counts, pr_cutoff)
                {
                    return None;
                }
                let al = alignment_text_pattern(
                    input,
                    text_block,
                    &self.arena[e.start..e.start + e.len],
                    &e.block,
                    pr_cutoff,
                    scratch,
                );
                (al.score >= pr_cutoff).then_some((idx, al.score))
            })
            .flatten()
            .collect();

        // 稳定降序（保持构建顺序作为并列时的次序）
        matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        matches
    }

    /// 粗筛：返回全部超过阈值的 `(热词, 分数, end_pos)`。
    ///
    /// `top_k <= 0` 表示不限制。
    pub fn search(&self, input_phonemes: &[Phoneme], top_k: usize) -> Vec<FastHit> {
        if input_phonemes.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }

        let num_symbols = self.encoder.num_symbols();

        // 1. 编码输入（未知音素值拿到彼此可区分的越界编码）
        let mut extras: HashMap<Box<str>, Code> = HashMap::new();
        let input: Vec<Code> = self.encoder.encode_phonemes(input_phonemes, &mut extras);
        if input.is_empty() {
            return Vec::new();
        }

        // 掩码表预编译一次，供全部热词复用（否则每个热词都要重建一遍）
        let text_block = BlockPattern::new(&input, num_symbols);

        // 输入的多重集计数，供 `can_reach_cutoff` 做必要条件预筛。
        // 越界编码不贡献交集——它们在任何热词模式里都匹配不上。
        let mut input_counts = vec![0u32; num_symbols];
        for &c in &input {
            if let Some(slot) = input_counts.get_mut(c as usize) {
                *slot += 1;
            }
        }

        // 2. 批量粗筛，**分两次调用**以复现 Python 的两桶结构：
        //
        //    ```python
        //    matches  = extract(input, _non_en, score_cutoff=pr_cutoff)
        //    matches += extract(input, _en,     score_cutoff=en_cutoff)
        //    ```
        //
        //    两次 extract 的结果按「先非英文、后英文」直接拼接，**没有跨桶重排**。
        //    这一点可观察：*ST智胜（英文开头）与 长城电工（全中文）同为 1.0 分时，
        //    Python 总是把中文的那条排在前面。若改成全局单次扫描 + 统一排序，
        //    同分项的相对次序会跟着构建顺序漂移，进而让下游 `seen_targets` 的迭代序变化。
        //
        //    索引不可变，按 entry 并行扫描即可；`map_init` 为每个工作线程各备一份
        //    `PartialRatioScratch`，既避免共享可变状态，也避免了逐条目堆分配。
        let mut matches = self.scan_group(&self.non_en_entries, &input, &text_block, &input_counts);
        matches.extend(self.scan_group(&self.en_entries, &input, &text_block, &input_counts));

        // 3. 掩码剥离，多位置召回
        //
        //    每个候选词的剥离互不依赖（各自持有 `remaining` 副本），可按 `matches`
        //    的既有序并行展开。rayon 的 indexed 迭代器收集时保序，故最终 `results`
        //    的顺序与串行版完全相同（`matches` 已按「分数降序 + 构建次序」排好）。
        //
        //    注意：这里**必须**用每轮的 `remaining` 重建掩码表，不能复用 `input` 的表。
        //    掩码位在单块模式下是恒等操作，但长输入会走多块路径，此时 `carrying_add`
        //    会让掩码位经进位给状态 +1，进而改变 LCS 与最终选中的窗口。
        let per_match: Vec<Vec<(usize, f64, usize)>> = matches
            .par_iter()
            .map_init(MatchScratch::default, |ms, (idx, _score)| {
                let e = &self.entries[*idx];
                let pattern = &self.arena[e.start..e.start + e.len];
                let hw_len = e.len;
                let osa_cutoff = e.osa_cutoff;
                let mut out: Vec<(usize, f64, usize)> = Vec::new();

                if ms.remaining.len() != input.len() {
                    ms.remaining.resize(input.len(), 0);
                }
                ms.remaining.copy_from_slice(&input);

                loop {
                    // `remaining` 每轮都在变，掩码表只能重建；这里用 `masks_only`
                    // 省掉 `distinct` 的统计与排序（该循环用不上它）。
                    let rem_block = BlockPattern::masks_only(&ms.remaining, num_symbols);
                    let al: ScoreAlignment = alignment_text_pattern(
                        &ms.remaining,
                        &rem_block,
                        pattern,
                        &e.block,
                        e.pr_cutoff,
                        &mut ms.pr,
                    );
                    // rapidfuzz 绑定层在 score == 0 时返回 None（低于 cutoff 或无匹配）
                    if al.score == 0.0 {
                        break;
                    }
                    // `src_*` 对应第一个实参 remaining_input —— Python 侧正是这样取的
                    let (ds, de) = (al.src_start, al.src_end);
                    if ds >= de || de > input.len() {
                        break;
                    }
                    // 已掩码区间与本次对齐重叠则停止
                    if ms.remaining[ds..de].contains(&MASKED) {
                        break;
                    }

                    // 注意：这里取的是**原始** input（Python 亦然），可能含 MASKED
                    let aligned = &input[ds..de];
                    let dist = osa_distance(aligned, pattern);
                    if dist <= osa_cutoff {
                        let score = 1.0 - (dist as f64 / hw_len as f64);
                        let end_pos = ds + hw_len;
                        out.push((*idx, round3(score), end_pos));
                    }

                    for slot in ms.remaining[ds..de].iter_mut() {
                        *slot = MASKED;
                    }
                }
                out
            })
            .collect();
        let results: Vec<(usize, f64, usize)> = per_match.into_iter().flatten().collect();

        // 4. 按 (热词名, end_pos) 去重，保留最高分；保留首次插入顺序以复现 Python dict。
        //    注意键必须是**热词名**而非 entry 下标——同一目标的多个别名要合并到一条。
        //
        //    `best` 的值是 `(order 槽位, 分数)`：槽位才是 `order` 的合法下标，
        //    `results` 的下标不能直接拿来写 `order`（前者是去重后的槽，后者是原始位置）。
        let mut order: Vec<usize> = Vec::new();
        let mut best: HashMap<(usize, usize), (usize, f64)> = HashMap::new();
        for (i, r) in results.iter().enumerate() {
            let key = (self.entries[r.0].name_idx, r.2);
            match best.get(&key) {
                Some(&(slot, prev_score)) => {
                    // Python `if score > prev:` —— 严格大于才覆盖，平分保留先到的那条
                    if r.1 > prev_score {
                        order[slot] = i;
                        best.insert(key, (slot, r.1));
                    }
                }
                None => {
                    best.insert(key, (order.len(), r.1));
                    order.push(i);
                }
            }
        }

        let mut out: Vec<FastHit> = order
            .iter()
            .map(|&i| {
                let r = results[i];
                (self.entries[r.0].hw.clone(), r.1, r.2)
            })
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if top_k > 0 && out.len() > top_k {
            out.truncate(top_k);
        }
        out
    }
}

/// Python `round(score, 3)`：按精确十进制做「银行家舍入」（half-to-even）。
///
/// 不能用 Rust 的 `f64::round`，后者是 half-away-from-zero，会在恰好 .0005 的
/// 分数上与 Python 分道扬镳，进而影响去重时的 `score >` 比较。
pub fn round3(x: f64) -> f64 {
    let scaled = x * 1000.0;
    let floor = scaled.floor();
    let frac = scaled - floor;
    let rounded = if frac > 0.5 {
        floor + 1.0
    } else if frac < 0.5 {
        floor
    } else {
        // 恰好 .5：取偶
        if (floor as i64) % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    };
    rounded / 1000.0
}

/// Optimal String Alignment 距离（Levenshtein + 相邻换位，各计 1 次）。
///
/// 对应 `rapidfuzz.distance.OSA`。这里用 DP 实现而非依赖 crate：
/// 音素长度极短（hot.txt 实测 avg 11.4 / max 18），且只在通过粗筛的对齐上调用。
pub fn osa_distance(a: &[Code], b: &[Code]) -> usize {
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev2: Vec<usize> = vec![0; n + 1];
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(prev2[j - 2] + 1);
            }
            curr[j] = best;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotword::phoneme::get_phoneme_info;

    fn seq(s: &str) -> Vec<Phoneme> {
        get_phoneme_info(s, true)
    }

    #[test]
    fn osa_basic() {
        assert_eq!(osa_distance(&[1, 2, 3], &[1, 2, 3]), 0);
        assert_eq!(osa_distance(&[1, 2, 3], &[1, 4, 3]), 1);
        assert_eq!(osa_distance(&[1, 2], &[2, 1]), 1, "相邻换位只算一次");
        assert_eq!(osa_distance(&[], &[1, 2]), 2);
        assert_eq!(osa_distance(&[1, 2], &[]), 2);
    }

    #[test]
    fn round3_is_bankers_rounding() {
        assert_eq!(round3(0.8125), 0.812, "Python round(0.8125, 3) == 0.812");
        assert_eq!(round3(1.0), 1.0);
        assert_eq!(round3(0.5), 0.5);
        assert_eq!(round3(0.3335), 0.334);
    }

    #[test]
    fn finds_hotword_in_input() {
        let mut rag = FastRag::new(0.55);
        let hw = seq("撒贝宁");
        let aliased: Vec<Vec<Phoneme>> = vec![hw.clone()];
        let pairs: Vec<(&str, &[Vec<Phoneme>])> = vec![("撒贝宁", aliased.as_slice())];
        rag.add_hotwords(pairs.iter().map(|(a, b)| (*a, *b)));
        rag.search(&seq("我非常喜欢撒贝你说的新闻"), 0);
        let hits = rag.search(&seq("我非常喜欢撒贝你说的新闻"), 0);
        assert!(!hits.is_empty(), "应能召回 撒贝宁");
        assert_eq!(hits[0].0, "撒贝宁");
    }

    #[test]
    fn empty_index_returns_nothing() {
        let rag = FastRag::new(0.55);
        assert!(rag.search(&seq("随便什么"), 0).is_empty());
    }
}
