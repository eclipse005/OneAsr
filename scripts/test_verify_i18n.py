#!/usr/bin/env python3
"""verify-i18n.py 自己的单测。

为什么门禁也要有单测
--------------------
门禁自己就是代码，也会被改坏：把正则写错、把某个 `continue` 挪错位置、把
回看范围缩窄——结果都是**静默地永远放绿**，而且没有编译错误、没有测试失败、
CI 一路全绿。这种失效比门禁本身的红更危险，因为它连「被拦住过」都不是了。

按 AGENTS.md 的「测试要断骨验证」，这里每个用例都对着一条**具体承诺**命名，
并在写完后逐条还原过被测行为，确认对应用例会红（见提交正文的「验证」）。

怎么跑
------
    python3 scripts/test_verify_i18n.py

只依赖标准库。全程在临时目录里搭夹具，不读也不写仓库里的真实文件。
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import sys
import tempfile

SCRIPTS = pathlib.Path(__file__).resolve().parent
# 被测脚本在模块顶层 import _verify_common，先把 scripts/ 放进搜索路径。
sys.path.insert(0, str(SCRIPTS))

_spec = importlib.util.spec_from_file_location(
    "verify_i18n_under_test", SCRIPTS / "verify-i18n.py"
)
v = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(v)


def scan(gui: dict | None = None, core_i18n: str = "", gui_i18n: str = "") -> list:
    """在临时目录搭一棵最小的 crates 树，把夹具喂给全部检查，返回诊断列表。"""
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        gui_src = root / "crates" / "oneasr" / "src"
        gui_src.mkdir(parents=True)
        for name, body in (gui or {}).items():
            p = gui_src / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(body, encoding="utf-8")

        core = root / "crates" / "oneasr-core" / "src" / "i18n.rs"
        core.parent.mkdir(parents=True, exist_ok=True)
        core.write_text(core_i18n, encoding="utf-8")

        gui_table = gui_src / "i18n.rs"
        gui_table.write_text(gui_i18n, encoding="utf-8")

        return v.run_all(
            gui_src=gui_src,
            i18n_files=(core, gui_table),
            crates_root=root / "crates",
        )


def reports(problems: list, needle: str, case: str) -> None:
    if not any(needle in p for p in problems):
        raise AssertionError(f"期望报出 {needle!r}，实际：{problems!r}")


def quiet(problems: list, case: str) -> None:
    if problems:
        raise AssertionError(f"期望放行，实际报了：{problems!r}")


CASES = []


def case(fn):
    CASES.append(fn)
    return fn


# ── 检查 1：渲染层的中文字面量必须在 UiLang::Zh 臂里 ──────────────────────


@case
def 渲染层硬编码单语中文必须被拦下():
    p = scan(gui={"app.rs": 'pub fn r() {\n    let _ = "请先添加文件";\n}\n'})
    reports(p, "绕过了双语表", "硬编码中文")


@case
def 合法的中文配对臂必须放行():
    p = scan(gui={"app.rs": (
        "pub fn l(lang: UiLang) -> String {\n"
        "    match lang {\n"
        '        UiLang::Zh => "请先添加文件".to_string(),\n'
        '        UiLang::En => "Add a file first".to_string(),\n'
        "    }\n"
        "}\n"
    )})
    quiet(p, "Zh 臂里的中文")


@case
def 中文写进En臂必须被拦下():
    """Zh/En 写反是最常见的手滑：它长得像合规写法，症状与硬编码一模一样。"""
    p = scan(gui={"app.rs": (
        "pub fn l(lang: UiLang) -> String {\n"
        "    match lang {\n"
        '        UiLang::Zh => "请先添加文件".to_string(),\n'
        '        UiLang::En => "先添加文件".to_string(),\n'
        "    }\n"
        "}\n"
    )})
    reports(p, "UiLang::En", "中文落在 En 臂")


@case
def En臂跨十几行的块臂也要认出来():
    p = scan(gui={"app.rs": (
        "pub fn l(lang: UiLang) -> String {\n"
        "    match lang {\n"
        "        UiLang::Zh => {\n"
        '            let mut s = String::new();\n'
        '            s.push_str("请先添加");\n'
        "            s\n"
        "        }\n"
        "        UiLang::En => {\n"
        '            let mut s = String::new();\n'
        '            s.push_str("请先添加");\n'
        "            s\n"
        "        }\n"
        "    }\n"
        "}\n"
    )})
    reports(p, "UiLang::En", "跨行块臂")


@case
def 超长块臂里的中文必须放行():
    """回看不能设行数上限：块臂比上限长时，臂内的合法中文会被误判成硬编码。

    断骨锚点：把 `lang_arm` 的 `range(ln, 0, -1)` 换回带上限的写法
    （曾经是 `max(0, ln - 40)`），这条立刻变红。
    """
    filler = "".join('            s.push_str("x%d");\n' % i for i in range(60))
    src = (
        "pub fn l(lang: UiLang) -> String {\n"
        "    match lang {\n"
        "        UiLang::Zh => {\n"
        "            let mut s = String::new();\n"
        f"{filler}"
        '            s.push_str("请先添加文件");\n'
        "            s\n"
        "        }\n"
        "        UiLang::En => {\n"
        "            let mut s = String::new();\n"
        '            s.push_str("Import first");\n'
        "            s\n"
        "        }\n"
        "    }\n"
        "}\n"
    )
    p = scan(gui={"app.rs": src})
    quiet(p, "超长 Zh 块臂里的中文")


@case
def 硬编码英文目前放行且被本用例锁住():
    """已知的同类漏洞：英文界面里混进单语英文串。

    现在加检查会大面积误报（源码里英文串远多于中文串），所以**故意锁住放行**。
    将来真要修这个缺口，先把这条用例改成 expect_reports——它就是提醒你别静默
    改行为的锚。
    """
    p = scan(gui={"app.rs": 'pub fn r() {\n    let _ = ".label(\\"Import\\")";\n}\n'})
    quiet(p, "硬编码英文（已知缺口）")


@case
def 测试块里的中文夹具必须放行():
    p = scan(gui={"app.rs": (
        "pub fn l(lang: UiLang) -> String {\n"
        "    match lang {\n"
        '        UiLang::Zh => "设置".to_string(),\n'
        '        UiLang::En => "Settings".to_string(),\n'
        "    }\n"
        "}\n"
        "\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    #[test]\n"
        "    fn fixtures() {\n"
        '        let _ = "第一行。\\n第二行。";\n'
        "    }\n"
        "}\n"
    )})
    quiet(p, "测试块里的中文")


@case
def 注释里的中文必须放行():
    p = scan(gui={"app.rs": (
        "pub fn r() {\n"
        "    // 这条注释是中文，不是界面文案\n"
        "    let _ = 1;\n"
        "}\n"
    )})
    quiet(p, "注释里的中文")


@case
def 字符串里的双斜杠不该被当成注释():
    p = scan(gui={"app.rs": 'pub fn r() {\n    let _ = "路径 // 中文";\n}\n'})
    reports(p, "绕过了双语表", "字符串里的 //")


@case
def strip_comments不把原始字符串里的双斜杠当成注释():
    """直接测 `strip_comments` 的契约，不走端到端。

    为什么不用端到端夹具：`STRING_LIT` 与 `strip_comments` 有**同一个**引号配对
    盲区——原始字符串里那个落单的 `"` 会让两者同时错位，两者互相抵消，于是
    「串处理坏了」这件事在检查 1 的输出里根本观测不到，端到端夹具只会假绿。
    契约本身是纯函数，直接断言它才对。
    """
    src = 'let _ = (r#"a " // x"#).to_string();\n'
    out = v.strip_comments(src)
    if "// x" not in out:
        raise AssertionError(f"原始字符串里的 // 被当成了注释：{out!r}")


@case
def strip_comments不把普通字符串里的双斜杠当成注释():
    src = 'let _ = "路径 // 中文"; // 真注释\n'
    out = v.strip_comments(src)
    if "路径 // 中文" not in out:
        raise AssertionError(f"字符串里的 // 被当成了注释：{out!r}")
    if "真注释" in out:
        raise AssertionError(f"真注释没有被涂白：{out!r}")


@case
def 跳过清单里的文件不参与检查一():
    """i18n.rs / main.rs / ui_font.rs 里的中文是「值」或字体名，不是漏译。"""
    p = scan(
        gui={
            "main.rs": 'fn pick() {\n    let _ = "雅黑";\n}\n',
            "ui_font.rs": 'fn font() {\n    let _ = "微软雅黑";\n}\n',
        },
        gui_i18n='pub const X: Str = Str::new("设置", "Settings");\n',
    )
    quiet(p, "跳过清单")


# ── 检查 2：Str::new 两侧占位符成对，且英文侧非空 ────────────────────────


@case
def 双语占位符不一致必须被拦下():
    p = scan(core_i18n='pub const X: Str = Str::new("共 {n} 行", "no placeholder");\n')
    reports(p, "占位符不一致", "占位符不一致")


@case
def 双语占位符一致必须放行():
    p = scan(core_i18n='pub const X: Str = Str::new("共 {n} 行", "{n} lines");\n')
    quiet(p, "占位符一致")


@case
def 英文侧留空必须被拦下():
    p = scan(core_i18n='pub const X: Str = Str::new("退出码", "");\n')
    reports(p, "英文侧是空串", "英文侧留空")


@case
def 两侧同文必须放行():
    """语种选择器的标签：这些词在两种语言下本来就该一样，写成检查就是常驻误报。"""
    p = scan(core_i18n='pub const X: Str = Str::new("语言", "语言");\n')
    quiet(p, "两侧同文（语种名）")


# ── 检查 3：expand 的 key 要与条目占位符对齐 ─────────────────────────────

_BROKE = 'pub const TRANSCRIPT_BROKE: Str = Str::new("已断为 {n} 行", "now {n} lines");\n'


@case
def expand的key少传一个必须被拦下():
    p = scan(
        gui={"w.rs": 'pub fn r() {\n    expand(t(L::TRANSCRIPT_BROKE), &[("wrong", "3")])\n}\n'},
        core_i18n=_BROKE,
    )
    reports(p, "不一致", "expand key 少传")


@case
def expand的key对得上必须放行():
    p = scan(
        gui={"w.rs": 'pub fn r() {\n    expand(t(L::TRANSCRIPT_BROKE), &[("n", "3")])\n}\n'},
        core_i18n=_BROKE,
    )
    quiet(p, "expand key 一致")


@case
def expand指向已改名的条目必须被拦下():
    p = scan(
        gui={"w.rs": 'pub fn r() {\n    expand(t(L::GONE), &[("n", "3")])\n}\n'},
        core_i18n=_BROKE,
    )
    reports(p, "不在双语表里", "expand 指向不存在的条目")


# ── 读不到 = 未知，不是「没问题」 ────────────────────────────────────────


@case
def 双语表读不到时门禁必须失败():
    """与上一条同理：GUI 源码与 crates 目录都换成**存在**的临时目录，
    保证 code=1 只可能来自「双语表读不到」这一件事。"""
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        gui_src = root / "gui-src"
        gui_src.mkdir()
        crates = root / "crates"
        crates.mkdir()
        absent = root / "definitely_absent.rs"

        saved = (v.GUI_SRC, v.I18N_FILES, v.CRATES_ROOT)
        try:
            v.GUI_SRC = gui_src
            v.I18N_FILES = (absent, absent)
            v.CRATES_ROOT = crates
            buf = io.StringIO()
            with contextlib.redirect_stderr(buf):
                code = v.main()
        finally:
            v.GUI_SRC, v.I18N_FILES, v.CRATES_ROOT = saved

        if code != 1:
            raise AssertionError(f"读不到双语表时应当失败(1)，实际 code={code}")
        if "读不到" not in buf.getvalue():
            raise AssertionError(f"失败原因应当说人话，实际：{buf.getvalue()!r}")


@case
def 界面源码目录读不到时门禁必须失败():
    """`Path.rglob` 对不存在的目录返回空——不显式挡住，检查 1 就成了空转。

    「读不到」被当成「没有问题」，正是 magpie LESSONS 里那条
    "a failed read means unknown, never empty" 的翻版。

    这里必须把 I18N_FILES 与 CRATES_ROOT 一并换成**真实存在**的临时文件：
    否则双语表读不到会让 main() 因别的理由返回 1，这条用例就成了「因错误的
    原因绿」——把守卫拆掉它照样绿，等于没测。断骨验证时已确认这条会变红。
    """
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        core = root / "oneasr-core-i18n.rs"
        core.write_text("", encoding="utf-8")
        gui_table = root / "gui-i18n.rs"
        gui_table.write_text("", encoding="utf-8")
        empty_crates = root / "crates"
        empty_crates.mkdir()

        saved = (v.GUI_SRC, v.I18N_FILES, v.CRATES_ROOT)
        try:
            v.GUI_SRC = root / "definitely_absent_dir"
            v.I18N_FILES = (core, gui_table)
            v.CRATES_ROOT = empty_crates
            buf = io.StringIO()
            with contextlib.redirect_stderr(buf):
                code = v.main()
        finally:
            v.GUI_SRC, v.I18N_FILES, v.CRATES_ROOT = saved

        if code != 1:
            raise AssertionError(f"源码目录缺失时应当失败(1)，实际 code={code}")
        if "读不到" not in buf.getvalue():
            raise AssertionError(f"失败原因应当说人话，实际：{buf.getvalue()!r}")


def main() -> int:
    failed = []
    for fn in CASES:
        try:
            fn()
        except Exception as exc:  # 崩溃也是失败：还原修复时它要变红，不是变蓝屏
            failed.append((fn.__name__, f"{type(exc).__name__}: {exc}"))
        else:
            print(f"  ok  {fn.__name__}", file=sys.stderr)

    if failed:
        print(f"\nverify-i18n.py 自测未通过（{len(failed)}/{len(CASES)} 失败）：", file=sys.stderr)
        for name, msg in failed:
            print(f"  - {name}: {msg}", file=sys.stderr)
        return 1

    print(f"\nverify-i18n.py 自测通过（{len(CASES)} 条）", file=sys.stderr)
    return 0


if __name__ == "__main__":
    v.utf8_when_redirected()
    sys.exit(main())