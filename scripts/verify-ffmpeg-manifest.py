#!/usr/bin/env python3
"""校验/生成随产物分发的 ffmpeg 归档哈希清单。

背景
----
打包脚本从上游 `Tyrrrz/FFmpegBin` 的**版本化 tag**（tag 名就是 ffmpeg 版本号）
下载预编译归档：

    https://github.com/Tyrrrz/FFmpegBin/releases/download/<版本>/<平台>.zip

我们不自己托管 ffmpeg。FFmpeg 官方只发布源码，预编译二进制都由第三方构建，
FFmpegBin 是其中被广泛使用的一家；钉它的版本化 tag（而不是 latest）是为了让
历史版本可以原样重新打包 —— 浮动 tag 随时可能被删掉重建。

GitHub 的 release 资产默认仍可被重新上传（该 release 的 `immutable` 当前为
false），所以真正的信任锚点是**内容哈希**，不是 URL：`scripts/ffmpeg-checksums.txt`
记下每个归档的 sha256，两个打包脚本在把 ffmpeg 写进产物前强制校验。

本脚本做两件事：

  默认            拉取 GitHub API 的该 tag，比对每个归档的 `digest` 与清单是否一致。
                  CI 里跑它 -> 上游重传了资产，CI 立刻失败，而不是等用户下到一份
                  被换过的 ffmpeg。
  --print-manifest 按清单格式打印上游摘要（用于重新生成清单文件）。

来源不在本脚本里硬编码：`base-url` 由清单文件自己声明（`# base-url <URL>` 一行），
换 ffmpeg 只改清单，不会出现"脚本与清单各说一处"。

只依赖标准库。
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import sys
import urllib.error
import urllib.request

MANIFEST = pathlib.Path(__file__).resolve().with_name("ffmpeg-checksums.txt")

# 接受 `https://github.com/<owner>/<repo>/releases/download/<tag>[/]`，
# owner/repo/tag 各取一段，够用且不含任何写死的仓库名。
BASE_URL_RE = re.compile(
    r"^https://github\.com/(?P<owner>[^/]+)/(?P<repo>[^/]+)"
    r"/releases/download/(?P<tag>[^/]+)/?$"
)


def parse_base_url(path: pathlib.Path) -> tuple[str, str]:
    """从清单里读出 base-url，返回 (owner/repo, tag)。"""
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("#"):
            parts = line.split()
            if len(parts) == 3 and parts[1] == "base-url":
                m = BASE_URL_RE.match(parts[2])
                if not m:
                    raise SystemExit(
                        f"{path}: base-url 不是合法的 GitHub release 下载地址：{parts[2]}"
                    )
                return f"{m['owner']}/{m['repo']}", m["tag"]
    raise SystemExit(f"{path}: 缺少 '# base-url <URL>' 行")


def parse_manifest(path: pathlib.Path) -> dict[str, str]:
    """读取清单：`<sha256>  <资产名>`，`#` 起头为注释。"""
    entries: dict[str, str] = {}
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 2:
            raise SystemExit(f"{path}:{lineno}: 期望 `<sha256>  <资产名>`，实际：{raw!r}")
        digest, name = parts
        if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise SystemExit(f"{path}:{lineno}: 不是合法的 sha256：{digest!r}")
        if name in entries:
            raise SystemExit(f"{path}:{lineno}: 资产名重复：{name}")
        entries[name] = digest
    return entries


def _request(url: str, token: str | None) -> dict:
    req = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json"})
    req.add_header("User-Agent", "oneasr-verify-ffmpeg-manifest")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)


def fetch_assets(repo: str, tag: str) -> dict[str, dict]:
    """从 GitHub API 取该 tag 的资产列表（含 digest）。"""
    url = f"https://api.github.com/repos/{repo}/releases/tags/{tag}"
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    try:
        payload = _request(url, token)
    except urllib.error.HTTPError as err:
        # 令牌在 fork PR 上可能没有读权限（403）或已失效（401）。这时退回匿名请求，
        # 而不是让「令牌问题」伪装成「供应链被篡改」——后者会让人去查错方向。
        # 匿名请求真撞上速率限制时，下面会照常失败并说清原因。
        if token and err.code in (401, 403):
            print(f"  提示：令牌被上游拒绝（{err.code}），改用匿名请求重试。", file=sys.stderr)
            try:
                payload = _request(url, None)
            except urllib.error.HTTPError as retry_err:
                raise SystemExit(
                    f"GitHub API {url} 返回 {retry_err.code} {retry_err.reason}"
                    "（令牌与匿名请求都失败；匿名请求常见于触发速率限制）"
                )
            except urllib.error.URLError as retry_err:
                raise SystemExit(f"连不上 GitHub API（{url}）：{retry_err.reason}")
        elif err.code == 403:
            raise SystemExit(
                f"GitHub API {url} 返回 403（多半是速率限制；等几分钟或设置 GITHUB_TOKEN）"
            )
        else:
            raise SystemExit(f"GitHub API {url} 返回 {err.code} {err.reason}")
    except urllib.error.URLError as err:
        raise SystemExit(f"连不上 GitHub API（{url}）：{err.reason}")

    assets: dict[str, dict] = {}
    for asset in payload.get("assets", []):
        # 老接口/未计算摘要时 digest 为空字符串：校验时会明确报出来，不假装通过。
        assets[asset["name"]] = {"digest": asset.get("digest") or "", "size": asset.get("size")}
    if not assets:
        raise SystemExit(f"{repo} 的 release {tag} 没有任何资产")
    return assets


def _bare_digest(name: str, assets: dict[str, dict]) -> str:
    """取资产 digest 并去掉 GitHub 的 `sha256:` 前缀。"""
    digest = assets[name]["digest"]
    if digest.startswith("sha256:"):
        digest = digest[len("sha256:") :]
    return digest


def print_manifest(base_url: str, assets: dict[str, dict], names: list[str]) -> None:
    """按清单格式打印（供 `--print-manifest` 重新生成文件）。"""
    print(f"# base-url {base_url}")
    print(f"# 下面这段由 scripts/verify-ffmpeg-manifest.py --print-manifest 生成，请勿手改哈希。")
    for name in names:
        if name not in assets:
            raise SystemExit(f"上游没有资产 {name}，无法生成清单")
        digest = _bare_digest(name, assets)
        if not digest:
            raise SystemExit(f"上游未提供 {name} 的 digest，无法生成清单（哈希清单会失去意义）")
        print(f"{digest}  {name}")


def verify(
    base_url: str, tag: str, manifest: dict[str, str], assets: dict[str, dict]
) -> int:
    """比对清单与上游，返回进程退出码。"""
    failures: list[str] = []
    checked = 0

    if not manifest:
        print("ffmpeg 哈希清单为空。", file=sys.stderr)
        return 1

    for name, want in sorted(manifest.items()):
        if name not in assets:
            failures.append(
                f"{name}: 清单里有，但上游 release {tag} 已经没有这个资产"
                f"（归档被删或改名，构建会失败）"
            )
            continue
        got = _bare_digest(name, assets)
        if not got:
            failures.append(f"{name}: 上游未提供 digest，无法校验（哈希清单失去意义）")
            continue
        if got != want:
            failures.append(
                f"{name}: 内容变了！\n"
                f"    清单: {want}\n"
                f"    上游: {got}\n"
                f"    上游该资产被重新上传过（{base_url}/{name}）。"
                f"确认新内容可信后，用 --print-manifest 更新清单。"
            )
            continue
        checked += 1
        print(f"  ok  {name}  {want}")

    # 上游是共享的第三方 release，会带我们不打包的平台（android / x86 等），
    # 所以"上游多出资产"不是错误，只是提醒：真要发它就得先补进清单。
    extra = sorted(set(assets) - set(manifest))
    if extra:
        print(f"\n  提示：上游还有 {len(extra)} 个资产不在清单里（我们不打包）：")
        for name in extra:
            print(f"        {name}")
        print(f"  其中某天要打包的话，先跑 --print-manifest --asset <资产名> 补进清单。")

    if failures:
        print("\nffmpeg 哈希清单校验失败：", file=sys.stderr)
        for line in failures:
            print(f"  - {line}", file=sys.stderr)
        return 1

    print(f"\nffmpeg 哈希清单与 {base_url} 一致（{checked} 个归档）。")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="校验 ffmpeg 归档哈希清单")
    parser.add_argument(
        "--print-manifest",
        action="store_true",
        help="按清单格式打印上游摘要（用于重新生成 ffmpeg-checksums.txt）",
    )
    parser.add_argument(
        "--asset",
        action="append",
        default=[],
        metavar="NAME",
        help="配合 --print-manifest：指定要写入清单的资产名，可重复；"
        "不指定时刷新清单里已有的资产",
    )
    args = parser.parse_args()

    repo, tag = parse_base_url(MANIFEST)
    base_url = f"https://github.com/{repo}/releases/download/{tag}"
    assets = fetch_assets(repo, tag)

    if args.print_manifest:
        names = args.asset or list(parse_manifest(MANIFEST))
        if not names:
            raise SystemExit("清单为空，请用 --asset 指定要生成的资产名")
        print_manifest(base_url, assets, names)
        return 0
    return verify(base_url, tag, parse_manifest(MANIFEST), assets)


if __name__ == "__main__":
    sys.exit(main())
