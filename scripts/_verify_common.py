#!/usr/bin/env python3
"""scripts/ 下各校验脚本共用的小工具。

为什么不各自写一份
------------------
同一件事写三遍，第三次修 bug 就会漏掉前两次。三个校验脚本
（verify-version.py / verify-i18n.py / verify-ffmpeg-manifest.py）都要往
stderr 打中文诊断，所以共用这一份；将来再加第四个脚本，从这里 import 即可。

这与本仓库 Rust 引擎与 Electron 壳共用同一套产物的做法是同一条原则：机制只有
一处，调用方各自取用，不复制。Rust 侧的双语门禁在 verify-i18n.py，Electron
渲染层的双语门禁在 apps/electron 的 vitest 里。
"""

from __future__ import annotations

import sys


def utf8_when_redirected() -> None:
    """输出被重定向时（管道 / 文件 / CI 日志）把 stdout、stderr 切到 UTF-8。

    中文 Windows 上 Python 默认按 cp936 编码输出，诊断信息一进管道就是乱码 ——
    而门禁的诊断看不清，等于没有门禁。CI（ubuntu）与 PowerShell 7 读的都是
    UTF-8，不切就会在日志里糊成一片。

    真连着控制台时不动编码：那里由控制台代码页自己解释，抢过来反而会在经典
    conhost 上变成另一种乱码。判断不了是不是终端时也照旧改，坏了只是这次的
    日志难看，不影响退出码。
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            if not stream.isatty():
                stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError, OSError):
            # 老解释器或替身流没有 reconfigure：诊断仍能打印，只是编码照旧。
            pass