#!/usr/bin/env python3
"""verify-i18n.py 自己的单测。

为什么门禁也要有单测
--------------------
门禁自己就是代码，也会被改坏：把正则写错、把某个 `continue` 挪错位置、
把回看范围缩窄——结果都是**静默地永远放绿**，而且没有编译错误、没有测试失败、
CI 一路全绿。这种失效比门禁本身的红更危险，因为它连「被拦住过」都不是。

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


def scan(core_i18n: str = "") -> list:
    """在临时目录搭一棵最小的 crates 树，把夹具喂给全部检查，返回诊断列表。"""
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        core = root / "crates" / "oneasr-core" / "src" / "i18n.rs"
        core.parent.mkdir(parents=True, exist_ok=True)
        core.write_text(core_i18n, encoding="utf-8")
        return v.run_all(i18n_files=(core,))


def reports(problems: list, needle: str, case: str) -> None:
    if not any(needle in p for p in problems):
        raise AssertionError(f"期望报出 {needle!r}（{case}），实际：{problems!r}")


def quiet(problems: list, case: str) -> None:
    if problems:
        raise AssertionError(f"期望放行（{case}），实际报了：{problems!r}")


CASES = []


def case(fn):
    CASES.append(fn)
    return fn


# ── 检查：Str::new 两侧占位符成对，且英文侧非空 ──────────────────────────


@case
def 双语占位符不一致必须被拦下():
    p = scan('pub const X: Str = Str::new("共 {n} 行", "no placeholder");\n')
    reports(p, "占位符不一致", "占位符不一致")


@case
def 双语占位符一致必须放行():
    p = scan('pub const X: Str = Str::new("共 {n} 行", "{n} lines");\n')
    quiet(p, "占位符一致")


@case
def 英文侧留空必须被拦下():
    p = scan('pub const X: Str = Str::new("退出码", "");\n')
    reports(p, "英文侧是空串", "英文侧留空")


@case
def 两侧同文必须放行():
    """语种选择器的标签：这些词在两种语言下本来就该一样，写成检查就是常驻误报。"""
    p = scan('pub const X: Str = Str::new("语言", "语言");\n')
    quiet(p, "两侧同文（语种名）")


@case
def 注释里的StrNew必须放行():
    """注释里的示例不是文案；strip_comments 坏掉会让它变成假警报。"""
    p = scan(
        '// Str::new("共 {n} 行", "")\n'
        'pub const X: Str = Str::new("设置", "Settings");\n'
    )
    quiet(p, "注释里的 Str::new")


# ── 读不到 = 未知，不是「没问题」 ────────────────────────────────────────


@case
def 双语表读不到时门禁必须失败():
    with tempfile.TemporaryDirectory() as td:
        absent = pathlib.Path(td) / "definitely_absent.rs"

        saved = v.I18N_FILES
        try:
            v.I18N_FILES = (absent,)
            buf = io.StringIO()
            with contextlib.redirect_stderr(buf):
                code = v.main()
        finally:
            v.I18N_FILES = saved

        if code != 1:
            raise AssertionError(f"读不到双语表时应当失败(1)，实际 code={code}")
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
