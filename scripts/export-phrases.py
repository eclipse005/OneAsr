#!/usr/bin/env python3
"""把 pypinyin 的 PHRASES_DICT 导出成热词模块用的二进制资产。

产出
----
    crates/oneasr-core/src/hotword/assets/phrases.bin             include_bytes! 编进二进制
    crates/oneasr-core/src/hotword/assets/phrases.manifest.json   来源 + 条目数 + sha256

怎么跑
------
需要一个装了 pypinyin 的 Python（3.8+）。仓库本身不要依赖它，所以这不是 CI 门禁，
是「改资产时手工跑一次」的工具：

    # 维护者本机的 conda 环境
    C:\\Users\\<你>\\miniconda3\\envs\\myenv\\python.exe scripts/export-phrases.py
    # 或者自己装一份
    python -m pip install pypinyin && python scripts/export-phrases.py

只想核对「仓库里那份资产是不是这份脚本产出的」而不写盘：

    python scripts/export-phrases.py --check     # 不一致就退出码 1、打印差在哪

为什么需要它
------------
`hotword/phoneme.rs::process_zh` 走的是「分词 + 词组读音」这条路：pypinyin 先做
最大正向分词（`pypinyin.seg.simpleseg.seg` → `mmseg.Seg`，词表正是 PHRASES_DICT 的
前缀集），再用 PHRASES_DICT 里**烘焙好的**词组读音覆盖逐字读音。多音字
（`银行` háng / `佛山` fó / `一致` yí）就是靠这一步对上的。

注意这条路径上 pypinyin 的 tone_sandhi **没有启用**（`UltimateConverter.__init__`
的 `tone_sandhi` 默认 False，模块级 `pinyin()` 也不传它），`一`/`不` 的变调是**直接
写在词条里**的（`一致 → yí`、`不客气 → bú`），所以 Rust 侧不需要另写变调规则。

资产格式（解析在 `hotword/phrases.rs`）
--------------------------------------
    magic   8B  b"PYPHRS\\x01\\x00"
    u32     n_syll      音节表条目数
    u32     n_entry     词条数
    音节表   n_syll × (u8 len + len B)          按码点序升序
    词条表   n_entry × (u16 len + len B 词文本
                        + u8 字数
                        + 字数 × u16 音节下标)   按词文本码点序升序

词条有序这一点很关键：Rust 侧一次二分就同时回答「该词是否是某个词条的前缀」
（mmseg 的 prefix_set）与「该词的读音是什么」（PHRASES_DICT 查询），无需另存前缀集。

改了资产之后
------------
`hotword/phrases.rs` 的单测拿 manifest 里的 sha256 / 条目数 / 字节数对账，所以重导
之后**manifest 要一起提交**；换了 pypinyin 版本这些数会变、测试会红——那是设计如此
（信任锚点是内容，不是来源，同 `scripts/ffmpeg-checksums.txt`）。
"""
from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from _verify_common import utf8_when_redirected  # noqa: E402

from pypinyin.constants import PHRASES_DICT, RE_HANS  # noqa: E402

MAGIC = b"PYPHRS\x01\x00"

REPO = Path(__file__).resolve().parent.parent
DEFAULT_OUT = REPO / "crates" / "oneasr-core" / "src" / "hotword" / "assets"

# pypinyin 的 `convert_styles` 在 heteronym=False 时只取 `item[0]`；
# `tone_to_tone3` 会把带调符号换成数字，故词条里出现的字符只能是这些。
from pypinyin.phonetic_symbol import phonetic_symbol  # noqa: E402

ALLOWED = set("abcdefghijklmnopqrstuvwxyz")
for _sym, _to in phonetic_symbol.items():
    ALLOWED.update(_sym)
ALLOWED.update("0123456789")


def build() -> tuple:
    syllables = set()
    entries = []

    for phrase, pys in PHRASES_DICT.items():
        # 分词器只在 RE_HANS 的 token 上做二次分词，非汉字 key 不会被用到
        if not RE_HANS.match(phrase):
            continue
        # `convert_styles` 用 `phrase[idx]` 索引 pinyin_list，长度必须一致
        if len(pys) != len(phrase):
            raise AssertionError(f"{phrase!r}: 拼音数 {len(pys)} != 词长 {len(phrase)}")
        firsts = []
        for p in pys:
            if not p:
                raise AssertionError(f"{phrase!r}: 存在空拼音")
            s = p[0]
            bad = {c for c in s} - ALLOWED
            if bad:
                raise AssertionError(f"{phrase!r}: 拼音 {s!r} 含非预期字符 {bad!r}")
            syllables.add(s)
            firsts.append(s)
        entries.append((phrase, firsts))

    # Python 的 str 比较按码点，UTF-8 字节序与之等价；
    # 这里显式按码点排一次，和 Rust 侧 `&str` 的 Ord 保持一致。
    entries.sort(key=lambda kv: [ord(c) for c in kv[0]])
    syl_table = sorted(syllables, key=lambda s: [ord(c) for c in s])
    syl_index = {s: i for i, s in enumerate(syl_table)}
    return entries, syl_table, syl_index


def serialize(entries, syl_table, syl_index) -> bytes:
    out = bytearray()
    out += MAGIC
    out += struct.pack("<II", len(syl_table), len(entries))
    for s in syl_table:
        b = s.encode("utf-8")
        if len(b) > 255:
            raise AssertionError(f"音节过长: {s!r}")
        out += struct.pack("<B", len(b))
        out += b
    for phrase, pys in entries:
        pb = phrase.encode("utf-8")
        out += struct.pack("<H", len(pb))
        out += pb
        out += struct.pack("<B", len(pys))
        # 音节下标用 u16（1247 个音节，u8 装不下）
        out += b"".join(struct.pack("<H", syl_index[p]) for p in pys)
    return bytes(out)


def manifest_for(entries, syl_table, blob: bytes) -> dict:
    import pypinyin

    return {
        "source": "pypinyin.constants.PHRASES_DICT",
        "pypinyin_version": pypinyin.__version__,
        "generator": "scripts/export-phrases.py",
        "magic": MAGIC.decode("utf-8", "replace"),
        "entries": len(entries),
        "syllables": len(syl_table),
        "bytes": len(blob),
        "sha256": hashlib.sha256(blob).hexdigest(),
    }


def main() -> int:
    utf8_when_redirected()
    ap = argparse.ArgumentParser(description="导出热词短语词典资产（phrases.bin + manifest）")
    ap.add_argument("--out", default=str(DEFAULT_OUT), help="输出目录（默认 crate 资产目录）")
    ap.add_argument(
        "--check",
        action="store_true",
        help="只核对仓库里的资产是否与本次生成一致，不写盘",
    )
    args = ap.parse_args()

    out_dir = Path(args.out)
    entries, syl_table, syl_index = build()
    blob = serialize(entries, syl_table, syl_index)
    manifest = manifest_for(entries, syl_table, blob)

    bin_path = out_dir / "phrases.bin"
    manifest_path = out_dir / "phrases.manifest.json"

    if args.check:
        problems = []
        if not bin_path.exists():
            problems.append(f"缺少 {bin_path}")
        elif bin_path.read_bytes() != blob:
            on_disk = bin_path.read_bytes()
            problems.append(
                f"{bin_path} 与重新生成的不一致"
                f"（磁盘 {len(on_disk):,} 字节 / 新生成 {len(blob):,} 字节）"
            )
        if not manifest_path.exists():
            problems.append(f"缺少 {manifest_path}")
        else:
            on_disk_manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            for key, want in manifest.items():
                if on_disk_manifest.get(key) != want:
                    problems.append(
                        f"manifest.{key}: 磁盘 {on_disk_manifest.get(key)!r} != 现算 {want!r}"
                    )
        if problems:
            for p in problems:
                print(f"不一致：{p}", file=sys.stderr)
            return 1
        print(f"一致：{bin_path}（{len(blob):,} 字节，sha256 {manifest['sha256'][:16]}…）")
        return 0

    out_dir.mkdir(parents=True, exist_ok=True)
    bin_path.write_bytes(blob)
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )

    print(f"词条数   : {len(entries)}")
    print(f"音节数   : {len(syl_table)}")
    print(f"体积     : {len(blob):,} 字节 ({len(blob) / 1024:.1f} KiB)")
    print(f"sha256   : {manifest['sha256'][:16]}...")
    print(f"输出     : {bin_path}")
    print(f"           {manifest_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
