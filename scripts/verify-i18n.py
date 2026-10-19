#!/usr/bin/env python3
"""Rust 管线文案一致性断言：把双语规则变成机器门禁。

Electron 界面自己的双语门禁在 `apps/electron`（vitest）：渲染层 zh/en 字典
键一致、两侧非空、占位符成对，并且渲染层源码里不许出现硬编码中文。本脚本
只查 Rust 侧——`oneasr-core` 里会经 CLI 直接显示给用户的双语文案。

为什么还需要它
--------------
`Str::new(zh, en)` 的成对构造让「漏译」通不过编译，但编译器看不见下面两类：

  1. 两侧占位符集合不一致（`{n}` 一侧有一侧无），一侧渲染出空洞；
  2. 英文侧是空串，英文界面整条文案显示为空白。

这两条都能静态查清，且当前全绿 —— 加门禁的最佳时机是没欠债的时候。

门禁自己也要有单测，否则改坏一条正则就会**静默地永远放绿**。用例见
scripts/test_verify_i18n.py；`bonebreak-verify-i18n.py` 负责把每处检查逐个
还原，确认对应用例真的变红。

用法
----
    python3 scripts/verify-i18n.py

标准输出只打印通过与否，诊断走 stderr。CI 在每次提交上跑（见 ci.yml）。
读不到文件时按「未知」处理并失败，不按「没问题」放行。
"""

from __future__ import annotations

import pathlib
import re
import sys

from _verify_common import utf8_when_redirected

ROOT = pathlib.Path(__file__).resolve().parent.parent

I18N_FILES = (ROOT / "crates" / "oneasr-core" / "src" / "i18n.rs",)

STR_NEW = re.compile(r'Str::new\(\s*("(?:[^"\\]|\\.)*")\s*,\s*("(?:[^"\\]|\\.)*")\s*\)')
PLACEHOLDER = re.compile(r"\{([a-z_][a-z_0-9]*)\}")


def strip_comments(src: str) -> str:
    """把注释空白化，保留字符串内容与换行，便于后续按位置定位行号。

    认真处理了行注释 / 块注释 / 字符串 / 原始字符串 / 生命周期 ——
    朴素的 `re.sub(r'//.*$', '')` 会被 `"https://…"` 截断。
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        if c == "/" and nxt == "*":
            j = i + 2
            while j + 1 < n and not (src[j] == "*" and src[j + 1] == "/"):
                j += 1
            end = min(j + 2, n)
            for k in range(i, end):
                if src[k] != "\n":
                    out[k] = " "
            i = end
            continue
        if c == "r" and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r'r(#*)"', src[i:])
            if m:
                hashes = m.group(1)
                close = '"' + hashes
                j = src.find(close, i + len(m.group()))
                i = n if j == -1 else j + len(close)
                continue
        if c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                if src[j] == "\\":
                    j += 1
                j += 1
            i = j + 1
            continue
        if c == "'":
            if nxt == "\\":
                j = src.find("'", i + 2)
                i = n if j == -1 else j + 1
                continue
            if nxt and i + 2 < n and src[i + 2] == "'":
                i = i + 3
                continue
            i += 1  # 生命周期：只吃掉引号本身
            continue
        i += 1
    return "".join(out)


def line_of(src: str, pos: int) -> int:
    return src.count("\n", 0, pos) + 1


def rel_to(path: pathlib.Path, base: pathlib.Path) -> str:
    """诊断信息里的路径：仓库内显示相对仓库根，自测的临时树显示相对临时根。"""
    for anchor in (ROOT, base):
        try:
            return str(path.relative_to(anchor))
        except ValueError:
            continue
    return path.name


def placeholders(text: str) -> set[str]:
    return set(PLACEHOLDER.findall(text))


def check_str_new_parity(problems: list[str], i18n_files=None) -> None:
    """`Str::new(zh, en)` 两侧的占位符集合必须一致，且英文侧不为空。"""
    files = tuple(i18n_files) if i18n_files is not None else I18N_FILES
    for path in files:
        path = pathlib.Path(path)
        raw = path.read_text(encoding="utf-8")
        stripped = strip_comments(raw)
        rel = rel_to(path, ROOT)
        for m in STR_NEW.finditer(stripped):
            zh_lit, en_lit = m.group(1), m.group(2)
            zh, en = placeholders(zh_lit), placeholders(en_lit)
            ln = line_of(stripped, m.start())
            if zh != en:
                problems.append(
                    f"{rel}:{ln}: 双语两侧占位符不一致 "
                    f"zh={sorted(zh)} en={sorted(en)}：{zh_lit[:40]}"
                )
            if en_lit[1:-1].strip() == "":
                problems.append(
                    f"{rel}:{ln}: 英文侧是空串：{zh_lit[:40]} —— "
                    f"英文界面会显示成空白"
                )


def run_all(i18n_files=None) -> list[str]:
    """跑完全部检查，返回诊断列表。路径可注入，自测据此用临时目录喂夹具。"""
    problems: list[str] = []
    check_str_new_parity(problems, i18n_files)
    return problems


def main() -> int:
    utf8_when_redirected()
    try:
        problems = run_all()
    except OSError as exc:
        # 「读不到」是未知，不是「没问题」。这里必须失败并说人话，
        # 而不是让异常栈飘过去、或更糟地当成通过。
        print(f"管线文案一致性校验未通过：读不到 {exc.filename}", file=sys.stderr)
        print(
            "  这是门禁自己没跑起来，不等于代码没问题。"
            "请先确认仓库文件齐全，再判断校验结果。",
            file=sys.stderr,
        )
        return 1

    if problems:
        print("管线文案一致性校验未通过：", file=sys.stderr)
        for item in problems:
            print(f"  - {item}", file=sys.stderr)
        print(
            "\n双语规则见 AGENTS.md 的「代码风格」；"
            "Rust 侧只许用 i18n.rs 的 Str::new 成对写文案。",
            file=sys.stderr,
        )
        return 1

    print(
        "管线文案一致性校验通过（双语占位符成对、英文侧非空）",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
