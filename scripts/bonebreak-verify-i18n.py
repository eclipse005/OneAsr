#!/usr/bin/env python3
"""verify-i18n.py 的断骨验证：把每处检查逐个还原，确认对应用例真的变红。

为什么需要这个工具
------------------
AGENTS.md 的「测试要断骨验证」要求：新测试写完，把被测的修复还原跑一遍，
测试必须变红；在原样代码上照样绿的测试等于没写。手做一次不难，但几处锚点
逐个还原很容易漏，而且漏掉的正是「我以为测了」的那些。

它做的事：把 scripts/ 复制到临时目录，在副本上逐条施加变异（把一处检查改坏），
跑自测，确认对应用例变红。**仓库工作树全程不动。**

为什么不在 CI 里跑
------------------
锚点是 verify-i18n.py 的源码片段。改动实现就会让锚点漂移，CI 会因为
「工具自己过期」而红——那是假警报，会让人开始忽略这个信号。所以它是维护者
在**改过 verify-i18n.py 或它的测试之后**手动跑的工具，不是门禁。

用法
----
    python3 scripts/bonebreak-verify-i18n.py

全绿 = 每条检查都有对应用例守着。出现 NOT-RED = 那条检查没有测试，或那条
测试是摆设。
"""

from __future__ import annotations

import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from _verify_common import utf8_when_redirected  # noqa: E402

SCRIPTS = pathlib.Path(__file__).resolve().parent
FILES = ("verify-i18n.py", "test_verify_i18n.py", "_verify_common.py")

# (变异名, 唯一锚点, 把它改成什么, 期望变红的用例名片段)
MUTATIONS = [
    ("M1 不查英文侧空串",
     '            if en_lit[1:-1].strip() == "":',
     "            if False:",
     "英文侧留空"),
    ("M2 不比占位符",
     "            if zh != en:",
     "            if False:",
     "双语占位符不一致"),
    ("M3 注释不涂白",
     '        if c == "/" and nxt == "/":',
     "        if False:",
     "注释里的StrNew必须放行"),
    ("M4 读不到时不报人话",
     "    except OSError as exc:",
     "    except ValueError as exc:",
     "双语表读不到"),
]


def run(td: pathlib.Path) -> tuple[int, str]:
    p = subprocess.run(
        [sys.executable, str(td / "test_verify_i18n.py")],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def main() -> int:
    for f in FILES:
        if not (SCRIPTS / f).exists():
            print(f"缺少 {f}，请从仓库根目录运行 scripts/ 下的本脚本。", file=sys.stderr)
            return 2

    results = []
    for name, old, new, expect in MUTATIONS:
        with tempfile.TemporaryDirectory() as td_:
            td = pathlib.Path(td_)
            for f in FILES:
                shutil.copy2(SCRIPTS / f, td / f)

            target = td / "verify-i18n.py"
            text = target.read_text(encoding="utf-8")
            if old not in text:
                results.append(
                    (name, "SKIP", f"锚点没匹配上，实现可能已重构，需更新本脚本：{old.strip()[:50]}")
                )
                continue
            if text.count(old) != 1:
                results.append((name, "SKIP", f"锚点出现 {text.count(old)} 次，不唯一"))
                continue
            target.write_text(text.replace(old, new), encoding="utf-8")

            code, out = run(td)
            red_cases = re.findall(r"^  - (.+?):", out, re.M)
            hit = [c for c in red_cases if expect in c]
            if code != 0 and hit:
                results.append((name, "RED", f"变红用例：{hit[0]}"))
            elif code == 0:
                results.append((name, "NOT-RED", "还原后自测仍然全绿 —— 这条检查没有测试在守"))
            else:
                results.append(
                    (name, "OTHER", f"红了，但不是预期的 {expect!r}；实际红：{red_cases}")
                )

    width = max((len(r[0]) for r in results), default=10)
    print(file=sys.stderr)
    for name, status, detail in results:
        print(f"  {status:<8} {name:<{width}}  {detail}", file=sys.stderr)
    bad = [r for r in results if r[1] != "RED"]
    print(
        f"\n断骨验证：{len(results) - len(bad)}/{len(results)} 条按预期变红",
        file=sys.stderr,
    )
    for name, status, detail in bad:
        print(f"  !! {name}: {status} — {detail}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    utf8_when_redirected()
    sys.exit(main())