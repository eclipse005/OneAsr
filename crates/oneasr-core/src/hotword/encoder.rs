//! 音素编码器：把音素值映射为**稠密整数编码**。
//!
//! 对齐 Python `rag_fast.PhonemeEncoder`。稠密编码（0..num_symbols）不只是省内存——
//! [`crate::hotword::partial_ratio`] 的位并行掩码表直接按编码索引，方码表查表是 O(1) 且无需哈希。

use std::collections::HashMap;

use crate::hotword::phoneme::Phoneme;

pub type Code = u32;

/// 构建期可变的编码器。
#[derive(Debug, Default)]
pub struct PhonemeEncoder {
    map: HashMap<Box<str>, Code>,
}

impl PhonemeEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个音素值，返回其编码。
    pub fn encode(&mut self, value: &str) -> Code {
        if let Some(&c) = self.map.get(value) {
            return c;
        }
        let code = self.map.len() as Code;
        self.map.insert(value.into(), code);
        code
    }

    pub fn encode_phonemes(&mut self, phonemes: &[Phoneme]) -> Vec<Code> {
        phonemes.iter().map(|p| self.encode(&p.value)).collect()
    }

    /// 冻结为只读视图（热路径用，无需加锁）。
    pub fn freeze(&self) -> EncoderView {
        EncoderView {
            map: self.map.clone(),
            num_symbols: self.map.len(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// 只读编码视图。
///
/// 搜索期输入文本可能出现构建期没见过的音素值（例如拼音表里缺字的整字兜底）。
/// 这些值不在任何热词模式中，位并行掩码查表会返回 0，即「永不匹配」——
/// 因此它们只需拿到一个 ≥ `num_symbols` 的、彼此可区分的编码即可，
/// 既不会误匹配，也不需要在 `&self` 上做内部可变的字典插入。
#[derive(Debug, Clone)]
pub struct EncoderView {
    map: HashMap<Box<str>, Code>,
    num_symbols: usize,
}

impl EncoderView {
    pub fn num_symbols(&self) -> usize {
        self.num_symbols
    }

    /// 编码单个音素值。未知值写入 `extras` 以保证同一搜索内彼此可区分。
    pub fn encode(&self, value: &str, extras: &mut HashMap<Box<str>, Code>) -> Code {
        if let Some(&c) = self.map.get(value) {
            return c;
        }
        if let Some(&c) = extras.get(value) {
            return c;
        }
        let code = (self.num_symbols + extras.len()) as Code;
        extras.insert(value.into(), code);
        code
    }

    pub fn encode_phonemes(
        &self,
        phonemes: &[Phoneme],
        extras: &mut HashMap<Box<str>, Code>,
    ) -> Vec<Code> {
        phonemes
            .iter()
            .map(|p| self.encode(&p.value, extras))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotword::phoneme::{Lang, get_phoneme_info};

    #[test]
    fn codes_are_dense_and_stable() {
        let mut enc = PhonemeEncoder::new();
        assert_eq!(enc.encode("s"), 0);
        assert_eq!(enc.encode("a"), 1);
        assert_eq!(enc.encode("s"), 0, "重复注册返回同一编码");
        assert_eq!(enc.encode("1"), 2);
        assert_eq!(enc.len(), 3);
    }

    #[test]
    fn unknown_values_get_distinct_codes() {
        let mut enc = PhonemeEncoder::new();
        enc.encode("s");
        let view = enc.freeze();
        let mut extras = HashMap::new();
        assert_eq!(view.encode("s", &mut extras), 0);
        let a = view.encode("龘", &mut extras);
        let b = view.encode("龗", &mut extras);
        assert_ne!(a, b, "两个未知值不能共用编码，否则会互相误匹配");
        assert!(a >= view.num_symbols() as u32);
    }

    #[test]
    fn encodes_real_phonemes() {
        let seq = get_phoneme_info("撒贝宁", true);
        let mut enc = PhonemeEncoder::new();
        let codes = enc.encode_phonemes(&seq);
        assert_eq!(codes.len(), 9);
        assert_eq!(seq[0].lang, Lang::Zh);
    }
}
