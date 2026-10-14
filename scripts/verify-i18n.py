#!/usr/bin/env python3
"""界面文案一致性断言：把 AGENTS.md 的双语规则变成机器门禁。

为什么需要它
------------
AGENTS.md 要求「界面文案必须双语，不许硬编码单语字符串」。这条规则以前只靠
记忆执行 —— 规则在文档里，代码里照样会犯。而双语机制本身（`Str::new(zh, en)`
成对构造 + `match lang` 编译期穷尽）已经把「漏译」挡在编译器外面，剩下的
四个漏洞全是**编译器看不见、只在渲染时显形**的：

  1. 渲染层绕过双语表直接写中文串（`.label("请先…")`），英文界面照样显示中文；
  2. 中文串被写进了 `UiLang::En` 臂（Zh/En 写反、复制分支抄错），症状同上，
     但它「长得像合规写法」，比第 1 条更容易溜过去；
  3. `Str::new` 两侧占位符集合不一致（`{n}` 一侧有一侧无），一侧渲染出空洞；
  4. `Str::new` 的英文侧是空串，英文界面整条文案显示为空白。

外加 `expand(t(L::X), &[("k", …)])` 的 key 与条目占位符对不上：`{k}` 会原样
留在界面上。这五条都能静态查清，且当前全绿 —— 加门禁的最佳时机是没欠债的时候。

门禁自己也要有单测，否则改坏一条正则就会**静默地永远放绿**。用例见
scripts/test_verify_i18n.py，CI 每次提交都跑。

为什么不查别的
--------------
试过三条更狠的，都会误报，所以**没做**。写在这里是为了下次有人（或有 agent）
想「顺手加强一下」时，先看见当初为什么没做：

  - 「GUI 里禁一切中文字面量」：合法的 `UiLang::Zh => "…"` 配对臂有 40 处，
    一刀切会把合规写法全拦下。改成「中文字面量必须在 `UiLang::` 配对臂里」，
    再按第 2 条收窄成「不能在 `En` 臂里」。
  - 「渲染层禁一切英文串」：与第 1 条同形的漏洞，但 Rust 源码里英文串远多于
    中文串（日志、错误、内部键名、CLI 参数……），当前形态下误报压不住。等哪天
    真出现第二次，规则再升级 —— 别在没有第二次证据时先写死。
  - 「`Str::new` 两侧同文就报错」：仓库里有 4 处**合法的**两侧同文，全是语种
    选择器的标签（`"语言"` / `"English"` / `"语"` / `"En"`）—— 这些词在两种语言
    下本来就该一样。静态分不出「忘了翻译」和「本来就一样」，写了就是 4 条常驻误报。
    （英文侧**留空**没有这个问题：空串在任何语言下都是 bug，所以第 4 条只查空。）
  - 「测试里禁盘符路径」：`settings.rs` 的测试故意用 `r"D:\…"` 当**合成输入**
    （AGENTS.md 只禁「当期望值」），静态分不清输入与期望值，误报一堆。

用法
----
    python3 scripts/verify-i18n.py

标准输出只打印通过与否，诊断走 stderr。CI 在每次提交上跑（见 ci.yml）。
读不到文件时按「未知」处理并失败，不按「没问题」放行。
"""

from __future__ import annotations

import errno
import pathlib
import re
import sys

from _verify_common import utf8_when_redirected

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 只查 GUI crate：oneasr-core 里大量中文是**被处理的文本**（断句规则、标点
# 归一、语种判定），不是界面文案，查它只会误报。
GUI_SRC = ROOT / "crates" / "oneasr" / "src"
I18N_FILES = (
    ROOT / "crates" / "oneasr-core" / "src" / "i18n.rs",
    ROOT / "crates" / "oneasr" / "src" / "i18n.rs",
)
CRATES_ROOT = ROOT / "crates"

# 这几个文件里的中文不是界面文案，跳过检查 1：
#   i18n.rs   —— 双语表本体，中文是「值」不是「漏译」
#   main.rs   —— 字体探测（"雅黑" 用来匹配 YaHei）
#   ui_font.rs —— 中文字体名（微软雅黑 / 等线 / 宋体…），按名匹配，不译
COPY_CHECK_SKIP = {"i18n.rs", "main.rs", "ui_font.rs"}

CJK = re.compile(r"[一-鿿]")
STRING_LIT = re.compile(r'"(?:[^"\\\n]|\\.)*"')
STR_NEW = re.compile(r'Str::new\(\s*("(?:[^"\\]|\\.)*")\s*,\s*("(?:[^"\\]|\\.)*")\s*\)')
EXPAND_CALL = re.compile(r"expand\(\s*t\(L::(\w+)\)\s*,\s*&\[(.*?)\]\s*\)", re.S)
PLACEHOLDER = re.compile(r"\{([a-z_][a-z_0-9]*)\}")
TEST_MARK = re.compile(r"#\[cfg\(test\)\]|^\s*(?:pub\(crate\)\s+)?mod\s+tests\s*\{")
LANG_ARM = re.compile(r"UiLang::(Zh|En)\b")
FN_LINE = re.compile(r"\bfn\s+\w+")


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


def test_lines(src: str) -> set[int]:
    """标记 `#[cfg(test)]` / `mod tests {` 块所占的行（1-based）。

    测试里的中文多半是夹具与期望值（"第一行。\\n第二行。"），不是界面文案，
    不该被检查 1 拦。`#[cfg(test)]` 在 `mod tests {` 的前一行，块的结束要等
    深度回落到进入之前，所以用 `entered` 把「还没进块」和「块已结束」区分开。
    """
    lines = src.splitlines()
    masked: set[int] = set()
    depth = 0
    in_test = False
    entered = False
    base = 0
    for idx, line in enumerate(lines, 1):
        if not in_test and TEST_MARK.search(line):
            in_test = True
            entered = False
            base = depth
        if in_test:
            masked.add(idx)
        depth += line.count("{") - line.count("}")
        if in_test:
            if depth > base:
                entered = True
            elif entered:
                in_test = False
    return masked


def blank_strings(src: str) -> str:
    """把字符串字面量整体抹成空格（保留换行）。

    `lang_arm` 靠花括号配平回看，字符串里的 `{queued}` 这类占位符会干扰
    计数 —— 抹掉之后，配平只反映真实的代码结构。
    """
    out = list(src)
    for m in STRING_LIT.finditer(src):
        for k in range(m.start(), m.end()):
            if src[k] != "\n":
                out[k] = " "
    return "".join(out)


def lang_arm(lines: list[str], ln: int) -> tuple[bool, str | None]:
    """判断第 `ln` 行落在哪个 `UiLang::` 语言臂内，返回 `(在臂内, 臂名)`。

    从字面量所在行向上走：先吃掉已闭合的内层块（`}` 向上与 `{` 配对），
    在 `fn` 边界之前任何一行命中 `UiLang::` 就算在臂内。既能认
    `UiLang::Zh => format!("…")` 单行臂，也能认 `UiLang::Zh => {` 开头、
    跨十几行、内层还有 `if` 块的块臂（如 ui/text.rs 的拼接）。
    `fn` 边界兜住「别的函数里恰好有 UiLang::」的假阴性。

    臂名要有返回值而不是只回一个 bool：只知道「在某个臂里」就分不出
    `UiLang::Zh => "中文"` 与 `UiLang::En => "中文"` —— 后者正是改 i18n 时
    最高频的手滑（全局替换写反、复制分支抄错），症状与硬编码一模一样，
    却长得像合规写法。命中 `UiLang::` 的其它变体时臂名为 None，保守放行。
    """
    skip = 0
    for i in range(ln, max(0, ln - 40), -1):
        line = lines[i - 1]
        # 向上回看时「净闭括号数」统一是 `}` 减 `{`：skip>0 表示正在找
        # 某个已闭合块的开括号，每见到一个 `{` 就吃掉一层，归零即跳完。
        if skip > 0:
            skip += line.count("}") - line.count("{")
            continue
        m = LANG_ARM.search(line)
        if m:
            return True, m.group(1)
        if "UiLang::" in line:
            return True, None  # 其它变体：不在本门禁的射程内，保守放行
        if FN_LINE.search(line):
            return False, None  # 撞上函数边界，所在函数里没有语言臂
        skip += line.count("}") - line.count("{")
        if skip < 0:
            skip = 0  # 进入了外层块，继续向上找臂头
    return False, None


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


def check_copy_literals(problems: list[str], gui_src=None, skip=None) -> None:
    """检查 1：GUI 渲染层的中文字面量必须落在 `UiLang::Zh` 臂里。

    一条含 CJK 的字符串字面量，从它向上回看（跳过已闭合的内层块）必须命中
    `UiLang::`。单行臂 `UiLang::Zh => format!("…",` 与块臂
    `UiLang::Zh => { … }` 都算在臂内；`.label("请先添加文件")` 这种单语
    硬编码会被拦下。

    检查 1b：命中的是 `UiLang::En` 臂时报错 —— 英文界面会显示中文。
    """
    base = pathlib.Path(gui_src) if gui_src is not None else GUI_SRC
    skipped = COPY_CHECK_SKIP if skip is None else skip
    # `Path.rglob` 对不存在的目录返回空而不是报错——不显式挡住，这个检查就
    # 悄悄变成了空转：没人改代码，门禁也一直绿，而其实什么都没查。
    # 「读不到」必须当成未知，不能当成「没问题」。
    if not base.is_dir():
        raise FileNotFoundError(errno.ENOENT, "界面源码目录不存在", str(base))
    for path in sorted(base.rglob("*.rs")):
        if path.name in skipped:
            continue
        raw = path.read_text(encoding="utf-8")
        masked = test_lines(raw)
        stripped = strip_comments(raw)
        # 回看用的行要先把字符串抹空：占位符 `{…}` 会干扰花括号配平。
        lines = blank_strings(stripped).splitlines()
        for m in STRING_LIT.finditer(stripped):
            if not CJK.search(m.group()):
                continue
            ln = line_of(stripped, m.start())
            if ln in masked:
                continue
            in_arm, arm = lang_arm(lines, ln)
            if in_arm and arm != "En":
                continue
            rel = rel_to(path, base)
            if in_arm:
                problems.append(
                    f"{rel}:{ln}: 中文字面量落在了 `UiLang::En` 臂里："
                    f"{m.group()[:40]} —— 英文界面照样显示中文；"
                    f"多半是 Zh/En 写反了或复制分支时抄错"
                )
            else:
                problems.append(
                    f"{rel}:{ln}: 界面文案绕过了双语表：{m.group()[:40]} —— "
                    f"改用 i18n.rs 的 Str::new 或 `match lang` 的 UiLang::Zh/En 配对臂"
                )


def check_str_new_parity(problems: list[str], i18n_files=None) -> None:
    """检查 2：`Str::new(zh, en)` 两侧的占位符集合必须一致，且英文侧不为空。

    一侧有 `{n}` 另一侧没有，渲染时那一侧要么漏掉值、要么把 `{n}` 原样漏出。
    英文侧留空则整条文案在英文界面显示为空白 —— 这与「两侧同文」不同，
    空串在任何语言下都是 bug，所以只禁空，不禁同文（见文件头说明）。
    """
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


def entry_placeholders(i18n_files=None) -> dict[str, set[str]]:
    """收集两份 i18n.rs 里每个 `L::X` 条目两侧的占位符并集。"""
    files = tuple(i18n_files) if i18n_files is not None else I18N_FILES
    entries: dict[str, set[str]] = {}
    for path in files:
        path = pathlib.Path(path)
        stripped = strip_comments(path.read_text(encoding="utf-8"))
        for m in STR_NEW.finditer(stripped):
            # 条目形如 `pub const NAME: Str = Str::new(…)`；往前找最近的名字。
            head = stripped[max(0, m.start() - 200): m.start()]
            nm = re.findall(r"(?:pub\s+)?const\s+(\w+)\s*:\s*Str\s*=", head)
            if nm:
                entries.setdefault(nm[-1], set()).update(
                    placeholders(m.group(1)) | placeholders(m.group(2))
                )
    return entries


def check_expand_keys(problems: list[str], crates_root=None, i18n_files=None) -> None:
    """检查 3：`expand(t(L::X), &[("k", …)])` 的 key 必须与条目占位符一致。

    少传一个 key，`{k}` 会原样留在界面上；多传一个，说明条目改名后调用点没跟。
    """
    base = pathlib.Path(crates_root) if crates_root is not None else CRATES_ROOT
    entries = entry_placeholders(i18n_files)
    for path in sorted(base.rglob("*.rs")):
        path = pathlib.Path(path)
        stripped = strip_comments(path.read_text(encoding="utf-8"))
        rel = rel_to(path, base)
        for m in EXPAND_CALL.finditer(stripped):
            name, body = m.group(1), m.group(2)
            keys = set(re.findall(r'\(\s*"(\w+)"\s*,', body))
            want = entries.get(name)
            if want is None:
                problems.append(
                    f"{rel}:{line_of(stripped, m.start())}: expand 用的 L::{name} "
                    f"不在双语表里（改名了？）"
                )
                continue
            if keys != want:
                problems.append(
                    f"{rel}:{line_of(stripped, m.start())}: expand(L::{name}) 的 key "
                    f"{sorted(keys)} 与条目占位符 {sorted(want)} 不一致 —— "
                    f"少了会把 `{{k}}` 原样显示给用户"
                )


def run_all(gui_src=None, i18n_files=None, crates_root=None) -> list[str]:
    """跑完全部检查，返回诊断列表。路径可注入，自测据此用临时目录喂夹具。"""
    problems: list[str] = []
    check_copy_literals(problems, gui_src)
    check_str_new_parity(problems, i18n_files)
    check_expand_keys(problems, crates_root, i18n_files)
    return problems


def main() -> int:
    utf8_when_redirected()
    try:
        problems = run_all()
    except OSError as exc:
        # 「读不到」是未知，不是「没问题」。这里必须失败并说人话，
        # 而不是让异常栈飘过去、或更糟地当成通过。
        print(f"界面文案一致性校验未通过：读不到 {exc.filename}", file=sys.stderr)
        print(
            "  这是门禁自己没跑起来，不等于代码没问题。"
            "请先确认仓库文件齐全，再判断校验结果。",
            file=sys.stderr,
        )
        return 1

    if problems:
        print("界面文案一致性校验未通过：", file=sys.stderr)
        for item in problems:
            print(f"  - {item}", file=sys.stderr)
        print(
            "\n双语规则见 AGENTS.md 的「代码风格」；"
            "渲染层只许用 i18n.rs 的 Str::new 或 UiLang::Zh/En 配对臂。",
            file=sys.stderr,
        )
        return 1

    print(
        "界面文案一致性校验通过（渲染层无单语硬编码、中文不在 En 臂、"
        "双语占位符成对、英文侧非空、expand 调用对齐）",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())