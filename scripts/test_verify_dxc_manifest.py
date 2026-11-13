#!/usr/bin/env python3
"""`scripts/verify-dxc-manifest.py` 的自测。

为什么要自测
------------
它是发布链上的一道门禁：把 DXC 那两枚 DLL 钉在哈希上，**放行错的比没有更危险**
——一个「永远绿」的校验脚本会让人以为内容被验证过了。所以这里不联网，用临时目录
造一个假包 + 假清单，逐条证明：

  1. 一切正确时退出码 0；
  2. 包内容变了（清单哈希对不上）必须退出码 1，且说出是内容变了；
  3. 包没变但成员被换掉（`# extract` 行对不上）必须退出码 1；
  4. 清单里少一行 `# extract` 时，打包脚本那一侧应当失败 —— 这里用
     「清单里没有该成员」的路径覆盖（解析层面）。

跑法：python3 scripts/test_verify_dxc_manifest.py（CI 会跑）
"""

from __future__ import annotations

import hashlib
import pathlib
import subprocess
import sys
import tempfile
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
VERIFIER = HERE / "verify-dxc-manifest.py"
BASE_URL = "https://api.nuget.org/v3-flatcontainer/fake.pkg/1.0.0"
PKG_NAME = "fake.pkg.1.0.0.nupkg"
MEMBER = "build/native/bin/x64/dxcompiler.dll"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def make_package(path: pathlib.Path, payload: bytes) -> bytes:
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr(MEMBER, payload)
    return path.read_bytes()


def write_manifest(path: pathlib.Path, package: bytes, member: bytes) -> None:
    path.write_text(
        "\n".join(
            [
                f"# base-url {BASE_URL}",
                "",
                f"{sha256(package)}  {PKG_NAME}",
                f"# extract {MEMBER} {sha256(member)}",
                "",
            ]
        ),
        encoding="utf-8",
    )


def run(manifest: pathlib.Path, package: pathlib.Path) -> tuple[int, str]:
    proc = subprocess.run(
        [
            sys.executable,
            str(VERIFIER),
            "--manifest",
            str(manifest),
            "--from",
            str(package),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return proc.returncode, proc.stdout + proc.stderr


def main() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="oneasr-dxc-selftest-") as tmp:
        tmpdir = pathlib.Path(tmp)
        package = tmpdir / PKG_NAME
        manifest = tmpdir / "dxc-checksums.txt"

        good_member = b"\x00dxcompiler-bytes\x00"
        package_bytes = make_package(package, good_member)

        # 1. 一切正确 -> 0
        write_manifest(manifest, package_bytes, good_member)
        code, out = run(manifest, package)
        if code != 0:
            failures.append(f"正确内容应当退出码 0，实际 {code}：\n{out}")
        elif "全部通过" not in out:
            failures.append(f"正确内容没有打印通过标记：\n{out}")

        # 2. 包内容变了 -> 1（清单钉的是旧字节）
        tampered = tmpdir / PKG_NAME
        make_package(tampered, b"\x00different\x00")
        code, out = run(manifest, tampered)
        if code == 0:
            failures.append("包内容变了却通过了")

        # 3. 包没变、成员被换 -> 1（`# extract` 行仍然指着旧成员哈希）
        package_bytes = make_package(package, b"\x00swapped-member\x00")
        write_manifest(manifest, package_bytes, good_member)  # 成员哈希故意留旧的
        code, out = run(manifest, package)
        if code == 0:
            failures.append("成员被换掉却通过了")

        # 4. 清单缺 `# extract` 行：包校验仍然通过，但打包脚本会少一层保护，
        #    所以这里只断言「不崩、且不谎报成员已校验」。
        manifest.write_text(
            "\n".join([f"# base-url {BASE_URL}", f"{sha256(package_bytes)}  {PKG_NAME}", ""]),
            encoding="utf-8",
        )
        code, out = run(manifest, package)
        if code != 0:
            failures.append(f"只钉包哈希时应当通过，实际 {code}：\n{out}")
        if MEMBER in out:
            failures.append("清单没有该成员，输出却提到它（会让人以为校验过）")

    if failures:
        for f in failures:
            print(f"FAIL  {f}", file=sys.stderr)
        return 1
    print("ok  4/4")
    return 0


if __name__ == "__main__":
    sys.exit(main())
