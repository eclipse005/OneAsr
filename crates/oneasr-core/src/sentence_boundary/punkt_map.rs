//! Map the sentence boundaries Punkt found in the joined text back onto word
//! token indices.
//!
//! Punkt works on the concatenated string and reports character offsets, while
//! the DP layout stage downstream needs word indices into `&[WordTokenDto]`.
//! This module is that translation.
//!
//! How it works:
//! 1. Walk the words with `text.find(word, search_from)` to accumulate each
//!    word's end offset in the text.
//! 2. For every Punkt boundary (a character offset), pick the word whose end
//!    lands nearest.
//! 3. Allow a 3-character tolerance, which absorbs the slight offsets Punkt's
//!    own tokenization can introduce.

use crate::sentence_boundary::WordTokenDto;

/// 字符位置 → word 索引映射的容差。
///
/// Punkt 内部会做自己的 tokenization，可能折叠多个空格或对 Unicode 标点
/// 做轻微调整。容差 3 字符足够覆盖这些情况，又不会误匹配到相邻 word。
const POSITION_TOLERANCE: usize = 3;

/// 把 Punkt 输出的句子列表转换成"每个句子末尾对应的 word 索引"列表。
///
/// 返回的索引指向每个句子的最后一个 word（即在 word 数组中的下标）。
/// 不包含最后一个句子的末尾（它就是 words 的最后一个 word，不需要切分）。
///
/// # 参数
/// - `text`: words 拼接后的完整文本（用 `join_words` 拼接）
/// - `sentences`: Punkt 返回的句子列表（`&str` 切片引用 `text`）
/// - `words`: word token 数组
pub(super) fn map_sentence_boundaries_to_word_indices(
    text: &str,
    sentences: &[&str],
    words: &[WordTokenDto],
) -> Vec<usize> {
    if words.is_empty() || sentences.len() <= 1 {
        return Vec::new();
    }

    // 预计算每个 word 在 text 中的结束字符位置
    let word_end_positions = compute_word_end_positions(text, words);

    // 预计算每个 Punkt 句子的结束字符位置
    let sentence_end_positions = compute_sentence_end_positions(text, sentences);

    // 对每个句子边界，找到结束位置最接近的 word 索引
    let mut result = Vec::with_capacity(sentence_end_positions.len());
    let mut word_idx = 0usize;

    for &sent_end in &sentence_end_positions {
        // 跳过结束位置明显早于当前句子边界的 word
        // （它们已经被前一个边界消费了）
        while word_idx < word_end_positions.len()
            && word_end_positions[word_idx] < sent_end.saturating_sub(POSITION_TOLERANCE)
        {
            word_idx += 1;
        }

        // 找到结束位置最接近 sent_end 的 word
        let mut best_idx: Option<usize> = None;
        let mut best_diff = usize::MAX;
        let mut probe = word_idx;
        while probe < word_end_positions.len() {
            let diff = word_end_positions[probe].abs_diff(sent_end);
            if diff <= POSITION_TOLERANCE && diff < best_diff {
                best_idx = Some(probe);
                best_diff = diff;
            }
            // 已经超过容差范围，不再继续
            if word_end_positions[probe] > sent_end + POSITION_TOLERANCE {
                break;
            }
            probe += 1;
        }

        if let Some(idx) = best_idx {
            // 避免重复添加同一个 word 索引（连续两个句子边界映射到同一 word 的情况）
            if result.last() != Some(&idx) {
                result.push(idx);
            }
            // 下次从下一个 word 开始搜索
            word_idx = idx + 1;
        }
    }

    // 移除指向最后一个 word 的边界（最后一个句子不需要切分）
    if result.last() == Some(&(words.len() - 1)) {
        result.pop();
    }

    result
}

/// 计算每个 word 在 `text` 中的结束字符位置（绝对偏移）。
///
/// 顺序扫描 words，用 `text.find(word, search_from)` 累计偏移。
/// 返回的 Vec 长度等于 words.len()。
fn compute_word_end_positions(text: &str, words: &[WordTokenDto]) -> Vec<usize> {
    let mut positions = Vec::with_capacity(words.len());
    let mut search_from = 0usize;

    for w in words {
        let trimmed = w.word.trim();
        if trimmed.is_empty() {
            // 空 word：保持上一个位置不变
            positions.push(search_from);
            continue;
        }
        match text[search_from..].find(trimmed) {
            Some(rel_pos) => {
                let abs_start = search_from + rel_pos;
                let abs_end = abs_start + trimmed.len();
                positions.push(abs_end);
                search_from = abs_end;
            }
            None => {
                // 找不到（理论上不应该发生）：保持当前位置
                positions.push(search_from);
            }
        }
    }

    positions
}

/// 计算每个 Punkt 句子的结束字符位置（绝对偏移），不含最后一个句子。
///
/// 句子可能是 `text` 的子切片（Punkt 正常返回），也可能是独立字符串
/// （测试场景），所以用 `find` 而不是指针差值来定位。
fn compute_sentence_end_positions(text: &str, sentences: &[&str]) -> Vec<usize> {
    let mut positions = Vec::with_capacity(sentences.len());
    let mut search_from = 0usize;

    // 不含最后一个句子（它是整段文本的末尾，不需要切分）
    for sent in sentences.iter().take(sentences.len().saturating_sub(1)) {
        let trimmed = sent.trim();
        if trimmed.is_empty() {
            continue;
        }
        match text[search_from..].find(trimmed) {
            Some(rel_pos) => {
                let abs_start = search_from + rel_pos;
                let abs_end = abs_start + trimmed.len();
                positions.push(abs_end);
                search_from = abs_end;
            }
            None => {
                // 找不到：用 sent 的原始长度作为 fallback
                let abs_end = search_from + sent.len();
                positions.push(abs_end);
                search_from = abs_end;
            }
        }
    }

    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_words(texts: &[&str]) -> Vec<WordTokenDto> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| WordTokenDto {
                start: i as f64,
                end: i as f64 + 0.5,
                word: t.to_string(),
            })
            .collect()
    }

    #[test]
    fn maps_simple_two_sentences() {
        let words = make_words(&["Hello", "world.", "Hi", "there."]);
        let text = "Hello world. Hi there.";
        let sentences: Vec<&str> = vec!["Hello world.", "Hi there."];

        let splits = map_sentence_boundaries_to_word_indices(text, &sentences, &words);
        // "world." 是索引 1，应该是切分点
        assert_eq!(splits, vec![1]);
    }

    #[test]
    fn maps_three_sentences() {
        let words = make_words(&["No.", "The", "answer", "is", "no.", "Next", "one."]);
        let text = "No. The answer is no. Next one.";
        let sentences: Vec<&str> = vec!["No.", "The answer is no.", "Next one."];

        let splits = map_sentence_boundaries_to_word_indices(text, &sentences, &words);
        // "No." = idx 0, "no." = idx 4
        assert_eq!(splits, vec![0, 4]);
    }

    #[test]
    fn handles_single_sentence_no_split() {
        let words = make_words(&["Hello", "world."]);
        let text = "Hello world.";
        let sentences: Vec<&str> = vec!["Hello world."];

        let splits = map_sentence_boundaries_to_word_indices(text, &sentences, &words);
        assert!(splits.is_empty());
    }

    #[test]
    fn handles_abbreviation_no_split() {
        let words = make_words(&["Mr.", "Smith", "went", "home."]);
        let text = "Mr. Smith went home.";
        let sentences: Vec<&str> = vec!["Mr. Smith went home."];

        let splits = map_sentence_boundaries_to_word_indices(text, &sentences, &words);
        assert!(splits.is_empty());
    }
}
