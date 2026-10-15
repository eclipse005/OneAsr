//! 短语词典 + 最大正向分词（mmseg），对齐 pypinyin 的 `PHRASES_DICT` 行为。
//!
//! # 为什么需要这一层
//!
//! Python 参考实现 `hotword/algo_phoneme.py::_process_zh` 把**整段**中文交给
//! `pypinyin.pinyin()`。pypinyin 在那之前会先做「最大正向分词」
//! （`pypinyin.seg.simpleseg.seg` → `mmseg.Seg`，词表 = `PHRASES_DICT` 的前缀集），
//! 再用 `PHRASES_DICT` 里**烘焙好的**词组读音覆盖逐字读音。多音字正是靠这一步
//! 对上的 —— `银行` háng（逐字是 xíng）、`佛山` fó（逐字是 fú）、`一致` yí。
//!
//! **注意**：这条路径上 pypinyin 的 tone_sandhi **没有启用**
//! （`UltimateConverter.__init__` 的 `tone_sandhi` 默认 `False`，而模块级 `pinyin()`
//! 不传该参数）。`一`/`不` 的各种变调是**直接写在词条里**的
//! （`一致 → yí`、`不客气 → bú`），因此移植时**不需要**另写变调规则。
//!
//! # 资产格式
//!
//! `assets/phrases.bin` 由 `scripts/export-phrases.py` 从 pypinyin 的 `PHRASES_DICT`
//! 导出（要一份装了 pypinyin 的 Python，仓库本身不依赖它），用 `include_bytes!`
//! 编进二进制，运行期零 IO。格式见该脚本的文档字符串；同目录的
//! `phrases.manifest.json` 记着来源、pypinyin 版本、条目数、字节数与 sha256，
//! 单测会和它逐项对账——重导之后 manifest 要一起提交。
//!
//! 词条按 UTF-8 字节序升序排列，这一点让**前缀集合无需单独存储**：
//! 「`p` 是否是某个词条的前缀」等价于「`lower_bound(p)` 处的词条以 `p` 开头」。
//! Python 的 `str` 比较按码点，UTF-8 字节序与之等价，故两侧排序一致。

use std::sync::OnceLock;

/// 导出自 pypinyin `PHRASES_DICT`（47111 条 / 1247 个音节 / 847 KiB）。
static ASSET: &[u8] = include_bytes!("assets/phrases.bin");

const MAGIC: &[u8; 8] = b"PYPHRS\x01\x00";

/// 一个词条：词文本 + 每个字的带调拼音下标。
struct Entry {
    phrase: &'static str,
    /// 指向 [`PhraseDict::flat`] 的起始下标
    off: u32,
    /// 字数
    len: u8,
}

/// 只读短语词典。解析一次后全局共享，无需锁。
pub struct PhraseDict {
    syllables: Vec<&'static str>,
    entries: Vec<Entry>,
    /// 所有词条的音节下标扁平存放，条目按 `off..off+len` 取用
    flat: Vec<u16>,
}

static DICT: OnceLock<PhraseDict> = OnceLock::new();

/// 获取全局词典（首次调用时解析）。
#[inline]
pub fn dict() -> &'static PhraseDict {
    DICT.get_or_init(PhraseDict::parse)
}

/// 资产字节流的只读游标。
///
/// 固定 `&'static [u8]`：资产由 `include_bytes!` 编进二进制，
/// 解析出的所有 `&str` 都能拿到 `'static` 生命周期，无需复制。
struct Reader {
    buf: &'static [u8],
    pos: usize,
}

impl Reader {
    fn new(buf: &'static [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn u8(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        let s = self.buf.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        let s = self.buf.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn take(&mut self, n: usize) -> Option<&'static [u8]> {
        let s = self.buf.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(s)
    }

    fn str_at(&self, start: usize, len: usize) -> Option<&'static str> {
        let s = self.buf.get(start..start + len)?;
        std::str::from_utf8(s).ok()
    }
}

impl PhraseDict {
    fn parse() -> Self {
        let mut r = Reader::new(ASSET);
        let magic = r.take(8).expect("phrases.bin 过短");
        assert_eq!(
            magic, MAGIC,
            "phrases.bin 格式不匹配，请重新运行 tools/export_phrases.py"
        );

        let n_syll = r.u32().expect("缺少音节数") as usize;
        let n_entry = r.u32().expect("缺少词条数") as usize;

        // 音节表
        let mut syllables = Vec::with_capacity(n_syll);
        for _ in 0..n_syll {
            let len = r.u8().expect("音节长度缺失") as usize;
            let start = r.pos;
            let s = r.str_at(start, len).expect("音节不是合法 UTF-8");
            r.pos += len;
            syllables.push(s);
        }

        // 词条表
        let mut entries = Vec::with_capacity(n_entry);
        let mut flat = Vec::new();
        for _ in 0..n_entry {
            let plen = r.u16().expect("词长缺失") as usize;
            let phrase = r.str_at(r.pos, plen).expect("词文本不是合法 UTF-8");
            r.pos += plen;
            let nchars = r.u8().expect("字数缺失");
            let off = flat.len() as u32;
            for _ in 0..nchars {
                let idx = r.u16().expect("音节下标缺失");
                assert!(
                    (idx as usize) < syllables.len(),
                    "音节下标 {idx} 越界（共 {} 个）",
                    syllables.len()
                );
                flat.push(idx);
            }
            entries.push(Entry {
                phrase,
                off,
                len: nchars,
            });
        }

        Self {
            syllables,
            entries,
            flat,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn syllables(&self) -> usize {
        self.syllables.len()
    }

    /// 精确查词，返回每个字的带调拼音（如 `国药` → `["guó", "yào"]`）。
    ///
    /// `heteronym=False` 时 pypinyin 只取 `item[0]`，导出时已按此折叠，
    /// 故这里每个字恒有一个读音（47111 条里只有 `朝阳`/`那些` 原本是多音字）。
    pub fn get(&self, word: &str) -> Option<impl Iterator<Item = &'static str> + '_> {
        let e = self.entries.get(self.lower_bound(word))?;
        if e.phrase != word {
            return None;
        }
        let (flat, syls) = (&self.flat, &self.syllables);
        Some((0..e.len as usize).map(move |k| syls[flat[e.off as usize + k] as usize]))
    }

    /// `p` 是否是某个词条的前缀 —— 对应 Python `PrefixSet.__contains__`。
    ///
    /// 所有以 `p` 开头的词条在有序数组里是连续的，起点正是 `lower_bound(p)`；
    /// 因此只需看该位置的词条是否以 `p` 开头。
    #[inline]
    pub fn is_prefix(&self, prefix: &str) -> bool {
        // Python 的 `PrefixSet.train` 只加入长度 1..len(word) 的前缀，空串不在其中
        if prefix.is_empty() {
            return false;
        }
        let i = self.lower_bound(prefix);
        self.entries
            .get(i)
            .is_some_and(|e| e.phrase.starts_with(prefix))
    }

    /// `lower_bound`：首个 `>= key` 的词条下标。
    #[inline]
    fn lower_bound(&self, key: &str) -> usize {
        self.entries.partition_point(|e| e.phrase < key)
    }

    /// 最大正向分词（严格模式），对应 Python
    /// `pypinyin.seg.mmseg.Seg(p_set, no_non_phrases=True).cut`。
    ///
    /// 返回每个词的 `(起始下标, 结束下标)`（字符下标，相对 `text`）。
    ///
    /// 严格模式的两个要点：
    /// - 匹配失败且此前没有「有效词」时，**只输出一个字符**（不输出最长前缀）；
    /// - 剩余文本整体可匹配前缀但整串不在词典里时，**拆成单字**。
    pub fn cut(&self, text: &[char]) -> Vec<(usize, usize)> {
        let n = text.len();

        // 整段字符一次性转成 UTF-8 字节串。后续每扩一个前缀就按字符边界切一刀给二分查找，
        // 免得为每个前缀都分配一个 String（长句会有上千次前缀试探）。
        let mut bytes: Vec<u8> = Vec::with_capacity(n * 3);
        let mut offsets: Vec<usize> = Vec::with_capacity(n + 1);
        for &c in text {
            offsets.push(bytes.len());
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        offsets.push(bytes.len());
        let key = |a: usize, b: usize| -> &str {
            std::str::from_utf8(&bytes[offsets[a]..offsets[b]])
                .expect("字符边界切片必然是合法 UTF-8")
        };

        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut remain = 0usize;

        while remain < n {
            let mut last_valid = 0usize;
            let mut broke = false;

            for index in 0..(n - remain) {
                let len = index + 1;
                let word = key(remain, remain + len);
                if self.is_prefix(word) {
                    if self
                        .entries
                        .get(self.lower_bound(word))
                        .is_some_and(|e| e.phrase == word)
                    {
                        last_valid = len;
                    }
                } else {
                    if last_valid > 0 {
                        out.push((remain, remain + last_valid));
                        remain += last_valid;
                    } else {
                        out.push((remain, remain + 1));
                        remain += 1;
                    }
                    broke = true;
                    break;
                }
            }

            if !broke {
                // 整个剩余文本都是某个词条的前缀（Python 的 `for ... else`）
                if last_valid > 0 {
                    out.push((remain, remain + last_valid));
                    remain += last_valid;
                } else {
                    // 整串不在词典里：拆成单字后结束
                    // （Python 里 `remain not in PHRASES_DICT` 在此恒成立，
                    //   因为若整串命中词典，`last_valid_word` 一定已被赋值）
                    for i in remain..n {
                        out.push((i, i + 1));
                    }
                    break;
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut_str(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        dict()
            .cut(&chars)
            .into_iter()
            .map(|(a, b)| chars[a..b].iter().collect())
            .collect()
    }

    fn phrase(text: &str) -> Vec<String> {
        let d = dict();
        let v: Vec<String> = d
            .get(text)
            .map(|it| it.map(|s| s.to_string()).collect())
            .unwrap_or_default();
        v
    }

    #[test]
    fn asset_parses() {
        let d = dict();
        assert_eq!(d.len(), 47111, "词条数应与导出脚本一致");
        assert_eq!(d.syllables(), 1247, "音节数应与导出脚本一致");
    }

    /// 资产与 manifest 逐项对账：哈希、字节数、条目数、音节数、magic。
    ///
    /// 这是「有人手工改过 .bin / 换了 pypinyin 版本重新导出」的硬闸门——信任锚点是
    /// 内容，不是来源（同 `scripts/ffmpeg-checksums.txt` 的规矩）。重导之后哈希会变、
    /// 测试会红，逼着把 manifest 与新资产一起提交，而不是悄悄换掉一份谁也不知道
    /// 从哪来的词表。重新生成：`python scripts/export-phrases.py`；只核对不写盘：
    /// 加 `--check`。
    #[test]
    fn asset_matches_its_manifest() {
        use sha2::{Digest, Sha256};

        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("assets/phrases.manifest.json"))
                .expect("manifest 必须是合法 JSON");
        let digest = format!("{:x}", Sha256::digest(ASSET));
        assert_eq!(
            manifest["sha256"].as_str(),
            Some(digest.as_str()),
            "资产与 manifest 的哈希不一致：重跑 scripts/export-phrases.py"
        );
        assert_eq!(manifest["bytes"].as_u64(), Some(ASSET.len() as u64));
        assert_eq!(manifest["entries"].as_u64(), Some(dict().len() as u64));
        assert_eq!(
            manifest["syllables"].as_u64(),
            Some(dict().syllables() as u64)
        );
        assert_eq!(manifest["magic"].as_str(), Some("PYPHRS\u{1}\u{0}"));
    }

    #[test]
    fn entries_are_sorted() {
        // is_prefix 的二分查找依赖有序性
        let d = dict();
        assert!(d.entries.windows(2).all(|w| w[0].phrase < w[1].phrase));
    }

    #[test]
    fn known_phrase_readings() {
        assert_eq!(phrase("国药"), vec!["guó", "yào"]);
        assert_eq!(phrase("银行"), vec!["yín", "háng"], "行 在 银行 读 háng");
        assert_eq!(phrase("行长"), vec!["háng", "zhǎng"]);
        assert_eq!(phrase("一致"), vec!["yí", "zhì"], "一 的变调烘焙在词条里");
        assert_eq!(phrase("不客气"), vec!["bú", "kè", "qì"]);
        assert_eq!(phrase("佛山"), vec!["fó", "shān"], "佛 在 佛山 读 fó");
    }

    #[test]
    fn segmentation_is_max_forward_match() {
        assert_eq!(cut_str("国药一致"), vec!["国药", "一致"]);
        assert_eq!(cut_str("银行行长"), vec!["银行", "行长"]);
        assert_eq!(cut_str("佛山市"), vec!["佛山市"]);
        assert_eq!(cut_str("中国人"), vec!["中国人"]);
    }

    #[test]
    fn strict_mode_falls_back_to_single_chars() {
        // 严格模式：即使匹配到前缀，只要不是词典里的词就退化成单字
        let got = cut_str("龘龘");
        assert_eq!(got, vec!["龘", "龘"]);
        // 无任何前缀可匹配时直接单字
        assert_eq!(cut_str("龘"), vec!["龘"]);
    }

    #[test]
    fn prefix_set_semantics() {
        let d = dict();
        assert!(d.is_prefix("国药"), "国药 是词条");
        assert!(d.is_prefix("国"), "单字也是前缀");
        assert!(!d.is_prefix("国药一"), "比词条更长，且没有任何词以它开头");
        assert!(
            !d.is_prefix("国药一致"),
            "一致 是独立词条，不存在以 国药一致 开头的词"
        );
        assert!(!d.is_prefix("龘"), "非词典字");
        assert!(
            !d.is_prefix(""),
            "空前缀不在前缀集内（Python 同样为 false）"
        );
    }

    #[test]
    fn empty_and_repeated_input() {
        assert!(cut_str("").is_empty());
        let r = cut_str("好好好好");
        assert_eq!(
            r.iter().map(|w| w.chars().count()).sum::<usize>(),
            4,
            "应完整覆盖输入"
        );
    }
}
