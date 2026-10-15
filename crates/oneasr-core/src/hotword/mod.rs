//! 音素级热词**筛选**（召回）：转录文本 → 相关热词 → Qwen3-ASR 的 `context`。
//!
//! 代码搬自上游 `asr-hotword`（Python 参考实现 + 它的 Rust 移植），但**只搬筛选
//! 那半边**：
//!
//! - **保留**：词表解析（一词一行）、文本→音素、短语词典（多音字消歧）、粗筛
//!   （[`fast_rag`] / [`partial_ratio`] / [`encoder`]）；
//! - **不搬**：`calc.rs`（模糊音权重编辑距离 + 字边界约束）与 `corrector.rs`
//!   （黑名单拦截、冲突消解、文本替换）。那两半是**纠错后处理**——下游要改用户
//!   的文字，所以阈值高、约束多；这里是**召回**，多给几条不致命，由 `top_k` 兜底。
//!
//! 热词表**只写正确写法**：用户不知道语音会被听错，只知道正确的词，所以没有别名
//! （`|`）与邻近黑名单（`~~~`）语法——听错由音素模糊匹配兜住（`逗神归来` 照样能
//! 召回 `窦神归来`，1.000）。那两样语法只服务上游的替换阶段，这里没有消费者。
//!
//! 用 [`HotwordIndex::recall`] 取相关热词、[`context_line`] 拼成提示词，注入
//! Qwen3-ASR 的第二趟解码（见 [`crate::asr`] 的粗转录阶段）。召回**不改文本**：
//! 热词在解码前就进提示词，模型一次成稿，不存在「把对的字改坏」。
//!
//! 与参考实现的取舍：本模块只走 [`fast_rag::FastRag::search`]（对应上游
//! `retrieve()`），不走 `correct()`+`similars`。上游那条路是为纠错分类服务的；
//! 恒等命中（原文本来就等于热词）在 `search` 里同样会以 1.0 分返回，不需要借道
//! `similars`——而恒等命中**必须保留**（不给提示时 Qwen3 会把 `API` 降成 `api`
//! 甚至逐字母拆开）。

pub mod encoder;
pub mod fast_rag;
pub mod partial_ratio;
pub mod phoneme;
pub mod phrases;
pub mod store;

pub use store::{
    DEFAULT_HOTWORD_GROUP_ID, DEFAULT_THRESHOLD, DEFAULT_TOP_K, HotwordGroup, HotwordLoadReport,
    HotwordRun, HotwordStore, count_group_words, display_name,
    display_name as hotword_group_display_name,
};

use fast_rag::FastRag;
use phoneme::{Phoneme, get_phoneme_info};

/// 解析热词表：按 `,` / `，` / 换行切分，去首尾空白、丢空项。
///
/// 只做这一件事——别名与黑名单语法不在这里，见模块头。中英文逗号与 `\n` / `\r`
/// 一视同仁，所以界面上的「逗号分隔文本框」、粘贴进来的换行、以及手改
/// `settings.json` 里的两种写法，落到这里都是同一份词表；界面展示与存储因此是
/// **同一个字符串**，不需要在 UI 边界做任何格式转换。
pub fn parse_word_list(text: &str) -> Vec<String> {
    text.split([',', '，', '\n', '\r'])
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 词表里有几个词（与 [`parse_word_list`] 同一口径）。
pub fn count_words(text: &str) -> usize {
    text.split([',', '，', '\n', '\r'])
        .filter(|word| !word.trim().is_empty())
        .count()
}

/// 热词索引：热词文本建一次，之后按转录文本递归召回。
pub struct HotwordIndex {
    rag: FastRag,
}

impl HotwordIndex {
    /// 用热词文本建索引——逗号或换行分隔（见 [`parse_word_list`]）。
    ///
    /// `threshold` 是粗筛分数下限（`0.0~1.0`）。索引构建后不可变，`recall`
    /// 取 `&self`——可并发读，无需加锁。
    pub fn new(hotword_text: &str, threshold: f64) -> Self {
        let mut by_name: Vec<(String, Vec<Vec<Phoneme>>)> = Vec::new();
        for word in parse_word_list(hotword_text) {
            let seq = get_phoneme_info(&word, true);
            // 音素化为空（纯标点、纯 emoji 之类）的词进不了索引——跳过。
            if !seq.is_empty() {
                by_name.push((word, vec![seq]));
            }
        }
        let mut rag = FastRag::new(threshold);
        rag.add_hotwords(
            by_name
                .iter()
                .map(|(name, seqs)| (name.as_str(), seqs.as_slice())),
        );
        Self { rag }
    }

    /// 生效的热词数。没有别名，所以等于词表里能被音素化的词条数。
    pub fn hotword_count(&self) -> usize {
        self.rag.hotword_count()
    }

    pub fn len(&self) -> usize {
        self.rag.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rag.is_empty()
    }

    /// 从一段文本里召回相关热词，**按分数降序**、最多 `top_k` 条。
    ///
    /// 重复的词只留最高分；空文本或空索引返回空。
    /// 顺序即相关度——注入时位置越靠前越有效（排在中后段的词基本不生效），
    /// 所以调用方**不能**在拿到结果后重排。
    pub fn recall(&self, text: &str, top_k: usize) -> Vec<(String, f64)> {
        if text.is_empty() || self.rag.is_empty() {
            return Vec::new();
        }
        let hits = self.rag.search(&get_phoneme_info(text, true), 0);
        let mut best: Vec<(String, f64)> = Vec::new();
        for (hotword, score, _end_pos) in hits {
            match best.iter_mut().find(|(word, _)| *word == hotword) {
                Some(slot) => slot.1 = slot.1.max(score),
                None => best.push((hotword, score)),
            }
        }
        best.sort_by(|a, b| b.1.total_cmp(&a.1));
        best.truncate(top_k);
        best
    }
}

/// 召回结果 → Qwen3-ASR 的 `context`（chat template 的 **system** turn 内容）。
///
/// 这个写法不是我们自己发明的，是照着官方原版来的，两边逐条对得上：
/// - **取值样例**：官方模型卡（`models/Qwen3-ASR-0.6B-hf/README.md` 的
///   “Context / hotwords” 一节）给的就是 `prompt="Vocabulary: Quilter, apostle, gospel."`
///   —— 前缀 `Vocabulary: `、逗号 + 空格分隔、句末一个句点；
/// - **落点**：官方把它塞进 system turn
///   （`qwen_asr/inference/qwen3_asr.py::_build_messages` 的
///   `{"role": "system", "content": context or ""}`）；本仓库经
///   `qwen3-asr-wgpu` 的 `TranscribeOptions::context` 走同一个槽位；
/// - **空串**：官方即使 `context=""` 也照发一个空 system turn，而那个 crate 的
///   `build_prompt` 在 `context` 为空时生成的 token 序列与之一致
///   （`<|im_start|>system\n<|im_end|>`），所以「没热词就整条不给」与官方
///   `context=""` 是完全相同的提示词——不存在「我们少了一个 system turn」。
///
/// 因此格式本身不要再自由发挥；改了它就该先去核对上面的官方两处。
/// **一条都没有时必须返回空串**：塞一个 `"Vocabulary: ."` 进去是在教模型一个
/// 不存在的词表，比不给提示更糟。
pub fn context_line(words: &[String]) -> String {
    if words.is_empty() {
        return String::new();
    }
    format!("Vocabulary: {}.", words.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同音误写照样召回：粗转录写错字，热词照样能被筛出来。
    #[test]
    fn recall_finds_a_hotword_from_a_misheard_transcript() {
        let index = HotwordIndex::new("窦神归来\n柳宗元\n", 0.85);
        let hits = index.recall("今天讲逗神归来的故事", 20);
        assert_eq!(
            hits.first().map(|(word, _)| word.as_str()),
            Some("窦神归来"),
            "同音误写必须召回：{hits:?}"
        );
    }

    /// 无关文本召回为空，不能把整个词表倒出来。
    #[test]
    fn unrelated_text_recalls_nothing() {
        let index = HotwordIndex::new("窦神归来\n柳宗元\n", 0.85);
        assert!(index.recall("今天天气不错", 20).is_empty());
    }

    /// 分数降序；同一个词重复写只算一条。
    #[test]
    fn recall_is_deduped_and_descending() {
        let index = HotwordIndex::new("Cloud\nCloud\n", 0.55);
        let hits = index.recall("use cloud models here", 10);
        assert_eq!(hits.len(), 1, "同一个词重复写不该算两条：{hits:?}");
        assert_eq!(hits[0].0, "Cloud");

        // 两个音素相近但不同的词：都该召回，且按分数降序。
        let two = HotwordIndex::new("Cloud\nClaude\n", 0.55);
        let scores: Vec<f64> = two
            .recall("cloud claude", 10)
            .iter()
            .map(|(_, s)| *s)
            .collect();
        assert!(scores.len() >= 2, "样本要能召回两条才有意义：{scores:?}");
        assert!(
            scores.windows(2).all(|w| w[0] >= w[1]),
            "必须降序：{scores:?}"
        );
    }

    /// `top_k` 截断生效。
    #[test]
    fn recall_honours_top_k() {
        let hot = "甲一\n甲二\n甲三\n甲四\n甲五\n";
        let index = HotwordIndex::new(hot, 0.3);
        assert_eq!(index.recall("甲一甲二甲三甲四甲五", 2).len(), 2);
    }

    /// 空热词表：不 panic、不召回。
    #[test]
    fn empty_hotword_text_is_inert() {
        let index = HotwordIndex::new("", 0.85);
        assert!(index.is_empty());
        assert!(index.recall("随便什么", 10).is_empty());
    }

    /// 纯标点 / 纯 emoji 的词音素化为空，不该进索引（也不该让构建 panic）。
    #[test]
    fn words_that_phonemize_to_nothing_are_skipped() {
        let index = HotwordIndex::new("。。。\n\n  \n窦神归来\n", 0.85);
        assert_eq!(index.len(), 1, "只应留下能音素化的那条");
    }

    /// CRLF / LF / 裸 CR 三种行尾都要能解析。
    #[test]
    fn line_endings_are_handled() {
        assert_eq!(parse_word_list("甲\n乙\n"), ["甲", "乙"]);
        assert_eq!(parse_word_list("甲\r\n乙\r\n"), ["甲", "乙"]);
        assert_eq!(parse_word_list("甲\r乙\r"), ["甲", "乙"]);
        assert_eq!(parse_word_list("  甲  \n\n乙"), ["甲", "乙"]);
    }

    /// 中英文逗号与换行是**同一种分隔**——界面上的逗号框、粘贴的换行、手改
    /// settings.json 的两种写法，落到词表必须是同一份结果。
    #[test]
    fn commas_and_newlines_split_alike() {
        assert_eq!(parse_word_list("甲,乙,丙"), ["甲", "乙", "丙"]);
        assert_eq!(parse_word_list("甲，乙，丙"), ["甲", "乙", "丙"]);
        assert_eq!(parse_word_list("甲, 乙，丙\n丁"), ["甲", "乙", "丙", "丁"]);
        // 空项（连续逗号、首尾逗号、纯空白）全部丢掉。
        assert_eq!(parse_word_list(",甲,,乙,"), ["甲", "乙"]);
        assert_eq!(parse_word_list("  ,  ,  "), Vec::<String>::new());
        assert_eq!(parse_word_list(""), Vec::<String>::new());
    }

    /// 计数与解析同口径，两处各写一份就会漂。
    #[test]
    fn count_matches_parse() {
        for text in [
            "",
            "   ",
            "甲",
            "甲,乙",
            "甲，乙,丙\n丁",
            ",甲,,乙,",
            "甲\r\n乙\r\n",
        ] {
            assert_eq!(
                count_words(text),
                parse_word_list(text).len(),
                "text={text:?}"
            );
        }
    }

    /// 提示词格式与「空则留空」的契约。
    ///
    /// 这条测试是**官方写法**的锚点：官方模型卡给的样例是
    /// `Vocabulary: Quilter, apostle, gospel.`（前缀 + 逗号空格 + 句点），
    /// 下面逐字符钉死，改了格式就必须先回去核对官方那两处（见
    /// [`context_line`] 的文档）。带空格的词（`Machine Translation`）原样进去，
    /// 不加引号、不做转义——官方那串就是自由文本。
    #[test]
    fn context_line_format_and_empty_contract() {
        assert_eq!(
            context_line(&["窦神归来".to_string(), "API".to_string()]),
            "Vocabulary: 窦神归来, API."
        );
        assert_eq!(
            context_line(&[
                "Machine Translation".to_string(),
                "A+".to_string(),
                "GPU".to_string()
            ]),
            "Vocabulary: Machine Translation, A+, GPU."
        );
        assert_eq!(context_line(&[]), "", "空召回必须留空，不能造假词表");
    }
}
