//! 位并行 `partial_ratio` —— 精确对齐 rapidfuzz C++ 的 `fuzz::partial_ratio_alignment`。
//!
//! Rust 的 `rapidfuzz` crate 只提供 `fuzz::ratio`，**没有** `partial_ratio` /
//! `partial_ratio_alignment` / `process`，而粗筛正是建立在这两个函数之上。因此这里
//! 按 rapidfuzz-cpp 的算法逐行复刻，以保证召回与对齐窗口完全一致——本项目的命中
//! 位置直接依赖 `alignment.src_start`。
//!
//! 「复刻得像不像」由 `tests/parity_partial_ratio.rs` 用两份**外部**黄金语料钉住：
//! `tests/golden/pr_exhaustive.json`（{0,1} 长度 1..4 的全部 900 对）与
//! `tests/golden/partial_ratio.json`（2000 组随机用例，含 cutoff），分数与对齐
//! 四元组逐条比对。改了这个文件就该重跑它。
//!
//! 算法出处（MIT）：
//! - `rapidfuzz/fuzz_impl.hpp::fuzz_detail::partial_ratio_impl`（分支定界搜索）
//! - `rapidfuzz/details/common.hpp::NormSim_to_NormDist`（注意其中的 `+0.00001` 松弛量）
//! - 位并行 LCS：Hyyrö《Bit-Parallel LCS-length Computation Revisited》
//!
//! # 性能设计
//!
//! 本模块被热路径调用 **十万次以上**（每个热词 × 每个候选窗口），因此：
//! 1. [`BlockPattern`] 的掩码表在**索引构建期**一次性算好，查询期零分配；
//! 2. 单块模式的 LCS 状态放在栈上，不进堆；
//! 3. 分支定界的 `scores` / `windows` 缓冲区通过 [`PartialRatioScratch`] 跨调用复用；
//! 4. [`BlockPattern::can_reach_cutoff`] 用多重集交集做**必要条件**预筛，
//!    在进入昂贵的窗口搜索前先砍掉绝大多数热词。

/// 音素编码。要求是**稠密小整数**（0..num_symbols），这样位并行掩码可以直接按符号索引。
pub type Code = u32;

/// 掩码剥离哨兵。对应 Python `rag_fast_batch.py` 中的 `remaining_input[idx] = -1`。
///
/// 取 `u32::MAX`，保证不与任何真实编码（从 0 起的稠密整数）相撞。
pub const MASKED: Code = u32::MAX;

/// 对应 C++ 的 `std::numeric_limits<size_t>::max()`，用作「尚未计算」标记。
const UNSET: u64 = u64::MAX;

/// `NormSim_to_NormDist`：归一化相似度阈值 → 归一化距离阈值。
///
/// rapidfuzz 在此加了 `1e-5` 的松弛量，**不能省略**，否则 `cutoff_dist` 会小 1，
/// 导致分支定界剪掉 rapidfuzz 会保留的窗口。
#[inline]
fn norm_sim_to_norm_dist(score_cutoff: f64) -> f64 {
    (1.0 - score_cutoff + 0.00001).min(1.0)
}

/// 带 `u64` 进位的加法，对应 rapidfuzz-rs 的 `carrying_add`。
#[inline]
fn carrying_add(lhs: u64, rhs: u64, carry: bool) -> (u64, bool) {
    let (partial, o1) = lhs.overflowing_add(rhs);
    let (result, o2) = partial.overflowing_add(carry as u64);
    (result, o1 || o2)
}

/// 对齐结果，对应 rapidfuzz 的 `ScoreAlignment<double>`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreAlignment {
    pub score: f64,
    pub src_start: usize,
    pub src_end: usize,
    pub dest_start: usize,
    pub dest_end: usize,
}

impl ScoreAlignment {
    /// 全 0 距离的初始状态，与 C++ `ScoreAlignment` 默认构造一致。
    fn zero(len: usize) -> Self {
        Self {
            score: 0.0,
            src_start: 0,
            src_end: len,
            dest_start: 0,
            dest_end: len,
        }
    }
}

/// 跨调用复用的临时缓冲区。
///
/// 分支定界每轮都要重算 `scores` 与窗口队列；把它们放到调用方持有的结构里，
/// 可以避免每窗口一次堆分配（实测这是原先的主要开销来源）。
#[derive(Debug, Default, Clone)]
pub struct PartialRatioScratch {
    /// 长度为 `len2 - len1`，初值 `UNSET` 表示该槽位尚未评估
    scores: Vec<u64>,
    windows: Vec<(usize, usize)>,
    new_windows: Vec<(usize, usize)>,
}

/// 位并行 LCS/Indel 模式（Hyyrö）的**预编译**形式。
///
/// 预编码 pattern 为每个符号的 64 位匹配掩码，之后每读入一个 haystack 元素只需
/// 一次「进位加 + 减 + 或」即可把 LCS 长度推进一格，整体 O(n·⌈m/64⌉)。
///
/// 掩码表在构建期算好后查询期只读，因此 [`crate::hotword::fast_rag`] 可以把它直接存进索引，
/// 让 `search(&self)` 全程零分配、只持共享引用。
#[derive(Debug, Clone)]
pub struct BlockPattern {
    len: usize,
    num_symbols: usize,
    /// `ceil(len / 64)`，至少为 1 以便掩码表始终可寻址
    words: usize,
    /// 扁平掩码表：`masks[symbol * words + word]`，长度 `num_symbols * words`
    masks: Vec<u64>,
    /// 「该符号在 pattern 中出现过」位图，长度 `num_symbols`，构建期由 [`Self::build`] 填好
    present: Vec<bool>,
    /// pattern 中出现的不同符号及其出现次数（按符号升序），供多重集预筛
    distinct: Vec<(u32, u32)>,
}

impl BlockPattern {
    pub fn new(pattern: &[Code], num_symbols: usize) -> Self {
        let mut sorted: Vec<Code> = pattern
            .iter()
            .copied()
            .filter(|&c| (c as usize) < num_symbols)
            .collect();
        sorted.sort_unstable();
        let mut distinct: Vec<(u32, u32)> = Vec::new();
        for c in sorted {
            match distinct.last_mut() {
                Some(last) if last.0 == c => last.1 += 1,
                _ => distinct.push((c, 1)),
            }
        }
        Self::build(pattern, num_symbols, distinct)
    }

    /// 只构建位并行掩码表，跳过 `distinct` 的统计。
    ///
    /// 供掩码剥离循环使用：`remaining` 每轮都在变，掩码表必须重建，而那里的
    /// `distinct` 永远用不上（[`BlockPattern::can_reach_cutoff`] 只在粗筛阶段跑）。
    /// **注意**：本构造器产出的对象不可用于 `can_reach_cutoff` / `multiset_overlap`，
    /// 它们会因 `distinct` 为空而恒返回 0。
    pub fn masks_only(pattern: &[Code], num_symbols: usize) -> Self {
        Self::build(pattern, num_symbols, Vec::new())
    }

    fn build(pattern: &[Code], num_symbols: usize, distinct: Vec<(u32, u32)>) -> Self {
        let len = pattern.len();
        let words = len.div_ceil(64).max(1);
        let mut masks = vec![0u64; num_symbols * words];
        let mut present = vec![false; num_symbols];

        // 越界编码（理论上不该出现）不参与匹配，与旧实现的 `mask()` 越界返回 0 一致
        for (i, &c) in pattern.iter().enumerate() {
            let s = c as usize;
            if s >= num_symbols {
                continue;
            }
            masks[s * words + i / 64] |= 1u64 << (i % 64);
            present[s] = true;
        }

        Self {
            len,
            num_symbols,
            words,
            masks,
            present,
            distinct,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 某符号在 pattern 中是否出现过。
    ///
    /// 等价于 C++ 里 `s1_chars` 预筛表的作用（窗口端字符不在 pattern 中则 Indel 必为
    /// `len1`、得分为 0），但不需要额外的分配。
    ///
    /// 查的是构建期填好的 `present` 位图，**不能**改成读 `masks[s * words]`：
    /// 掩码表按 `[符号][块]` 展平，`s * words` 起步只落在符号的第 0 块上，
    /// pattern 超过 64 个符号时，住在后面几块的符号会被误判成「没出现过」，
    /// 前缀/后缀窗口预筛就会把合法窗口整段丢掉。
    #[inline]
    pub fn contains(&self, ch: Code) -> bool {
        let s = ch as usize;
        s < self.num_symbols && self.present[s]
    }

    /// LCS 长度，等价于 C++ `lcs_seq_similarity`（无带状剪枝，结果精确）。
    pub fn lcs_similarity(&self, text: &[Code]) -> u32 {
        if self.len == 0 || text.is_empty() {
            return 0;
        }
        let ns = self.num_symbols;

        // 单块快路径：热词音素长度实测 ≤ 18，恒走这里；状态留在栈上不进堆
        if self.words == 1 {
            let m = &self.masks;
            let mut s = u64::MAX;
            for &ch in text {
                let matches = if (ch as usize) < ns {
                    m[ch as usize]
                } else {
                    0
                };
                let u = s & matches;
                // Hyyrö 单状态向量：s = (s + u) | (s - u)，进位直接丢弃
                s = s.wrapping_add(u) | s.wrapping_sub(u);
            }
            return (!s).count_ones();
        }

        let mut s = vec![u64::MAX; self.words];
        for &ch in text {
            let mut carry = false;
            let base = if (ch as usize) < ns {
                ch as usize * self.words
            } else {
                usize::MAX
            };
            for (word, sw) in s.iter_mut().enumerate() {
                let matches = if base == usize::MAX {
                    0
                } else {
                    self.masks[base + word]
                };
                let u = *sw & matches;
                let (x, c) = carrying_add(*sw, u, carry);
                carry = c;
                *sw = x | sw.wrapping_sub(u);
            }
        }
        s.iter().map(|x| (!x).count_ones()).sum()
    }

    /// Indel 距离 = `(len1 + len2) - 2·LCS`，对应 C++ `indel_distance`。
    ///
    /// `CachedIndel` 的默认 cutoff 是 `int64::max()`，因此 rapidfuzz 在
    /// `partial_ratio_impl` 中取到的距离**不会被截断**，这里返回精确值即可。
    #[inline]
    pub fn indel_distance(&self, text: &[Code]) -> u64 {
        (self.len + text.len()) as u64 - 2 * self.lcs_similarity(text) as u64
    }

    /// `fuzz::ratio(pattern, text)` = Indel 归一化相似度 × 100。
    #[inline]
    pub fn ratio(&self, text: &[Code]) -> f64 {
        let maximum = (self.len + text.len()) as f64;
        if maximum == 0.0 {
            return 100.0;
        }
        (1.0 - (self.indel_distance(text) as f64 / maximum)) * 100.0
    }

    /// pattern 与 text 的多重集交集 `Σ_c min(count_pattern(c), count_text(c))`。
    ///
    /// `text_counts` 按编码索引，长度至少覆盖 `num_symbols`；越界编码不贡献交集。
    pub fn multiset_overlap(&self, text_counts: &[u32]) -> usize {
        let mut overlap = 0usize;
        for &(sym, cnt) in &self.distinct {
            overlap += text_counts.get(sym as usize).copied().unwrap_or(0).min(cnt) as usize;
        }
        overlap
    }

    /// **必要条件**预筛：判定在给定相似度阈值下是否可能存在达标的窗口。
    ///
    /// 推导：C++ 侧会先交换，保证 `len1 = min(模式长, 文本长)` 落在 s1（较短者），
    /// 而 `partial_ratio_impl` 的归一化分母 `maximum` 恒为 `2 · len1`——
    /// **不是** `模式长 + 窗口长`。s1 也可能是文本（例如模式比整段输入还长），
    /// 此时 `maximum` 取的是输入长度的两倍，因此这里必须用 `min` 而不是模式长度。
    ///
    /// 于此，窗口想拿到 ≥ `cutoff` 的分数，其 Indel 距离必须
    /// `< 2·len1·(1 - cutoff/100 + 1e-5)`；而 Indel ≥ `len1 - |多重集交集|`
    /// （LCS 既不超过较短侧，也不超过公共多重集的元素数），于是
    /// `len1 - overlap < 2·len1·(1 - cutoff/100 + 1e-5)` 是必要条件。
    ///
    /// 该条件**只会误杀**——返回 false 时必然不可能达标，返回 true 时仍需完整搜索，
    /// 因此不影响召回，只影响速度。
    #[inline]
    pub fn can_reach_cutoff(
        &self,
        text_len: usize,
        text_counts: &[u32],
        score_cutoff: f64,
    ) -> bool {
        let len1 = self.len.min(text_len) as f64;
        if len1 == 0.0 {
            // 空输入 / 空模式的特例交给 `alignment_text_pattern` 判定
            return true;
        }
        let overlap = self.multiset_overlap(text_counts) as f64;
        let limit = 2.0 * len1 * norm_sim_to_norm_dist(score_cutoff / 100.0);
        len1 - overlap < limit
    }
}

/// `partial_ratio` 的分数（等价于 `partial_ratio_alignment(..).score`）。
///
/// 注意：低于 `score_cutoff` 时 rapidfuzz 返回 **0.0**（而不是真实分数），
/// Python 绑定层同样返回 `0.0` 而非 `None`。
pub fn partial_ratio(s1: &[Code], s2: &[Code], score_cutoff: f64, num_symbols: usize) -> f64 {
    partial_ratio_alignment(s1, s2, score_cutoff, num_symbols).score
}

/// 便捷入口：自行构建两侧的 [`BlockPattern`]。热路径请改用
/// [`alignment_text_pattern`] 复用预编译的掩码表。
pub fn partial_ratio_alignment(
    s1: &[Code],
    s2: &[Code],
    score_cutoff: f64,
    num_symbols: usize,
) -> ScoreAlignment {
    let b1 = BlockPattern::new(s1, num_symbols);
    let b2 = BlockPattern::new(s2, num_symbols);
    let mut scratch = PartialRatioScratch::default();
    alignment_text_pattern(s1, &b1, s2, &b2, score_cutoff, &mut scratch)
}

/// 以**预编译掩码表**执行对齐。
///
/// 返回值中 `src_*` 指向 `text`、`dest_*` 指向 `pattern`——
/// 这正是 Python 侧 `partial_ratio_alignment(remaining_input, phonemes)` 的取向。
///
/// 注意 C++ 把「长度交换」放在所有边界检查之前，这里保持同样的次序：
/// `text` 比 `pattern` 长时，内部会把 text 放到 s2 位置再把结果交换回来。
pub fn alignment_text_pattern(
    text: &[Code],
    text_block: &BlockPattern,
    pattern: &[Code],
    pattern_block: &BlockPattern,
    score_cutoff: f64,
    scratch: &mut PartialRatioScratch,
) -> ScoreAlignment {
    if score_cutoff > 100.0 {
        return ScoreAlignment::zero(text.len());
    }
    if text.is_empty() || pattern.is_empty() {
        return ScoreAlignment {
            score: if text.len() == pattern.len() {
                100.0
            } else {
                0.0
            },
            ..ScoreAlignment::zero(text.len())
        };
    }

    // s1 为较短者，约定与 C++ 一致
    let swapped = text.len() > pattern.len();
    let (s1, b1, s2, b2) = if swapped {
        (pattern, pattern_block, text, text_block)
    } else {
        (text, text_block, pattern, pattern_block)
    };

    let mut al = align_oriented(s1, b1, s2, b2, score_cutoff, scratch);
    if swapped {
        std::mem::swap(&mut al.src_start, &mut al.dest_start);
        std::mem::swap(&mut al.src_end, &mut al.dest_end);
    }
    al
}

/// 约定 `s1` 为较短者。长度相等且未满分时 rapidfuzz 会再跑一次反向比较。
fn align_oriented(
    s1: &[Code],
    b1: &BlockPattern,
    s2: &[Code],
    b2: &BlockPattern,
    score_cutoff: f64,
    scratch: &mut PartialRatioScratch,
) -> ScoreAlignment {
    let al = partial_ratio_impl(s1, b1, s2, score_cutoff, scratch);

    if al.score != 100.0 && s1.len() == s2.len() {
        let cutoff2 = score_cutoff.max(al.score);
        let mut other = partial_ratio_impl(s2, b2, s1, cutoff2, scratch);
        if other.score > al.score {
            std::mem::swap(&mut other.src_start, &mut other.dest_start);
            std::mem::swap(&mut other.src_end, &mut other.dest_end);
            return other;
        }
    }
    al
}

/// 对应 C++ `fuzz_detail::partial_ratio_impl`（分支定界 + 前缀/后缀补扫）。
///
/// 约定 `len1 <= len2`，`s1` 为模式（较短者）。
fn partial_ratio_impl(
    s1: &[Code],
    block: &BlockPattern,
    s2: &[Code],
    score_cutoff: f64,
    scratch: &mut PartialRatioScratch,
) -> ScoreAlignment {
    let len1 = s1.len();
    let len2 = s2.len();
    let mut res = ScoreAlignment::zero(len1);
    // C++ 中 `score_cutoff` 是运行中不断抬升的局部变量：每次刷新最优值后，
    // 低于该分数的候选被 `indel_normalized_similarity` 直接归零。
    let mut cutoff = score_cutoff;

    // ---- 1. 等长窗口上的分支定界搜索
    if len2 > len1 {
        let maximum = len1 * 2;
        let mut cutoff_dist =
            (maximum as f64 * norm_sim_to_norm_dist(score_cutoff / 100.0)).ceil() as u64;
        let mut best_dist = UNSET;

        let n_slots = len2 - len1;
        scratch.scores.clear();
        scratch.scores.resize(n_slots, UNSET);
        scratch.windows.clear();
        scratch.new_windows.clear();
        scratch.windows.push((0, n_slots - 1));

        while !scratch.windows.is_empty() {
            let pending = scratch.windows.len();
            for idx in 0..pending {
                let (first, second) = scratch.windows[idx];

                if scratch.scores[first] == UNSET {
                    let d = block.indel_distance(&s2[first..first + len1]);
                    scratch.scores[first] = d;
                    if d < cutoff_dist {
                        cutoff_dist = d;
                        best_dist = d;
                        res.dest_start = first;
                        res.dest_end = first + len1;
                        if best_dist == 0 {
                            res.score = 100.0;
                            return res;
                        }
                    }
                }

                if scratch.scores[second] == UNSET {
                    let d = block.indel_distance(&s2[second..second + len1]);
                    scratch.scores[second] = d;
                    if d < cutoff_dist {
                        cutoff_dist = d;
                        best_dist = d;
                        res.dest_start = second;
                        res.dest_end = second + len1;
                        if best_dist == 0 {
                            res.score = 100.0;
                            return res;
                        }
                    }
                }

                let cell_diff = second - first;
                if cell_diff == 1 {
                    continue;
                }

                // 中点已知代价可以外推：区间内最多还有这么多个格子能改善分数。
                // 这里必须用回绕算术对齐 C++ 的 size_t 无符号语义。
                let known_edits = scratch.scores[first].abs_diff(scratch.scores[second]);
                let max_score_improvement = cell_diff
                    .wrapping_sub((known_edits / 2) as usize)
                    .wrapping_div(2)
                    .wrapping_mul(2);
                let min_score = scratch.scores[first]
                    .min(scratch.scores[second])
                    .wrapping_sub(max_score_improvement as u64)
                    as i64;

                if min_score < cutoff_dist as i64 {
                    let center = cell_diff / 2;
                    scratch.new_windows.push((first, first + center));
                    scratch.new_windows.push((first + center, second));
                }
            }
            std::mem::swap(&mut scratch.windows, &mut scratch.new_windows);
            scratch.new_windows.clear();
        }

        let score = (1.0 - (best_dist as f64 / maximum as f64)) * 100.0;
        if score >= cutoff {
            cutoff = score;
            res.score = score;
        }
    }

    // ---- 2. 前缀窗口 `s2[0..i]`（i < len1）
    // rapidfuzz 用 s1 的字符集预筛：窗口末字符若不在 s1 中则 Indel 必为 len1，得分 0
    for i in 1..len1 {
        let last = s2[i - 1];
        if !block.contains(last) {
            continue;
        }
        // 低于当前 cutoff 的一律归零，等价于 C++ 的 indel_normalized_similarity
        let raw = block.ratio(&s2[0..i]);
        let ls = if raw >= cutoff { raw } else { 0.0 };
        if ls > res.score {
            cutoff = ls;
            res.score = ls;
            res.dest_start = 0;
            res.dest_end = i;
            if res.score == 100.0 {
                return res;
            }
        }
    }

    // ---- 3. 后缀窗口 `s2[i..len2]`（i >= len2 - len1）
    for i in (len2 - len1)..len2 {
        let first = s2[i];
        if !block.contains(first) {
            continue;
        }
        let raw = block.ratio(&s2[i..len2]);
        let ls = if raw >= cutoff { raw } else { 0.0 };
        if ls > res.score {
            cutoff = ls;
            res.score = ls;
            res.dest_start = i;
            res.dest_end = len2;
            if res.score == 100.0 {
                return res;
            }
        }
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotword::encoder::PhonemeEncoder;

    const NS: usize = 8;

    fn v(xs: &[u32]) -> Vec<Code> {
        xs.to_vec()
    }

    // ---------------------------------------------------------- 位并行原语

    /// 位并行 LCS 必须与 O(mn) DP 完全一致。
    #[test]
    fn bitparallel_lcs_matches_dp() {
        fn dp(a: &[Code], b: &[Code]) -> u32 {
            let mut prev = vec![0u32; b.len() + 1];
            let mut cur = vec![0u32; b.len() + 1];
            for i in 1..=a.len() {
                for j in 1..=b.len() {
                    cur[j] = if a[i - 1] == b[j - 1] {
                        prev[j - 1] + 1
                    } else {
                        prev[j].max(cur[j - 1])
                    };
                }
                std::mem::swap(&mut prev, &mut cur);
                cur.iter_mut().for_each(|x| *x = 0);
            }
            prev[b.len()]
        }

        // 确定性伪随机，覆盖长/短、重复/互不相同等形态
        let mut seed: u64 = 0x2545F491_4F6CDD1D;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..3000 {
            let m = (next() % 40) as usize + 1;
            let n = (next() % 40) as usize + 1;
            let alpha = (next() % 5) as u32 + 1;
            let a: Vec<Code> = (0..m).map(|_| (next() % alpha as u64) as Code).collect();
            let b: Vec<Code> = (0..n).map(|_| (next() % alpha as u64) as Code).collect();
            let bp = BlockPattern::new(&a, alpha as usize);
            assert_eq!(
                bp.lcs_similarity(&b),
                dp(&a, &b),
                "LCS 不一致: a={a:?} b={b:?}"
            );
        }
    }

    /// 超过 64 个符号时走多块路径，同样要与 DP 一致。
    #[test]
    fn bitparallel_lcs_multiblock() {
        fn dp(a: &[Code], b: &[Code]) -> u32 {
            let mut prev = vec![0u32; b.len() + 1];
            let mut cur = vec![0u32; b.len() + 1];
            for i in 1..=a.len() {
                for j in 1..=b.len() {
                    cur[j] = if a[i - 1] == b[j - 1] {
                        prev[j - 1] + 1
                    } else {
                        prev[j].max(cur[j - 1])
                    };
                }
                std::mem::swap(&mut prev, &mut cur);
                cur.iter_mut().for_each(|x| *x = 0);
            }
            prev[b.len()]
        }
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..300 {
            let m = (next() % 200) as usize + 60; // 强制跨块
            let n = (next() % 150) as usize + 60;
            let alpha = (next() % 6) as u32 + 1;
            let a: Vec<Code> = (0..m).map(|_| (next() % alpha as u64) as Code).collect();
            let b: Vec<Code> = (0..n).map(|_| (next() % alpha as u64) as Code).collect();
            let bp = BlockPattern::new(&a, alpha as usize);
            assert_eq!(bp.lcs_similarity(&b), dp(&a, &b), "多块 LCS 不一致");
        }
    }

    /// `contains` 必须覆盖 pattern 的**全部**掩码块。
    ///
    /// 旧实现读的是 `masks[s * words]`，恒等于第 0 块，于是只出现在 code 下标
    /// ≥ 64 的符号会被判成「没出现过」，前缀/后缀窗口预筛（`align_oriented`
    /// 第 2、3 步）会把本该保留的窗口直接跳过。下面两条带「旧代码上变红」标注
    /// 的断言就是断骨点；「pattern 里根本没有」与「越界编码」两条对照断言
    /// 新旧都绿，说明失败确实来自多块漏查而不是判定方向搞反。
    #[test]
    fn contains_covers_all_mask_blocks() {
        const NSY: usize = 4;
        // 下标 0..=63 全是符号 0，符号 1 从下标 64 起才出现 —— 它只住在第 1 块里
        let mut pattern: Vec<Code> = vec![0; 64];
        pattern.push(1);
        pattern.push(2);
        let bp = BlockPattern::new(&pattern, NSY);

        assert!(bp.contains(0), "第 0 块的符号");
        // ↓ 旧代码红在这里：`masks[1 * words]` 是符号 1 的第 0 块槽位，未被填过就是 0
        assert!(
            bp.contains(1),
            "只出现在第 1 块（下标 ≥ 64）的符号必须判为出现过"
        );
        assert!(bp.contains(2), "越界编码以外的普通符号");
        // ↓ 对照：pattern 里真的没有 / 越界，新旧都应判为没出现
        assert!(!bp.contains(3), "pattern 里根本没有的符号必须判为没出现");
        assert!(!bp.contains(NSY as Code), "越界编码一律判为没出现");

        // `masks_only` 走的是同一条 `build`，预筛结果必须与 `new` 一致
        let mo = BlockPattern::masks_only(&pattern, NSY);
        assert!(
            mo.contains(1),
            "masks_only 构造的预筛位图同样必须覆盖全部块"
        );
    }

    /// `contains` 与「直接看 pattern 有没有这个符号」必须逐一对齐（含跨块形态）。
    #[test]
    fn contains_matches_reference_on_multiblock_patterns() {
        let mut seed: u64 = 0x2545_F491_4F6C_DD1E;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let ns = 6usize;
        let mut late_only = 0usize;
        for _ in 0..500 {
            // 强制跨块，并**刻意**制造「只在后面几块出现」的符号：前 64 位全是符号 0，
            // 第 64 位起才引入其余符号。若整段随机取码，每个符号几乎必然在前 64 位
            // 就露过面，块偏移漏查会被完全掩盖——那样这条测试在旧代码上照样绿，等于没写。
            let m = (next() % 136) as usize + 65;
            let alpha = (next() % (ns as u64 - 1)) as u32 + 2;
            let mut a: Vec<Code> = vec![0; 64];
            a.extend((64..m).map(|_| (next() % alpha as u64) as Code));
            let bp = BlockPattern::new(&a, ns);
            let mut reference = vec![false; ns];
            for &c in &a {
                reference[c as usize] = true;
            }
            for (s, &expected) in reference.iter().enumerate() {
                assert_eq!(
                    bp.contains(s as Code),
                    expected,
                    "contains 与参照不一致: 符号 {s}，m={m}，alpha={alpha}"
                );
            }
            late_only += 1;
        }
        assert!(late_only > 0, "样本应全部为跨块 pattern");
    }

    // ---------------------------------------------------------- 语义回归

    #[test]
    fn semantics_match_rapidfuzz_observed() {
        // 这几条取自 rapidfuzz 实测（全量回放在 tests/parity_partial_ratio.rs）
        // pr([0], [1,0,0,0]) = 100, dest=(2,3) —— 分支定界先评估 window.first=0
        let al = partial_ratio_alignment(&v(&[0]), &v(&[1, 0, 0, 0]), 0.0, NS);
        assert_eq!(al.score, 100.0);
        assert_eq!((al.dest_start, al.dest_end), (2, 3));

        // pr([0,0], [0,1,0]) = 66.667（不是枚举子区间能拿到的 80.0）
        let al = partial_ratio_alignment(&v(&[0, 0]), &v(&[0, 1, 0]), 0.0, NS);
        assert_eq!(al.score, 200.0 / 3.0);
        assert_eq!((al.dest_start, al.dest_end), (0, 1));

        // pr([1,1], [0,1,0]) = 50
        let al = partial_ratio_alignment(&v(&[1, 1]), &v(&[0, 1, 0]), 0.0, NS);
        assert_eq!(al.score, 50.0);
        assert_eq!((al.dest_start, al.dest_end), (0, 2));

        // 对称性：pr(a,b) == pr(b,a)
        let a = v(&[1, 2, 3, 4, 5]);
        let b = v(&[5, 1, 2, 0, 9, 3]);
        assert_eq!(
            partial_ratio(&a, &b, 0.0, NS),
            partial_ratio(&b, &a, 0.0, NS)
        );
    }

    #[test]
    fn empty_inputs_follow_cpp() {
        assert_eq!(partial_ratio_alignment(&[], &[], 0.0, NS).score, 100.0);
        assert_eq!(partial_ratio_alignment(&[], &[1], 0.0, NS).score, 0.0);
        assert_eq!(partial_ratio_alignment(&[1], &[], 0.0, NS).score, 0.0);
        // score_cutoff > 100 直接返回 0
        assert_eq!(partial_ratio_alignment(&[1], &[1], 101.0, NS).score, 0.0);
    }

    /// 低于 cutoff 时分数被归零（Python 侧返回 0.0 而非真实分数）。
    #[test]
    fn below_cutoff_score_is_zero() {
        let hay = v(&[9, 9, 9, 9, 9, 9, 9, 9]);
        let pat = v(&[1, 2, 3, 4]);
        let loose = partial_ratio(&pat, &hay, 0.0, NS);
        let strict = partial_ratio(&pat, &hay, 99.0, NS);
        assert!(loose < 99.0);
        assert_eq!(strict, 0.0, "低于 cutoff 必须归零");
    }

    #[test]
    fn masked_sentinel_never_matches() {
        // 掩码位参与比较时不应产生匹配
        let hay = v(&[1, 2, MASKED, 3]);
        let al = partial_ratio_alignment(&v(&[1, 3]), &hay, 0.0, NS);
        assert!(al.score < 100.0, "掩码位不应被当作可匹配元素");
    }

    /// 预编译入口必须与便捷入口逐位一致（含 src/dest 方向）。
    #[test]
    fn precompiled_entry_matches_convenience() {
        let mut enc = PhonemeEncoder::new();
        let codes: Vec<Code> = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]
            .iter()
            .map(|s| enc.encode(s))
            .collect();
        let ns = enc.len();
        let mut seed: u64 = 0xDEADBEEF_CAFEF00D;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut scratch = PartialRatioScratch::default();
        for _ in 0..500 {
            let pl = (next() % 14) as usize + 1;
            let tl = (next() % 40) as usize + 1;
            let cutoff = [0.0, 40.0, 55.0, 80.0][(next() % 4) as usize];
            let pat: Vec<Code> = (0..pl).map(|_| codes[(next() % 12) as usize]).collect();
            let txt: Vec<Code> = (0..tl).map(|_| codes[(next() % 12) as usize]).collect();

            let expect = partial_ratio_alignment(&txt, &pat, cutoff, ns);
            let tb = BlockPattern::new(&txt, ns);
            let pb = BlockPattern::new(&pat, ns);
            let got = alignment_text_pattern(&txt, &tb, &pat, &pb, cutoff, &mut scratch);
            assert_eq!(
                got, expect,
                "预编译入口与便捷入口不一致 pat={pat:?} txt={txt:?}"
            );
        }
    }

    /// 必要条件预筛绝不能误杀真正的达标项。
    #[test]
    fn prefilter_never_rejects_a_true_positive() {
        let mut seed: u64 = 0x5DEE_CE66_D000_0001;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let ns = 12usize;
        let mut scratch = PartialRatioScratch::default();
        let mut checked = 0usize;
        for _ in 0..4000 {
            let pl = (next() % 16) as usize + 1;
            let tl = (next() % 50) as usize + 1;
            let cutoff = [30.0, 55.0, 70.0, 85.0][(next() % 4) as usize];
            let pat: Vec<Code> = (0..pl).map(|_| (next() % ns as u64) as Code).collect();
            let txt: Vec<Code> = (0..tl).map(|_| (next() % ns as u64) as Code).collect();

            let pb = BlockPattern::new(&pat, ns);
            let mut counts = vec![0u32; ns];
            for &c in &txt {
                counts[c as usize] += 1;
            }
            let prefilter_says_yes = pb.can_reach_cutoff(txt.len(), &counts, cutoff);

            let tb = BlockPattern::new(&txt, ns);
            let real =
                alignment_text_pattern(&txt, &tb, &pat, &pb, cutoff, &mut scratch).score >= cutoff;

            assert!(
                !real || prefilter_says_yes,
                "预筛误杀：pat={pat:?} txt={txt:?} cutoff={cutoff}"
            );
            if real {
                checked += 1;
            }
        }
        assert!(checked > 50, "样本里应有一定比例的真阳性，实际 {checked}");
    }
}
