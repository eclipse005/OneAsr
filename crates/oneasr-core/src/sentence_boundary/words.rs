use crate::subtitle::segmenter::WordToken;

use super::WordTokenDto;

pub(super) fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

pub(super) fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}
