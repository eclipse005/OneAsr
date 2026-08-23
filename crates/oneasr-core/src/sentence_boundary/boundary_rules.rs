//! Grammar guards for the DP layer: function words that must never dangle a
//! line, phrase-closing particles, discourse markers, "to"-binding, connector
//! binding, and Japanese orthographic glue.

/// A gap this long (seconds, from ASR word timestamps) is a real pause and
/// counts as a "good" cut point even without VAD data.
pub(super) const GOOD_SILENCE_SEC: f64 = 0.35;

/// Gaps at or below this are time-glued (no audible break); cutting there is
/// penalized above a plain word boundary.
pub(super) const GLUE_GAP_SEC: f64 = 0.08;

/// Cost of cutting at a time-glued word pair (interpolated down to the plain
/// word-boundary cost as the gap grows to `GLUE_GAP_SEC`).
pub(super) const GLUED_WORD_COST: f64 = 8.5;

/// Extra DP cost for candidate segments of 1–2 units ("台" / "风" alone).
pub(super) const SHORT_SEGMENT_PENALTY: f64 = 3.5;

/// Units of slack above `limit` before a span must be force-cut; within the
/// grace band only linguistically good cut points are allowed.
pub(super) const LENGTH_GRACE_UNITS: f64 = 2.0;

/// Char grace ≈ unit grace × 5.5 (the display-char budget per word).
pub(super) const LENGTH_GRACE_CHARS: f64 = 11.0;

/// Fragments at or below this many units are absorbed into a neighbor when
/// the merged segment stays within the hard limits.
pub(super) const MIN_FRAGMENT_UNITS: f64 = 3.0;

/// Boundary base costs (lower = better cut).
pub(super) const TERMINAL_COST: f64 = 0.5;
pub(super) const SOFT_COST: f64 = 1.0;
pub(super) const COMMA_COST: f64 = 1.5;
pub(super) const CONNECTOR_COST: f64 = 2.5;
pub(super) const WORD_COST: f64 = 6.0;
pub(super) const FORBIDDEN_COST: f64 = f64::INFINITY;

pub(super) fn strip_token(token: &str) -> &str {
    // Keep letters, numbers, and combining marks (\p{L}\p{N}\p{M}). Hindi
    // matras / Arabic harakat must stay so function-word tables still match.
    let start = token
        .char_indices()
        .find(|(_, c)| is_token_core(*c))
        .map(|(i, _)| i)
        .unwrap_or(token.len());
    let end = token
        .char_indices()
        .rev()
        .find(|(_, c)| is_token_core(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(start);
    if start >= end || start == token.len() {
        ""
    } else {
        &token[start..end]
    }
}

fn is_token_core(c: char) -> bool {
    if c.is_alphanumeric() {
        return true;
    }
    is_unicode_mark(c)
}

/// Combining marks (Mn/Mc/Me) used by the scripts in the U3.5 function-word
/// table. `char::is_alphanumeric` is false for these, so they must be listed.
fn is_unicode_mark(c: char) -> bool {
    matches!(
        c as u32,
        0x0300..=0x036F // combining diacritical
            | 0x0591..=0x05C7 // Hebrew points
            | 0x064B..=0x065F
            | 0x0670
            | 0x06D6..=0x06ED // Arabic harakat
            | 0x08E4..=0x08FF
            | 0x0900..=0x0903
            | 0x093A..=0x094D
            | 0x0951..=0x0957
            | 0x0962..=0x0963 // Devanagari matras / virama
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
    )
}

pub(super) fn is_soft_punctuation(c: char) -> bool {
    // Semicolon / colon only. Fullwidth comma `，` is a comma so discourse
    // markers like "Now，" can suppress the comma discount.
    matches!(c, ';' | ':' | '；' | '：')
}

pub(super) fn is_opening_punctuation(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | '{' | '（' | '【' | '「' | '『' | '《' | '“' | '‘'
    )
}

pub(super) fn is_closing_punctuation(c: char) -> bool {
    matches!(
        c,
        ')' | ']' | '}' | '）' | '】' | '」' | '』' | '》' | '”' | '’'
    )
}

/// English function words: cutting AFTER any of these splits a phrase
/// ("the | price", "in | the market", "need | to go"). Deliberately excludes
/// "that" — it is both a demonstrative and a complement introducer
/// ("see that | the..." is a good cut).
const FUNCTION_WORDS_LEFT: &[&str] = &[
    "a", "an", "the", "this", "these", "those", "my", "your", "his", "her", "its",
    "our", "their", "of", "to", "in", "on", "at", "for", "with", "by", "from",
    "can", "could", "will", "would", "shall", "should", "may", "might", "must",
    "do", "does", "did", "is", "are", "was", "were", "be", "been", "being", "am",
    "have", "has", "had", "not", "and", "but", "or", "nor", "so", "as", "than",
    "towards", "into", "onto", "above", "below", "under", "over", "through",
    "across", "along", "around", "against", "between", "during", "within",
    "without", "upon", "near", "behind", "beyond", "among", "inside", "outside",
    "beside", "off", "via", "per",
];

/// Chinese function words: cutting after these splits "在|教育", "把|门".
const CJK_FUNCTION_WORDS_LEFT: &[&str] = &[
    "和", "与", "及", "或", "在", "是", "把", "被", "将", "从",
    "对", "向", "往", "于", "给", "让", "使", "还", "也", "都", "就", "又",
    "而", "但", "会", "要", "能",
];

/// Japanese adnominal demonstratives: cutting after その|リオ is "the | price".
const JA_DEMONSTRATIVES: &[&str] = &[
    "この", "その", "あの", "どの", "こんな", "そんな", "あんな",
    "こんなに", "そんなに", "あんなに",
];

/// Chinese phrase-closing particles: cutting AFTER these is a good cut
/// ("台风的 | 形状" — the particle ends a modifier phrase).
const CJK_PHRASE_CLOSE: &[&str] = &["的", "了", "着", "过", "吗", "呢", "吧", "啊"];

/// Japanese (and Korean) phrase-closing particles / postpositions.
const JA_PHRASE_CLOSE: &[&str] = &[
    "は", "が", "を", "に", "の", "と", "で", "も", "へ", "や", "より", "まで", "から",
    "ので", "のに", "なので", "けど", "けれど", "けれども", "ですので", "ですから",
];
const KO_PHRASE_CLOSE: &[&str] = &[
    "은", "는", "이", "가", "을", "를", "의", "에", "와", "과", "도", "로", "으로",
];

/// Discourse markers followed by a comma ("Okay," / "Now,"). Cutting after
/// them isolates a flash line; the comma discount must not apply.
const DISCOURSE_MARKERS: &[&str] = &[
    "okay", "ok", "now", "so", "well", "right", "alright", "look", "listen",
    "then", "hey", "oh", "uh", "um", "hmm",
];

/// Words that bind a following "to" ("need to", "going to", "want to"):
/// cutting after them destroys the modal structure.
const TO_BINDING_LEFT: &[&str] = &[
    "need", "needs", "needed", "want", "wants", "wanted", "going", "have", "has",
    "had", "try", "tries", "trying", "tried", "able", "supposed", "expected",
    "likely", "unlikely", "required", "meant", "forced", "bound", "about",
    "prepared", "ready", "willing", "reluctant", "tend", "tends", "tended",
    "plan", "plans", "planned", "hope", "hopes", "hoped",
];

/// Chinese connectors that bind the previous character into a compound
/// ("只因为", "并不是因为"): the connector is NOT a new clause start there.
const CJK_CONNECTOR_BIND_LEFT: &[&str] = &[
    "只", "正", "就", "是", "不", "并", "都", "也", "还", "又", "才", "却", "之",
    "并不是", "不是", "只是",
];

/// Japanese small kana / lengthening / glottal stops that must stay glued to
/// the previous syllable ("ニュース" ≠ "ニ | ュース").
#[rustfmt::skip]
pub(super) fn is_japanese_orthographic_bind(left: &str, right: &str) -> bool {
    let l = strip_token(left);
    let r = strip_token(right);
    if l.is_empty() || r.is_empty() {
        return false;
    }
    let mut r_chars = r.chars();
    let r0 = r_chars.next().unwrap_or_default();
    let l_last = l.chars().last().unwrap_or_default();
    if r0 == 'ー' || r0 == 'ｰ' {
        return true;
    }
    if matches!(
        r0,
        'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'っ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ'
            | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ッ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ'
    ) {
        return true;
    }
    if l_last == 'っ' || l_last == 'ッ' {
        return true;
    }
    if r == "ん" || r == "ン" {
        return true;
    }
    false
}

fn in_list(token: &str, list: &[&str]) -> bool {
    let t = strip_token(token).to_lowercase();
    list.contains(&t.as_str())
}

/// True when `token` (stripped, lowercased) is an English or CJK function
/// word that must not end a line. `extras` comes from the language profile
/// (U3.5 spaced languages: articles/prepositions/clitics).
pub(super) fn is_function_word_left(token: &str, extras: &[&str]) -> bool {
    let t = strip_token(token).to_lowercase();
    if t.is_empty() {
        return false;
    }
    if FUNCTION_WORDS_LEFT.contains(&t.as_str()) || CJK_FUNCTION_WORDS_LEFT.contains(&t.as_str()) {
        return true;
    }
    if JA_DEMONSTRATIVES.contains(&t.as_str()) {
        return true;
    }
    extras.contains(&t.as_str())
}

/// True when cutting after `token` closes a phrase (の/的/は/을...). Only
/// standalone particle tokens count for JA/KO (a Korean 어절 "국방부가" is a
/// whole word, not a dangling particle).
pub(super) fn is_phrase_close_particle(token: &str) -> bool {
    let t = strip_token(token);
    if t.is_empty() {
        return false;
    }
    if JA_DEMONSTRATIVES.contains(&t) {
        return false;
    }
    if JA_PHRASE_CLOSE.contains(&t) || CJK_PHRASE_CLOSE.contains(&t) {
        return true;
    }
    if KO_PHRASE_CLOSE.contains(&t) {
        return true;
    }
    let chars = t.chars().collect::<Vec<_>>();
    if chars.len() < 2 {
        return false;
    }
    let last = chars[chars.len() - 1];
    if JA_PHRASE_CLOSE.contains(&t) || is_ja_single_particle_char(last) {
        return true;
    }
    // 末字「的」：所有的 | 人 (NOT 「了」— 为了 is a connector, cutting
    // after 了 would be wrong in 为了).
    last == '的'
}

fn is_ja_single_particle_char(c: char) -> bool {
    // Only single-char particles can be a token's LAST char (より/まで/から
    // are two-char particles matched whole by JA_PHRASE_CLOSE).
    matches!(
        c,
        'は' | 'が' | 'を' | 'に' | 'の' | 'と' | 'で' | 'も' | 'へ' | 'や'
    )
}

/// Grammatical forms where の is NOT an open genitive waiting for a noun
/// (`ので` / `のに` / `のだ` / `のです` / `のか`).
const JA_NOMINALIZER_TAILS: &[&str] = &["ので", "のに", "のだ", "のです", "のか", "のよ", "のね"];

/// Bound particles that must not start a cue (kinsoku / 禁则).
const JA_LINE_START_PARTICLES: &[&str] = &[
    "は", "が", "を", "に", "の", "と", "で", "も", "へ", "や", "て", "って",
    "より", "まで", "から", "です", "ます", "だ", "た", "し",
    "よ", "ね", "さ", "わ", "か", "よね", "かな", "かい", "かしら",
    // Conjunctive particles belong on the previous clause, not a new cue.
    "ので", "のに", "なので", "けど", "けれど", "けれども", "ですので", "ですから",
];
const KO_LINE_START_PARTICLES: &[&str] = &[
    "은", "는", "이", "가", "을", "를", "의", "에", "와", "과", "도", "로", "으로",
];
const ZH_LINE_START_PARTICLES: &[&str] = &["的", "了", "着", "过", "吗", "呢", "吧"];

/// Cutting before this token would start the next cue with a bound particle.
pub(super) fn is_line_start_bound_particle(token: &str) -> bool {
    let t = strip_token(token);
    if t.is_empty() {
        return false;
    }
    if JA_LINE_START_PARTICLES.contains(&t)
        || KO_LINE_START_PARTICLES.contains(&t)
        || ZH_LINE_START_PARTICLES.contains(&t)
    {
        return true;
    }
    // Clitic の stuck on the following word as one ASR token (のラストコール).
    // Do NOT treat とても / ところで as line-start と.
    t.starts_with('の') && t.chars().count() > 1
}

/// Consecutive katakana tokens are a name/loan run — do not split inside.
pub(super) fn is_katakana_run_bind(left: &str, right: &str) -> bool {
    is_katakana_token(left) && is_katakana_token(right)
}

/// ASR often splits ました/でした/だった as まし|た.
pub(super) fn is_split_copula_ta(prev: &str, left: &str) -> bool {
    strip_token(left) == "た"
        && matches!(strip_token(prev), "まし" | "でし" | "だっ")
}

/// ASR often splits はい as は + い/いじゃあ.
pub(super) fn is_split_hai(left: &str, right: &str) -> bool {
    if strip_token(left) != "は" {
        return false;
    }
    let r = strip_token(right);
    r == "い"
        || r.starts_with("いじゃあ")
        || r.starts_with("いえ")
        || r == "いはい"
}

/// 漢語+する (対する / 参加した).
pub(super) fn is_suru_compound_bind(left: &str, right: &str) -> bool {
    let r = strip_ja_end_particles(strip_token(right));
    const SURU: &[&str] = &[
        "する", "した", "して", "します", "できる", "させる", "される", "しよう",
    ];
    if !SURU.contains(&r) {
        return false;
    }
    strip_token(left).chars().any(is_kanji)
}

/// 皆さん / 木原さん / リサちゃん.
pub(super) fn is_ja_name_suffix_bind(left: &str, right: &str) -> bool {
    let r = strip_token(right);
    matches!(r, "さん" | "ちゃん" | "くん" | "様" | "さま" | "氏" | "君")
        && ja_has_content(left)
}

/// て-form auxiliary (見てほしい / してください / している).
pub(super) fn is_te_auxiliary_bind(left: &str, right: &str) -> bool {
    let l = strip_token(left);
    let r = strip_ja_end_particles(strip_token(right));
    let te = l.ends_with('て') || l.ends_with("で");
    if !te || l.chars().count() < 1 {
        return false;
    }
    matches!(
        r,
        "いる" | "います" | "いた" | "いて" | "ほしい" | "欲しい" | "ください" | "下さい"
            | "もらう" | "くれる" | "あげる" | "しまう" | "おく" | "みる" | "いく" | "くる"
            | "ある" | "やる"
    )
}

/// Kanji/kana stem + following hiragana (盛りだくさん) is one lexical word.
/// Particles, copula, and する are handled by more specific binds.
pub(super) fn is_hiragana_continuation_bind(left: &str, right: &str) -> bool {
    let l = strip_token(left);
    let r = strip_token(right);
    if l.is_empty() || r.chars().count() < 2 {
        return false;
    }
    if is_line_start_bound_particle(right) || is_ja_bare_particle(r) {
        return false;
    }
    // Discourse/turn words are new moves, not okurigana.
    const TURN: &[&str] = &[
        "はい", "ええ", "えっと", "じゃあ", "でも", "また", "あと", "まず", "みんな",
        "もっと", "こんにちは", "こんばんは",
    ];
    if TURN.contains(&r) || JA_DEMONSTRATIVES.contains(&r) {
        return false;
    }
    if !r.chars().all(|c| is_hiragana(c) || c == 'ー' || c == 'ｰ') {
        return false;
    }
    // Okurigana follows a kanji stem. Pure-hiragana copula ます|あの is a
    // new move, not 盛りだくさん.
    l.chars().any(is_kanji)
}

/// Spoken turn / new-move starters. Cutting BEFORE these is a hard
/// sentence boundary, even inside the length target (same role as 。).
pub(super) const JA_TURN_STARTERS: &[&str] = &[
    "はい", "じゃあ", "それでは", "では", "なるほど", "えっと", "ええ",
    "皆さん", "みなさん", "みんな", "こんにちは", "こんばんは",
    "まずは", "まず", "次に", "ところで", "ちなみに",
];

#[allow(dead_code)]
pub(super) fn is_ja_turn_start(token: &str, next: &str) -> bool {
    is_ja_turn_start_after("", token, next)
}

pub(super) fn is_ja_turn_start_after(prev: &str, token: &str, next: &str) -> bool {
    if is_ja_address_greeting_bind(prev, token) {
        return false;
    }
    if is_connector_like(token, JA_TURN_STARTERS) {
        return true;
    }
    is_split_connector_pair(token, next, JA_TURN_STARTERS)
}

fn is_hiragana(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x309F)
}

fn is_kanji(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

/// なる+ほど / 皆+さん when the concatenation is a known connector.
pub(super) fn is_split_connector_pair(left: &str, right: &str, connectors: &[&str]) -> bool {
    let comb = format!("{}{}", strip_token(left), strip_token(right));
    connectors.contains(&comb.as_str())
}

/// Structural Japanese binds that must not be cut.
pub(super) fn is_japanese_lexical_bind(left: &str, right: &str) -> bool {
    is_japanese_orthographic_bind(left, right)
        || is_katakana_run_bind(left, right)
        || is_ja_address_greeting_bind(left, right)
        || is_suru_compound_bind(left, right)
        || is_ja_name_suffix_bind(left, right)
        || is_te_auxiliary_bind(left, right)
        || is_hiragana_continuation_bind(left, right)
        || is_split_hai(left, right)
        || is_split_copula_ta(left, right)
}

/// 皆さんこんにちは / おはようございます stay one greeting.
pub(super) fn is_ja_address_greeting_bind(left: &str, right: &str) -> bool {
    let l = strip_token(left);
    let r = strip_token(right);
    const ADDRESS: &[&str] = &[
        "皆さん", "みなさん", "みんな", "さん", "ちゃん", "くん",
        "こんにちは", "こんばんは", "おはよう",
    ];
    const GREETING: &[&str] = &[
        "こんにちは", "こんばんは", "おはよう", "ございます", "よろしくお願いします",
    ];
    ADDRESS.contains(&l) && GREETING.contains(&r)
}

fn is_katakana_token(token: &str) -> bool {
    let t = strip_token(token);
    if t.is_empty() {
        return false;
    }
    let mut saw = false;
    for ch in t.chars() {
        if is_katakana_char(ch) || ch == 'ー' || ch == 'ｰ' || ch == '・' {
            if is_katakana_char(ch) {
                saw = true;
            }
            continue;
        }
        return false;
    }
    saw
}

fn is_katakana_char(ch: char) -> bool {
    matches!(ch as u32, 0x30A0..=0x30FF | 0xFF66..=0xFF9D) && ch != '・' && ch != '゠'
}

/// Time-glued content words are one speaking unit (キャバ|嬢).
pub(super) fn is_time_glued_content(left: &str, right: &str, gap_sec: Option<f64>) -> bool {
    let Some(g) = gap_sec else {
        return false;
    };
    if g > GLUE_GAP_SEC {
        return false;
    }
    if is_line_start_bound_particle(left) || is_line_start_bound_particle(right) {
        return false;
    }
    ja_has_content(left) && ja_has_content(right)
}

fn ja_has_content(token: &str) -> bool {
    let t = strip_token(token);
    if t.is_empty() || is_line_start_bound_particle(t) {
        return false;
    }
    t.chars().any(|c| c.is_alphanumeric() || is_cjk_letter(c))
}

/// Cutting AFTER の would split a genitive NP (`Z世代の | 選手`).
/// の is a linker to the following head, not a phrase closer.
pub(super) fn is_open_genitive_link(left: &str, right: &str) -> bool {
    let l = strip_token(left);
    let r = strip_token(right);
    if l.is_empty() || r.is_empty() {
        return false;
    }
    if JA_NOMINALIZER_TAILS.iter().any(|tail| l.ends_with(tail)) {
        return false;
    }
    if !l.ends_with('の') {
        return false;
    }
    if JA_DEMONSTRATIVES.contains(&l) {
        return false;
    }
    // 準体言/終助詞の (何してるの / 好きなの) is not a genitive linker.
    if is_explanatory_or_question_no(l) {
        return false;
    }
    is_ja_content_start(r)
}

/// の after a hiragana verb/adjective ending is a question/nominalizer
/// (`あるの` / `好きなの`), not `東京の天気`.
fn is_explanatory_or_question_no(token: &str) -> bool {
    let t = strip_token(token);
    let Some(stem) = t.strip_suffix('の') else {
        return false;
    };
    let Some(c) = stem.chars().last() else {
        return false;
    };
    matches!(
        c,
        'る' | 'う' | 'く' | 'ぐ' | 'す' | 'つ' | 'ぬ' | 'む' | 'ぶ' | 'い' | 'た' | 'だ' | 'な'
    )
}

/// Spoken clause end: です/ます/ました/だ. Equivalent to a missing 。
/// て-form is not treated as a sentence end (serial verbs / てください).
#[allow(dead_code)]
pub(super) fn is_japanese_clause_ending(left: &str, right: &str) -> bool {
    is_japanese_clause_ending_with_peek(left, right, "")
}

pub(super) fn is_japanese_spoken_end(prev: &str, left: &str, right: &str, right2: &str) -> bool {
    if is_line_start_bound_particle(right) && !is_split_hai(right, right2) {
        return false;
    }
    if is_split_copula_ta(prev, left) {
        return true;
    }
    is_japanese_clause_ending_with_peek(left, right, right2)
}

pub(super) fn is_japanese_clause_ending_with_peek(left: &str, right: &str, right2: &str) -> bool {
    // です|が / です|けど stay one clause; kinsoku forbids the particle start.
    // ます|は|い… is はい, not particle は.
    if is_line_start_bound_particle(right) && !is_split_hai(right, right2) {
        return false;
    }
    if strip_token(left) == "か" {
        return true;
    }
    let l = strip_ja_end_particles(strip_token(left));
    if l.is_empty() {
        return false;
    }
    const COPULA: &[&str] = &[
        "でした",
        "ました",
        "ませんでした",
        "ません",
        "です",
        "ます",
        "だった",
        "でしょう",
        "ましょう",
        "ください",
        "んです",
        "のです",
        "じゃありません",
    ];
    if COPULA.iter().any(|s| l == *s || l.ends_with(s)) {
        return true;
    }
    is_da_copula(l)
}

fn is_da_copula(token: &str) -> bool {
    if token == "だ" || token == "んだ" || token == "のだ" {
        return true;
    }
    // Adverbs that happen to end in だ, not the copula.
    const NOT_COPULA: &[&str] = &["ただ", "まだ", "未だ"];
    if NOT_COPULA.contains(&token) {
        return false;
    }
    // Hiragana だ only — 池田 is kanji and will not match.
    token.ends_with('だ') && token.chars().count() >= 2
}

/// Case particle (で/に/を/と) immediately before its predicate.
/// Cheap "phrase-close" cuts are wrong here: the verb belongs on this line.
pub(super) fn is_case_particle_before_predicate(left: &str, right: &str) -> bool {
    let l = strip_token(left);
    let r = strip_token(right);
    if l.is_empty() || r.is_empty() {
        return false;
    }
    let last = l.chars().last().unwrap_or_default();
    // 方がいい / 問題がない: は/が is the subject of a short predicate, not a
    // topic-comment break.
    if matches!(last, 'は' | 'が') {
        return is_short_ja_predicate(r);
    }
    if !matches!(last, 'で' | 'に' | 'を' | 'と') {
        return false;
    }
    looks_japanese_predicate(r)
}

fn is_short_ja_predicate(token: &str) -> bool {
    let t = strip_ja_end_particles(strip_token(token));
    matches!(
        t,
        "いい" | "良い" | "よい" | "ない" | "ほしい" | "欲しい" | "ある" | "いる" | "です" | "だ"
            | "だった"
    )
}

fn is_ja_bare_particle(token: &str) -> bool {
    JA_PHRASE_CLOSE.contains(&token)
        || matches!(token, "って" | "です" | "ます" | "だ" | "た" | "て")
}

fn is_ja_content_start(token: &str) -> bool {
    if token.is_empty() || is_ja_bare_particle(token) {
        return false;
    }
    let Some(c) = token.chars().next() else {
        return false;
    };
    c.is_alphanumeric() || is_cjk_letter(c)
}

fn is_cjk_letter(c: char) -> bool {
    matches!(
        c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF66..=0xFF9D
    )
}

fn looks_japanese_predicate(token: &str) -> bool {
    let trimmed = strip_ja_end_particles(strip_token(token));
    if trimmed.is_empty() {
        return false;
    }
    const STEMS: &[&str] = &[
        "する", "した", "して", "します", "できる", "ある", "いる", "なる", "やる", "いう",
        "思う", "見る", "行く", "来る",
    ];
    if STEMS.contains(&trimmed) {
        return true;
    }
    const SUFFIXES: &[&str] = &[
        "しました",
        "ました",
        "します",
        "ました",
        "して",
        "した",
        "ます",
        "です",
        "たい",
        "ない",
        "る",
        "た",
        "て",
        "う",
        "く",
        "ぐ",
        "す",
        "つ",
        "ぬ",
        "む",
        "ぶ",
        "い",
    ];
    SUFFIXES.iter().any(|s| trimmed.ends_with(s))
}

fn strip_ja_end_particles(token: &str) -> &str {
    let mut t = token;
    for _ in 0..3 {
        let next = t
            .strip_suffix('よ')
            .or_else(|| t.strip_suffix('ね'))
            .or_else(|| t.strip_suffix('さ'))
            .or_else(|| t.strip_suffix('わ'))
            .or_else(|| t.strip_suffix('か'))
            .or_else(|| t.strip_suffix('な'));
        match next {
            Some(stripped) if !stripped.is_empty() => t = stripped,
            _ => break,
        }
    }
    t
}

/// "Okay," / "Now," — comma discount suppressed so the marker stays glued.
pub(super) fn is_discourse_marker_comma(token: &str) -> bool {
    let trimmed = token.trim_end();
    if !trimmed.ends_with(',') && !trimmed.ends_with('，') {
        return false;
    }
    in_list(trimmed, DISCOURSE_MARKERS)
}

/// Discourse marker after stripping sentence-edge punctuation ("Okay." / "Now,").
pub(crate) fn is_discourse_marker_text(text: &str) -> bool {
    let core = text
        .trim()
        .trim_start_matches(|c: char| !is_token_core(c))
        .trim_end_matches(|c: char| {
            matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '…' | ',' | '，' | '、')
        });
    in_list(core, DISCOURSE_MARKERS)
}

/// "need to" / "want to" — the following "to" is not an independent start.
pub(super) fn is_to_binding_left(token: &str) -> bool {
    in_list(token, TO_BINDING_LEFT)
}

/// Connector-before bonus must be suppressed when the left token binds the
/// connector into a compound ("正因为" / "只因为").
pub(super) fn is_bound_connector(left: &str, right: &str, connectors: &[&str]) -> bool {
    let r = strip_token(right).to_lowercase();
    if !connectors.contains(&r.as_str()) {
        return false;
    }
    let l = strip_token(left);
    CJK_CONNECTOR_BIND_LEFT.contains(&l)
}

/// Is `token` a connector (stripped + lowercased vs. the profile table)?
pub(super) fn is_connector_like(token: &str, connectors: &[&str]) -> bool {
    let t = strip_token(token).to_lowercase();
    if t.is_empty() {
        return false;
    }
    if connectors.contains(&t.as_str()) {
        return true;
    }
    // ASR fusion: いじゃあ ≈ じゃあ (one extra mora stuck on the front).
    for connector in connectors {
        if connector.chars().count() < 2 || !t.ends_with(connector) {
            continue;
        }
        let prefix_len = t.len().saturating_sub(connector.len());
        let prefix = &t[..prefix_len];
        if prefix.chars().count() == 1 {
            return true;
        }
    }
    false
}

/// Inter-word gap in seconds from ASR timestamps (negative → 0).
pub(super) fn token_gap_sec(left_end: Option<f64>, right_start: Option<f64>) -> Option<f64> {
    match (left_end, right_start) {
        (Some(l), Some(r)) => Some((r - l).max(0.0)),
        _ => None,
    }
}

/// Time-based cost for a plain word boundary (no structure): glued pairs are
/// the worst legal cut; the cost decays toward 3.2 as the gap approaches the
/// GOOD_SILENCE threshold (silence handling lives in the caller).
pub(super) fn lexical_cut_cost(gap_sec: Option<f64>) -> f64 {
    let Some(g) = gap_sec.map(|g| g.max(0.0)) else {
        return WORD_COST;
    };
    if g >= GOOD_SILENCE_SEC {
        return 2.0 - 0.5 * (g - GOOD_SILENCE_SEC).min(0.9) / 0.9;
    }
    if g <= GLUE_GAP_SEC {
        if GLUE_GAP_SEC <= 0.0 {
            return GLUED_WORD_COST;
        }
        return GLUED_WORD_COST - ((GLUED_WORD_COST - WORD_COST) * g) / GLUE_GAP_SEC;
    }
    let t = (g - GLUE_GAP_SEC) / (GOOD_SILENCE_SEC - GLUE_GAP_SEC);
    WORD_COST - t * (WORD_COST - 3.2)
}

/// Numeric continuation protection ("3.14", "$10", "2026-03", "1,000").
pub(super) fn is_numeric_continuation(left: &str, right: &str) -> bool {
    let left_has_digit = left.chars().any(|c| c.is_ascii_digit());
    let right_has_digit = right.chars().any(|c| c.is_ascii_digit());
    if !left_has_digit || !right_has_digit {
        return false;
    }
    let left_tail = left.trim_end().chars().last();
    let right_head = right.trim_start().chars().next();
    matches!(left_tail, Some('$' | '¥' | '€' | '£' | '.' | ',' | '%'))
        || matches!(right_head, Some('%' | '.' | ',' | '$' | '¥' | '€' | '£'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_words_forbid_cut_after_the_of_and_cjk() {
        assert!(is_function_word_left("the", &[]));
        assert!(is_function_word_left("of", &[]));
        assert!(is_function_word_left("in", &[]));
        assert!(is_function_word_left("to", &[]));
        assert!(is_function_word_left("and", &[]));
        assert!(is_function_word_left("在", &[]));
        assert!(is_function_word_left("把", &[]));
        assert!(!is_function_word_left("price", &[]));
        assert!(!is_function_word_left("that", &[])); // deliberate
        assert!(is_function_word_left("その", &[]));
        assert!(is_function_word_left("この", &[]));
        assert!(!is_phrase_close_particle("その"));
        assert!(!is_open_genitive_link("その", "リオ"));
    }

    #[test]
    fn u35_extras_cover_articles_and_prepositions() {
        assert!(is_function_word_left("der", &["der", "die", "das"]));
        assert!(is_function_word_left("la", &["la", "le", "les"]));
        assert!(!is_function_word_left("señor", &["el", "la"]));
    }

    #[test]
    fn strip_token_keeps_combining_marks() {
        assert_eq!(strip_token("का"), "का");
        assert_eq!(strip_token("का,"), "का");
        assert_eq!(strip_token("في"), "في");
        assert!(is_function_word_left("का", &["का", "के", "की"]));
        assert!(is_function_word_left("का,", &["का", "के", "की"]));
    }

    #[test]
    fn phrase_close_particles_mark_good_cuts() {
        assert!(is_phrase_close_particle("的"));
        assert!(is_phrase_close_particle("了"));
        assert!(is_phrase_close_particle("は"));
        assert!(is_phrase_close_particle("を"));
        assert!(is_phrase_close_particle("을"));
        // 末字「的」within a longer token.
        assert!(is_phrase_close_particle("所有的"));
        // 为了 is a connector — NOT a close particle.
        assert!(!is_phrase_close_particle("为了"));
        // Korean 어절 is a whole word, not a bare particle.
        assert!(!is_phrase_close_particle("국방부가"));
    }

    #[test]
    fn open_genitive_blocks_cut_before_the_head_noun() {
        assert!(is_open_genitive_link("の", "選手"));
        assert!(is_open_genitive_link("世代の", "ラストコール"));
        assert!(is_open_genitive_link("今日の", "試合"));
        assert!(!is_open_genitive_link("ので", "行く"));
        assert!(!is_open_genitive_link("のに", "負けた"));
        assert!(!is_open_genitive_link("は", "選手"));
        assert!(!is_open_genitive_link("の", "は"));
        assert!(!is_open_genitive_link("の", "を"));
    }

    #[test]
    fn line_start_bound_particles_cover_kinsoku() {
        assert!(is_line_start_bound_particle("の"));
        assert!(is_line_start_bound_particle("は"));
        assert!(is_line_start_bound_particle("のラストコール"));
        assert!(is_line_start_bound_particle("を"));
        assert!(is_line_start_bound_particle("은"));
        assert!(is_line_start_bound_particle("的"));
        assert!(is_line_start_bound_particle("か"));
        assert!(!is_line_start_bound_particle("かわいい"));
        assert!(!is_line_start_bound_particle("とても"));
        assert!(!is_line_start_bound_particle("選手"));
        assert!(!is_line_start_bound_particle("ラストコール"));
    }

    #[test]
    fn katakana_run_binds_name_tokens() {
        assert!(is_katakana_run_bind("プリンセス", "キャニオン"));
        assert!(is_katakana_run_bind("ラスト", "コール"));
        assert!(!is_katakana_run_bind("キャニオン", "です"));
        assert!(!is_katakana_run_bind("リサ", "木原"));
    }

    #[test]
    fn address_greeting_binds() {
        assert!(is_ja_address_greeting_bind("皆さん", "こんにちは"));
        assert!(is_ja_address_greeting_bind("さん", "こんにちは"));
        assert!(is_ja_address_greeting_bind("おはよう", "ございます"));
        assert!(!is_ja_address_greeting_bind("登場", "皆さん"));
        assert!(!is_ja_address_greeting_bind("皆さん", "次は"));
    }

    #[test]
    fn japanese_lexical_binds_are_generic() {
        assert!(is_hiragana_continuation_bind("盛り", "だくさん"));
        assert!(is_suru_compound_bind("対", "する"));
        assert!(is_suru_compound_bind("人対", "する"));
        assert!(is_ja_name_suffix_bind("皆", "さん"));
        assert!(is_ja_name_suffix_bind("木原", "さん"));
        assert!(is_te_auxiliary_bind("て", "ほしい"));
        assert!(is_te_auxiliary_bind("して", "ください"));
        assert!(is_split_hai("は", "い"));
        assert!(is_split_hai("は", "いじゃあ"));
        assert!(is_split_copula_ta("まし", "た"));
        assert!(is_split_copula_ta("でし", "た"));
        assert!(is_japanese_spoken_end("まし", "た", "次", ""));
        assert!(!is_japanese_spoken_end("食べ", "た", "人", ""));
        assert!(!is_hiragana_continuation_bind("ぜひ", "はい"));
        assert!(!is_hiragana_continuation_bind("挑む", "もっと"));
        assert!(!is_hiragana_continuation_bind("ます", "あの"));
        assert!(!is_hiragana_continuation_bind("です", "だから"));
        assert!(is_ja_turn_start("はい", ""));
        assert!(is_ja_turn_start("皆", "さん"));
        assert!(!is_ja_turn_start("登場", "皆"));
        assert!(!is_suru_compound_bind("は", "する"));
        assert!(!is_split_hai("は", "選手"));
        assert!(is_japanese_clause_ending_with_peek("ます", "は", "いじゃあ"));
        assert!(!is_japanese_clause_ending("ます", "は"));
        assert!(is_connector_like("いじゃあ", &["じゃあ", "はい"]));
        assert!(is_split_connector_pair("なる", "ほど", &["なるほど"]));
        assert!(is_split_connector_pair("皆", "さん", &["皆さん"]));
    }

    #[test]
    fn explanatory_no_is_not_a_genitive_link() {
        assert!(!is_open_genitive_link("あるの", "明日"));
        assert!(!is_open_genitive_link("好きなの", "ご飯"));
        assert!(!is_open_genitive_link("してるの", "これから"));
        assert!(is_open_genitive_link("東京の", "天気"));
        assert!(is_open_genitive_link("担当の", "先生"));
    }

    #[test]
    fn spoken_copula_is_a_clause_ending() {
        assert!(is_japanese_clause_ending("美味しいです", "今日"));
        assert!(is_japanese_clause_ending("行きます", "それから"));
        assert!(is_japanese_clause_ending("始まりました", "次"));
        assert!(is_japanese_clause_ending("してください", "こちら"));
        assert!(is_japanese_clause_ending("好きだ", "でも"));
        assert!(is_japanese_clause_ending("学生なんだ", "明日"));
        assert!(!is_japanese_clause_ending("美味しいです", "が"));
        assert!(!is_japanese_clause_ending("行きます", "けど"));
        assert!(!is_japanese_clause_ending("ただ", "それ"));
        assert!(!is_japanese_clause_ending("して", "いる"));
        assert!(!is_japanese_clause_ending("見て", "ほしい"));
        assert!(!is_japanese_clause_ending("して", "ください"));
    }

    #[test]
    fn hou_ga_ii_is_not_a_topic_cut() {
        assert!(is_case_particle_before_predicate("方が", "いい"));
        assert!(is_case_particle_before_predicate("問題が", "ない"));
        assert!(!is_case_particle_before_predicate("は", "面白い"));
        assert!(!is_case_particle_before_predicate("たちが", "今日"));
    }

    #[test]
    fn case_particle_before_predicate_is_not_a_cheap_cut() {
        assert!(is_case_particle_before_predicate("を", "見る"));
        assert!(is_case_particle_before_predicate("話で", "感動した"));
        assert!(is_case_particle_before_predicate("に", "なる"));
        assert!(!is_case_particle_before_predicate("は", "面白い"));
        assert!(!is_case_particle_before_predicate("で", "本当に"));
    }

    #[test]
    fn discourse_markers_require_trailing_comma() {
        assert!(is_discourse_marker_comma("Okay,"));
        assert!(is_discourse_marker_comma("Now，"));
        assert!(!is_discourse_marker_comma("Okay"));
        assert!(is_discourse_marker_comma("Ok,")); // case-insensitive via strip
        assert!(!is_discourse_marker_comma("market,"));
        assert!(is_discourse_marker_text("Okay."));
        assert!(is_discourse_marker_text("Now,"));
        assert!(!is_discourse_marker_text("market"));
    }

    #[test]
    fn to_binding_left_covers_modal_verbs() {
        assert!(is_to_binding_left("need"));
        assert!(is_to_binding_left("want"));
        assert!(is_to_binding_left("going"));
        assert!(is_to_binding_left("trying"));
        assert!(!is_to_binding_left("market"));
    }

    #[test]
    fn bound_connectors_suppress_bonus() {
        let connectors = ["因为", "所以", "但是"];
        assert!(is_bound_connector("只", "因为", &connectors));
        assert!(is_bound_connector("正", "因为", &connectors));
        assert!(!is_bound_connector("他", "因为", &connectors));
    }

    #[test]
    fn japanese_orthographic_binds() {
        assert!(is_japanese_orthographic_bind("ニ", "ュース"));
        assert!(is_japanese_orthographic_bind("待っ", "て"));
        assert!(is_japanese_orthographic_bind("ニュース", "ー"));
        assert!(is_japanese_orthographic_bind("サ", "ン"));
        assert!(!is_japanese_orthographic_bind("友達", "を"));
    }

    #[test]
    fn numeric_continuation_protects_numbers() {
        // Both sides must contain a digit for the rule to apply at all —
        // currency prefixes/unit suffixes are merged earlier by the token
        // normalizer (voxtrans-core segmenter.rs), so "$"|"10" never reaches
        // the DP as separate tokens in practice.
        // Decimal: "3" | "." | "14" → the "."/digit pairs must not split.
        assert!(is_numeric_continuation("3.", "14"));
        assert!(is_numeric_continuation("3", ".14"));
        assert!(is_numeric_continuation("3.", ".14"));
        // Percent: "12" | ".5%" (right side still contains digits).
        assert!(is_numeric_continuation("12", ".5%"));
        assert!(!is_numeric_continuation("12.5", "%")); // no digit on the right
        // Thousands separator: "1" | ",000".
        assert!(is_numeric_continuation("1", ",000"));
        assert!(is_numeric_continuation("1,", "000"));
        // Symbol-only sides without digits are not continuations.
        assert!(!is_numeric_continuation("$", "10"));
        assert!(!is_numeric_continuation("10", "%"));
        // Plain adjacent tokens with digits are NOT continuations ("3" "14"
        // in speech is two numbers) — mirrors EggTranslate.
        assert!(!is_numeric_continuation("3", "14"));
        assert!(!is_numeric_continuation("hello", "world"));
    }

    #[test]
    fn lexical_cost_prefers_real_pauses() {
        assert!(lexical_cut_cost(Some(0.0)) > lexical_cut_cost(Some(0.05)));
        assert!(lexical_cut_cost(Some(0.05)) > lexical_cut_cost(Some(0.2)));
        assert!(lexical_cut_cost(Some(0.2)) > lexical_cut_cost(Some(0.35)));
        assert!(lexical_cut_cost(Some(0.5)) < 2.0);
        // No timestamps → plain word cost.
        assert_eq!(lexical_cut_cost(None), WORD_COST);
    }
}