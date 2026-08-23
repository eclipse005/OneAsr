//! Language-aware segmentation data + word-boundary advice.
//!
//! The segmentation pipeline operates on ASR-produced tokens. All language-
//! specific knowledge is centralized here behind the [`LanguageProfile`] trait,
//! so the segmentation skeleton (semantic.rs hard-split + subtitle_layout.rs
//! DP) never branches on `if lang == "zh"`: it queries the profile for:
//!
//! - **abbreviations** — dotted tokens that must NOT split (Mr., p.m., U.S.)
//! - **connectors** — coordinator/subordinator words that mark a good DP cut
//!   (and/but/so …, 但是/因为/所以 …, しかし/だから …)
//! - **length units** — CJK/Thai count characters, Latin/Germanic count words
//! - **word-boundary advice** — jieba for Chinese (char-level ASR), None
//!   everywhere else
//!
//! `profile_for_lang` dispatches to one struct per supported language; an
//! unknown language falls back to [`DefaultProfile`] (empty tables, word
//! counting, no advisor). Adding a language = adding one struct + one match
//! arm, with zero changes to the segmentation skeleton.
//!
//! **Token data is never modified** — jieba only informs the cost, it does
//! not merge or split tokens. Token ↔ timestamp alignment stays sacred.

use std::collections::HashSet;
use std::sync::OnceLock;

use jieba_rs::{Jieba, TokenizeMode};

use crate::subtitle_length::SubtitleLengthPreset;

// ============================================================================
// Word-boundary advisors (used by Chinese profile)
// ============================================================================

/// Default advisor for languages where ASR token boundaries align with word
/// boundaries (English, Japanese, Korean, etc.). Returns `None` everywhere —
/// no extra cost penalty.
pub(super) struct DefaultAdvisor;

impl DefaultAdvisor {
    #[inline]
    fn is_word_boundary(&self, _: usize) -> Option<bool> {
        None
    }
}

/// Chinese advisor using jieba word segmentation.
///
/// Constructed from the concatenated text of a semantic span. jieba tokenizes
/// it once; we record the byte offsets where words end (= word boundaries).
/// A gap at a word-boundary offset returns `Some(true)`; a gap inside a word
/// returns `Some(false)`.
pub(super) struct JiebaAdvisor {
    /// Byte offsets (into the span text) that are word boundaries.
    boundaries: HashSet<usize>,
}

impl JiebaAdvisor {
    fn new(span_text: &str) -> Self {
        static JIEBA: OnceLock<Jieba> = OnceLock::new();
        let jieba = JIEBA.get_or_init(Jieba::new);
        let tokens = jieba.tokenize(span_text, TokenizeMode::Default, true);
        // Record BYTE offsets of word boundaries — the DP queries this set with
        // byte offsets (see subtitle_layout.rs `byte_offset[k]`).
        let boundaries: HashSet<usize> = tokens.iter().map(|t| t.byte_end).collect();
        Self { boundaries }
    }

    fn is_word_boundary(&self, byte_offset: usize) -> Option<bool> {
        if self.boundaries.contains(&byte_offset) {
            Some(true)
        } else {
            // Not a jieba word boundary → it's inside a word.
            Some(false)
        }
    }
}

/// Boxed advisor enum. Constructed once per span, queried O(1) per candidate
/// cut. Enum dispatch avoids vtable overhead in the DP hot loop.
pub(super) enum Advisor {
    #[allow(dead_code)]
    Default(DefaultAdvisor),
    Jieba(JiebaAdvisor),
}

impl Advisor {
    #[inline]
    pub(super) fn is_word_boundary(&self, byte_offset: usize) -> Option<bool> {
        match self {
            Advisor::Default(a) => a.is_word_boundary(byte_offset),
            Advisor::Jieba(a) => a.is_word_boundary(byte_offset),
        }
    }
}

// ============================================================================
// LanguageProfile trait + per-language data
// ============================================================================

/// All language-specific segmentation knowledge, queried uniformly by the
/// segmentation skeleton. One struct per supported language.
///
/// Methods are grouped by consumer:
/// - Layer 1 (semantic.rs): [`Self::abbreviations`]
/// - Layer 2 (subtitle_layout.rs DP): [`Self::connectors`],
///   [`Self::word_boundary_advisor`], [`Self::token_units`]
/// - Length budget: [`Self::source_limit`]
///
/// `key` and `is_char_based` are part of the trait's introspection surface
/// (used by tests and useful for debugging); they aren't queried by the
/// current segmentation skeleton, hence the allow.
#[allow(dead_code)]
pub(super) trait LanguageProfile {
    fn key(&self) -> &'static str;

    /// Dotted tokens (lowercased) that must NOT cause a split — abbreviations
    /// and titles where the `.` is part of the token, not a sentence end
    /// (e.g. "mr.", "p.m.", "u.s."). Empty for languages without a relevant
    /// Latin-dot convention.
    fn abbreviations(&self) -> &'static [&'static str];

    /// Lowercased coordinator/subordinator words whose *start* marks a good
    /// DP cut point inside an overlong span (e.g. "and", "but", "但是",
    /// "しかし"). The DP prefers cutting just before these.
    fn connectors(&self) -> &'static [&'static str];

    /// Word-boundary advisor for the concatenated span text. Chinese/jieba
    /// returns [`Advisor::Jieba`]; all others return [`Advisor::Default`].
    fn word_boundary_advisor(&self, span_text: &str) -> Advisor;

    /// Language-aware length units of a single token (word count or character
    /// count depending on the writing system). Used by the DP prefix sums.
    fn token_units(&self, token: &str) -> f64;

    /// Per-preset source-side subtitle length limit (words or chars).
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32;

    /// Per-preset display-CHARACTER limit (width budget). Latin languages use
    /// word limit × 5.5 so long words can't overflow the subtitle line; CJK
    /// languages already count by character and leave this unbounded.
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64;

    /// Additional function words (articles/prepositions/clitics) for this
    /// language beyond the global EN/CJK tables. Spaced Universal-3.5
    /// languages (es/fr/de/pt/it...) get the shared table; others default to
    /// none.
    fn function_words_left(&self) -> &'static [&'static str] {
        &[]
    }

    /// Whether this language uses Punkt sentence boundary detection
    /// (statistical abbreviation detection) instead of the hardcoded
    /// abbreviation table. Currently only English; adding a language
    /// here requires its TrainingData to be available.
    fn uses_punkt_sentence_boundary(&self) -> bool {
        false
    }

    /// Whether this language counts characters (not words) as length units.
    fn is_char_based(&self) -> bool;

    /// Extra units allowed above [`Self::source_limit`] in quality mode.
    /// Japanese/Korean keep a bunsetsu together (助词/属格) instead of
    /// slicing at the raw cap.
    fn length_grace_units(&self) -> f64 {
        super::boundary_rules::LENGTH_GRACE_UNITS
    }

    /// Force-mode ceiling. JA/KO may exceed the target so a bunsetsu is not
    /// split; other languages keep the raw cap so existing tests hold.
    fn force_unit_ceiling(&self, limit: f64) -> f64 {
        limit
    }
}

/// Resolve a BCP-47-ish language tag to a profile.
pub(super) fn profile_for_lang(lang: &str) -> Box<dyn LanguageProfile> {
    match language_key(lang).as_str() {
        "zh" | "yue" => Box::new(ChineseProfile),
        "ja" => Box::new(JapaneseProfile),
        "ko" => Box::new(KoreanProfile),
        "en" => Box::new(EnglishProfile),
        "fr" => Box::new(FrenchProfile),
        "de" => Box::new(GermanProfile),
        "es" => Box::new(SpanishProfile),
        "pt" => Box::new(PortugueseProfile),
        "it" => Box::new(ItalianProfile),
        "ar" => Box::new(ArabicProfile),
        "th" => Box::new(ThaiProfile),
        _ => Box::new(DefaultProfile),
    }
}

fn language_key(lang: &str) -> String {
    let trimmed = lang.trim();
    let end = trimmed.find(['-', '_']).unwrap_or(trimmed.len());
    trimmed[..end].to_ascii_lowercase()
}


/// Shared function-word guards for spaced EU/misc languages (articles,
/// prepositions, clitics).
const U35_SPACED_CONNECTORS: &[&str] = &[
    "y", "o", "pero", "porque", "cuando", "mientras", "entonces", "aunque", "donde", "sino",
    "et", "ou", "mais", "parce", "quand", "pendant", "donc", "alors",
    "und", "oder", "aber", "weil", "wenn", "während", "dass", "obwohl", "deshalb",
    "e", "mas", "quando", "enquanto", "então", "embora",
    "ma", "perché", "mentre", "allora", "però",
    "en", "maar", "omdat", "wanneer", "terwijl", "als",
    "och", "eller", "men", "för", "när", "om", "og", "fordi", "hvis",
    "ja", "tai", "mutta", "koska", "kun", "jos",
    "i", "lub", "ale", "ponieważ", "kiedy", "jeśli",
    "ve", "veya", "ama", "çünkü", "eğer", "sonra",
    "và", "hoặc", "nhưng", "vì", "khi", "nếu", "thì", "nên",
    "और", "या", "लेकिन", "क्योंकि", "जब", "यदि", "तो",
    "و", "أو", "لكن", "لأن", "عندما", "إذا", "ثم",
    "או", "אבל", "כי", "אם", "אז",
    "и", "или", "но", "потому", "когда", "если",
];

const U35_SPACED_FUNCTION_WORDS: &[&str] = &[
    // articles / prep / clitic (es/fr/de/pt/it/nl)
    "el", "la", "los", "las", "un", "una", "unos", "unas", "de", "del", "al",
    "por", "para", "con", "sin", "su", "sus", "mi", "mis", "tu", "tus",
    "es", "son", "está", "están", "se", "lo", "le", "les",
    "une", "du", "des", "au", "aux", "à", "par", "pour", "avec",
    "sans", "sur", "son", "sa", "ses", "mon", "ma", "mes", "ton", "ta", "tes",
    "est", "sont", "ce", "cet", "cette", "ces", "je", "il", "elle", "nous", "vous",
    "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem",
    "einer", "eines", "von", "zu", "mit", "für", "auf", "aus", "bei", "nach",
    "über", "unter", "ist", "sind", "im", "am", "vom", "zum", "zur", "sich",
    "o", "os", "as", "um", "uma", "do", "da", "dos", "das", "em", "no", "na",
    "nos", "nas", "é", "são", "seu", "sua", "seus", "suas",
    "il", "gli", "uno", "della", "dello", "dei", "delle", "nel", "nella",
    "è", "sono", "si", "suo", "sua", "suoi", "sue",
    "het", "een", "van", "te", "voor", "op", "aan", "zijn",
    // Vietnamese / Hindi / Arabic / Hebrew / Turkish
    "của", "là", "các", "những", "một", "trong", "với", "để", "không",
    "được", "đã", "sẽ", "này", "đó",
    "का", "के", "की", "में", "से", "को", "है", "हैं", "था", "थे", "एक", "यह", "वह",
    "तो", "ही", "भी",
    "في", "من", "إلى", "على", "أن", "إن", "هذا", "هذه", "التي", "الذي", "هو", "هي",
    "של", "את", "על", "אל", "עם", "זה", "זו", "היא", "הוא",
    "bir", "bu", "şu", "ile", "için",
];

/// Latin display-character cap ≈ word limit × 5.5 (rounded), matching
/// EggTranslate's CHARS_PER_WORD_BUDGET (short 66 / standard 88 / loose 110).
fn latin_char_limit_for(word_limit: u32) -> f64 {
    (f64::from(word_limit) * 5.5).round()
}

// ---- char-unit counting (shared, single source of truth) ----

/// Character-based unit count: each CJK/alphabetic char is 1 unit; a run of
/// ASCII alphanumeric chars (e.g. "GDP") counts as 1 unit so Latin words
/// embedded in CJK text don't inflate the count.
fn count_char_units(text: &str) -> usize {
    let mut total = 0usize;
    let mut in_ascii_group = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if in_ascii_group {
                total += 1;
                in_ascii_group = false;
            }
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            in_ascii_group = true;
            continue;
        }
        if in_ascii_group {
            total += 1;
            in_ascii_group = false;
        }
        if is_cjk_char(ch) || ch.is_alphanumeric() {
            total += 1;
        }
    }
    if in_ascii_group {
        total += 1;
    }
    total
}

pub(super) fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF
    )
}

// ============================================================================
// English
// ============================================================================

struct EnglishProfile;

/// 英文缩写表已清空：Punkt 统计学习负责识别缩写（Mr./Dr./p.m./U.S. 等）。
/// 保留空数组只是为了让 `abbreviations()` 的返回类型与其他 profile 一致。
const ENGLISH_ABBREVIATIONS: &[&str] = &[];

const ENGLISH_CONNECTORS: &[&str] = &[
    "and", "but", "or", "so", "because", "when", "while", "which", "that", "if", "then", "though",
    "although", "however", "therefore", "before", "where", "who", "what", "whom", "whose", "as",
    "until", "once", "since", "unless", "whereas",
];

impl LanguageProfile for EnglishProfile {
    fn key(&self) -> &'static str {
        "en"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        ENGLISH_ABBREVIATIONS
    }
    fn connectors(&self) -> &'static [&'static str] {
        ENGLISH_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 12,
            SubtitleLengthPreset::Standard => 16,
            SubtitleLengthPreset::Loose => 20,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn uses_punkt_sentence_boundary(&self) -> bool {
        true
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

// ============================================================================
// Chinese (zh / yue) — char-based, jieba advisor
// ============================================================================

struct ChineseProfile;

const CHINESE_CONNECTORS: &[&str] = &[
    "但是", "因为", "所以", "而且", "或者", "如果", "虽然", "因此", "不过", "然后", "可是", "然而",
    "另外", "并且", "所以", "为了", "以及", "还有", "及其",
];

impl LanguageProfile for ChineseProfile {
    fn key(&self) -> &'static str {
        "zh"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &[]
    }
    fn connectors(&self) -> &'static [&'static str] {
        CHINESE_CONNECTORS
    }
    fn word_boundary_advisor(&self, span_text: &str) -> Advisor {
        Advisor::Jieba(JiebaAdvisor::new(span_text))
    }
    fn token_units(&self, token: &str) -> f64 {
        cjk_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 16,
            SubtitleLengthPreset::Standard => 22,
            SubtitleLengthPreset::Loose => 28,
        }
    }
    fn source_char_limit(&self, _preset: SubtitleLengthPreset) -> f64 {
        f64::INFINITY // char-based: units already are chars
    }
    fn is_char_based(&self) -> bool {
        true
    }
}

// ============================================================================
// Japanese — char-based, no jieba (whitespace tokenization)
// ============================================================================

struct JapaneseProfile;

const JAPANESE_CONNECTORS: &[&str] = &[
    "しかし", "だから", "そして", "でも", "また", "または", "ゆえに",
    "そのため", "さらに", "それで", "それから",
    // Spoken turn-takers / new-move starters: cutting BEFORE them is good.
    "はい", "じゃあ", "それでは", "なるほど", "えっと", "ええ",
    "まずは", "まず", "次に", "ところで", "ちなみに", "実は", "もっと",
    "皆さん", "みなさん", "みんな", "こんにちは", "こんばんは",
];

impl LanguageProfile for JapaneseProfile {
    fn key(&self) -> &'static str {
        "ja"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &[]
    }
    fn connectors(&self) -> &'static [&'static str] {
        JAPANESE_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        cjk_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 16,
            SubtitleLengthPreset::Standard => 22,
            SubtitleLengthPreset::Loose => 28,
        }
    }
    fn source_char_limit(&self, _preset: SubtitleLengthPreset) -> f64 {
        f64::INFINITY
    }
    fn is_char_based(&self) -> bool {
        true
    }
    fn length_grace_units(&self) -> f64 {
        6.0
    }
    fn force_unit_ceiling(&self, limit: f64) -> f64 {
        limit + self.length_grace_units()
    }
}

// ============================================================================
// Korean — char-based
// ============================================================================

struct KoreanProfile;

const KOREAN_CONNECTORS: &[&str] = &[
    "하지만", "그래서", "왜냐하면", "그리고", "또는", "만약", "비록", "따라서", "그러나", "그러므로",
    "그런데", "및", "왜냐면",
];

impl LanguageProfile for KoreanProfile {
    fn key(&self) -> &'static str {
        "ko"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &[]
    }
    fn connectors(&self) -> &'static [&'static str] {
        KOREAN_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        cjk_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 15,
            SubtitleLengthPreset::Standard => 20,
            SubtitleLengthPreset::Loose => 26,
        }
    }
    fn source_char_limit(&self, _preset: SubtitleLengthPreset) -> f64 {
        f64::INFINITY
    }
    fn is_char_based(&self) -> bool {
        true
    }
    fn length_grace_units(&self) -> f64 {
        6.0
    }
    fn force_unit_ceiling(&self, limit: f64) -> f64 {
        limit + self.length_grace_units()
    }
}

// ============================================================================
// Thai — char-based (no inter-word spaces)
// ============================================================================

struct ThaiProfile;

impl LanguageProfile for ThaiProfile {
    fn key(&self) -> &'static str {
        "th"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &[]
    }
    fn connectors(&self) -> &'static [&'static str] {
        &[]
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        cjk_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 24,
            SubtitleLengthPreset::Standard => 32,
            SubtitleLengthPreset::Loose => 42,
        }
    }
    fn source_char_limit(&self, _preset: SubtitleLengthPreset) -> f64 {
        f64::INFINITY
    }
    fn is_char_based(&self) -> bool {
        true
    }
}

// ============================================================================
// Latin/Germanic European languages — word-based
// ============================================================================

struct FrenchProfile;
const FRENCH_CONNECTORS: &[&str] =
    &["donc", "mais", "et", "ou", "parce", "quand", "bien", "cependant", "néanmoins"];
impl LanguageProfile for FrenchProfile {
    fn key(&self) -> &'static str {
        "fr"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        // French keeps the Mr./Dr. convention but lowercased variants only;
        // multi-letter Latin abbreviations like "etc." are shared.
        &["m.", "mme.", "mlle.", "dr.", "prof.", "etc.", "p.m.", "a.m."]
    }
    fn connectors(&self) -> &'static [&'static str] {
        FRENCH_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    // French has long compound words; uses the long-word source limit.
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 11,
            SubtitleLengthPreset::Standard => 14,
            SubtitleLengthPreset::Loose => 18,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn function_words_left(&self) -> &'static [&'static str] {
        U35_SPACED_FUNCTION_WORDS
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

struct GermanProfile;
const GERMAN_CONNECTORS: &[&str] = &["und", "oder", "aber", "weil", "wenn", "daher", "doch", "jedoch"];
impl LanguageProfile for GermanProfile {
    fn key(&self) -> &'static str {
        "de"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &["usw.", "bzw.", "etc.", "ca.", "u.a.", "z.b."]
    }
    fn connectors(&self) -> &'static [&'static str] {
        GERMAN_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 11,
            SubtitleLengthPreset::Standard => 14,
            SubtitleLengthPreset::Loose => 18,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn function_words_left(&self) -> &'static [&'static str] {
        U35_SPACED_FUNCTION_WORDS
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

struct SpanishProfile;
const SPANISH_CONNECTORS: &[&str] = &[
    "pero", "porque", "y", "o", "cuando", "aunque", "sin", "por", "además", "entonces",
];
impl LanguageProfile for SpanishProfile {
    fn key(&self) -> &'static str {
        "es"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &["sr.", "sra.", "srta.", "dr.", "prof.", "etc."]
    }
    fn connectors(&self) -> &'static [&'static str] {
        SPANISH_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 12,
            SubtitleLengthPreset::Standard => 16,
            SubtitleLengthPreset::Loose => 20,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn function_words_left(&self) -> &'static [&'static str] {
        U35_SPACED_FUNCTION_WORDS
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

struct PortugueseProfile;
const PORTUGUESE_CONNECTORS: &[&str] = &[
    "mas", "porque", "e", "ou", "quando", "embora", "portanto", "então", "além",
];
impl LanguageProfile for PortugueseProfile {
    fn key(&self) -> &'static str {
        "pt"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &["sr.", "sra.", "dr.", "prof.", "etc."]
    }
    fn connectors(&self) -> &'static [&'static str] {
        PORTUGUESE_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 12,
            SubtitleLengthPreset::Standard => 16,
            SubtitleLengthPreset::Loose => 20,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn function_words_left(&self) -> &'static [&'static str] {
        U35_SPACED_FUNCTION_WORDS
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

struct ItalianProfile;
const ITALIAN_CONNECTORS: &[&str] = &[
    "quindi", "però", "ma", "perché", "e", "o", "quando", "anche", "dunque", "inoltre",
];
impl LanguageProfile for ItalianProfile {
    fn key(&self) -> &'static str {
        "it"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &["sig.", "sig.ra", "dr.", "prof.", "etc."]
    }
    fn connectors(&self) -> &'static [&'static str] {
        ITALIAN_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 12,
            SubtitleLengthPreset::Standard => 16,
            SubtitleLengthPreset::Loose => 20,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn function_words_left(&self) -> &'static [&'static str] {
        U35_SPACED_FUNCTION_WORDS
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

struct ArabicProfile;
impl LanguageProfile for ArabicProfile {
    fn key(&self) -> &'static str {
        "ar"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &[]
    }
    fn connectors(&self) -> &'static [&'static str] {
        &[
            "ولكن", "لأن", "لذلك", "ثم", "أو", "عندما", "حيث", "لكن", "بسبب", "إضافة",
        ]
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 12,
            SubtitleLengthPreset::Standard => 16,
            SubtitleLengthPreset::Loose => 20,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn function_words_left(&self) -> &'static [&'static str] {
        U35_SPACED_FUNCTION_WORDS
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

/// Fallback for unrecognized spaced languages: U3.5 connector/function-word
/// tables, word counting, no advisor.
struct DefaultProfile;
impl LanguageProfile for DefaultProfile {
    fn key(&self) -> &'static str {
        "default"
    }
    fn abbreviations(&self) -> &'static [&'static str] {
        &[]
    }
    fn connectors(&self) -> &'static [&'static str] {
        U35_SPACED_CONNECTORS
    }
    fn word_boundary_advisor(&self, _: &str) -> Advisor {
        Advisor::Default(DefaultAdvisor)
    }
    fn token_units(&self, token: &str) -> f64 {
        latin_token_units(token)
    }
    fn source_limit(&self, preset: SubtitleLengthPreset) -> u32 {
        match preset {
            SubtitleLengthPreset::Short => 12,
            SubtitleLengthPreset::Standard => 16,
            SubtitleLengthPreset::Loose => 20,
        }
    }
    fn source_char_limit(&self, preset: SubtitleLengthPreset) -> f64 {
        latin_char_limit_for(self.source_limit(preset))
    }
    fn function_words_left(&self) -> &'static [&'static str] {
        U35_SPACED_FUNCTION_WORDS
    }
    fn is_char_based(&self) -> bool {
        false
    }
}

// ============================================================================
// token_units helpers — single source of truth shared by all profiles
// ============================================================================

fn latin_token_units(token: &str) -> f64 {
    // A Latin-language token that nonetheless contains CJK chars (rare mixed
    // ASR output like "GDP增长率") falls back to character counting so its
    // length isn't undercounted as a single word.
    if token.chars().any(is_cjk_char) {
        let units = count_char_units(token);
        return if units == 0 { 0.0 } else { units as f64 };
    }
    if token.chars().any(|ch| ch.is_alphanumeric()) {
        1.0
    } else {
        0.0
    }
}

fn cjk_token_units(token: &str) -> f64 {
    // Char-based languages count characters; a single token may contain
    // multiple CJK chars (zh ASR emits per-char tokens but mixed tokens exist).
    let units = count_char_units(token);
    if units == 0 {
        0.0
    } else {
        units as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_profile_has_abbreviations_and_connectors() {
        let p = EnglishProfile;
        assert_eq!(p.key(), "en");
        // 英文缩写表已清空，由 Punkt 统计学习负责识别
        assert!(p.abbreviations().is_empty());
        assert!(p.uses_punkt_sentence_boundary());
        assert!(p.connectors().contains(&"and"));
        assert!(p.connectors().contains(&"where"));
        assert!(!p.is_char_based());
    }

    #[test]
    fn chinese_profile_uses_jieba_and_char_units() {
        let p = ChineseProfile;
        assert_eq!(p.key(), "zh");
        assert!(matches!(
            p.word_boundary_advisor("电影制片厂"),
            Advisor::Jieba(_)
        ));
        assert!(p.connectors().contains(&"但是"));
        assert!(p.is_char_based());
        // Each CJK char = 1 unit
        assert_eq!(p.token_units("你"), 1.0);
        assert_eq!(p.token_units("你好"), 2.0);
    }

    #[test]
    fn japanese_korean_are_char_based() {
        assert!(JapaneseProfile.is_char_based());
        assert!(KoreanProfile.is_char_based());
        assert!(!JapaneseProfile.connectors().is_empty());
        assert!(!KoreanProfile.connectors().is_empty());
    }

    #[test]
    fn european_profiles_are_word_based() {
        for key in ["fr", "de", "es", "pt", "it"] {
            let p = profile_for_lang(key);
            assert!(!p.is_char_based(), "{key} should be word-based");
            assert!(
                !p.connectors().is_empty(),
                "{key} should have connectors"
            );
        }
    }

    #[test]
    fn unknown_language_falls_back_to_default() {
        let p = profile_for_lang("klingon");
        assert_eq!(p.key(), "default");
        assert!(p.abbreviations().is_empty());
        assert!(p.connectors().contains(&"и"));
        assert!(p.connectors().contains(&"và"));
        assert!(!p.is_char_based());
    }

    #[test]
    fn language_key_handles_variants() {
        assert_eq!(language_key("zh-CN"), "zh");
        assert_eq!(language_key("en_US"), "en");
        assert_eq!(language_key("yue-HK"), "yue");
        assert_eq!(language_key("  FR  "), "fr");
    }

    #[test]
    fn char_units_handle_mixed_scripts() {
        // Pure-ASCII Latin token under English profile: counts as 1 word.
        let en = EnglishProfile;
        assert_eq!(en.token_units("GDP"), 1.0);
        // A Latin-language token that contains CJK chars falls back to
        // character counting so its length isn't undercounted.
        assert_eq!(en.token_units("GDP增长率"), 4.0); // GDP=1 + 3 CJK
        // Pure CJK tokens under a CJK profile: char counting.
        let zh = ChineseProfile;
        assert_eq!(zh.token_units("你"), 1.0);
        assert_eq!(zh.token_units("你好"), 2.0);
        // count_char_units shared helper: ASCII group = 1, each CJK = 1.
        assert_eq!(count_char_units("GDP"), 1);
        assert_eq!(count_char_units("你好"), 2);
        assert_eq!(count_char_units("GDP增长率"), 4);
    }

    #[test]
    fn profile_source_limits_match_existing_presets() {
        // English standard = 16 (unchanged from subtitle_length.rs)
        assert_eq!(
            profile_for_lang("en").source_limit(SubtitleLengthPreset::Standard),
            16
        );
        // Chinese standard = 22 (CJK char limit)
        assert_eq!(
            profile_for_lang("zh").source_limit(SubtitleLengthPreset::Standard),
            22
        );
        // Korean standard = 20
        assert_eq!(
            profile_for_lang("ko").source_limit(SubtitleLengthPreset::Standard),
            20
        );
    }
}