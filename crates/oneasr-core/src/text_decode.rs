//! 用户文本文件的字节 → 文字：**唯一**的编码入口。
//!
//! 为什么要有这一层：读文稿的地方以前写的是 `String::from_utf8_lossy`，它把
//! 「解不出来」伪装成「解出来了，只是内容是 U+FFFD」。一份 Windows 记事本按
//! ANSI 存的 GBK 文稿因此能被顺利读进管线：字数算得出、语速不报警、对齐器照样
//! 插值，最后写进字幕的是一片替换字符——**每一层都觉得自己成功了**。
//!
//! 所以这一层的契约是：*看不懂就失败，绝不产出自己都不确认的文字*；确实做过
//! 换码的，把用了哪种编码留在 [`DecodedText::encoding`] 里，让上层看得见、说得
//! 出来。猜错也要猜得明明白白。
//!
//! # 判定阶梯（每一步都以结构性证据为准，不做统计猜谜）
//!
//! 1. **BOM**：`EF BB BF` → UTF-8；`FF FE` / `FE FF` → UTF-16 LE/BE（都由
//!    解码方吃掉）；`FF FE 00 00` / `00 00 FE FF` → UTF-32，明确拒绝。BOM 是
//!    唯一能被**证明**的编码声明，所以它排第一。
//! 2. **NUL 字节**：没有 BOM 却含 `0x00` 的不是文本。UTF-16 的 NUL 是结构性的
//!    （每个 BMP 字符的一半），而无 BOM 的 UTF-16 到不了这一步之后的任何一档；
//!    这条同时挡住「把 .docx 改名成 .txt」。以前这种文件会变成夹着 NUL 的乱码。
//! 3. **严格 UTF-8**：UTF-8 自校验，合法即成立。实测：20 字节的随机串
//!    2000 个里 **0 个**能通过（它就是靠这个把「不是 UTF-8」变成可判定的）。
//!    唯一失败方式是「末尾半截多字节序列」时**直接拒绝**：那说明它本来就是
//!    UTF-8，只是被截断了——放它去走 GB18030 会把整篇读成另一种乱码。
//! 4. **GB18030**：只有 UTF-8 不成立才轮到它，GBK / GB2312 都是它的子集。它很
//!    宽松（同样的 2000 个随机串里有 **210 个**能「解成功」），所以既只能放
//!    最后，也必须回报结果。
//! 5. 都不成立 → [`DecodeError`]，由调用方翻成给人看的话。
//!
//! # 明确不做的事
//!
//! **不做多编码统计判别**（Big5 / Shift_JIS / EUC-KR）。这几家的字节在 GB18030
//! 下几乎都能「解码成功」（实测：Shift_JIS 的 `これは日本語のテストです。` 解成
//! 一片像模像样的汉字），靠合法性分不开，只能靠统计打分；打分在短文稿上会翻车，
//! 而「猜」本身正是这一层要消灭的东西。当前口径：中文（GBK/GB18030）与 UTF-16
//! 覆盖到，其余编码请用户另存为 UTF-8——错误文案与 [`DecodedText::encoding`]
//! 就是给这件事留的两条路。
//!
//! # 只处理「用户给的文本文件」
//!
//! `settings.json` / `hotword.json` / `.timeline.json` 是本程序自己写的，永远是
//! UTF-8，坏掉就该硬失败并走各自的自愈逻辑——把它们也塞进这里，等于让「编码猜
//! 测」有机会改写用户的配置。这一层只服务**别人的**文件。

use encoding_rs::{GB18030, UTF_16BE, UTF_16LE};

/// 一份文本文件实际是用什么编码读出来的。
///
/// 存下来不是为了调试：GB18030 是**猜**的，猜错的可能性必须让用户看得见
/// （界面 / `transcript inspect --json` 说一句「按 GB18030 读取」），否则又
/// 回到「静默出错」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// 无 BOM 的 UTF-8：最常见的正常情况。
    Utf8,
    /// 带 BOM 的 UTF-8（Windows 记事本「UTF-8 带 BOM」）。
    Utf8Bom,
    /// 带 BOM 的 UTF-16 小端（记事本「Unicode」）。
    Utf16Le,
    /// 带 BOM 的 UTF-16 大端。
    Utf16Be,
    /// GBK / GB2312 / GB18030 一族，由 GB18030（超集）解出。
    Gb18030,
}

impl TextEncoding {
    /// 机器可读的标识：`transcript inspect --json` 的 `encoding` 字段用它。
    pub fn id(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf8Bom => "utf-8-bom",
            Self::Utf16Le => "utf-16le",
            Self::Utf16Be => "utf-16be",
            Self::Gb18030 => "gb18030",
        }
    }

    /// 给人看的名字（技术名，两种语言同形，不进双语表）。
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 | Self::Utf8Bom => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Gb18030 => "GB18030",
        }
    }

    /// 是不是 UTF-8 家族——也就是「读法没有歧义」的那一半。
    ///
    /// 上层拿它决定要不要提示编码：UTF-8 去提示用户只会变成噪音，GB18030 与
    /// UTF-16 则值得说一句。
    pub fn is_utf8(self) -> bool {
        matches!(self, Self::Utf8 | Self::Utf8Bom)
    }
}

/// 解出来的文字 + 用的是哪种编码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedText {
    pub text: String,
    pub encoding: TextEncoding,
}

/// 解不出来。**没有**「勉强解出来」这一档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// 头部是 UTF-32 的 BOM。不解，直接拒绝：`encoding_rs` 不支持它，
    /// 而把它当 UTF-16 读会得到夹满 NUL 的乱码。
    Utf32,
    /// 没有 BOM 却含 NUL 字节——不是文本文件（或是不带 BOM 的 UTF-16）。
    NotText,
    /// 既不是 UTF-8，也不是合法的 GB18030。
    Unknown,
}

const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";
const UTF32_LE_BOM: &[u8] = b"\xff\xfe\x00\x00";
const UTF32_BE_BOM: &[u8] = b"\x00\x00\xfe\xff";
const UTF16_LE_BOM: &[u8] = b"\xff\xfe";
const UTF16_BE_BOM: &[u8] = b"\xfe\xff";

/// 把一份用户文本文件解成文字。判定顺序见模块文档，**不要重排**。
pub fn decode(bytes: &[u8]) -> Result<DecodedText, DecodeError> {
    // 1. BOM。UTF-32 必须先于 UTF-16 判：`FF FE 00 00` 也以 `FF FE` 开头。
    if bytes.starts_with(UTF32_LE_BOM) || bytes.starts_with(UTF32_BE_BOM) {
        return Err(DecodeError::Utf32);
    }
    if let Some(body) = bytes.strip_prefix(UTF8_BOM) {
        // BOM 声称是 UTF-8，那就必须是——不成立也不许改猜 GB18030。
        return match std::str::from_utf8(body) {
            Ok(text) => Ok(DecodedText {
                text: text.to_string(),
                encoding: TextEncoding::Utf8Bom,
            }),
            Err(_) => Err(DecodeError::Unknown),
        };
    }
    if let Some(body) = bytes.strip_prefix(UTF16_LE_BOM) {
        return decode_utf16(UTF_16LE, body, TextEncoding::Utf16Le);
    }
    if let Some(body) = bytes.strip_prefix(UTF16_BE_BOM) {
        return decode_utf16(UTF_16BE, body, TextEncoding::Utf16Be);
    }
    // 2. 无 BOM 却有 NUL：不是文本。
    if bytes.contains(&0) {
        return Err(DecodeError::NotText);
    }
    // 3. 严格 UTF-8。
    //
    //    `error_len() == None` 只有一个含义：「输入在末尾被截断了」——即错误位置
    //    之前的字节**全部是合法 UTF-8**。一个真正的 GBK 文件早就会在前面某个
    //    高位字节上报 `error_len() == Some`，所以这条不可能误伤它。
    //
    //    而如果放它继续往下走，GB18030 几乎总能“解成功”，于是一份被截断的
    //    UTF-8 文稿会整篇变成另一种乱码——那正是这一层要消灭的那种失败。
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            return Ok(DecodedText {
                text: text.to_string(),
                encoding: TextEncoding::Utf8,
            });
        }
        Err(e) if e.error_len().is_none() => return Err(DecodeError::Unknown),
        Err(_) => {}
    }
    // 4. GB18030 兜底。**只**在 UTF-8 不成立之后——否则 UTF-8 文件也可能被
    //    它「解码成功」，两者结果不同。
    let (text, had_errors) = GB18030.decode_without_bom_handling(bytes);
    if had_errors {
        return Err(DecodeError::Unknown);
    }
    Ok(DecodedText {
        text: text.into_owned(),
        encoding: TextEncoding::Gb18030,
    })
}

/// UTF-16 解码（BOM 已在调用方剥掉）。半截代理对、奇数个字节都算解不出来。
fn decode_utf16(
    encoding: &'static encoding_rs::Encoding,
    body: &[u8],
    id: TextEncoding,
) -> Result<DecodedText, DecodeError> {
    let (text, had_errors) = encoding.decode_without_bom_handling(body);
    if had_errors {
        return Err(DecodeError::Unknown);
    }
    Ok(DecodedText {
        text: text.into_owned(),
        encoding: id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GBK / GB18030 字节字面量（"今天我们聊一聊字幕。"）。写死字节而不是把
    /// 字符串按某种编码编回去：测试要断言的是「这些字节能读成这句话」，用编码器
    /// 造输入等于拿被测规则自己证明自己，且 GBK 编码器在依赖里根本没有。
    const GBK_LINE: &[u8] = &[
        0xbd, 0xf1, 0xcc, 0xec, 0xce, 0xd2, 0xc3, 0xc7, 0xc1, 0xc4, 0xd2, 0xbb, 0xc1, 0xc4, 0xd7,
        0xd6, 0xc4, 0xbb, 0xa1, 0xa3,
    ];
    const GBK_LINE_TEXT: &str = "今天我们聊一聊字幕。";

    /// UTF-16LE 的同一句（不含 BOM）。从字符直接编出来，跨平台安全。
    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    fn utf16be(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    }

    /// 这条是这次缺陷的主因：GBK 文稿以前被 lossy 成一片 U+FFFD，还返回成功。
    #[test]
    fn gbk_bytes_decode_to_readable_chinese() {
        let d = decode(GBK_LINE).expect("GBK must decode");
        assert_eq!(d.text, GBK_LINE_TEXT);
        assert_eq!(d.encoding, TextEncoding::Gb18030);
        assert!(!d.text.contains('\u{fffd}'), "no replacement chars: {d:?}");
    }

    /// 纯 ASCII 走 UTF-8，不许被当成 GB18030——编码回显会因此变成噪音。
    #[test]
    fn plain_ascii_is_utf8_not_gb18030() {
        let d = decode(b"hello\nworld\n").expect("ascii is utf-8");
        assert_eq!(d.encoding, TextEncoding::Utf8);
        assert_eq!(d.text, "hello\nworld\n");
    }

    #[test]
    fn utf8_with_and_without_bom_are_both_utf8() {
        let d = decode("第一行。".as_bytes()).expect("utf-8");
        assert_eq!(
            (d.text.as_str(), d.encoding),
            ("第一行。", TextEncoding::Utf8)
        );

        let mut with_bom = UTF8_BOM.to_vec();
        with_bom.extend_from_slice("第一行。".as_bytes());
        let d = decode(&with_bom).expect("utf-8 with bom");
        assert_eq!(
            (d.text.as_str(), d.encoding),
            ("第一行。", TextEncoding::Utf8Bom),
            "BOM 必须在字节层就吃掉，不能留成 U+FEFF"
        );
    }

    /// 记事本「Unicode」存出来的就是这条：BOM + UTF-16LE。以前读成夹 NUL 的乱码。
    #[test]
    fn utf16_with_bom_decodes_even_though_utf8_is_impossible() {
        let mut le = UTF16_LE_BOM.to_vec();
        le.extend_from_slice(&utf16le(GBK_LINE_TEXT));
        let d = decode(&le).expect("utf-16le");
        assert_eq!(d.text, GBK_LINE_TEXT);
        assert_eq!(d.encoding, TextEncoding::Utf16Le);

        let mut be = UTF16_BE_BOM.to_vec();
        be.extend_from_slice(&utf16be(GBK_LINE_TEXT));
        let d = decode(&be).expect("utf-16be");
        assert_eq!(d.text, GBK_LINE_TEXT);
        assert_eq!(d.encoding, TextEncoding::Utf16Be);
    }

    /// 无 BOM 的 UTF-16 一律按「不是文本」拒绝：没有 BOM 时它和不带 BOM 的
    /// 二进制没法分开，宁可能失败，也不许猜成一片乱码。
    #[test]
    fn utf16_without_bom_is_rejected_not_guessed() {
        assert_eq!(decode(&utf16le("第一行。")), Err(DecodeError::NotText));
        assert_eq!(decode(&utf16be("第一行。")), Err(DecodeError::NotText));
    }

    /// 改名成 .txt 的二进制（zip/docx 之类）在这条上停住。
    #[test]
    fn nul_bytes_mean_it_is_not_a_text_file() {
        assert_eq!(decode(b"PK\x03\x04\x00\x00junk"), Err(DecodeError::NotText));
    }

    #[test]
    fn utf32_is_refused_instead_of_read_as_utf16() {
        let mut le = UTF32_LE_BOM.to_vec();
        le.extend("第一行。".chars().flat_map(|c| (c as u32).to_le_bytes()));
        assert_eq!(decode(&le), Err(DecodeError::Utf32));

        let mut be = UTF32_BE_BOM.to_vec();
        be.extend("第一行。".chars().flat_map(|c| (c as u32).to_be_bytes()));
        assert_eq!(decode(&be), Err(DecodeError::Utf32));
    }

    /// 两头都不成立时报错，而不是 lossy 出一片 U+FFFD。
    #[test]
    fn bytes_that_are_neither_utf8_nor_gb18030_fail() {
        assert_eq!(decode(&[0xff, 0xff, 0xff]), Err(DecodeError::Unknown));
        // BOM 说是 UTF-8，正文却不是：不许改猜 GB18030。
        let mut broken = UTF8_BOM.to_vec();
        broken.extend_from_slice(&[0xff, 0xff]);
        assert_eq!(decode(&broken), Err(DecodeError::Unknown));
    }

    /// 编了码但内容半截（UTF-16 被截在第奇数个字节）：报 decode 错，而不是
    /// 把剩下的字节硬翻成乱码。文案里那句「内容也可能已损坏」就是说它。
    #[test]
    fn a_truncated_utf16_file_is_an_error_not_half_garbage() {
        let mut bytes = UTF16_LE_BOM.to_vec();
        bytes.extend_from_slice(&utf16le(GBK_LINE_TEXT)[..9]);
        assert_eq!(decode(&bytes), Err(DecodeError::Unknown));
    }

    /// 末尾被截断的 UTF-8 不许改走 GB18030：那份文件本来就是 UTF-8，只是断了
    /// 一个字节；放它去 GB18030 会把整篇读成另一种乱码（实测会解出
    /// `浠婂ぉ鎴戜滑鑱婁…` 这类东西），比报错糟糕得多。
    #[test]
    fn a_truncated_utf8_tail_is_refused_instead_of_reread_as_gbk() {
        let mut bytes = "今天我们聊一聊字幕这件事，以及它为什么重要。"
            .as_bytes()
            .to_vec();
        bytes.pop();
        assert_eq!(decode(&bytes), Err(DecodeError::Unknown));
    }

    /// 空文件解成空串（「文稿是空的」由文稿层说，不是编码层的判断）。
    #[test]
    fn an_empty_file_decodes_to_empty_text() {
        assert_eq!(
            decode(b"").map(|d| d.text),
            Ok(String::new()),
            "empty input is not an encoding error"
        );
    }

    /// 已知边界，写下来当成契约：Shift_JIS 在 GB18030 下「能解」，于是会被当成
    /// 中文读出来。所以 GB18030 的结果必须回报给用户，见模块文档「不做的事」。
    #[test]
    fn sjis_is_a_documented_limitation_not_a_silent_failure() {
        // 「これは」的 Shift_JIS 字节
        let d = decode(&[0x82, 0xb1, 0x82, 0xea, 0x82, 0xcd]);
        assert_eq!(
            d.map(|d| d.encoding),
            Ok(TextEncoding::Gb18030),
            "能解但解错——所以这个结果必须被回显，不能静默"
        );
    }
}
