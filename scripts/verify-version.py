#!/usr/bin/env python3
"""版本号一致性断言：Cargo.toml 是唯一来源，tag 与两份 README 必须与它一致。

为什么需要它
------------
发布一个版本要同时改动四处：Cargo.toml 的版本、git tag、README.md 与
README.zh-CN.md 顶部的版本号和 5 条下载链接。这些以前全靠手工保持一致，
而发布是 tag 触发的 —— 只要 tag 与 Cargo.toml 不同，发布流程会**照样成功**，
最后生出一个文件名带版本号、内容却是另一个版本的产物，链接 404 也没人知道。

现在：`cargo metadata` 读到的 [workspace.package] version 是唯一真值，
本脚本把 tag 和两份 README 全部对着它核一遍。CI 在每次提交上跑，
release.yml 在打包**之前**再跑一次。

用法
----
    python3 scripts/verify-version.py                 # 只校验 README
    python3 scripts/verify-version.py --tag v1.1.0    # 额外断言 tag 与 manifest 一致

标准输出只打印版本号（release.yml 直接取它当构建版本），所有诊断走 stderr。
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
READMES = ("README.md", "README.zh-CN.md")

# 一份发布必须挂上这 5 个产物；两份 README 的下载表都要覆盖它们。
ARTIFACT_SUFFIXES = (
    "windows_setup.exe",
    "windows_portable.zip",
    "linux_x64.deb",
    "linux_x64.tar.gz",
    "macos.dmg",
)


def fail(problems: list[str]) -> None:
    print("版本一致性校验未通过：", file=sys.stderr)
    for item in problems:
        print(f"  - {item}", file=sys.stderr)
    print(
        "\n唯一真值是 Cargo.toml 的 [workspace.package] version；"
        "tag 与两份 README 都要跟着它改（见 AGENTS.md 的发布流程）。",
        file=sys.stderr,
    )
    sys.exit(1)


def manifest_version() -> str:
    """读 Cargo.toml 的 [workspace.package] version。

    用 toml 解析而不是正则匹配：不会再被文件里的其他 `version = ...` 骗到，
    也不需要装 Rust 工具链 —— 这个检查要能在最便宜的 job 里跑。
    """
    try:
        import tomllib
    except ModuleNotFoundError:  # pragma: no cover - 只可能在 Python < 3.11 上出现
        fail(["需要 Python 3.11+（tomllib）；CI 用的是 ubuntu-latest 自带的 Python"])

    try:
        with CARGO_TOML.open("rb") as handle:
            data = tomllib.load(handle)
        version = data["workspace"]["package"]["version"]
    except FileNotFoundError:
        fail([f"读不到 {CARGO_TOML}"])
    except (KeyError, tomllib.TOMLDecodeError) as err:
        fail([f"Cargo.toml 里取不到 [workspace.package] version：{err!r}"])

    if not isinstance(version, str) or not version:
        fail([f"[workspace.package] version 不是非空字符串：{version!r}"])
    return version


def check_readme(name: str, version: str, problems: list[str]) -> None:
    path = ROOT / name
    if not path.is_file():
        problems.append(f"{name}: 文件不存在")
        return
    text = path.read_text(encoding="utf-8")
    head = text[:6000]  # 版本号与下载表都在文件顶部

    # 所有 releases/download/v<X>/ 里的 <X> 必须是 manifest 版本。
    for found in sorted(set(re.findall(r"releases/download/v([^/)\s]+)/", head))):
        if found != version:
            problems.append(
                f"{name}: 下载链接指向 v{found}，但 Cargo.toml 是 {version}"
            )

    # 顶部那句 releases/tag/v<X> 同理。
    for found in sorted(set(re.findall(r"releases/tag/v([^)\s]+)", head))):
        if found != version:
            problems.append(f"{name}: 版本链接指向 v{found}，但 Cargo.toml 是 {version}")

    # 5 个产物名都要在下载表里出现，且带正确版本号。
    for suffix in ARTIFACT_SUFFIXES:
        expected = f"OneAsr_{version}_{suffix}"
        if expected not in head:
            problems.append(f"{name}: 下载表里找不到 {expected}")

    # 任何形如 OneAsr_<ver>_ 的旧版本残留都要报出来（漏改一行的典型症状）。
    for found in sorted(set(re.findall(r"OneAsr_([0-9][0-9A-Za-z.\-]*)_", head)) - {version}):
        problems.append(f"{name}: 残留了旧版本产物名 OneAsr_{found}_…")


def main() -> int:
    parser = argparse.ArgumentParser(description="校验版本号一致性")
    parser.add_argument("--tag", default="", help="要一并断言的 git tag，例如 v1.1.0")
    args = parser.parse_args()

    version = manifest_version()
    problems: list[str] = []

    if args.tag:
        tag = args.tag
        if not tag.startswith("v"):
            problems.append(f"tag `{tag}` 不符合 `v<版本>` 约定")
        elif tag[1:] != version:
            problems.append(
                f"tag `{tag}` 与 Cargo.toml 的 {version} 不一致 —— "
                f"请先改 Cargo.toml 再打 tag（tag 打错只能删掉重打）"
            )

    for readme in READMES:
        check_readme(readme, version, problems)

    if problems:
        fail(problems)

    print(f"版本一致性校验通过：{version}（Cargo.toml / README.md / README.zh-CN.md"
          + (f" / tag {args.tag}" if args.tag else "") + "）", file=sys.stderr)
    # 唯一写入 stdout 的东西就是版本号。
    print(version)
    return 0


if __name__ == "__main__":
    sys.exit(main())
