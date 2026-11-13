#!/usr/bin/env python3
"""校验/生成随产物分发的 DXC（DirectX Shader Compiler）包哈希清单。

背景
----
D3D12 的着色器编译是**应用的责任**，不在显卡驱动里（Vulkan 相反：驱动自己吃
SPIR-V）。Windows 只自带冻结在 Shader Model 5.1 的老编译器
`d3dcompiler_47.dll`（FXC）；新编译器 DXC 由微软独立发行、由应用自带。wgpu 在
DX12 上「找到 `dxcompiler.dll` 就用，找不到就静默退 FXC」。

于是不随产物带一份 DXC 的机器，落到 DX12 时首次装载实测 **1051 s（17.5 分钟）**，
而且停在「加载识别模型」，看起来就是卡死。带了这两枚 DLL 之后是 **4.7–5.6 s**。

来源
----
微软官方 NuGet 包 `Microsoft.Direct3D.DXC`：NuGet 的包版本**不可变**（只能下架，
不能重传成不同内容），所以它比 GitHub release 资产更适合当固定来源。信任锚点
仍然是**内容哈希**，不是 URL —— 清单 `scripts/dxc-checksums.txt` 记下 nupkg 的
sha256，以及从包里取出的两枚 DLL 各自的 sha256。

本脚本做两件事：

  默认             下载清单里那个包，比对 sha256；再把 `# extract …` 列出的成员
                   解出来逐枚比对。CI 里跑它 -> 上游换了内容，这里立刻失败，而不是
                   等打包或用户下载之后才发现。
  --print-manifest 按清单格式打印（用于换版本时重新生成哈希段）。

`--from <nupkg>` 直接读一个本地包（离线重新生成清单时用）；`--cache <dir>` 把包
留在本地，重复跑不必重下 53 MB（命中缓存也照样校验）。

只依赖标准库。
"""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import sys
import tempfile
import urllib.error
import urllib.request
import zipfile

from _verify_common import utf8_when_redirected

MANIFEST = pathlib.Path(__file__).with_name("dxc-checksums.txt")
NUGET_PREFIX = "https://api.nuget.org/v3-flatcontainer/"


def parse_base_url(path: pathlib.Path) -> str:
    """从清单里读出 base-url（NuGet flat container 目录地址）。"""
    for line in path.read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[0] == "#" and parts[1] == "base-url":
            if not parts[2].startswith(NUGET_PREFIX):
                raise SystemExit(
                    f"{path}: base-url 不是 NuGet flat container 地址：{parts[2]}"
                )
            return parts[2]
        # 一行以 `# base-url` 开头但不止三段：说明说明文字被写进了指令行，
        # 直接报错而不是取到半截 URL（ffmpeg 清单那边踩过这个坑）。
        if len(parts) > 3 and parts[0] == "#" and parts[1] == "base-url":
            raise SystemExit(f"{path}: '# base-url' 行必须是单独一行，后面不能跟说明文字")
    raise SystemExit(f"{path}: 缺少 '# base-url <URL>' 行")


def parse_manifest(path: pathlib.Path) -> dict[str, str]:
    """读出 `# extract` 行与哈希行；返回 {包内成员或资产名: sha256}。"""
    hashes: dict[str, str] = {}
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        parts = line.split()
        if not parts:
            continue
        if parts[0].startswith("#"):
            if len(parts) == 4 and parts[1] == "extract":
                member, digest = parts[2], parts[3]
                if not _is_sha256(digest):
                    raise SystemExit(f"{path}:{lineno}: 不是合法的 sha256：{digest!r}")
                hashes[member] = digest
            continue
        if len(parts) != 2:
            raise SystemExit(
                f"{path}:{lineno}: 期望 `<sha256>  <文件名>`，实际是 {line.strip()!r}"
            )
        digest, name = parts
        if not _is_sha256(digest):
            raise SystemExit(f"{path}:{lineno}: 不是合法的 sha256：{digest!r}")
        hashes[name] = digest
    return hashes


def _is_sha256(text: str) -> bool:
    return len(text) == 64 and all(c in "0123456789abcdef" for c in text)


def package_name(hashes: dict[str, str], manifest: pathlib.Path) -> str:
    """哈希行里不是 `# extract` 的那一条就是包本身。"""
    names = [n for n in hashes if "/" not in n]
    if len(names) != 1:
        raise SystemExit(
            f"{manifest}: 需要恰好一个 nupkg 哈希行，实际有 {len(names)} 个：{names}"
        )
    return names[0]


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def download(url: str, dest: pathlib.Path, manifest: pathlib.Path) -> None:
    print(f"  下载 {url}", file=sys.stderr)
    try:
        with urllib.request.urlopen(url, timeout=120) as resp, dest.open("wb") as out:
            while True:
                chunk = resp.read(1 << 20)
                if not chunk:
                    break
                out.write(chunk)
    except urllib.error.HTTPError as err:
        if err.code == 404:
            raise SystemExit(
                f"{url} 返回 404：这个包版本已被下架（NuGet 允许下架但不会改内容）。\n"
                f"要么换一个仍可下载的版本并更新 {manifest}，要么把已有的包提交进缓存目录。"
            )
        raise SystemExit(f"{url} 返回 {err.code} {err.reason}")
    except urllib.error.URLError as err:
        raise SystemExit(f"连不上 NuGet（{url}）：{err.reason}")


def fetch_package(
    base_url: str, name: str, cache: pathlib.Path | None, manifest: pathlib.Path
) -> pathlib.Path:
    """取一份包：缓存优先，命中与否都要过哈希。"""
    if cache is not None:
        cache.mkdir(parents=True, exist_ok=True)
        cached = cache / name
        if cached.is_file():
            print(f"  缓存命中 {cached}（仍会校验哈希）", file=sys.stderr)
            return cached
    dest = (cache / name) if cache is not None else pathlib.Path(
        tempfile.mkdtemp(prefix="oneasr-dxc-")
    ) / name
    download(f"{base_url.rstrip('/')}/{name}", dest, manifest)
    return dest


def verify(
    base_url: str,
    base_hashes: dict[str, str],
    package: pathlib.Path,
    manifest: pathlib.Path,
) -> int:
    pinned = package_name(base_hashes, manifest)
    want = base_hashes[pinned]
    got = sha256_file(package)
    if got != want:
        print(
            f"{package.name}: 内容变了！\n    清单: {want}\n    实际: {got}\n"
            f"    {base_url}/{pinned} 已经不是清单钉住的那份包。不要打包它；\n"
            f"    确认真实内容可信之后用 --print-manifest 更新 {manifest}。",
            file=sys.stderr,
        )
        return 1
    print(f"  ok  {package.name}  {want}")

    checked = 0
    with zipfile.ZipFile(package) as zf:
        names = set(zf.namelist())
        for member, want_member in sorted(base_hashes.items()):
            if "/" not in member:
                continue  # 包自身那一条，上面已经校验过
            if member not in names:
                print(
                    f"{member}: 包里没有这个成员（上游改了归档布局？）",
                    file=sys.stderr,
                )
                return 1
            got_member = hashlib.sha256(zf.read(member)).hexdigest()
            if got_member != want_member:
                print(
                    f"{member}: 内容变了！\n    清单: {want_member}\n    实际: {got_member}",
                    file=sys.stderr,
                )
                return 1
            checked += 1
            print(f"  ok  {member}  {want_member}")
    print(f"\n  {checked + 1} 项全部通过。")
    return 0


def print_manifest(base_url: str, package: pathlib.Path, members: list[str]) -> None:
    print(f"# base-url {base_url}")
    print(
        "# 下面这段由 scripts/verify-dxc-manifest.py --print-manifest 生成，请勿手改哈希。"
    )
    print(f"{sha256_file(package)}  {package.name}")
    with zipfile.ZipFile(package) as zf:
        for member in members:
            digest = hashlib.sha256(zf.read(member)).hexdigest()
            print(f"# extract {member} {digest}")


def main() -> int:
    utf8_when_redirected()
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--manifest", type=pathlib.Path, default=MANIFEST, help="清单文件（默认同目录）"
    )
    ap.add_argument(
        "--from",
        dest="from_path",
        type=pathlib.Path,
        default=None,
        help="读本地 nupkg，不联网",
    )
    ap.add_argument(
        "--cache", type=pathlib.Path, default=None, help="把包留在本地目录，重复跑不必重下"
    )
    ap.add_argument(
        "--print-manifest",
        action="store_true",
        help="按清单格式打印哈希段（换版本时用）",
    )
    args = ap.parse_args()

    base_url = parse_base_url(args.manifest)
    hashes = parse_manifest(args.manifest)

    if args.print_manifest:
        package = args.from_path
        if package is None:
            package = fetch_package(
                base_url, package_name(hashes, args.manifest), args.cache, args.manifest
            )
        members = [m for m in sorted(hashes) if "/" in m]
        print_manifest(base_url, package, members)
        return 0

    package = args.from_path
    if package is None:
        package = fetch_package(
            base_url, package_name(hashes, args.manifest), args.cache, args.manifest
        )
    return verify(base_url, hashes, package, args.manifest)


if __name__ == "__main__":
    sys.exit(main())
